// Include the git parser module
#[path = "git-parser.rs"]
pub mod git_parser;

// Include the mail parser module
#[path = "mail-parser.rs"]
pub mod mail_parser;

// Include the database module
#[path = "database.rs"]
pub mod database;

// Import the Emitter trait for window.emit()
use tauri::Emitter;

// Re-export git parser types for easy access
pub use git_parser::ParseError;
// Re-export mail parser types for easy access
pub use mail_parser::{EmailInfo, parse_email_from_content, normalize_subject};
// Re-export database types for easy access
pub use database::{DatabaseConfig, DatabaseSetupResult, DatabasePopulationResult, Author, Patch};

// Tauri command to get BPF commit hashes (first 10 by default)
#[tauri::command]
fn get_bpf_commits() -> Result<Vec<String>, ParseError> {
    git_parser::get_all_commits()
}

// Tauri command to get BPF commit hashes with configurable limit
#[tauri::command]
fn get_bpf_commits_with_limit(limit: Option<usize>) -> Result<Vec<String>, ParseError> {
    git_parser::get_all_commits_with_limit(limit)
}

// Tauri command to get a specific BPF email by commit hash
#[tauri::command]
fn get_bpf_email(commit_hash: &str) -> Result<EmailInfo, String> {
    match git_parser::get_email_content(commit_hash) {
        Ok(email_content) => {
            match mail_parser::parse_email_from_content(commit_hash, &email_content) {
                Ok(email_info) => Ok(email_info),
                Err(e) => Err(format!("Failed to parse email: {}", e)),
            }
        }
        Err(e) => Err(format!("Failed to get email content: {}", e)),
    }
}

// Tauri command to get the total count of emails
#[tauri::command]
fn get_bpf_email_count() -> Result<usize, ParseError> {
    git_parser::get_email_count()
}

// Tauri command to get the total count of all git commits
#[tauri::command]
fn get_total_git_commits() -> Result<usize, ParseError> {
    git_parser::get_total_git_commits()
}

// Tauri command to get recent commits (same as get_bpf_commits for backward compatibility)
#[tauri::command]
fn get_recent_bpf_commits() -> Result<Vec<String>, ParseError> {
    git_parser::get_all_commits()
}

// Tauri command to search emails by subject keyword
#[tauri::command]
fn search_bpf_emails(keyword: &str, limit: Option<usize>) -> Result<Vec<EmailInfo>, String> {
    match git_parser::get_all_commits_with_limit(limit) {
        Ok(all_commits) => {
            let mut results = Vec::new();

            for commit_hash in all_commits {
                if let Ok(email_content) = git_parser::get_email_content(&commit_hash) {
                    if let Ok(email) = mail_parser::parse_email_from_content(&commit_hash, &email_content) {
                        if email.subject.to_lowercase().contains(&keyword.to_lowercase()) {
                            results.push(email);
                        }
                    }
                }
            }

            Ok(results)
        }
        Err(e) => Err(format!("Failed to get commits: {}", e)),
    }
}

// Tauri command to search emails by author (database-based)
#[tauri::command]
async fn search_emails_by_author(author_pattern: String, limit: Option<usize>) -> Result<Vec<EmailInfo>, String> {
    let mut db_manager = database::DatabaseManager::new(DatabaseConfig::from_env());

    match db_manager.search_patches_by_author(&author_pattern, limit).await {
        Ok(patches_with_authors) => {
            // Convert patches with authors to EmailInfo format
            let emails: Vec<EmailInfo> = patches_with_authors.into_iter().map(|(patch, author)| {
                let author_name = author.name.unwrap_or_else(|| author.email.clone());
                EmailInfo {
                    commit_hash: patch.commit_hash.unwrap_or_else(|| patch.message_id.clone()),
                    subject: patch.subject.clone(),
                    normalized_subject: crate::mail_parser::normalize_subject(&patch.subject),
                    from: format!("{} <{}>", author_name, author.email),
                    to: "bpf@vger.kernel.org".to_string(),
                    date: patch.sent_at.to_rfc3339(),
                    message_id: patch.message_id,
                    body: patch.body_text.unwrap_or_default(),
                    headers: std::collections::HashMap::new(),
                }
            }).collect();

            Ok(emails)
        }
        Err(e) => Err(format!("Failed to search by author: {}", e)),
    }
}

// Database setup command (async)
#[tauri::command]
async fn setup_database() -> Result<DatabaseSetupResult, String> {
    let mut db_manager = database::DatabaseManager::new(DatabaseConfig::from_env());

    match db_manager.setup_database().await {
        Ok(result) => Ok(result),
        Err(e) => Err(format!("Database setup failed: {}", e)),
    }
}

// Database population command with progress callback (async)
#[tauri::command]
async fn populate_database(limit: Option<usize>, window: tauri::Window) -> Result<DatabasePopulationResult, String> {
    let mut db_manager = database::DatabaseManager::new(DatabaseConfig::from_env());

    // Use Tauri event system for progress tracking
    let progress_fn = move |current: u32, total: u32, commit_hash: String| {
        let payload = serde_json::json!({
            "current": current,
            "total": total,
            "commit_hash": commit_hash
        });
        // Emit event to the window
        let _ = window.emit("populate-progress", payload);
    };

    match db_manager.populate_database(limit, Some(progress_fn)).await {
        Ok(result) => Ok(result),
        Err(e) => Err(format!("Database population failed: {}", e)),
    }
}

// Test database connection (async)
#[tauri::command]
async fn test_database_connection() -> Result<bool, String> {
    let mut db_manager = database::DatabaseManager::new(DatabaseConfig::from_env());

    match db_manager.test_connection().await {
        Ok(success) => Ok(success),
        Err(e) => Err(format!("Database connection test failed: {}", e)),
    }
}

// Get database statistics (async)
#[tauri::command]
async fn get_database_stats() -> Result<serde_json::Value, String> {
    let mut db_manager = database::DatabaseManager::new(DatabaseConfig::from_env());

    match db_manager.get_database_stats().await {
        Ok(stats) => Ok(stats),
        Err(e) => Err(format!("Failed to get database stats: {}", e)),
    }
}

// Reset database (drop all tables) (async)
#[tauri::command]
async fn reset_database() -> Result<String, String> {
    let mut db_manager = database::DatabaseManager::new(DatabaseConfig::from_env());

    match db_manager.reset_database().await {
        Ok(message) => Ok(message),
        Err(e) => Err(format!("Database reset failed: {}", e)),
    }
}

// Get all authors (async)
#[tauri::command]
async fn get_authors() -> Result<Vec<Author>, String> {
    let mut db_manager = database::DatabaseManager::new(DatabaseConfig::from_env());

    match db_manager.get_authors().await {
        Ok(authors) => Ok(authors),
        Err(e) => Err(format!("Failed to get authors: {}", e)),
    }
}

// Get patches by author (async)
#[tauri::command]
async fn get_patches_by_author(author_id: i64) -> Result<Vec<Patch>, String> {
    let mut db_manager = database::DatabaseManager::new(DatabaseConfig::from_env());

    match db_manager.get_patches_by_author(author_id).await {
        Ok(patches) => Ok(patches),
        Err(e) => Err(format!("Failed to get patches: {}", e)),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_bpf_commits,
            get_bpf_commits_with_limit,
            get_bpf_email,
            get_bpf_email_count,
            get_total_git_commits,
            get_recent_bpf_commits,
            search_bpf_emails,
            search_emails_by_author,
            setup_database,
            populate_database,
            test_database_connection,
            get_database_stats,
            reset_database,
            get_authors,
            get_patches_by_author
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}