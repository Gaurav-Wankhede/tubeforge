//! Thumbnail Generator (Phase 3, workstream A).
//!
//! PRD §5.7: templates are HTML + Tailwind CSS, raw assets live in a
//! temporary per-render `/assets` dir and are deleted immediately after
//! generation (RAII guard, see [`assets::AssetDir`]). The compiled Tailwind
//! CSS is committed and embedded via `include_str!`, so rendering produces a
//! single-file HTML document (no file:// or network dependencies).
//!
//! Placeholder convention (documented in `templates/default.html`):
//! `{{TITLE}}`, `{{CHANNEL}}`, `{{CHANNEL_INITIAL}}`, `{{DURATION}}`,
//! `{{CATEGORY}}`, `{{VIDEO_ID}}`. Empty values self-hide their element via
//! Tailwind `empty:` utilities, so filling needs no conditional markup.

pub mod assets;
pub mod render;

pub use assets::AssetDir;

use crate::error::TubeforgeError;
use crate::storage::db::VideoRow;

/// Output canvas: YouTube standard thumbnail size.
pub const THUMB_WIDTH: u32 = 1280;
pub const THUMB_HEIGHT: u32 = 720;

/// The built-in template (authored at `templates/default.html`).
pub const DEFAULT_TEMPLATE: &str = include_str!("../templates/default.html");
/// Compiled Tailwind CSS for the templates (authored at `templates/input.css`,
/// built with the standalone Tailwind CLI; committed for offline builds).
const DEFAULT_CSS: &str = include_str!("../templates/tailwind.css");

/// Values substituted into a template. All optional; `None` renders an empty
/// string and the `empty:` utilities hide the element.
#[derive(Debug, Clone, Default)]
pub struct TemplateValues {
    pub title: Option<String>,
    pub channel: Option<String>,
    pub channel_initial: Option<String>,
    pub duration: Option<String>,
    pub category: Option<String>,
    pub video_id: Option<String>,
}

/// Names of the templates available for `thumbnail render --template`.
pub fn template_names() -> &'static [&'static str] {
    &["default"]
}

/// Load an embedded template by name; unknown names are a usage error (exit
/// code 2 per LLD §4.3).
pub fn load_template(name: &str) -> Result<&'static str, TubeforgeError> {
    match name {
        "default" => Ok(DEFAULT_TEMPLATE),
        other => Err(TubeforgeError::Usage(format!(
            "unknown template '{other}' (available: {})",
            template_names().join(", ")
        ))),
    }
}

/// Substitute the placeholder values into a template.
///
/// Errors if any `{{...}}` placeholder is left unfilled afterwards, so a
/// half-filled thumbnail can never be rendered silently. HTML comments (which
/// document the placeholder convention) are stripped first.
pub fn fill_template(template: &str, values: &TemplateValues) -> Result<String, TubeforgeError> {
    let mut out = strip_html_comments(template);
    for (key, value) in [
        ("{{TITLE}}", values.title.as_deref()),
        ("{{CHANNEL}}", values.channel.as_deref()),
        ("{{CHANNEL_INITIAL}}", values.channel_initial.as_deref()),
        ("{{DURATION}}", values.duration.as_deref()),
        ("{{CATEGORY}}", values.category.as_deref()),
        ("{{VIDEO_ID}}", values.video_id.as_deref()),
    ] {
        out = out.replace(key, value.unwrap_or(""));
    }
    if let Some(leftover) = leftover_placeholder(&out) {
        return Err(TubeforgeError::Usage(format!(
            "template placeholder left unfilled: {leftover}"
        )));
    }
    Ok(out)
}

/// Replace the template's `<link rel="stylesheet" href="...">` with an
/// inline `<style>` block — single-file HTML so headless Chromium has no
/// relative-path dependency at render time (deterministic, offline-first).
pub fn inline_css(html: &str) -> String {
    const LINK: &str = r#"<link rel="stylesheet" href="tailwind.css">"#;
    if html.contains(LINK) {
        html.replacen(LINK, &format!("<style>\n{DEFAULT_CSS}</style>"), 1)
    } else {
        // No stylesheet link: nothing to inline (template without Tailwind).
        html.to_string()
    }
}

/// Build the single-file HTML for a render: fill, then inline the CSS.
pub fn build_single_file(
    template: &str,
    values: &TemplateValues,
) -> Result<String, TubeforgeError> {
    Ok(inline_css(&fill_template(template, values)?))
}

/// Format `duration_sec` as `M:SS` or `H:MM:SS` (YouTube badge convention).
pub fn format_duration(secs: i64) -> String {
    let secs = secs.max(0);
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Avatar letter: first character of the channel name, uppercased.
pub fn initial_of(channel: Option<&str>) -> Option<String> {
    channel
        .and_then(|c| c.chars().next())
        .map(|ch| ch.to_uppercase().collect::<String>())
}

/// Default output path: `<cwd>/<video_id>.png`, or `thumbnail.png` for a
/// draft render without a video id.
pub fn default_out_path(video_id: Option<&str>) -> std::path::PathBuf {
    let name = video_id.filter(|s| !s.is_empty()).unwrap_or("thumbnail");
    std::path::PathBuf::from(format!("{name}.png"))
}

/// Template values for a stored video row (title, channel title, duration,
/// category display name via the 32-entry map, video id).
pub fn values_from_video(row: &VideoRow, channel: Option<&str>) -> TemplateValues {
    TemplateValues {
        title: Some(row.title.clone()),
        channel: channel.map(str::to_string),
        channel_initial: initial_of(channel),
        duration: row.duration_sec.map(format_duration),
        category: row
            .category_id
            .as_deref()
            .and_then(crate::categories::category_name)
            .map(str::to_string)
            // Unknown ids render as the raw id (categories.rs contract).
            .or_else(|| row.category_id.clone()),
        video_id: Some(row.video_id.clone()),
    }
}

/// First `{{...}}` still present in `html`, if any.
fn leftover_placeholder(html: &str) -> Option<String> {
    let start = html.find("{{")?;
    let end = html[start + 2..].find("}}")?;
    Some(html[start..start + 2 + end + 2].to_string())
}

/// Strip `<!-- ... -->` comments: they document the placeholder convention
/// and must not trip the leftover-placeholder check.
fn strip_html_comments(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start + 4..].find("-->") {
            Some(end) => rest = &rest[start + 4 + end + 3..],
            None => return out, // unterminated comment: drop the tail
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const TPL: &str = "<h1>{{TITLE}}</h1><b>{{CHANNEL}}</b><i>{{DURATION}}</i>";

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

    #[test]
    fn fill_substitutes_all_placeholders() {
        let out = fill_template(TPL, &values()).expect("fill");
        assert_eq!(
            out,
            "<h1>Rust in 60 seconds</h1><b>TubeForge</b><i>4:20</i>"
        );
        assert!(!out.contains("{{"));
    }

    #[test]
    fn fill_with_missing_values_renders_empty() {
        // Missing values become empty strings; the template's `empty:`
        // utilities hide the element. Only leftover `{{...}}` errors.
        let out = fill_template(TPL, &TemplateValues::default()).expect("fill");
        assert_eq!(out, "<h1></h1><b></b><i></i>");
    }

    #[test]
    fn fill_errors_on_leftover_placeholder() {
        let err = fill_template("<h1>{{TITLE}}</h1><p>{{UNKNOWN}}</p>", &values())
            .expect_err("unknown placeholder must error");
        assert!(err.to_string().contains("{{UNKNOWN}}"), "{err}");
    }

    #[test]
    fn fill_strips_doc_comments_first() {
        // The template header comment documents the placeholders; it must not
        // count as a leftover.
        let tpl = "<!-- {{TITLE}} {{CHANNEL}} --><h1>{{TITLE}}</h1>";
        let out = fill_template(tpl, &values()).expect("fill");
        assert!(!out.contains("<!--"));
        assert_eq!(out, "<h1>Rust in 60 seconds</h1>");
    }

    #[test]
    fn inline_css_replaces_link_with_style() {
        let out = inline_css(&format!(
            "<head>{LINK}</head>",
            LINK = r#"<link rel="stylesheet" href="tailwind.css">"#
        ));
        assert!(out.contains("<style>"));
        assert!(!out.contains("rel=\"stylesheet\""));
        assert!(out.contains(".font-thumb"), "compiled css embedded");
    }

    #[test]
    fn inline_css_returns_html_unchanged_without_link() {
        let html = "<html></html>";
        assert_eq!(inline_css(html), html);
    }

    #[test]
    fn build_single_file_is_self_contained() {
        let html = build_single_file(DEFAULT_TEMPLATE, &values()).expect("build");
        assert!(html.contains("Rust in 60 seconds"));
        assert!(html.contains("<style>"));
        assert!(!html.contains("href=\"tailwind.css\""));
    }

    #[test]
    fn duration_formatting() {
        assert_eq!(format_duration(0), "0:00");
        assert_eq!(format_duration(59), "0:59");
        assert_eq!(format_duration(60), "1:00");
        assert_eq!(format_duration(260), "4:20");
        assert_eq!(format_duration(3661), "1:01:01");
        assert_eq!(format_duration(-5), "0:00", "negative clamps");
    }

    #[test]
    fn channel_initial_is_first_char_uppercased() {
        assert_eq!(initial_of(Some("mrburns")), Some("M".into()));
        assert_eq!(initial_of(Some("")), None);
        assert_eq!(initial_of(None), None);
    }

    #[test]
    fn default_out_path_naming() {
        assert_eq!(
            default_out_path(Some("abc123")),
            std::path::PathBuf::from("abc123.png")
        );
        assert_eq!(
            default_out_path(Some("")),
            std::path::PathBuf::from("thumbnail.png")
        );
        assert_eq!(
            default_out_path(None),
            std::path::PathBuf::from("thumbnail.png")
        );
    }

    #[test]
    fn values_from_video_maps_stored_fields() {
        let row = VideoRow {
            video_id: "abc123".into(),
            title: "My Video".into(),
            duration_sec: Some(3661),
            category_id: Some("28".into()),
            ..Default::default()
        };
        let v = values_from_video(&row, Some("TubeForge"));
        assert_eq!(v.title.as_deref(), Some("My Video"));
        assert_eq!(v.channel.as_deref(), Some("TubeForge"));
        assert_eq!(v.channel_initial.as_deref(), Some("T"));
        assert_eq!(v.duration.as_deref(), Some("1:01:01"));
        assert_eq!(v.category.as_deref(), Some("Science & Technology"));
        assert_eq!(v.video_id.as_deref(), Some("abc123"));
    }

    #[test]
    fn values_from_video_falls_back_to_raw_category_id() {
        let row = VideoRow {
            category_id: Some("999".into()),
            ..Default::default()
        };
        let v = values_from_video(&row, None);
        assert_eq!(v.category.as_deref(), Some("999"), "unknown id renders raw");
        assert_eq!(v.channel, None);
        assert_eq!(v.channel_initial, None);
    }
}
