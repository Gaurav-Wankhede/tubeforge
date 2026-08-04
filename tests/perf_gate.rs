//! Phase 4 performance smoke gate (LLD §12): 5k videos ingest + reindex +
//! top-k ideas < 30s on M4.
//!
//! This test is OPT-IN (`#[ignore]`): it synthesizes a deterministic 5,000
//! video corpus and pushes it through the REAL pipeline paths — the storage
//! batch write path (`upsert_channel`/`upsert_video`/`log_ingest` in one
//! transaction, exactly what `ingest` runs), the ingest index sync (one
//! tantivy writer, add all, single commit), the post-ingest scoring pass
//! (`scoring::score_video`, the code `ingest` calls for changed videos), the
//! `reindex` command (full rebuild from the videos table), and the `ideas`
//! command (top-k Next Ideas pool). No network, no wiremock — the corpus is
//! generated locally by a seeded xorshift64* PRNG.
//!
//! Run the gate:
//! ```text
//! # Official gate — release profile (meaningful timings; this is the LLD
//! # §12 budget: <30s total on M4):
//! cargo test --release --test perf_gate -- --ignored --nocapture
//!
//! # Debug-build check (same thresholds apply, but expect 3-10x slower —
//! # a debug run may legitimately exceed the budget; the OFFICIAL gate is
//! # the release run above):
//! cargo test --test perf_gate -- --ignored --nocapture
//! ```
//!
//! Per-phase budgets (generous on purpose; the 30s total is the hard gate).
//! Measured on M4 release (Aug 4, 2026): a 5k first-sync ingest costs ~20s
//! of BM25 scoring alone — `post_ingest_scoring` runs 3 self-excluding
//! corpus-resonance searches per video, each fetching the matching docs —
//! so the ingest budget is 25s. Reindex ~0.2s, ideas ~0.3s. The test prints
//! a single summary line `PERF GATE [...]` with all four timings.

use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, Instant};

use tubeforge::commands::{ideas, reindex};
use tubeforge::config::Config;
use tubeforge::scoring::weights::Weights;
use tubeforge::search::bm25::Bm25;
use tubeforge::search::{open_or_create, VideoDoc};
use tubeforge::storage::db::{ChannelRow, VideoRow};
use tubeforge::storage::Db;

// ---------------------------------------------------------------------------
// Gate constants (LLD §12: 5k videos, ingest + reindex + top-k ideas < 30s)
// ---------------------------------------------------------------------------

/// Corpus size — the LLD §12 gate volume.
const N_VIDEOS: u64 = 5_000;
/// Channel count: 1 user channel + 9 competitors.
const N_CHANNELS: u64 = 10;
/// Tracked keywords seeded before the gate (feeds scoring + ideas).
const N_KEYWORDS: usize = 8;
/// Top-k idea pool size (`tubeforge ideas` limit).
const TOP_K: usize = 10;
/// User-channel share of the corpus (60% — the rest spreads over rivals).
const USER_VIDEO_RATIO: u64 = 6;

/// Ingest phase budget: storage batch + index sync + 5k scoring passes.
/// Scoring dominates (~20s on M4 release — 3 self-excluding BM25 searches
/// per video over a 5k corpus), so the budget sits at 25s.
const LIMIT_INGEST: Duration = Duration::from_secs(25);
/// Reindex phase budget: full tantivy rebuild from the videos table.
const LIMIT_REINDEX: Duration = Duration::from_secs(10);
/// Ideas phase budget: graph build + BM25 neighborhoods + top-k persist.
const LIMIT_IDEAS: Duration = Duration::from_secs(5);
/// THE hard gate (LLD §12): 5k ingest + reindex + top-k ideas < 30s on M4.
const LIMIT_TOTAL: Duration = Duration::from_secs(30);

const KEYWORDS: [&str; N_KEYWORDS] = [
    "rust", "database", "sqlite", "tutorial", "guide", "performance", "storage", "indexing",
];

// ---------------------------------------------------------------------------
// Deterministic dataset: xorshift64* PRNG (no rand dependency)
// ---------------------------------------------------------------------------

/// xorshift64* — deterministic, full period, NOT cryptographic. Seed fixed
/// so the dataset is byte-identical on every run.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Lcg(seed.max(1))
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
    fn pick<'a>(&mut self, items: &'a [&'a str]) -> &'a str {
        items[self.below(items.len() as u64) as usize]
    }
}

/// YouTube id alphabet (64 chars — same set real ids use).
const ID_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn video_id(rng: &mut Lcg) -> String {
    (0..11).map(|_| ID_ALPHABET[rng.below(ID_ALPHABET.len() as u64) as usize] as char).collect()
}

fn channel_id(rng: &mut Lcg) -> String {
    let mut s = String::from("UC");
    for _ in 0..22 {
        s.push(ID_ALPHABET[rng.below(ID_ALPHABET.len() as u64) as usize] as char);
    }
    s
}

/// One series title template per channel (0 = user). All share vocabulary
/// (rust/database/sqlite/tutorial/guide) so BM25 neighborhoods and overlap
/// edges are non-trivial — the ideas machinery has real signals to work on.
const TITLE_TEMPLATES: [&str; 10] = [
    "How to Build a Database in Rust — Part {p}",
    "SQLite Internals Explained — Part {p}",
    "Rust Backend Guide: Storage Engines — Part {p}",
    "Database Performance Tips: WAL & Transactions — Part {p}",
    "Rust Tutorial: Building Query Engines — Part {p}",
    "The Complete Guide to Rust Databases — Part {p}",
    "SQL Performance Secrets in Rust — Part {p}",
    "Beginner Rust Database Tutorial — Part {p}",
    "Advanced Indexing Techniques (Rust) — Part {p}",
    "Fast SQLite for Rust Apps — Part {p}",
];

/// Valid YouTube category ids (LLD §3.1 category map).
const CATEGORIES: [&str; 10] = ["28", "27", "22", "24", "26", "2", "1", "20", "17", "23"];
const TAG_POOL: [&str; 16] = [
    "rust", "database", "sqlite", "tutorial", "guide", "performance", "backend", "storage",
    "indexing", "transactions", "wal", "query", "engine", "beginner", "advanced", "programming",
];

struct SynthDataset {
    channels: Vec<ChannelRow>,
    videos: Vec<VideoRow>,
}

/// Deterministic 5k-video corpus: 10 channels (1 user + 9 competitors),
/// realistic series titles, keyword-rich descriptions, JSON tags, spread
/// published dates, LCG-scaled views/likes/comments, rss/api source mix.
fn synth_dataset() -> SynthDataset {
    let mut rng = Lcg::new(0x9E37_79B9_7F4A_7C15);
    let now = tubeforge::util::now_rfc3339();
    let base = chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z").expect("epoch");

    let mut channels: Vec<ChannelRow> = Vec::with_capacity(N_CHANNELS as usize);
    for i in 0..N_CHANNELS {
        let cid = channel_id(&mut rng);
        channels.push(ChannelRow {
            channel_id: cid.clone(),
            handle: if i == 0 {
                Some("@perfforge".to_string())
            } else {
                Some(format!("@rival{i}"))
            },
            title: if i == 0 {
                "PerfForge Labs".to_string()
            } else {
                format!("Rust Rival {i}")
            },
            description: Some("Synthetic perf-gate channel".to_string()),
            country: Some("US".to_string()),
            subscriber_count: Some(rng.below(2_000_000) as i64),
            video_count: Some(0),
            source: if i == 0 { "api".to_string() } else { "rss".to_string() },
            fetched_at: now.clone(),
            updated_at: now.clone(),
            ..Default::default()
        });
    }

    let mut videos: Vec<VideoRow> = Vec::with_capacity(N_VIDEOS as usize);
    let mut parts = [0u64; N_CHANNELS as usize];
    for i in 0..N_VIDEOS {
        // 60% user-channel videos, the rest round-robins over 9 rivals.
        let ci: usize = if rng.below(10) < USER_VIDEO_RATIO {
            0
        } else {
            1 + (i % 9) as usize
        };
        let part = parts[ci];
        parts[ci] += 1;

        let id = video_id(&mut rng);
        let title = TITLE_TEMPLATES[ci].replace("{p}", &part.to_string());
        let tags: Vec<&str> = (0..4 + rng.below(3)).map(|_| rng.pick(&TAG_POOL)).collect();
        let tags_json = serde_json::to_string(&tags).expect("tags json");
        let views = rng.below(2_000_000);
        let published_at =
            (base + chrono::Duration::minutes(rng.below(365 * 2 * 24 * 60) as i64)).to_rfc3339();

        videos.push(VideoRow {
            video_id: id.clone(),
            channel_id: Some(channels[ci].channel_id.clone()),
            title,
            description: format!(
                "In this {kw} guide for Rust developers we cover the full pipeline. \
                 Real SQLite and database internals, WAL modes and query plans.\n\n\
                 - storage engine deep dive\n- benchmark numbers\n- code walkthrough\n\n\
                 #rust #database #tutorial\n0:00 intro",
                kw = rng.pick(&TAG_POOL)
            ),
            tags: tags_json,
            category_id: Some(rng.pick(&CATEGORIES).to_string()),
            duration_sec: Some((90 + rng.below(3600)) as i64),
            published_at,
            view_count: Some(views as i64),
            like_count: Some((views / 13 + 5) as i64),
            comment_count: Some((views / 97 + 1) as i64),
            thumb_url: Some(format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg")),
            source: if rng.below(10) < 6 { "rss".to_string() } else { "api".to_string() },
            fetched_at: now.clone(),
            updated_at: now.clone(),
            topic_categories: "[]".to_string(),
            ..Default::default()
        });
    }

    let mut per_channel = [0u64; N_CHANNELS as usize];
    for v in &videos {
        let ci = channels
            .iter()
            .position(|c| Some(&c.channel_id) == v.channel_id.as_ref())
            .expect("channel lookup");
        per_channel[ci] += 1;
    }
    for (c, n) in channels.iter_mut().zip(per_channel) {
        c.video_count = Some(n as i64);
    }

    SynthDataset { channels, videos }
}

/// The `videos` rows in tantivy document form (same mapping as ingest.rs
/// `video_to_doc` — LLD §3.2).
fn to_docs(videos: &[VideoRow]) -> Vec<VideoDoc> {
    videos
        .iter()
        .map(|v| VideoDoc {
            video_id: v.video_id.clone(),
            channel_id: v.channel_id.clone(),
            title: v.title.clone(),
            description: v.description.clone(),
            tags: serde_json::from_str(&v.tags).unwrap_or_default(),
            published_at: chrono::DateTime::parse_from_rfc3339(&v.published_at)
                .ok()
                .map(|d| d.with_timezone(&chrono::Utc).timestamp()),
        })
        .collect()
}

fn test_config(dir: &Path) -> Config {
    Config {
        db_path: dir.join("tubeforge.db"),
        data_dir: dir.join("data"),
        backup_dir: dir.join("backups"),
        backup_keep: 10,
        log_level: "info".to_string(),
        youtube_api_key: None,
        quota_warn_at: 90,
        chromium_dir: dir.join("chromium"),
    }
}

// ---------------------------------------------------------------------------
// Cheap sanity tests (non-ignored — the corpus generator is shared state)
// ---------------------------------------------------------------------------

/// The synthetic generator is deterministic (same seed → same corpus) and
/// produces 5,000 unique, realistic 11-char ids, all anchored to a known
/// channel.
#[test]
fn perf_dataset_is_unique_and_deterministic() {
    let a = synth_dataset();
    let b = synth_dataset();
    assert_eq!(a.videos.len(), N_VIDEOS as usize);
    assert_eq!(a.channels.len(), N_CHANNELS as usize);

    let mut seen = HashSet::with_capacity(a.videos.len());
    for v in &a.videos {
        assert!(seen.insert(v.video_id.clone()), "duplicate video_id {}", v.video_id);
        assert_eq!(v.video_id.len(), 11, "realistic youtube id length");
        let cid = v.channel_id.as_deref().expect("every video has a channel");
        assert!(
            a.channels.iter().any(|c| c.channel_id == cid),
            "video {} references unknown channel {cid}",
            v.video_id
        );
    }

    assert_eq!(a.videos[0].video_id, b.videos[0].video_id);
    assert_eq!(a.videos[4999].title, b.videos[4999].title);
    let titles: HashSet<&str> = a.videos.iter().map(|v| v.title.as_str()).collect();
    assert!(titles.len() > 100, "realistic title variety, got {}", titles.len());
    let rss = a.videos.iter().filter(|v| v.source == "rss").count();
    let api = a.videos.len() - rss;
    assert!(rss > 0 && api > 0, "rss/api source mix present");
}

// ---------------------------------------------------------------------------
// THE GATE (opt-in)
// ---------------------------------------------------------------------------

/// LLD §12 performance smoke gate: 5k videos ingest + reindex + top-k ideas
/// < 30s total on M4. Uses the real pipeline paths end to end.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "perf gate: opt-in. cargo test --release --test perf_gate -- --ignored --nocapture (official gate); debug runs may exceed the budget"]
async fn perf_gate_5k_videos_ingest_reindex_ideas_under_30s() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let ds = synth_dataset();
    let mut db = Db::open(&cfg.db_path).await.expect("open db");

    // Tracked keywords — seeded before the timed phases (the real flow runs
    // `keywords add` before `ideas`; scoring + ideas consume the list).
    db.add_keywords(
        &KEYWORDS.iter().map(|s| s.to_string()).collect::<Vec<String>>(),
        Some("database"),
    )
    .await
    .expect("add keywords");

    let wall = Instant::now();

    // ---- Phase 1: ingest ----------------------------------------------
    // The storage write path of ingest.rs: ONE transaction holding channel
    // upserts, video upserts, and the per-item ingest_log; then the index
    // sync (one tantivy writer, add all, single commit + freshness stamp);
    // then the post-ingest scoring pass over every changed video.
    let t = Instant::now();
    let batch_id = tubeforge::util::batch_id();
    {
        let mut batch = db.begin_batch().await.expect("batch");
        for c in &ds.channels {
            batch.upsert_channel(c).await.expect("upsert_channel");
        }
        for v in &ds.videos {
            batch.upsert_video(v).await.expect("upsert_video");
        }
        for v in &ds.videos {
            batch
                .log_ingest(&batch_id, &v.video_id, "ok", None)
                .await
                .expect("ingest_log");
        }
        batch.commit().await.expect("commit");
    }
    let docs = to_docs(&ds.videos);
    let index = open_or_create(&cfg.index_dir()).expect("open index");
    {
        let fields = index.schema();
        let mut writer = index.writer(50_000_000).expect("index writer");
        for d in &docs {
            tubeforge::search::index::upsert(&mut writer, &fields, d).expect("index upsert");
        }
        writer.commit().expect("index commit");
    }
    db.meta_set("last_reindex_at", &tubeforge::util::now_rfc3339())
        .await
        .expect("stamp");
    let weights = Weights::from_env().expect("weights");
    {
        let mut bm25 = Bm25::open(index).expect("bm25");
        bm25.reload().expect("reload");
        for v in &ds.videos {
            tubeforge::scoring::score_video(&db, &bm25, v, &weights)
                .await
                .expect("score_video");
        }
    }
    let ingest = t.elapsed();

    // ---- Phase 2: reindex (real command path) -------------------------
    let t = Instant::now();
    let out = reindex::run(&cfg).await.expect("reindex");
    let reindexed = out["docs"].as_u64().expect("reindex docs count");
    assert_eq!(reindexed, N_VIDEOS, "reindex rebuilt the full corpus");
    let reindex = t.elapsed();

    // ---- Phase 3: top-k ideas (real command path) ---------------------
    let t = Instant::now();
    let out = ideas::run(&cfg, TOP_K, Some("rust database tutorial"), None)
        .await
        .expect("ideas");
    let idea_rows = out["ideas"].as_array().expect("ideas array");
    assert_eq!(idea_rows.len(), TOP_K, "top-k idea pool generated");
    let ideas = t.elapsed();

    let total = wall.elapsed();

    // ---- Verify the corpus actually landed -----------------------------
    assert_eq!(
        db.count("SELECT count(*) FROM videos").await.unwrap(),
        N_VIDEOS as i64
    );
    assert_eq!(
        db.count("SELECT count(*) FROM channels").await.unwrap(),
        N_CHANNELS as i64
    );
    assert_eq!(
        db.count("SELECT count(*) FROM scores").await.unwrap(),
        N_VIDEOS as i64,
        "post-ingest scoring covered the whole corpus"
    );
    assert_eq!(
        db.count("SELECT count(*) FROM ingest_log").await.unwrap(),
        N_VIDEOS as i64,
        "ingest_log rows mirror the ingest"
    );
    assert_eq!(bm25_num_docs(&cfg), N_VIDEOS, "index holds the corpus");

    // ---- The budgets ----------------------------------------------------
    assert!(ingest < LIMIT_INGEST, "ingest {ingest:?} >= budget {LIMIT_INGEST:?}");
    assert!(reindex < LIMIT_REINDEX, "reindex {reindex:?} >= budget {LIMIT_REINDEX:?}");
    assert!(ideas < LIMIT_IDEAS, "ideas {ideas:?} >= budget {LIMIT_IDEAS:?}");
    assert!(
        total < LIMIT_TOTAL,
        "HARD GATE FAILED: total {total:?} >= 30s (LLD §12)"
    );

    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    println!(
        "PERF GATE [{profile}] videos={N_VIDEOS} channels={N_CHANNELS} keywords={N_KEYWORDS} \
         ingest={:.2}s reindex={:.2}s ideas={:.2}s total={:.2}s \
         budgets(ingest<25 reindex<10 ideas<5 total<30) PASS",
        ingest.as_secs_f64(),
        reindex.as_secs_f64(),
        ideas.as_secs_f64(),
        total.as_secs_f64(),
    );
}

/// Document count straight from the tantivy index (independent of the Db).
fn bm25_num_docs(cfg: &Config) -> u64 {
    let index = open_or_create(&cfg.index_dir()).expect("open index");
    Bm25::open(index).expect("bm25").num_docs()
}
