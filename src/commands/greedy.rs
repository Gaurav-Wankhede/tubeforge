//! `greedy` commands: automated topic research on autopilot.
//!
//! Subcommands:
//!   run   — generate candidates and research as many as possible
//!   status — stats on the research history
//!   seeds — add / list / deactivate seed topics

use std::collections::HashSet;
use std::path::PathBuf;

use serde_json::{json, Value};

use crate::analytics::history_tracker;
use crate::analytics::topic_generator;
use crate::config::Config;
use crate::error::TubeforgeError;
use crate::fetch::FetchClients;
use crate::storage::Db;

/// PID file path: `~/.tubeforge/greedy.pid`.
fn pid_path(cfg: &Config) -> PathBuf {
    cfg.data_dir.join("greedy.pid")
}

/// Write the current process PID to the PID file.
fn write_pid(cfg: &Config) -> Result<(), TubeforgeError> {
    let path = pid_path(cfg);
    let pid = std::process::id();
    std::fs::write(&path, pid.to_string()).map_err(|e| {
        TubeforgeError::Usage(format!("failed to write PID file {}: {e}", path.display()))
    })?;
    Ok(())
}

/// Remove the PID file if it exists.
fn remove_pid(cfg: &Config) {
    let _ = std::fs::remove_file(pid_path(cfg));
}

/// Read the PID from the file. Returns `None` if missing or malformed.
fn read_pid(cfg: &Config) -> Option<u32> {
    let content = std::fs::read_to_string(pid_path(cfg)).ok()?;
    content.trim().parse().ok()
}

/// `greedy run [--max N]`: generate candidates and research eligible ones.
pub async fn run_research(cfg: &Config, max: usize) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let clients = FetchClients::new()?;
    let candidates = topic_generator::generate_candidates(&db, &clients, &cfg.niche_terms).await?;
    let cooldown_hours = None; // default 24h
    let mut researched = Vec::new();
    let mut skipped = Vec::new();

    for cand in &candidates {
        if researched.len() >= max {
            break;
        }
        let eligible = history_tracker::is_eligible(&db, &cand.topic, cooldown_hours).await?;
        if !eligible {
            history_tracker::log_attempt(&db, &cand.topic, "skipped", "cooldown").await?;
            skipped.push(cand.topic.clone());
            continue;
        }

        // Call the existing `keywords research` pipeline (ytsearch SERP + tags + autocomplete).
        let ytdlp = crate::fetch::ytdlp::YtdlpClient::new(
            cfg.ytdlp_path.clone(),
            cfg.ytdlp_enabled,
            cfg.ytdlp_client.clone(),
            cfg.ytdlp_js_runtime.clone(),
        )?;
        let bm25 = crate::search::open_or_create(&cfg.index_dir())
            .ok()
            .and_then(|index| crate::search::bm25::Bm25::open(index).ok());

        let start = std::time::Instant::now();
        let result = crate::analytics::research::inspect(
            &db,
            bm25.as_ref(),
            &ytdlp,
            &clients,
            &cand.topic,
            crate::analytics::research::DEFAULT_SERP,
        )
        .await;
        let elapsed = start.elapsed().as_millis() as u64;

        match result {
            Ok(research) => {
                let video_ids: Vec<String> =
                    research.serp.iter().map(|r| r.video_id.clone()).collect();
                let mean_views = research.serp_mean_views;

                history_tracker::record_research(
                    &db,
                    &cand.topic,
                    &video_ids,
                    mean_views,
                    &cand.source.to_string(),
                    elapsed,
                )
                .await?;
                history_tracker::log_attempt(&db, &cand.topic, "success", "").await?;
                researched.push(cand.topic.clone());
            }
            Err(e) => {
                history_tracker::log_attempt(&db, &cand.topic, "error", &e.to_string()).await?;
                skipped.push(cand.topic.clone());
            }
        }
    }

    Ok(json!({
        "candidates_total": candidates.len(),
        "researched": researched,
        "skipped": skipped,
        "researched_count": researched.len(),
    }))
}

/// `greedy status`: aggregate stats over research history.
pub async fn run_status(cfg: &Config) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    history_tracker::stats(&db).await
}

/// `greedy seeds add <seed>...`: add seed topics.
pub async fn run_seeds_add(db: &Db, seeds: &[String]) -> Result<Value, TubeforgeError> {
    let mut added = 0;
    for s in seeds {
        db.insert_greedy_seed(s, "cli").await?;
        added += 1;
    }
    let all = db.list_greedy_seeds().await?;
    let seeds_json: Vec<Value> = all
        .iter()
        .map(|s| {
            json!({
                "seed_id": s.seed_id,
                "seed": s.seed,
                "source": s.source,
                "added_at": s.added_at,
                "active": s.active,
            })
        })
        .collect();
    Ok(json!({ "added": added, "seeds": seeds_json }))
}

/// `greedy seeds list`: list all seeds.
pub async fn run_seeds_list(db: &Db) -> Result<Value, TubeforgeError> {
    let all = db.list_greedy_seeds().await?;
    let seeds_json: Vec<Value> = all
        .iter()
        .map(|s| {
            json!({
                "seed_id": s.seed_id,
                "seed": s.seed,
                "source": s.source,
                "added_at": s.added_at,
                "active": s.active,
            })
        })
        .collect();
    Ok(json!({ "seeds": seeds_json }))
}

/// `greedy seeds deactivate <id>`: mark a seed as inactive.
pub async fn run_seeds_deactivate(db: &Db, seed_id: i64) -> Result<Value, TubeforgeError> {
    let ok: bool = db.deactivate_greedy_seed(seed_id).await?;
    Ok(json!({ "deactivated": ok, "seed_id": seed_id }))
}

/// `greedy seeds init`: autonomously discover and seed topics from the
/// channel's existing data (competitor tags, tracked keywords, video tags,
/// research results). Skips any seed that already exists.
pub async fn run_seeds_init(db: &Db) -> Result<Value, TubeforgeError> {
    let existing: HashSet<String> = db
        .list_greedy_seeds()
        .await?
        .iter()
        .map(|s| s.seed.to_lowercase())
        .collect();

    let discovered = db.greedy_discover_seeds(200).await?;

    let mut added = 0;
    let mut skipped = 0;
    for seed in &discovered {
        if existing.contains(&seed.to_lowercase()) {
            skipped += 1;
            continue;
        }
        db.insert_greedy_seed(seed, "auto_discover").await?;
        added += 1;
    }

    let all = db.list_greedy_seeds().await?;
    let seeds_json: Vec<Value> = all
        .iter()
        .map(|s| {
            json!({
                "seed_id": s.seed_id,
                "seed": s.seed,
                "source": s.source,
                "added_at": s.added_at,
                "active": s.active,
            })
        })
        .collect();
    Ok(json!({
        "added": added,
        "skipped_duplicates": skipped,
        "discovered_from_data": discovered.len(),
        "total_seeds": seeds_json.len(),
        "seeds": seeds_json,
    }))
}

/// `greedy daemon --interval SECS --max N`: long-running scheduler that
/// researches topics on a fixed interval. Writes a PID file for clean
/// shutdown. Gracefully handles SIGINT / SIGTERM.
pub async fn run_daemon(
    cfg: &Config,
    interval_secs: u64,
    max: usize,
) -> Result<Value, TubeforgeError> {
    // Refuse to start if a daemon is already running.
    if let Some(old_pid) = read_pid(cfg) {
        if is_alive(old_pid) {
            return Err(TubeforgeError::Usage(format!(
                "greedy daemon already running (PID {old_pid}). \
                 Stop it first with `tubeforge greedy stop`."
            )));
        }
        // Stale PID file — remove it.
        remove_pid(cfg);
    }

    write_pid(cfg)?;
    eprintln!(
        "greedy daemon started: interval={interval_secs}s, max={max}, pid={}",
        std::process::id()
    );

    let shutdown = async {
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        {
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("failed to register SIGTERM handler");
            tokio::select! {
                _ = ctrl_c => eprintln!("\ngreedy daemon: SIGINT received, shutting down…"),
                _ = sigterm.recv() => eprintln!("\ngreedy daemon: SIGTERM received, shutting down…"),
            }
        }
        #[cfg(not(unix))]
        {
            ctrl_c.await.ok();
            eprintln!("\ngreedy daemon: SIGINT received, shutting down…");
        }
    };

    tokio::pin!(shutdown);

    let mut tick = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    // Channel to receive research results from the spawned task.
    let (done_tx, mut done_rx) = tokio::sync::mpsc::channel::<Result<Value, TubeforgeError>>(1);
    let mut research_in_progress = false;

    loop {
        tokio::select! {
            _ = &mut shutdown => {
                eprintln!("greedy daemon: shutdown signal received");
                break;
            }
            _ = tick.tick() => {
                if research_in_progress {
                    eprintln!("greedy daemon: previous research still running, skipping tick");
                    continue;
                }
                eprintln!("greedy daemon: tick — researching up to {max} topics…");
                let cfg_clone = cfg.clone();
                let tx = done_tx.clone();
                research_in_progress = true;
                tokio::spawn(async move {
                    let result = run_research(&cfg_clone, max).await;
                    let _ = tx.send(result).await;
                });
            }
            result = done_rx.recv() => {
                research_in_progress = false;
                match result {
                    Some(Ok(val)) => {
                        let researched = val
                            .get("researched_count")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        eprintln!("greedy daemon: researched {researched} topics");
                    }
                    Some(Err(e)) => {
                        eprintln!("greedy daemon: error — {e}");
                    }
                    None => {
                        // Channel closed — spawned task panicked.
                        eprintln!("greedy daemon: research task failed unexpectedly");
                    }
                }
            }
        }
    }

    remove_pid(cfg);
    eprintln!("greedy daemon: stopped");
    Ok(json!({ "stopped": true }))
}

/// `greedy stop`: send SIGTERM to a running greedy daemon via its PID file.
pub async fn run_stop(cfg: &Config) -> Result<Value, TubeforgeError> {
    let pid = read_pid(cfg).ok_or_else(|| {
        TubeforgeError::Usage(
            "no greedy daemon running (PID file not found). \
             Start one with `tubeforge greedy daemon`."
                .into(),
        )
    })?;

    if !is_alive(pid) {
        remove_pid(cfg);
        return Err(TubeforgeError::Usage(format!(
            "greedy daemon PID {pid} is not alive. Stale PID file removed."
        )));
    }

    #[cfg(not(unix))]
    {
        Err(TubeforgeError::Usage(
            "greedy stop is only supported on Unix".into(),
        ))
    }

    #[cfg(unix)]
    {
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
        // Wait briefly for the process to exit.
        for _ in 0..50 {
            if !is_alive(pid) {
                remove_pid(cfg);
                return Ok(json!({ "stopped": true, "pid": pid }));
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        Err(TubeforgeError::Usage(format!(
            "sent SIGTERM to PID {pid}, but process did not exit within 5s. \
             Check manually: kill {pid}"
        )))
    }
}

/// Check if a process with the given PID is alive.
fn is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}
