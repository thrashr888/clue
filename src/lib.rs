pub mod api;
pub mod credentials;
pub mod search;
pub mod skills;

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
    #[serde(default)]
    pub lexical_score: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relevance: Option<Judgment>,
}

fn default_source() -> String {
    "input".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Judgment {
    pub score: f64,
    pub confidence: f64,
    pub probabilities: std::collections::BTreeMap<String, f64>,
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
