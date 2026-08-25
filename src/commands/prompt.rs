//! `prompt` (Phase 6.5): the AI bridge. TubeForge stays AI-free — this
//! command assembles the research-validated prompt templates (Master
//! Competitor Transcript Analyzer, Multi-Video Pattern, Comment+Transcript)
//! from stored transcripts/metadata/comments and emits a markdown bundle
//! ready to paste into OpenCode / Claude Code / Codex.
//!
//! The templates mirror the 2026 gap-mining research document verbatim
//! (section structure preserved) so mined output is consistent across runs.

use std::path::PathBuf;

use serde_json::{json, Value};

use crate::config::Config;
use crate::error::TubeforgeError;
use crate::storage::db::{CommentRow, Db, TranscriptRow, VideoRow};

/// Master Competitor Transcript Analyzer template — one video.
fn master_template(
    video: &VideoRow,
    transcript: &TranscriptRow,
    comments: &[CommentRow],
    include_comments: bool,
) -> String {
    let mut out = String::new();
    out.push_str("You are an expert YouTube content strategist specializing in technical and educational channels (programming, system design, developer tools, concept explainers).\n\n");
    out.push_str("Analyze the following video transcript thoroughly.\n\n");
    out.push_str("Video metadata:\n");
    out.push_str(&format!("- Title: {}\n", video.title));
    out.push_str(&format!(
        "- Channel: {}\n",
        video.channel_id.clone().unwrap_or_default()
    ));
    out.push_str(&format!("- Published: {}\n", video.published_at));
    if let Some(v) = video.view_count {
        out.push_str(&format!("- Views: {v}\n"));
    }
    if let Some(d) = video.duration_sec {
        out.push_str(&format!("- Duration: {d}s\n"));
    }
    out.push_str(&format!(
        "- Transcript source: {} ({})\n\n",
        transcript.source, transcript.lang
    ));
    out.push_str("Transcript:\n\"\"\"\n");
    out.push_str(&transcript.text);
    out.push_str("\n\"\"\"\n");

    if include_comments && !comments.is_empty() {
        out.push_str("\nTop comments:\n\"\"\"\n");
        for c in comments.iter().take(50) {
            out.push_str(&format!(
                "- {} ({} likes): {}\n",
                c.author, c.like_count, c.text
            ));
        }
        out.push_str("\"\"\"\n");
    }

    out.push_str(r#"

Provide a structured analysis with these exact sections:

1. Core Topic & Audience Intent
   - Primary topic
   - Secondary topics covered
   - Target audience skill level (beginner / intermediate / advanced)
   - Main viewer problem or desire this video tries to solve

2. First-Screen Retention Contract & Pacing Breakdown (0:00–1:00)
   - 0:00–0:15 High-Stakes Hook: Core runtime crash / failure mode / unexpected tension
   - 0:15–0:35 Explicit Contract: Concrete payoff and mental model promised
   - 0:35–1:00 Engineering Vehicle: Codebase, tool, or framework introduced
   - Fluff & Friction Audit: Any generic intros, channel branding, or delayed payoffs in 0–30s
   - Code density & pacing: Defect -> Root Cause -> Clean Fix structure

3. Strengths
   - What this video does particularly well
   - Strong explanations or unique angles

4. Weaknesses & Gaps
   - Concepts mentioned but not fully explained
   - Questions the video raises but leaves unanswered
   - Areas that feel rushed, incomplete, or confusing
   - Missing practical examples, edge cases, or real-world applications
   - Assumptions the creator makes about viewer knowledge

5. Unanswered Audience Questions
   - List specific questions a viewer is likely to still have after watching
   - Phrase them as natural viewer questions

6. Content Gap Opportunities (Obviously Awesome / Blue Ocean Positioning)
   - 5–8 specific video ideas that fill gaps left by this video, positioned to make the competitor's version look incomplete
   - For each idea include: Title suggestion + Why it fills a gap + Recommended format (70% Core Search / 20% Deep Dive / 10% Edge)

7. Packaging & Title Insights (Loewenstein Information Gap & Cialdini Influence)
   - High-CTR title suggestions engineered with Loewenstein curiosity gaps, definite referring expressions, and threat prevention
   - Thumbnail concept ideas: Single focal mark on pure black `#000000`, 2-line headline, zero decorative container clutter

8. Downstream Multimodal Asset Funnel (Converting Traffic into Owned Equity)
   - Markdown Study Guide Outline: Core rungs, trade-off matrix, mental model diagram
   - Interactive Self-Assessment Quiz: 3–5 conceptual & compiler-level verification questions
   - SEO Technical Blog Outline: For Dev.to, GitHub README, or technical newsletter
   - GitHub Companion Repo Blueprint: Structure for runnable code companion

Be specific, practical, and focused on actionable opportunities for a channel that explains concepts clearly with visual motion graphics and real-world problem solving.
"#);
    out
}

/// Multi-Video Pattern & Gap Mining template — 2+ videos.
fn multi_template(videos: &[(VideoRow, TranscriptRow)]) -> String {
    let mut out = String::new();
    out.push_str("You are a senior YouTube strategy analyst. I will give you transcripts from multiple competitor videos in the same niche.\n\n");
    out.push_str(
        "Your job is to find patterns, repeated themes, and most importantly — content gaps using the Founder Playbook frameworks (Made to Stick, StoryBrand, Obviously Awesome, Blue Ocean Strategy).\n\n",
    );
    out.push_str("Transcripts:\n\"\"\"\n");
    for (v, t) in videos {
        out.push_str(&format!("\nVideo: {}\n", v.title));
        out.push_str(&t.text);
        out.push('\n');
    }
    out.push_str("\"\"\"\n");
    out.push_str(
        r#"

Analyze across all videos and output:

1. Recurring Themes & Topics
   - Topics that appear in multiple videos
   - Topics that appear only once
   - Topics that are mentioned briefly but never deeply covered

2. Structural & Explanatory Patterns (StoryBrand & SUCCESs Lenses)
   - Common ways these creators structure explanations (Viewer as Hero vs Passive Listener)
   - Common weaknesses in how they teach (abstract jargon vs concrete visual analogies)

3. High-Frequency Unanswered Questions
   - Questions that surface (explicitly or implicitly) across multiple transcripts

4. Clear Content Gaps (Blue Ocean & Category Positioning)
   Rank the strongest opportunities:
   - Gap description
   - Evidence from the transcripts
   - Why current coverage is insufficient
   - Suggested video angle + format
   - Estimated difficulty to produce a superior version

5. Series & Multi-Video Pillar Opportunities (Traction & Content Sequencing)
   - Topics that naturally form a multi-video series that no competitor has fully owned

Focus only on high-signal, practical gaps suitable for an educational technical channel.
"#,
    );
    out
}

/// Resolve video + transcript rows; `id` must be stored and transcribed.
async fn load_video_with_transcript(
    db: &Db,
    videos: &[VideoRow],
    id: &str,
) -> Result<(VideoRow, TranscriptRow), TubeforgeError> {
    let video = videos
        .iter()
        .find(|v| v.video_id == id)
        .ok_or_else(|| TubeforgeError::Usage(format!("video not in database: {id}")))?;
    let transcript = db.get_transcript(id).await?.ok_or_else(|| {
        TubeforgeError::Usage(format!(
            "no stored transcript for {id} — run `tubeforge transcript get --video-id {id}` first"
        ))
    })?;
    Ok((video.clone(), transcript))
}

/// `prompt`: assemble and emit the bundle.
pub async fn run(
    cfg: &Config,
    video_id: Option<&str>,
    multi: &[String],
    include_comments: bool,
    out: Option<&PathBuf>,
    json: bool,
) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let videos = db.all_videos().await?;

    let bundle: Value = if !multi.is_empty() {
        let mut pairs = Vec::new();
        for id in multi {
            let (v, t) = load_video_with_transcript(&db, &videos, id).await?;
            pairs.push((v, t));
        }
        if json {
            json!({
                "template": "multi",
                "videos": pairs.iter().map(|(v, t)| json!({
                    "title": v.title,
                    "video_id": v.video_id,
                    "transcript": t.text,
                })).collect::<Vec<_>>(),
            })
        } else {
            Value::String(multi_template(&pairs))
        }
    } else if let Some(id) = video_id {
        let (v, t) = load_video_with_transcript(&db, &videos, id).await?;
        let comments = if include_comments {
            db.list_comments(id).await?
        } else {
            Vec::new()
        };
        if json {
            json!({
                "template": "master",
                "video_id": v.video_id,
                "title": v.title,
                "channel_id": v.channel_id,
                "published_at": v.published_at,
                "views": v.view_count,
                "duration_sec": v.duration_sec,
                "transcript": t.text,
                "comments": comments.iter().take(50).map(|c| json!({
                    "author": c.author,
                    "text": c.text,
                    "likes": c.like_count,
                })).collect::<Vec<_>>(),
            })
        } else {
            Value::String(master_template(&v, &t, &comments, include_comments))
        }
    } else {
        return Err(TubeforgeError::Usage(
            "prompt needs --video-id <ID> or --multi <ID1,ID2,...>".to_string(),
        ));
    };

    match out {
        Some(path) => {
            let text = match &bundle {
                Value::String(s) => s.clone(),
                other => serde_json::to_string_pretty(other)?,
            };
            std::fs::write(path, &text).map_err(|e| TubeforgeError::Storage {
                code: "PROMPT_WRITE".to_string(),
                message: format!("{}: {e}", path.display()),
            })?;
            Ok(json!({
                "written": path.display().to_string(),
                "bytes": text.len(),
            }))
        }
        None => Ok(bundle),
    }
}
