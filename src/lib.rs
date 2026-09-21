pub mod api;
pub mod config;
pub mod credentials;
pub mod input;
pub mod provider;
pub mod search;
pub mod skills;
pub mod sqlite;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub text: String,
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<Event>,
    #[serde(default)]
    pub lexical_score: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relevance: Option<Judgment>,
    /// Original mapped input. Retained locally; never sent as an API field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub calendar: String,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub is_all_day: Option<bool>,
    pub location: Option<String>,
}

fn default_source() -> String {
    "input".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Judgment {
    pub score: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probabilities: Option<std::collections::BTreeMap<String, f64>>,
    #[serde(default = "distribution_kind")]
    pub kind: String,
}

pub fn clip(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

pub fn validate_query(query: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !query.trim().is_empty() && query.chars().count() <= 500,
        "query must contain 1–500 characters"
    );
    Ok(())
}

fn distribution_kind() -> String {
    "native_distribution".into()
}
