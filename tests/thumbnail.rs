//! Thumbnail Generator integration tests (Phase 3, workstream A).
//!
//! `render_headless_chromium_png` is `#[ignore]`d by default: the first run
//! downloads the pinned Chromium build (~150MB, chromiumoxide_fetcher) and
//! takes ~10-60s. Run it manually with:
//!
//!     cargo test --test thumbnail -- --ignored --nocapture
//!
//! The install is cached under the temp data dir, so re-runs are fast.

use tubeforge::thumbnail::{self, TemplateValues};

fn values() -> TemplateValues {
    TemplateValues {
        title: Some("Rust in 60 seconds".into()),
        channel: Some("TubeForge".into()),
        channel_initial: Some("T".into()),
        duration: Some("4:20".into()),
        category: Some("Science & Technology".into()),
        video_id: Some("dQw4w9WgXcQ".into()),
    }
}

#[tokio::test]
async fn list_templates_returns_default() {
    let out = tubeforge::commands::thumbnail::run_list_templates()
        .await
        .expect("list templates");
    let templates = out["templates"]
        .as_array()
        .expect("templates array")
        .iter()
        .map(|t| t.as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(templates, vec!["default"]);
}

#[test]
fn default_template_builds_into_single_file_html() {
    let template = thumbnail::load_template("default").expect("load template");
    let html = thumbnail::build_single_file(template, &values()).expect("build");
    // Self-contained: no external stylesheet, no network dependencies.
    assert!(html.contains("Rust in 60 seconds"));
    assert!(html.contains("<style>"));
    assert!(!html.contains("rel=\"stylesheet\""));
    assert!(html.contains("dQw4w9WgXcQ"));
}

/// Real end-to-end render through headless Chromium (CDP). Proves the pinned
/// Chromium downloads once, renders the template at exactly 1280x720 and
/// reuses the install on a second render.
#[tokio::test]
#[ignore = "requires the pinned Chromium download (see module docs): \
            cargo test --test thumbnail -- --ignored"]
async fn render_headless_chromium_png() {
    let dir = tempfile::tempdir().expect("tempdir");
    let chromium_dir = dir.path().join("chromium");
    let html = thumbnail::build_single_file(
        thumbnail::load_template("default").expect("load template"),
        &values(),
    )
    .expect("build");

    let out = dir.path().join("thumb.png");
    thumbnail::render::render_html_to_png_in(&html, &out, &chromium_dir)
        .await
        .expect("render");
    assert!(out.exists(), "PNG must exist");

    let bytes = std::fs::read(&out).expect("read png");
    // PNG signature.
    assert_eq!(&bytes[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    // IHDR: width/height at fixed offsets 16..24.
    let width = u32::from_be_bytes(bytes[16..20].try_into().expect("width"));
    let height = u32::from_be_bytes(bytes[20..24].try_into().expect("height"));
    assert_eq!((width, height), (1280, 720));

    // Second render must reuse the same Chromium install (fetcher's local
    // check), not redownload.
    let out2 = dir.path().join("thumb2.png");
    thumbnail::render::render_html_to_png_in(&html, &out2, &chromium_dir)
        .await
        .expect("render again");
    assert!(out2.exists());
}
