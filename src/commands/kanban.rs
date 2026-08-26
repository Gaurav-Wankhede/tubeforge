//! `kanban` command (TODO & Workflow Management for Future Videos).
//!
//! Interconnects video project planning with TubeForge's research corpus,
//! keyword analytics, SEO scoring, and prompt generation without data duplication.

use serde_json::{json, Value};

use crate::config::Config;
use crate::error::TubeforgeError;
use crate::storage::db::{Db, KanbanTicketRow};

/// Input parameters for creating a Kanban ticket manually.
#[derive(Debug, Clone)]
pub struct CreateTicketInput {
    pub title: String,
    pub channel: String,
    pub status: Option<String>,
    pub topic: Option<String>,
    pub framework: Option<String>,
    pub optimal_duration_sec: Option<i64>,
    pub target_keyword: Option<String>,
    pub youtube_url: Option<String>,
    pub notes: Option<String>,
}

/// Create a new Kanban ticket in the database.
pub async fn run_create(cfg: &Config, input: &CreateTicketInput) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let now = crate::util::now_rfc3339();
    let status = input.status.as_deref().unwrap_or("todo").to_lowercase();
    let ticket_id = format!("ticket-{}", &crate::util::nanoid(8));

    let ticket = KanbanTicketRow {
        ticket_id: ticket_id.clone(),
        title: input.title.clone(),
        channel: input.channel.to_uppercase(),
        status,
        topic: input.topic.clone(),
        framework: input.framework.clone(),
        optimal_duration_sec: input.optimal_duration_sec,
        target_keyword: input.target_keyword.clone(),
        youtube_url: input.youtube_url.clone(),
        video_id: None,
        research_ref: input.topic.clone(),
        notes: input.notes.clone(),
        created_at: now.clone(),
        updated_at: now,
    };

    db.create_kanban_ticket(&ticket).await?;

    Ok(json!({
        "ticket": ticket,
        "message": format!("Kanban ticket {} created successfully", ticket_id)
    }))
}

/// Create a Kanban ticket interconnected with existing keyword research.
pub async fn run_from_research(
    cfg: &Config,
    topic: &str,
    channel: &str,
    title_override: Option<&str>,
    framework: Option<&str>,
    duration_sec: Option<i64>,
) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let now = crate::util::now_rfc3339();

    // Query existing research from `keyword_research` table
    let research_opt = db.get_keyword_research(topic).await?;
    // Build the fallback title only when no override was given — the old code
    // allocated the default eagerly on every call. Titles follow the house
    // style law: no "Topic: Subtitle" colon patterns.
    let title = match title_override {
        Some(t) => t.to_string(),
        None => match &research_opt {
            Some(r) => format!("{} — Visual Breakdown & Mental Model", r.keyword),
            None => format!("{topic} — Visual Breakdown & Mental Model"),
        },
    };
    let target_kw = Some(match &research_opt {
        Some(r) => r.keyword.clone(),
        None => topic.to_string(),
    });
    let suggested_tags_count = research_opt
        .as_ref()
        .and_then(|r| serde_json::from_str::<Vec<Value>>(&r.suggested_tags).ok())
        .map_or(0, |tags| tags.len());

    let ticket_id = format!("ticket-{}", &crate::util::nanoid(8));
    let ticket = KanbanTicketRow {
        ticket_id: ticket_id.clone(),
        title,
        channel: channel.to_uppercase(),
        status: "todo".to_string(),
        topic: Some(topic.to_string()),
        framework: framework.map(str::to_string),
        optimal_duration_sec: duration_sec.or(Some(720)), // Default 12 min (720s)
        target_keyword: target_kw,
        youtube_url: None,
        video_id: None,
        research_ref: Some(topic.to_string()),
        notes: Some(format!(
            "Mapped from research topic '{}' (linked suggested tags: {})",
            topic, suggested_tags_count
        )),
        created_at: now.clone(),
        updated_at: now,
    };

    db.create_kanban_ticket(&ticket).await?;

    Ok(json!({
        "ticket": ticket,
        "research_interconnected": research_opt.is_some(),
        "message": format!("Kanban ticket {} created from research for '{}'", ticket_id, topic)
    }))
}

/// List Kanban tickets with optional status and channel filters.
pub async fn run_list(
    cfg: &Config,
    status: Option<&str>,
    channel: Option<&str>,
) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let tickets = db.list_kanban_tickets(status, channel).await?;

    let mut todo_count = 0;
    let mut inprogress_count = 0;
    let mut done_count = 0;
    let mut published_count = 0;

    for t in &tickets {
        match t.status.as_str() {
            "todo" => todo_count += 1,
            "inprogress" => inprogress_count += 1,
            "done" => done_count += 1,
            "published" => published_count += 1,
            _ => {}
        }
    }

    Ok(json!({
        "summary": {
            "total": tickets.len(),
            "todo": todo_count,
            "inprogress": inprogress_count,
            "done": done_count,
            "published": published_count,
        },
        "tickets": tickets,
    }))
}

/// Move/transition a ticket to a new status.
pub async fn run_move(
    cfg: &Config,
    ticket_id: &str,
    status: &str,
    youtube_url: Option<&str>,
    video_id: Option<&str>,
) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let updated = db
        .move_kanban_ticket(ticket_id, status, youtube_url, video_id)
        .await?;

    Ok(json!({
        "ticket": updated,
        "message": format!("Ticket {} status updated to '{}'", ticket_id, status)
    }))
}

/// Update fields on an existing Kanban ticket.
#[allow(clippy::too_many_arguments)]
pub async fn run_update(
    cfg: &Config,
    ticket_id: &str,
    title: Option<&str>,
    status: Option<&str>,
    topic: Option<&str>,
    framework: Option<&str>,
    duration: Option<i64>,
    keyword: Option<&str>,
    notes: Option<&str>,
) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let updated = db
        .update_kanban_ticket_fields(
            ticket_id, title, status, topic, framework, duration, keyword, notes,
        )
        .await?;

    Ok(json!({
        "ticket": updated,
        "message": format!("Ticket {} updated successfully", ticket_id)
    }))
}

/// Show comprehensive interconnected details of a Kanban ticket.
pub async fn run_show(cfg: &Config, ticket_id: &str) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let Some(ticket) = db.get_kanban_ticket(ticket_id).await? else {
        return Err(TubeforgeError::Usage(format!(
            "Kanban ticket not found: {ticket_id}"
        )));
    };

    let research = if let Some(topic) = &ticket.topic {
        db.get_keyword_research(topic).await?
    } else {
        None
    };

    Ok(json!({
        "ticket": ticket,
        "interconnected_research": research,
    }))
}

/// Delete a Kanban ticket.
pub async fn run_delete(cfg: &Config, ticket_id: &str) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let deleted = db.delete_kanban_ticket(ticket_id).await?;

    Ok(json!({
        "ticket_id": ticket_id,
        "deleted": deleted,
        "message": if deleted {
            format!("Ticket {ticket_id} deleted")
        } else {
            format!("Ticket {ticket_id} not found")
        }
    }))
}

/// Generate an interconnected SCRIPT & STORYBOARD prompt blueprint for a Kanban ticket.
pub async fn run_prompt(cfg: &Config, ticket_id: &str) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let Some(ticket) = db.get_kanban_ticket(ticket_id).await? else {
        return Err(TubeforgeError::Usage(format!(
            "Kanban ticket not found: {ticket_id}"
        )));
    };

    let research = if let Some(topic) = &ticket.topic {
        db.get_keyword_research(topic).await?
    } else {
        None
    };

    let duration_min = ticket.optimal_duration_sec.unwrap_or(720) / 60;
    let framework = ticket.framework.as_deref().unwrap_or("Core Mental Model");
    let topic = ticket.topic.as_deref().unwrap_or(&ticket.title);

    let prompt = format!(
        r#"# Production Blueprint: {title}
Channel: {channel} | Target Duration: {duration_min} min ({duration_sec}s)
Framework: {framework} | Topic: {topic}
Status: {status}

## 1. FIRST-SCREEN RETENTION CONTRACT (0:00 - 1:00)
- 0:00 - 0:15 [HOOK]: Introduce the central contradiction in {framework}. Zero fluff.
- 0:15 - 0:35 [EXPLICIT PAYOFF]: Guarantee what the viewer will understand in the next {duration_min} minutes.
- 0:35 - 1:00 [ENGINEERING / CONCEPTUAL VEHICLE]: Establish the core visual mental model on pure black `#000000`.

## 2. INTERCONNECTED RESEARCH SIGNALS
- Target Keyword: {kw}
- SEO Competition / Opportunity Score: {opp_score}
- Interconnected Research Topic: {research_topic}

## 3. VISUAL GRAPHICS SPECIFICATION
- Mobile-First Minimalist Diagramming: Max 3–5 floating nodes per state.
- Pure black `#000000` canvas, 0 card wrappers, 0 text walls.
- Spoken voiceover carries the verbal story; visual canvas carries the spatial diagram.
- 100% self-explanatory on screen in <2 seconds.
"#,
        title = ticket.title,
        channel = ticket.channel,
        duration_min = duration_min,
        duration_sec = ticket.optimal_duration_sec.unwrap_or(720),
        framework = framework,
        topic = topic,
        status = ticket.status,
        kw = ticket.target_keyword.as_deref().unwrap_or("N/A"),
        opp_score = research
            .as_ref()
            .map(|r| format!("{:.1}", r.opportunity_score))
            .unwrap_or_else(|| "N/A".to_string()),
        research_topic = ticket.research_ref.as_deref().unwrap_or("N/A"),
    );

    Ok(json!({
        "ticket_id": ticket.ticket_id,
        "title": ticket.title,
        "channel": ticket.channel,
        "prompt": prompt,
    }))
}
