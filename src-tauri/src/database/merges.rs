use sqlx::{PgPool, Row};
use chrono::{DateTime, Utc};
use crate::mail_parser::MergeInfo;

/// Update an existing patch to mark it as a merge notification
/// and populate merge metadata fields
pub async fn mark_patch_as_merge(
    pool: &PgPool,
    patch_id: i64,
    merge_info: &MergeInfo,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE patches 
         SET is_merge_notification = TRUE,
             merge_repository = $1,
             merge_branch = $2,
             merge_applied_by = $3,
             merge_commit_links = $4
         WHERE patch_id = $5"
    )
    .bind(&merge_info.repository)
    .bind(&merge_info.branch)
    .bind(&merge_info.applied_by)
    .bind(&merge_info.commit_links)
    .bind(patch_id)
    .execute(pool)
    .await?;
    
    Ok(())
}

/// Get merge status for a specific thread
/// Returns merge info if thread has a merge notification
pub async fn get_thread_merge_status(
    pool: &PgPool,
    thread_id: i64,
) -> Result<Option<ThreadMergeStatus>, sqlx::Error> {
    let result = sqlx::query_as::<_, ThreadMergeStatus>(
        "SELECT 
            p.merge_repository,
            p.merge_branch,
            p.merge_applied_by,
            p.sent_at as merge_date,
            array_length(p.merge_commit_links, 1) as commit_count,
            p.patch_id as merge_notification_patch_id
         FROM patch_replies pr
         JOIN patches p ON pr.patch_id = p.patch_id
         WHERE pr.thread_id = $1 
           AND p.is_merge_notification = TRUE
         LIMIT 1"
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await?;
    
    Ok(result)
}

/// Reprocess all patches to identify and mark merge notifications
/// Returns count of patches updated
pub async fn reprocess_merge_notifications(
    pool: &PgPool,
) -> Result<ReprocessResult, Box<dyn std::error::Error>> {
    // Fetch all patches from patchwork bot that aren't already marked
    let patches = sqlx::query(
        "SELECT p.patch_id, p.subject, p.body_text, ae.email
         FROM patches p
         JOIN author_emails ae ON p.email_id = ae.email_id
         WHERE ae.email ILIKE '%patchwork%'
           AND p.is_merge_notification = FALSE
           AND p.body_text IS NOT NULL"
    )
    .fetch_all(pool)
    .await?;
    
    let mut updated_count = 0;
    let mut failed_count = 0;
    let mut errors = Vec::new();
    let total_checked = patches.len();
    
    for row in patches {
        let patch_id: i64 = row.try_get("patch_id")?;
        let subject: String = row.try_get("subject")?;
        let body: Option<String> = row.try_get("body_text").ok();
        let email: String = row.try_get("email")?;
        
        // Create a minimal EmailInfo for detection
        let email_info = crate::mail_parser::EmailInfo {
            commit_hash: String::new(),
            subject: subject.clone(),
            normalized_subject: String::new(),
            from: String::new(),
            author_email: email,
            author_first_name: String::new(),
            author_last_name: None,
            author_display_name: String::new(),
            to: String::new(),
            date: String::new(),
            message_id: String::new(),
            body: body.unwrap_or_default(),
            headers: std::collections::HashMap::new(),
            in_reply_to: None,
            references: Vec::new(),
            is_reply: false,
        };
        
        let (is_merge, merge_info_opt) = crate::mail_parser::detect_and_parse_merge(&email_info);
        
        if is_merge {
            if let Some(merge_info) = merge_info_opt {
                match mark_patch_as_merge(pool, patch_id, &merge_info).await {
                    Ok(_) => updated_count += 1,
                    Err(e) => {
                        failed_count += 1;
                        errors.push(format!("Patch {}: {}", patch_id, e));
                    }
                }
            } else {
                failed_count += 1;
                errors.push(format!("Patch {}: Could not parse merge metadata", patch_id));
            }
        }
    }
    
    Ok(ReprocessResult {
        total_checked,
        updated_count,
        failed_count,
        errors,
    })
}

/// Result of reprocessing operation
#[derive(Debug, serde::Serialize)]
pub struct ReprocessResult {
    pub total_checked: usize,
    pub updated_count: usize,
    pub failed_count: usize,
    pub errors: Vec<String>,
}

/// Merge status for a thread
#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct ThreadMergeStatus {
    pub merge_repository: String,
    pub merge_branch: String,
    pub merge_applied_by: String,
    pub merge_date: DateTime<Utc>,
    pub commit_count: Option<i32>,
    pub merge_notification_patch_id: i64,
}

