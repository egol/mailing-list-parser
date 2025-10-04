use serde::Serialize;
use chrono::{DateTime, Utc};
use sqlx::FromRow;

/// Author information
#[derive(Debug, Serialize, Clone, FromRow)]
pub struct Author {
    pub author_id: i64,
    pub first_name: String,
    pub last_name: Option<String>,
    pub display_name: String,
    pub first_seen: Option<DateTime<Utc>>,
    pub patch_count: i32,
}

/// Author email address
#[derive(Debug, Serialize, Clone, FromRow)]
pub struct AuthorEmail {
    pub email_id: i64,
    pub author_id: i64,
    pub email: String,
    pub is_primary: bool,
    pub first_seen: Option<DateTime<Utc>>,
}

/// Patch (email) information
#[derive(Debug, Serialize, Clone, FromRow)]
pub struct Patch {
    pub patch_id: i64,
    pub author_id: i64,
    pub email_id: Option<i64>,
    pub message_id: String,
    pub subject: String,
    pub sent_at: DateTime<Utc>,
    pub commit_hash: Option<String>,
    pub body_text: Option<String>,
    pub is_series: Option<bool>,
    pub series_number: Option<i32>,
    pub series_total: Option<i32>,
    pub created_at: Option<DateTime<Utc>>,
}

/// Internal patch data structure used during batch processing
#[derive(Debug)]
pub(crate) struct PatchData {
    pub author_id: i64,
    pub email_id: i64,
    pub message_id: String,
    pub subject: String,
    pub sent_at: DateTime<Utc>,
    pub commit_hash: String,
    pub body_text: Option<String>,
    pub is_series: bool,
    pub series_number: Option<i32>,
    pub series_total: Option<i32>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub is_reply: bool,
    // Merge notification fields
    pub is_merge_notification: bool,
    pub merge_info: Option<crate::mail_parser::MergeInfo>,
}

/// Result of database setup operation
#[derive(Debug, Serialize)]
pub struct DatabaseSetupResult {
    pub success: bool,
    pub message: String,
    pub tables_created: Vec<String>,
}

/// Result of database population operation
#[derive(Debug, Serialize)]
pub struct DatabasePopulationResult {
    pub success: bool,
    pub total_processed: u32,
    pub total_authors_inserted: u32,
    pub total_emails_inserted: u32,
    pub errors: Vec<String>,
}

/// Statistics from thread building operation
#[derive(Debug, Serialize)]
pub struct ThreadBuildStats {
    pub total_threads: u32,
    pub total_replies: u32,
    pub orphaned_messages: u32,
    pub max_depth: i32,
    pub processing_time_ms: u64,
}

