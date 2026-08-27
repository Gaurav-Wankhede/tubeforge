//! `playbook` CLI commands (Founder Playbook Content Psychology Engine).

use serde_json::{json, Value};

use crate::error::TubeforgeError;
use crate::playbook;

/// `playbook score --title "..." [--description "..."] [--hook "..."]`
pub fn run_score(
    title: &str,
    description: Option<&str>,
    hook: Option<&str>,
) -> Result<Value, TubeforgeError> {
    let desc = description.unwrap_or("");
    let audit = playbook::audit_content(title, desc, hook);
    Ok(json!(audit))
}

/// `playbook contract --channel "..." --topic "..." --keyword "..." --title "..." --framework "..."`
pub fn run_contract(
    channel: &str,
    topic: &str,
    keyword: &str,
    title: &str,
    framework: &str,
) -> Result<Value, TubeforgeError> {
    let contract = playbook::contracts::generate_contract(channel, topic, keyword, title, framework);
    Ok(json!({
        "contract": contract,
        "channel": channel,
        "title": title,
    }))
}
