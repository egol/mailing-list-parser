pub mod models;
pub mod parser;
pub mod database;
mod search;
mod api;
mod update;

use models::{Config, DEFAULT_MAILING_LIST_GIT_PATH};
use api::ApiService;
use std::sync::Arc;

struct AppState {
    api_service: Arc<ApiService>,
}

// Initialize logging
fn init_logging() {
    env_logger::init();
}

// Initialize the app state
async fn init_app_state() -> AppState {
    // Create configuration with default mailing list path
    let mailing_list_path = std::env::var("MAILING_LIST_PATH")
        .unwrap_or_else(|_| DEFAULT_MAILING_LIST_GIT_PATH.to_string());

    let config = Config {
        database_url: std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgresql://localhost/mailing_list".to_string()),
        mailing_list_path,
        max_emails_per_batch: 1000,
        enable_auto_update: false,
        update_interval_minutes: 60,
    };

    // Create API service with proper async database initialization
    let api_service = Arc::new(ApiService::new(config).await.expect("Failed to create API service"));

    AppState {
        api_service,
    }
}

#[tokio::main]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub async fn run() {
    // Initialize logging
    init_logging();

    // Initialize the app state
    let app_state = init_app_state().await;

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            parse_email_cmd,
            get_parsed_emails_cmd,
            crate::api::search_emails_cmd,
            crate::api::get_recent_emails_cmd,
            crate::api::get_emails_by_author_cmd,
            crate::api::get_patches_cmd,
            crate::api::advanced_search_cmd,
            crate::api::pull_updates_cmd,
            crate::api::full_sync_cmd,
            crate::api::get_statistics_cmd,
            crate::api::get_email_thread_cmd,
            crate::api::get_email_by_id_cmd,
            crate::api::test_connection_cmd,
            crate::api::get_config_status_cmd
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// Tauri command to parse a raw email (legacy - for testing)
#[tauri::command]
async fn parse_email_cmd(
    state: tauri::State<'_, AppState>,
    raw_email: String
) -> std::result::Result<String, String> {
    // This is a legacy command for testing - use search functionality instead
    match state.api_service.search_emails(raw_email, Some(1), Some(0)).await {
        Ok(result) => std::result::Result::Ok(format!("Email parsed and stored: {}", result)),
        Err(e) => std::result::Result::Err(format!("Failed to parse email: {}", e))
    }
}

// Tauri command to get all parsed emails (legacy - for testing)
#[tauri::command]
async fn get_parsed_emails_cmd(
    state: tauri::State<'_, AppState>
) -> std::result::Result<String, String> {
    // This is a legacy command - use get_recent_emails instead
    match state.api_service.get_recent_emails(50).await {
        Ok(result) => std::result::Result::Ok(result),
        Err(e) => std::result::Result::Err(e.to_string())
    }
}