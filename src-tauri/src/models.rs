use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// Default path to the mailing list git repository
pub const DEFAULT_MAILING_LIST_GIT_PATH: &str = "E:/bpf/git/0.git";

/// Represents an email message in the mailing list
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Email {
    pub id: String,
    pub message_id: String,
    pub subject: String,
    pub from: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub date: DateTime<Utc>,
    pub body: String,
    pub references: Vec<String>,
    pub in_reply_to: Option<String>,
    pub patch_number: Option<i32>,
    pub patch_version: Option<i32>,
    pub is_patch: bool,
    pub patch_filename: Option<String>,
    pub commit_hash: Option<String>,
}

/// Represents a patch series with multiple versions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchSeries {
    pub id: String,
    pub subject: String,
    pub author: String,
    pub versions: Vec<PatchVersion>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Statistics about the mailing list
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailListStats {
    pub total_emails: i64,
    pub patch_emails: i64,
    pub recent_emails: i64,
}

/// Result of an update operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateResult {
    pub new_emails: usize,
    pub updated_threads: usize,
    pub latest_commit: String,
}

/// Represents a specific version of a patch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchVersion {
    pub version: i32,
    pub patches: Vec<PatchFile>,
    pub cover_letter: Option<String>,
    pub date: DateTime<Utc>,
}

/// Represents an individual patch file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchFile {
    pub filename: String,
    pub content: String,
    pub patch_number: i32,
}

/// Represents a thread of discussion (tree structure)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    pub id: String,
    pub root_email_id: String,
    pub subject: String,
    pub emails: Vec<ThreadNode>,
}

/// Represents a node in the thread tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadNode {
    pub email_id: String,
    pub parent_id: Option<String>,
    pub children: Vec<String>,
    pub depth: i32,
}

/// Search criteria for filtering emails
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchCriteria {
    pub query: Option<String>,
    pub author: Option<String>,
    pub subject_contains: Option<String>,
    pub date_from: Option<DateTime<Utc>>,
    pub date_to: Option<DateTime<Utc>>,
    pub is_patch: Option<bool>,
    pub patch_series: Option<String>,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

/// Search results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub emails: Vec<Email>,
    pub total_count: i64,
    pub has_more: bool,
}

/// Configuration for the mailing list parser
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub database_url: String,
    pub mailing_list_path: String,
    pub max_emails_per_batch: i32,
    pub enable_auto_update: bool,
    pub update_interval_minutes: i64,
}

/// Error types for the application
#[derive(Debug, thiserror::Error)]
pub enum ParserError {
    #[error("Database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("Email parsing error: {0}")]
    EmailParsing(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Search error: {0}")]
    Search(String),
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, ParserError>;

/// Helper function to serialize Display types
fn serialize_display<T, S>(value: &T, serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    T: std::fmt::Display,
    S: serde::Serializer,
{
    serializer.collect_str(value)
}