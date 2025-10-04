/// Database API for frontend - handles translation between DB schema and frontend needs
use serde::Serialize;
use sqlx::Row;
use std::collections::HashMap;
use crate::database::DatabaseManager;
use crate::mail_parser::EmailInfo;

/// Simplified author info for frontend display
#[derive(Debug, Serialize, Clone)]
pub struct AuthorInfo {
    pub author_id: i64,
    pub display_name: String,
    pub first_name: String,
    pub last_name: Option<String>,
    pub emails: Vec<String>,
    pub patch_count: i32,
    pub first_seen: Option<String>,
}

/// Patch with author info for frontend
#[derive(Debug, Serialize, Clone)]
pub struct PatchWithAuthor {
    pub patch_id: i64,
    pub subject: String,
    pub sent_at: String,
    pub commit_hash: Option<String>,
    pub author_display_name: String,
    pub author_email: Option<String>,
    pub is_series: Option<bool>,
    pub series_info: Option<String>, // "2/5" format
}

/// Database statistics for frontend
#[derive(Debug, Serialize)]
pub struct DatabaseStats {
    pub total_authors: i64,
    pub total_patches: i64,
    pub total_emails: i64,
    pub unique_email_addresses: i64,
    pub patches_with_series: i64,
    pub top_contributors: Vec<TopContributor>,
    pub recent_activity: Vec<ActivityDay>,
}

#[derive(Debug, Serialize)]
pub struct TopContributor {
    pub display_name: String,
    pub patch_count: i32,
}

#[derive(Debug, Serialize)]
pub struct ActivityDay {
    pub date: String,
    pub patch_count: i64,
}

/// Get all authors with their email addresses
pub async fn get_authors_with_emails(db: &mut DatabaseManager) -> Result<Vec<AuthorInfo>, Box<dyn std::error::Error>> {
    db.ensure_connected().await?;
    let pool = db.get_pool()?;
    
    // Use a single query with array_agg to get all authors and their emails at once
    // This is much faster than N+1 queries (one per author)
    let rows = sqlx::query(
        "SELECT 
            a.author_id,
            a.display_name,
            a.first_name,
            a.last_name,
            a.first_seen,
            a.patch_count,
            COALESCE(
                array_agg(ae.email ORDER BY ae.is_primary DESC, ae.email) 
                FILTER (WHERE ae.email IS NOT NULL),
                ARRAY[]::TEXT[]
            ) as emails
        FROM authors a
        LEFT JOIN author_emails ae ON a.author_id = ae.author_id
        GROUP BY a.author_id, a.display_name, a.first_name, a.last_name, a.first_seen, a.patch_count
        ORDER BY a.patch_count DESC"
    )
    .fetch_all(pool)
    .await?;
    
    let author_infos: Vec<AuthorInfo> = rows.iter().map(|row| {
        AuthorInfo {
            author_id: row.get(0),
            display_name: row.get(1),
            first_name: row.get(2),
            last_name: row.get(3),
            first_seen: row.get::<Option<chrono::DateTime<chrono::Utc>>, _>(4).map(|dt| dt.to_rfc3339()),
            patch_count: row.get(5),
            emails: row.get(6),
        }
    }).collect();
    
    Ok(author_infos)
}

/// Search patches by author and return frontend-friendly format
pub async fn search_patches_for_frontend(
    db: &mut DatabaseManager,
    author_pattern: &str,
    limit: Option<usize>
) -> Result<Vec<EmailInfo>, Box<dyn std::error::Error>> {
    let results = db.search_patches_by_author(author_pattern, limit).await?;
    
    let mut emails = Vec::new();
    for (patch, author) in results {
        // Get the email used for this patch
        let email = if let Some(email_id) = patch.email_id {
            db.ensure_connected().await?;
            let pool = db.get_pool()?;
            let email_row: Option<(String,)> = sqlx::query_as(
                "SELECT email FROM author_emails WHERE email_id = $1"
            )
            .bind(email_id)
            .fetch_optional(pool)
            .await?;
            email_row.map(|(e,)| e).unwrap_or_else(|| "unknown@example.com".to_string())
        } else {
            "unknown@example.com".to_string()
        };
        
        emails.push(EmailInfo {
            commit_hash: patch.commit_hash.unwrap_or_else(|| patch.message_id.clone()),
            subject: patch.subject.clone(),
            normalized_subject: crate::mail_parser::normalize_subject(&patch.subject),
            from: format!("{} <{}>", author.display_name, email),
            author_email: email,
            author_first_name: author.first_name,
            author_last_name: author.last_name,
            author_display_name: author.display_name,
            to: "bpf@vger.kernel.org".to_string(),
            date: patch.sent_at.to_rfc3339(),
            message_id: patch.message_id,
            body: patch.body_text.unwrap_or_default(),
            headers: std::collections::HashMap::new(),
            in_reply_to: None,      // Not stored in legacy query
            references: Vec::new(), // Not stored in legacy query
            is_reply: false,        // Not stored in legacy query
        });
    }
    
    Ok(emails)
}

/// Get comprehensive database statistics
pub async fn get_enhanced_stats(db: &mut DatabaseManager) -> Result<DatabaseStats, Box<dyn std::error::Error>> {
    db.ensure_connected().await?;
    let pool = db.get_pool()?;
    
    // Combine all COUNT queries into a single query for better performance
    let stats_row = sqlx::query(
        "SELECT 
            (SELECT COUNT(*) FROM authors) as total_authors,
            (SELECT COUNT(*) FROM patches) as total_patches,
            (SELECT COUNT(*) FROM author_emails) as total_emails,
            (SELECT COUNT(*) FROM patches WHERE is_series = true) as patches_with_series"
    )
    .fetch_one(pool)
    .await?;
    
    let total_authors: i64 = stats_row.get(0);
    let total_patches: i64 = stats_row.get(1);
    let total_emails: i64 = stats_row.get(2);
    let patches_with_series: i64 = stats_row.get(3);
    
    // Top 10 contributors
    let top_rows = sqlx::query(
        "SELECT display_name, patch_count FROM authors ORDER BY patch_count DESC LIMIT 10"
    )
    .fetch_all(pool)
    .await?;
    
    let top_contributors: Vec<TopContributor> = top_rows.iter().map(|row| TopContributor {
        display_name: row.get(0),
        patch_count: row.get(1),
    }).collect();
    
    // Recent activity (last 30 days)
    let activity_rows = sqlx::query(
        "SELECT DATE(sent_at) as day, COUNT(*) as count 
         FROM patches 
         WHERE sent_at > NOW() - INTERVAL '30 days'
         GROUP BY DATE(sent_at)
         ORDER BY day DESC
         LIMIT 30"
    )
    .fetch_all(pool)
    .await?;
    
    let recent_activity: Vec<ActivityDay> = activity_rows.iter().map(|row| {
        let date: chrono::NaiveDate = row.get(0);
        ActivityDay {
            date: date.to_string(),
            patch_count: row.get(1),
        }
    }).collect();
    
    Ok(DatabaseStats {
        total_authors,
        total_patches,
        total_emails,
        unique_email_addresses: total_emails,
        patches_with_series,
        top_contributors,
        recent_activity,
    })
}

// Threading API

#[derive(Debug, Serialize, Clone)]
pub struct ThreadSummary {
    pub thread_id: i64,
    pub root_subject: String,
    pub root_author: String,
    pub reply_count: i32,
    pub participant_count: i32,
    pub created_at: String,
    pub last_activity: String,
    pub root_patch_id: i64,
    pub merge_status: Option<MergeStatusInfo>,
}

#[derive(Debug, Serialize, Clone)]
pub struct MergeStatusInfo {
    pub is_merged: bool,
    pub merge_date: String,
    pub repository: String,
    pub branch: String,
    pub applied_by: String,
    pub commit_count: i32,
}

#[derive(Debug, Serialize, Clone)]
pub struct ThreadNode {
    pub patch_id: i64,
    pub subject: String,
    pub author_name: String,
    pub author_email: String,
    pub sent_at: String,
    pub depth: i32,
    pub message_id: String,
    pub body_preview: String,  // Smart preview of actual content
    pub is_reply: bool,        // True if subject starts with "Re:"
    pub is_series: bool,       // True if part of a patch series
    pub series_info: Option<String>,  // e.g., "3/12" for patch series
    pub has_diff: bool,        // True if body contains git diff/patch content
    pub reply_count: i32,      // Direct reply count for this node
    pub commit_hash: Option<String>,  // Git commit hash for debugging
    pub children: Vec<ThreadNode>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ThreadTree {
    pub thread_id: i64,
    pub summary: ThreadSummary,
    pub root: ThreadNode,
}

/// Get all thread summaries (for thread list view)
pub async fn get_all_threads(
    db: &mut DatabaseManager,
    limit: Option<usize>,
    offset: Option<usize>,
    sort_by: Option<String>,
    merge_filter: Option<String>
) -> Result<Vec<ThreadSummary>, Box<dyn std::error::Error>> {
    db.ensure_connected().await?;
    let pool = db.get_pool()?;
    
    let limit_val = limit.unwrap_or(50) as i64;
    let offset_val = offset.unwrap_or(0) as i64;
    
    // Determine sort order
    let order_by = match sort_by.as_deref() {
        Some("oldest") => "created_at ASC",
        Some("newest") => "created_at DESC",
        Some("most_replies") => "reply_count DESC",
        Some("most_participants") => "participant_count DESC",
        _ => "last_activity_at DESC", // Default: most recent activity
    };
    
    // Determine merge filter
    let merge_filter_clause = match merge_filter.as_deref() {
        Some("merged") => "WHERE mt.thread_id IS NOT NULL",
        Some("unmerged") => "WHERE mt.thread_id IS NULL",
        _ => "", // Default: show all
    };
    
    let query = format!(
        "SELECT 
            ts.thread_id,
            ts.root_subject,
            ts.root_author,
            ts.reply_count,
            ts.participant_count,
            ts.created_at,
            ts.last_activity_at,
            ts.root_patch_id,
            mt.merge_repository,
            mt.merge_branch,
            mt.merge_applied_by,
            mt.merge_date,
            mt.commit_count
         FROM thread_summary ts
         LEFT JOIN merged_threads mt ON ts.thread_id = mt.thread_id
         {}
         ORDER BY {}
         LIMIT $1 OFFSET $2",
        merge_filter_clause,
        order_by
    );
    
    let rows = sqlx::query(&query)
    .bind(limit_val)
    .bind(offset_val)
    .fetch_all(pool)
    .await?;
    
    let threads = rows.iter().map(|row| {
        let merge_status = if let Ok(Some(repo)) = row.try_get::<Option<String>, _>(8) {
            Some(MergeStatusInfo {
                is_merged: true,
                merge_date: row.get::<chrono::DateTime<chrono::Utc>, _>(11).to_rfc3339(),
                repository: repo,
                branch: row.get::<String, _>(9),
                applied_by: row.get::<String, _>(10),
                commit_count: row.get::<Option<i32>, _>(12).unwrap_or(0),
            })
        } else {
            None
        };
        
        ThreadSummary {
            thread_id: row.get(0),
            root_subject: row.get(1),
            root_author: row.get(2),
            reply_count: row.get(3),
            participant_count: row.get(4),
            created_at: row.get::<chrono::DateTime<chrono::Utc>, _>(5).to_rfc3339(),
            last_activity: row.get::<chrono::DateTime<chrono::Utc>, _>(6).to_rfc3339(),
            root_patch_id: row.get(7),
            merge_status,
        }
    }).collect();
    
    Ok(threads)
}

fn remove_attribution_lines(text: &str) -> String {
    let result = text.to_string();
    
    // Remove email attribution patterns like "On Wed, Sep 24, 2025 at 1:43 AM ... wrote:"
    let result_lines: Vec<&str> = result.lines().collect();
    let mut cleaned_lines = Vec::new();
    
    for line in result_lines {
        let trimmed = line.trim();
        
        // Skip empty lines
        if trimmed.is_empty() {
            cleaned_lines.push(line);
            continue;
        }
        
        // Skip email attribution lines (various patterns)
        // Pattern 1: "On ... wrote:" (most common)
        if trimmed.starts_with("On ") && trimmed.contains(" wrote:") {
            continue;
        }
        
        // Pattern 2: Contains date patterns with email addresses and "wrote:"
        // Example: "On Wed, Sep 24, 2025 at 1:43 AM Brahmajit Das <...> wrote:"
        if trimmed.starts_with("On ") 
            && (trimmed.contains("@") || trimmed.contains('<'))
            && trimmed.contains(" wrote:") {
            continue;
        }
        
        // Pattern 3: Date-based attribution patterns ending with colon
        if (trimmed.starts_with("On ") || trimmed.starts_with("Am ")) 
            && (trimmed.contains(", 20") || trimmed.contains(", 19"))
            && trimmed.ends_with(':') {
            continue;
        }
        
        // Pattern 4: Lines that start with date and contain <email> and wrote
        if trimmed.contains(", 20") && trimmed.contains('<') && trimmed.contains('>') 
            && trimmed.to_lowercase().contains("wrote") {
            continue;
        }
        
        cleaned_lines.push(line);
    }
    
    cleaned_lines.join("\n")
}

/// Check if body contains git diff/patch content (not quoted)
/// This should return true only for actual patches, not replies quoting patches
/// Improved: requires multiple consecutive diff lines to avoid false positives
fn has_diff_content(body: &str) -> bool {
    let mut consecutive_diff_lines = 0;
    const MIN_DIFF_LINES: i32 = 3; // Require at least 3 consecutive diff lines
    
    for line in body.lines() {
        let trimmed = line.trim();
        
        // Skip empty lines (don't reset counter)
        if trimmed.is_empty() {
            continue;
        }
        
        // Skip quoted lines (these are not the actual patch, just quoted content)
        if trimmed.starts_with('>') {
            consecutive_diff_lines = 0; // Reset counter
            continue;
        }
        
        // Check for actual diff markers in non-quoted lines
        let is_diff_line = trimmed.starts_with("diff --git") 
            || trimmed.starts_with("--- a/")
            || trimmed.starts_with("+++ b/")
            || (trimmed.starts_with("@@") && trimmed.contains("@@"))
            || (trimmed.starts_with("index ") && trimmed.len() > 10)
            || trimmed.starts_with("new file mode")
            || trimmed.starts_with("deleted file mode")
            || (consecutive_diff_lines > 0 && (trimmed.starts_with('+') || trimmed.starts_with('-') || trimmed.starts_with(' ')));
        
        if is_diff_line {
            consecutive_diff_lines += 1;
            if consecutive_diff_lines >= MIN_DIFF_LINES {
                return true;
            }
        } else {
            // Non-diff line found, reset counter
            consecutive_diff_lines = 0;
        }
    }
    
    false
}

/// Wrap text to fit within specified character width
fn wrap_text_to_width(text: &str, max_width: usize) -> String {
    let mut result = Vec::new();
    
    for line in text.lines() {
        if line.len() <= max_width {
            result.push(line.to_string());
        } else {
            // Split long lines at word boundaries
            let mut current_line = String::new();
            for word in line.split_whitespace() {
                if current_line.is_empty() {
                    current_line = word.to_string();
                } else if current_line.len() + word.len() + 1 <= max_width {
                    current_line.push(' ');
                    current_line.push_str(word);
                } else {
                    result.push(current_line);
                    current_line = word.to_string();
                }
            }
            if !current_line.is_empty() {
                result.push(current_line);
            }
        }
    }
    
    result.join("\n")
}

/// Extract the actual reply content, filtering out noise
/// Remove quoted lines, email encoding artifacts, and unwanted formatting
fn extract_reply_content(body: &str) -> String {
    // Content should already be decoded by mail-parser.rs based on Content-Transfer-Encoding header
    // Don't try to guess/re-decode here - just use the raw text as-is
    
    // Remove attribution lines
    let cleaned = remove_attribution_lines(body);
    
    let mut result = Vec::new();
    let mut in_signature = false;
    let mut in_diff = false;
    
    for line in cleaned.lines() {
        let trimmed = line.trim();
        
        // Skip empty lines
        if trimmed.is_empty() {
            continue;
        }
        
        // Detect signature separator (-- or -- with space)
        if trimmed == "--" || trimmed == "-- " {
            in_signature = true;
            continue;
        }
        
        // Skip lines after signature
        if in_signature {
            continue;
        }
        
        // Skip quoted lines (starting with >)
        if trimmed.starts_with('>') {
            continue;
        }
        
        // Detect diff/patch content (non-quoted)
        if trimmed.starts_with("diff --git") || 
           trimmed.starts_with("--- a/") ||
           trimmed.starts_with("+++ b/") {
            in_diff = true;
        }
        
        // Skip lines in diff blocks
        if in_diff {
            // Exit diff mode if we see normal text (not starting with +, -, @, or space)
            if !trimmed.starts_with('+') && 
               !trimmed.starts_with('-') && 
               !trimmed.starts_with('@') &&
               !trimmed.starts_with(' ') &&
               !trimmed.chars().next().unwrap_or(' ').is_ascii_punctuation() {
                in_diff = false;
                result.push(line); // Include this line as it's regular text
            }
            // Skip diff lines
            continue;
        }
        
        // Keep everything else
        result.push(line);
    }
    
    let joined = result.join("\n").trim().to_string();
    
    // Wrap to 80 characters for readability
    wrap_text_to_width(&joined, 80)
}

/// Strip "RE:" and similar reply prefixes from subject for display
/// This is different from normalize_subject which is for matching/comparison
fn strip_reply_prefix(subject: &str) -> String {
    let mut cleaned = subject.trim().to_string();
    
    // Remove reply prefixes but keep [PATCH] tags
    let prefixes = ["re:", "Re:", "RE:", "fwd:", "Fwd:", "FWD:", "fw:", "Fw:", "FW:"];
    loop {
        let mut changed = false;
        for prefix in &prefixes {
            if cleaned.starts_with(prefix) {
                cleaned = cleaned[prefix.len()..].trim_start().to_string();
                changed = true;
                break;
            }
        }
        if !changed {
            break;
        }
    }
    
    cleaned.trim().to_string()
}

/// Get full thread tree with nested structure
pub async fn get_thread_tree(
    db: &mut DatabaseManager,
    thread_id: i64
) -> Result<ThreadTree, Box<dyn std::error::Error>> {
    db.ensure_connected().await?;
    let pool = db.get_pool()?;
    
    // Get all messages in thread with series and reply information
    let messages = sqlx::query(
        "SELECT 
            pr.patch_id,
            pr.parent_patch_id,
            pr.depth_level,
            p.subject,
            p.message_id,
            p.body_text,
            p.sent_at,
            a.display_name,
            ae.email,
            p.is_reply,
            p.is_series,
            p.series_number,
            p.series_total,
            p.commit_hash
         FROM patch_replies pr
         JOIN patches p ON pr.patch_id = p.patch_id
         JOIN authors a ON p.author_id = a.author_id
         LEFT JOIN author_emails ae ON p.email_id = ae.email_id
         WHERE pr.thread_id = $1
         ORDER BY pr.position_in_thread ASC"
    )
    .bind(thread_id)
    .fetch_all(pool)
    .await?;
    
    // Build node map
    let mut nodes: HashMap<i64, ThreadNode> = HashMap::new();
    let mut root_id = None;
    
    for row in &messages {
        let patch_id: i64 = row.get(0);
        let parent_id: Option<i64> = row.get(1);
        let body: Option<String> = row.get(5);
        let is_reply: bool = row.get(9);
        let is_series: bool = row.try_get(10).unwrap_or(false);
        let series_number: Option<i32> = row.try_get(11).ok();
        let series_total: Option<i32> = row.try_get(12).ok();
        let commit_hash: Option<String> = row.try_get(13).ok();
        
        let body_text = body.unwrap_or_default();
        
        // Check if body contains diff/patch content
        // IMPORTANT: Replies (Re:) should never be marked as having patches,
        // even if they quote patch content
        let has_diff = !is_reply && has_diff_content(&body_text);
        
        // Extract actual reply content (removes quoted lines, signatures, diffs)
        // Don't truncate here - let frontend handle display truncation
        let cleaned_body = extract_reply_content(&body_text);
        let body_preview = if !cleaned_body.is_empty() {
            cleaned_body
        } else {
            // Fallback: if extraction resulted in empty, show first few lines
            body_text.lines()
                .take(20)
                .collect::<Vec<_>>()
                .join("\n")
        };
        
        // Format series info
        let series_info = if is_series {
            match (series_number, series_total) {
                (Some(num), Some(total)) => Some(format!("{}/{}", num, total)),
                _ => None,
            }
        } else {
            None
        };
        
        // Clean subject: strip "RE:" from replies for display
        let raw_subject: String = row.get(3);
        let display_subject = strip_reply_prefix(&raw_subject);
        
        let node = ThreadNode {
            patch_id,
            subject: display_subject,
            author_name: row.get(7),
            author_email: row.get::<Option<String>, _>(8).unwrap_or_default(),
            sent_at: row.get::<chrono::DateTime<chrono::Utc>, _>(6).to_rfc3339(),
            depth: row.get(2),
            message_id: row.get(4),
            body_preview,
            is_reply,
            is_series,
            series_info,
            has_diff,
            reply_count: 0,  // Will be populated when building tree
            commit_hash,
            children: Vec::new(),
        };
        
        if parent_id.is_none() {
            root_id = Some(patch_id);
        }
        
        nodes.insert(patch_id, node);
    }
    
    // Build tree structure
    let mut children_map: HashMap<i64, Vec<i64>> = HashMap::new();
    for row in &messages {
        let patch_id: i64 = row.get(0);
        let parent_id: Option<i64> = row.get(1);
        
        if let Some(parent) = parent_id {
            children_map.entry(parent).or_insert_with(Vec::new).push(patch_id);
        }
    }
    
    // Recursive function to build tree
    fn build_tree(
        node_id: i64,
        nodes: &mut HashMap<i64, ThreadNode>,
        children_map: &HashMap<i64, Vec<i64>>
    ) -> ThreadNode {
        let mut node = nodes.remove(&node_id).unwrap();
        
        if let Some(child_ids) = children_map.get(&node_id) {
            node.reply_count = child_ids.len() as i32;
            for child_id in child_ids {
                let child_node = build_tree(*child_id, nodes, children_map);
                node.children.push(child_node);
            }
        }
        
        node
    }
    
    let root = build_tree(root_id.unwrap(), &mut nodes, &children_map);
    
    // Get thread summary with merge status
    let summary_row = sqlx::query(
        "SELECT 
            ts.thread_id,
            ts.root_subject,
            ts.root_author,
            ts.reply_count,
            ts.participant_count,
            ts.created_at,
            ts.last_activity_at,
            ts.root_patch_id,
            mt.merge_repository,
            mt.merge_branch,
            mt.merge_applied_by,
            mt.merge_date,
            mt.commit_count
         FROM thread_summary ts
         LEFT JOIN merged_threads mt ON ts.thread_id = mt.thread_id
         WHERE ts.thread_id = $1"
    )
    .bind(thread_id)
    .fetch_one(pool)
    .await?;
    
    let merge_status = if let Ok(Some(repo)) = summary_row.try_get::<Option<String>, _>(8) {
        Some(MergeStatusInfo {
            is_merged: true,
            merge_date: summary_row.get::<chrono::DateTime<chrono::Utc>, _>(11).to_rfc3339(),
            repository: repo,
            branch: summary_row.get::<String, _>(9),
            applied_by: summary_row.get::<String, _>(10),
            commit_count: summary_row.get::<Option<i32>, _>(12).unwrap_or(0),
        })
    } else {
        None
    };
    
    let summary = ThreadSummary {
        thread_id: summary_row.get(0),
        root_subject: summary_row.get(1),
        root_author: summary_row.get(2),
        reply_count: summary_row.get(3),
        participant_count: summary_row.get(4),
        created_at: summary_row.get::<chrono::DateTime<chrono::Utc>, _>(5).to_rfc3339(),
        last_activity: summary_row.get::<chrono::DateTime<chrono::Utc>, _>(6).to_rfc3339(),
        root_patch_id: summary_row.get(7),
        merge_status,
    };
    
    Ok(ThreadTree {
        thread_id,
        summary,
        root,
    })
}

/// Get full patch body including diff
pub async fn get_patch_body(
    db: &mut DatabaseManager,
    patch_id: i64
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    db.ensure_connected().await?;
    let pool = db.get_pool()?;
    
    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT body_text FROM patches WHERE patch_id = $1"
    )
    .bind(patch_id)
    .fetch_optional(pool)
    .await?;
    
    Ok(row.and_then(|(body,)| body))
}

/// Find thread containing a specific patch
pub async fn get_thread_for_patch(
    db: &mut DatabaseManager,
    patch_id: i64
) -> Result<Option<ThreadTree>, Box<dyn std::error::Error>> {
    db.ensure_connected().await?;
    let pool = db.get_pool()?;
    
    // Find thread_id for this patch
    let thread_row: Option<(i64,)> = sqlx::query_as(
        "SELECT thread_id FROM patch_replies WHERE patch_id = $1"
    )
    .bind(patch_id)
    .fetch_optional(pool)
    .await?;
    
    if let Some((thread_id,)) = thread_row {
        Ok(Some(get_thread_tree(db, thread_id).await?))
    } else {
        Ok(None)
    }
}

/// Search threads by subject keyword
pub async fn search_threads(
    db: &mut DatabaseManager,
    keyword: &str,
    limit: Option<usize>
) -> Result<Vec<ThreadSummary>, Box<dyn std::error::Error>> {
    db.ensure_connected().await?;
    let pool = db.get_pool()?;
    
    let limit_val = limit.unwrap_or(50) as i64;
    let pattern = format!("%{}%", keyword.to_lowercase());
    
    let rows = sqlx::query(
        "SELECT 
            ts.thread_id,
            ts.root_subject,
            ts.root_author,
            ts.reply_count,
            ts.participant_count,
            ts.created_at,
            ts.last_activity_at,
            ts.root_patch_id,
            mt.merge_repository,
            mt.merge_branch,
            mt.merge_applied_by,
            mt.merge_date,
            mt.commit_count
         FROM thread_summary ts
         LEFT JOIN merged_threads mt ON ts.thread_id = mt.thread_id
         WHERE LOWER(ts.root_subject) LIKE $1
         ORDER BY ts.last_activity_at DESC
         LIMIT $2"
    )
    .bind(&pattern)
    .bind(limit_val)
    .fetch_all(pool)
    .await?;
    
    let threads = rows.iter().map(|row| {
        let merge_status = if let Ok(Some(repo)) = row.try_get::<Option<String>, _>(8) {
            Some(MergeStatusInfo {
                is_merged: true,
                merge_date: row.get::<chrono::DateTime<chrono::Utc>, _>(11).to_rfc3339(),
                repository: repo,
                branch: row.get::<String, _>(9),
                applied_by: row.get::<String, _>(10),
                commit_count: row.get::<Option<i32>, _>(12).unwrap_or(0),
            })
        } else {
            None
        };
        
        ThreadSummary {
            thread_id: row.get(0),
            root_subject: row.get(1),
            root_author: row.get(2),
            reply_count: row.get(3),
            participant_count: row.get(4),
            created_at: row.get::<chrono::DateTime<chrono::Utc>, _>(5).to_rfc3339(),
            last_activity: row.get::<chrono::DateTime<chrono::Utc>, _>(6).to_rfc3339(),
            root_patch_id: row.get(7),
            merge_status,
        }
    }).collect();
    
    Ok(threads)
}
