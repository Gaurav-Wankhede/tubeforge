//! `thumbnail render` / `thumbnail list-templates` (Phase 3, PRD §5.7).
//!
//! `render` loads a stored video (title, channel name, duration, category) or
//! renders a bare `--draft-title`, fills the chosen template, inlines the
//! compiled Tailwind CSS into a single-file HTML document and renders it to a
//! 1280x720 PNG with headless Chromium. Raw assets live under a per-render
//! `/assets` dir that is deleted the moment rendering finishes — success or
//! error (RAII guard); `--keep-assets` is the debug-only opt-out.

use std::path::PathBuf;

use serde_json::{json, Value};

use crate::config::Config;
use crate::error::TubeforgeError;
use crate::storage::Db;
use crate::thumbnail::{self, AssetDir, TemplateValues};

pub struct RenderInput {
    pub video_id: Option<String>,
    pub draft_title: Option<String>,
    pub template: String,
    pub out: Option<PathBuf>,
    pub keep_assets: bool,
}

/// `thumbnail render`: resolve values, fill + inline the template, render PNG.
pub async fn run_render(cfg: &Config, input: &RenderInput) -> Result<Value, TubeforgeError> {
    let template = thumbnail::load_template(&input.template)?;
    let (video_id, values) = resolve_values(cfg, input).await?;
    let html = thumbnail::build_single_file(template, &values)?;

    // PRD §5.7: raw assets (source thumbnails, ...) live in a per-render dir;
    // the guard deletes it as soon as this scope ends — success or error.
    // `--keep-assets` (debug only) is the sole exception.
    let assets = AssetDir::create(&cfg.data_dir, input.keep_assets)?;
    tracing::debug!(
        dir = %assets.path().display(),
        keep = assets.keep(),
        "render assets dir"
    );

    let out = input
        .out
        .clone()
        .unwrap_or_else(|| thumbnail::default_out_path(video_id.as_deref()));

    thumbnail::render::render_html_to_png_in(&html, &out, &cfg.chromium_dir).await?;

    Ok(json!({
        "video_id": video_id,
        "template": input.template,
        "out": out.display().to_string(),
        "width": thumbnail::THUMB_WIDTH,
        "height": thumbnail::THUMB_HEIGHT,
        "assets_cleaned": !assets.keep(),
        "assets_dir": assets.path().display().to_string(),
    }))
}

/// `thumbnail list-templates`: names of the embedded templates.
pub async fn run_list_templates() -> Result<Value, TubeforgeError> {
    Ok(json!({ "templates": thumbnail::template_names() }))
}

/// Title/channel/duration/category from the DB (stored video) or the draft
/// flags. `--video-id` wins; `--draft-title` renders without a DB row.
async fn resolve_values(
    cfg: &Config,
    input: &RenderInput,
) -> Result<(Option<String>, TemplateValues), TubeforgeError> {
    if let Some(vid) = &input.video_id {
        let db = Db::open(&cfg.db_path).await?;
        let row = db
            .get_video(vid)
            .await?
            .ok_or_else(|| TubeforgeError::Usage(format!("video not in database: {vid}")))?;
        let channel = match &row.channel_id {
            Some(cid) => db.get_channel(cid).await?.map(|c| c.title),
            None => None,
        };
        Ok((
            Some(vid.clone()),
            thumbnail::values_from_video(&row, channel.as_deref()),
        ))
    } else {
        let title = input.draft_title.clone().unwrap_or_default();
        if title.trim().is_empty() {
            return Err(TubeforgeError::Usage(
                "thumbnail render needs --video-id or a non-empty --draft-title".into(),
            ));
        }
        Ok((
            None,
            TemplateValues {
                title: Some(title),
                ..Default::default()
            },
        ))
    }
}
