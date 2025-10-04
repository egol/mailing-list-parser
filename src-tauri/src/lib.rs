// Include the git parser module
#[path = "git-parser.rs"]
pub mod git_parser;

// Include the mail parser module
#[path = "mail-parser.rs"]
pub mod mail_parser;

// Include the database module
pub mod database;

// Include the database API module
#[path = "database_api.rs"]
pub mod database_api;

// Include the test threading module (for development)
#[cfg(test)]
#[path = "test_threading.rs"]
pub mod test_threading;

#[cfg(test)]
#[path = "test_threading_full.rs"]
pub mod test_threading_full;

#[cfg(test)]
#[path = "test_threading_db.rs"]
pub mod test_threading_db;

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
            // Get commit metadata for author and subject
            match git_parser::get_single_commit_metadata(commit_hash) {
                Ok(metadata) => {
                    match mail_parser::parse_email_from_content(commit_hash, &email_content, &metadata) {
                        Ok(email_info) => Ok(email_info),
                        Err(e) => Err(format!("Failed to parse email: {}", e)),
                    }
                }
                Err(e) => Err(format!("Failed to get commit metadata: {}", e)),
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

            // Get metadata for all commits
            let metadata_list = match git_parser::get_commit_metadata(&all_commits) {
                Ok(list) => list,
                Err(e) => return Err(format!("Failed to get commit metadata: {}", e)),
            };

            for (commit_hash, metadata) in all_commits.iter().zip(metadata_list.iter()) {
                if let Ok(email_content) = git_parser::get_email_content(commit_hash) {
                    if let Ok(email) = mail_parser::parse_email_from_content(commit_hash, &email_content, metadata) {
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

    match database_api::search_patches_for_frontend(&mut db_manager, &author_pattern, limit).await {
        Ok(emails) => Ok(emails),
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

// Get database statistics (async) - returns simple stats for compatibility
#[tauri::command]
async fn get_database_stats() -> Result<serde_json::Value, String> {
    let mut db_manager = database::DatabaseManager::new(DatabaseConfig::from_env());

    // Get enhanced stats and convert to simple format for compatibility
    match database_api::get_enhanced_stats(&mut db_manager).await {
        Ok(stats) => {
            let simple_stats = serde_json::json!({
                "total_authors": stats.total_authors,
                "total_patches": stats.total_patches,
                "total_emails": stats.total_emails,
                "unique_authors": stats.total_authors,
                "unique_threads": 0  // Not tracked in new schema
            });
            Ok(simple_stats)
        },
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

// Get all authors with their emails (async)
#[tauri::command]
async fn get_authors() -> Result<Vec<database_api::AuthorInfo>, String> {
    let mut db_manager = database::DatabaseManager::new(DatabaseConfig::from_env());

    match database_api::get_authors_with_emails(&mut db_manager).await {
        Ok(authors) => Ok(authors),
        Err(e) => Err(format!("Failed to get authors: {}", e)),
    }
}

// Get enhanced database statistics (async)
#[tauri::command]
async fn get_enhanced_database_stats() -> Result<database_api::DatabaseStats, String> {
    let mut db_manager = database::DatabaseManager::new(DatabaseConfig::from_env());

    match database_api::get_enhanced_stats(&mut db_manager).await {
        Ok(stats) => Ok(stats),
        Err(e) => Err(format!("Failed to get enhanced stats: {}", e)),
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

// Threading commands

/// Build thread relationships for all patches
#[tauri::command]
async fn build_threads() -> Result<database::ThreadBuildStats, String> {
    let mut db_manager = database::DatabaseManager::new(DatabaseConfig::from_env());

    match db_manager.build_thread_relationships().await {
        Ok(stats) => Ok(stats),
        Err(e) => Err(format!("Failed to build threads: {}", e)),
    }
}

/// Get all threads (paginated with sorting)
#[tauri::command]
async fn get_threads(limit: Option<usize>, offset: Option<usize>, sort_by: Option<String>) -> Result<Vec<database_api::ThreadSummary>, String> {
    let mut db_manager = database::DatabaseManager::new(DatabaseConfig::from_env());

    match database_api::get_all_threads(&mut db_manager, limit, offset, sort_by).await {
        Ok(threads) => Ok(threads),
        Err(e) => Err(format!("Failed to get threads: {}", e)),
    }
}

/// Get full thread tree by thread ID
#[tauri::command]
async fn get_thread_tree(thread_id: i64) -> Result<database_api::ThreadTree, String> {
    let mut db_manager = database::DatabaseManager::new(DatabaseConfig::from_env());

    match database_api::get_thread_tree(&mut db_manager, thread_id).await {
        Ok(tree) => Ok(tree),
        Err(e) => Err(format!("Failed to get thread tree: {}", e)),
    }
}

/// Find thread for a specific patch
#[tauri::command]
async fn get_thread_for_patch(patch_id: i64) -> Result<Option<database_api::ThreadTree>, String> {
    let mut db_manager = database::DatabaseManager::new(DatabaseConfig::from_env());

    match database_api::get_thread_for_patch(&mut db_manager, patch_id).await {
        Ok(thread) => Ok(thread),
        Err(e) => Err(format!("Failed to find thread for patch: {}", e)),
    }
}

/// Search threads by keyword
#[tauri::command]
async fn search_threads(keyword: String, limit: Option<usize>) -> Result<Vec<database_api::ThreadSummary>, String> {
    let mut db_manager = database::DatabaseManager::new(DatabaseConfig::from_env());

    match database_api::search_threads(&mut db_manager, &keyword, limit).await {
        Ok(threads) => Ok(threads),
        Err(e) => Err(format!("Failed to search threads: {}", e)),
    }
}

/// Get full patch body with diff
#[tauri::command]
async fn get_patch_body(patch_id: i64) -> Result<Option<String>, String> {
    let mut db_manager = database::DatabaseManager::new(DatabaseConfig::from_env());

    match database_api::get_patch_body(&mut db_manager, patch_id).await {
        Ok(body) => Ok(body),
        Err(e) => Err(format!("Failed to get patch body: {}", e)),
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
            get_enhanced_database_stats,
            reset_database,
            get_authors,
            get_patches_by_author,
            build_threads,
            get_threads,
            get_thread_tree,
            get_thread_for_patch,
            search_threads,
            get_patch_body
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}