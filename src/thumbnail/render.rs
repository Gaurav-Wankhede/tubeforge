//! Headless-Chromium PNG renderer (chromiumoxide over CDP).
//!
//! The Chromium build is pinned by `chromiumoxide_fetcher` and auto-downloaded
//! on first use into `<TUBEFORGE_CHROMIUM_DIR>` (default `<data>/chromium`);
//! later renders reuse the install. Downloads never fail silently: they map
//! to a `STORAGE/RENDER`-family error with an actionable message, never an
//! empty PNG.
//!
//! Determinism: `--headless=new` + `--window-size=1280x720` + the browser
//! viewport (which issues CDP `Emulation.setDeviceMetricsOverride`), then a
//! `Page.captureScreenshot` clipped to 1280x720. A 30s budget bounds the
//! whole render so a hung page can never block the CLI; the Chromium process
//! is closed explicitly and reaped on every path.

use std::path::{Path, PathBuf};
use std::time::Duration;

use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::page::{CaptureScreenshotFormat, Viewport as Clip};
use chromiumoxide::handler::viewport::Viewport;
use chromiumoxide::page::ScreenshotParams;
use chromiumoxide_fetcher::{BrowserFetcher, BrowserFetcherOptions};
use futures::StreamExt;

use crate::config::{self, Config};
use crate::error::{storage_err, TubeforgeError};
use crate::thumbnail::{THUMB_HEIGHT, THUMB_WIDTH};

/// Total budget for one render including Chromium launch.
const RENDER_TIMEOUT: Duration = Duration::from_secs(30);
/// Fixed settle after content load so fonts/layout finish (deterministic).
const SETTLE: Duration = Duration::from_millis(150);

/// Render `html` to a PNG at `out_path`, resolving the Chromium install dir
/// from `TUBEFORGE_CHROMIUM_DIR` / `TUBEFORGE_DATA_DIR` / defaults.
pub async fn render_html_to_png(
    html: &str,
    out_path: &Path,
) -> Result<(), TubeforgeError> {
    let dir = chromium_dir();
    render_html_to_png_in(html, out_path, &dir).await
}

/// Render `html` to a PNG at `out_path` with an explicit Chromium install
/// dir (the CLI passes `Config::chromium_dir` here).
pub async fn render_html_to_png_in(
    html: &str,
    out_path: &Path,
    chromium_dir: &Path,
) -> Result<(), TubeforgeError> {
    let executable = ensure_chromium(chromium_dir).await?;
    let png = tokio::time::timeout(RENDER_TIMEOUT, render_inner(&executable, html))
        .await
        .map_err(|_| {
            storage_err(
                "RENDER_TIMEOUT",
                format!(
                    "thumbnail render exceeded {RENDER_TIMEOUT:?}; check the template for a hung page"
                ),
            )
        })??;
    std::fs::write(out_path, &png).map_err(|e| {
        storage_err(
            "RENDER_WRITE",
            format!("cannot write {}: {e}", out_path.display()),
        )
    })?;
    tracing::debug!(
        out = %out_path.display(),
        bytes = png.len(),
        "thumbnail written"
    );
    Ok(())
}

/// The Chromium install dir: `TUBEFORGE_CHROMIUM_DIR`, else `<data>/chromium`
/// (env `TUBEFORGE_DATA_DIR`, else `~/.tubeforge/chromium`).
fn chromium_dir() -> PathBuf {
    if let Ok(v) = std::env::var("TUBEFORGE_CHROMIUM_DIR") {
        if !v.trim().is_empty() {
            return config::expand_tilde(&v);
        }
    }
    let data_dir = std::env::var("TUBEFORGE_DATA_DIR")
        .map(|d| config::expand_tilde(&d))
        .unwrap_or_else(|_| Config::defaults().data_dir);
    data_dir.join("chromium")
}

/// Locate or download the pinned Chromium build into `dir`.
///
/// The fetcher returns the existing install when present, so only the first
/// render downloads (~150MB); failures surface as an actionable error.
async fn ensure_chromium(dir: &Path) -> Result<PathBuf, TubeforgeError> {
    std::fs::create_dir_all(dir).map_err(|e| {
        storage_err(
            "CHROMIUM_SETUP",
            format!("cannot create chromium dir {}: {e}", dir.display()),
        )
    })?;
    let options = BrowserFetcherOptions::builder()
        .with_path(dir)
        .build()
        .map_err(|e| {
            storage_err(
                "CHROMIUM_SETUP",
                format!("cannot configure the browser fetcher: {e}"),
            )
        })?;
    let installation = BrowserFetcher::new(options).fetch().await.map_err(|e| {
        storage_err(
            "CHROMIUM_DOWNLOAD",
            format!(
                "failed to fetch the pinned Chromium build into {}: {e} \
                 (check the network; the build is cached once downloaded)",
                dir.display()
            ),
        )
    })?;
    tracing::info!(
        bin = %installation.executable_path.display(),
        "chromium ready"
    );
    Ok(installation.executable_path)
}

/// Launch headless Chromium, load the HTML, screenshot 1280x720.
async fn render_inner(exe: &Path, html: &str) -> Result<Vec<u8>, TubeforgeError> {
    let config = BrowserConfig::builder()
        .chrome_executable(exe)
        .new_headless_mode()
        .arg("disable-gpu")
        .window_size(THUMB_WIDTH, THUMB_HEIGHT)
        // Per-page `Emulation.setDeviceMetricsOverride` (chromiumoxide applies
        // the configured viewport to every new target).
        .viewport(Viewport {
            width: THUMB_WIDTH,
            height: THUMB_HEIGHT,
            ..Default::default()
        })
        .build()
        .map_err(|e| storage_err("RENDER", format!("browser config: {e}")))?;

    let (mut browser, mut handler) = Browser::launch(config)
        .await
        .map_err(|e| storage_err("RENDER", format!("cannot launch headless Chromium: {e}")))?;
    // The handler must be polled for the CDP connection to make progress.
    let handler_task = tokio::spawn(async move { while handler.next().await.is_some() {} });

    let shot: Result<Vec<u8>, TubeforgeError> = async {
        let page = browser
            .new_page("about:blank")
            .await
            .map_err(|e| storage_err("RENDER", format!("new page: {e}")))?;
        page.set_content(html)
            .await
            .map_err(|e| storage_err("RENDER", format!("set content: {e}")))?;
        // Let fonts/layout settle before the capture.
        tokio::time::sleep(SETTLE).await;
        let clip = Clip::builder()
            .x(0.0)
            .y(0.0)
            .width(THUMB_WIDTH as f64)
            .height(THUMB_HEIGHT as f64)
            .scale(1.0)
            .build()
            .map_err(|e| storage_err("RENDER", format!("clip: {e}")))?;
        page.screenshot(
            ScreenshotParams::builder()
                .format(CaptureScreenshotFormat::Png)
                .clip(clip)
                .capture_beyond_viewport(false)
                .build(),
        )
        .await
        .map_err(|e| storage_err("RENDER", format!("screenshot: {e}")))
    }
    .await;

    // Explicit close + reap on every path that reaches here; `Browser`'s Drop
    // also kills the child (with a warning) as a last resort.
    let _ = browser.close().await;
    let _ = browser.wait().await;
    let _ = handler_task.await;

    shot
}
