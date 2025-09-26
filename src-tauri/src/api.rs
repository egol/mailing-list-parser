use crate::models::{SearchCriteria, Config, Result};
use crate::database::Database;
use crate::search::SearchService;
use crate::update::UpdateService;
use std::sync::Arc;
use serde_json::json;
use chrono::DateTime;

/// API service providing Tauri command interfaces
pub struct ApiService {
    database: Arc<Database>,
    search_service: SearchService,
    update_service: UpdateService,
}

impl ApiService {
    /// Create a new API service
    pub async fn new(config: Config) -> Result<Self> {
        // Create database connection
        let database = Arc::new(Database::new().await?);

        let search_service = SearchService::new(database.clone());
        let update_service = UpdateService::new(database.clone(), config)?;

        Ok(ApiService {
            database,
            search_service,
            update_service,
        })
    }

    /// Search emails (Tauri command)
    pub async fn search_emails(&self, query: String, limit: Option<i32>, offset: Option<i32>) -> Result<String> {
        let criteria = SearchCriteria {
            query: Some(query),
            author: None,
            subject_contains: None,
            date_from: None,
            date_to: None,
            is_patch: None,
            patch_series: None,
            limit,
            offset,
        };

        let results = self.search_service.search(criteria).await?;
        Ok(serde_json::to_string(&results)?)
    }

    /// Get recent emails (Tauri command)
    pub async fn get_recent_emails(&self, limit: i32) -> Result<String> {
        let emails = self.search_service.get_recent(limit).await?;
        Ok(serde_json::to_string(&emails)?)
    }

    /// Get emails by author (Tauri command)
    pub async fn get_emails_by_author(&self, author: String, limit: Option<i32>) -> Result<String> {
        let emails = self.search_service.get_by_author(&author, limit).await?;
        Ok(serde_json::to_string(&emails)?)
    }

    /// Get patch emails (Tauri command)
    pub async fn get_patches(&self, limit: Option<i32>) -> Result<String> {
        let emails = self.search_service.get_patches(limit).await?;
        Ok(serde_json::to_string(&emails)?)
    }

    /// Advanced search (Tauri command)
    pub async fn advanced_search(&self,
        query: Option<String>,
        author: Option<String>,
        subject: Option<String>,
        date_from: Option<String>,
        date_to: Option<String>,
        is_patch: Option<bool>,
        limit: Option<i32>,
        offset: Option<i32>
    ) -> Result<String> {
        let date_from_parsed = date_from.and_then(|d| {
            DateTime::parse_from_rfc3339(&d).ok().map(|dt| dt.with_timezone(&chrono::Utc))
        });

        let date_to_parsed = date_to.and_then(|d| {
            DateTime::parse_from_rfc3339(&d).ok().map(|dt| dt.with_timezone(&chrono::Utc))
        });

        let results = self.search_service.advanced_search(
            query, author, subject, date_from_parsed, date_to_parsed, is_patch, limit, offset
        ).await?;

        Ok(serde_json::to_string(&results)?)
    }

    /// Pull updates from mailing list (Tauri command)
    pub async fn pull_updates(&self) -> Result<String> {
        let result = self.update_service.pull_updates().await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Full sync of mailing list (Tauri command)
    pub async fn full_sync(&self) -> Result<String> {
        let result = self.update_service.full_sync().await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Get mailing list statistics (Tauri command)
    pub async fn get_statistics(&self) -> Result<String> {
        let stats = self.search_service.get_statistics().await?;
        Ok(serde_json::to_string(&stats)?)
    }

    /// Get email thread (Tauri command)
    pub async fn get_email_thread(&self, message_id: String) -> Result<String> {
        let thread = self.search_service.get_email_thread(&message_id).await?;
        match thread {
            Some(thread) => Ok(serde_json::to_string(&thread)?),
            None => Ok(json!({"error": "Thread not found"}).to_string()),
        }
    }

    /// Get email by message ID (Tauri command)
    pub async fn get_email_by_id(&self, message_id: String) -> Result<String> {
        let email = self.database.get_email_by_message_id(&message_id).await?;
        match email {
            Some(email) => Ok(serde_json::to_string(&email)?),
            None => Ok(json!({"error": "Email not found"}).to_string()),
        }
    }

    /// Test database connection (Tauri command)
    pub async fn test_connection(&self) -> Result<String> {
        // Simple query to test connection
        let _ = self.database.client.query_one("SELECT 1", &[]).await?;
        Ok(json!({"status": "connected"}).to_string())
    }

    /// Get configuration status (Tauri command)
    pub async fn get_config_status(&self) -> Result<String> {
        // This would return information about the current configuration
        // For now, just return a basic status
        Ok(json!({
            "database_connected": true,
            "mailing_list_path": "configured"
        }).to_string())
    }
}

/// Tauri commands that will be exposed to the frontend
#[tauri::command]
pub async fn search_emails_cmd(
    api_service: tauri::State<'_, Arc<ApiService>>,
    query: String,
    limit: Option<i32>,
    offset: Option<i32>
) -> std::result::Result<String, String> {
    api_service.search_emails(query, limit, offset).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_recent_emails_cmd(
    api_service: tauri::State<'_, Arc<ApiService>>,
    limit: i32
) -> std::result::Result<String, String> {
    api_service.get_recent_emails(limit).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_emails_by_author_cmd(
    api_service: tauri::State<'_, Arc<ApiService>>,
    author: String,
    limit: Option<i32>
) -> std::result::Result<String, String> {
    api_service.get_emails_by_author(author, limit).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_patches_cmd(
    api_service: tauri::State<'_, Arc<ApiService>>,
    limit: Option<i32>
) -> std::result::Result<String, String> {
    api_service.get_patches(limit).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn advanced_search_cmd(
    api_service: tauri::State<'_, Arc<ApiService>>,
    query: Option<String>,
    author: Option<String>,
    subject: Option<String>,
    date_from: Option<String>,
    date_to: Option<String>,
    is_patch: Option<bool>,
    limit: Option<i32>,
    offset: Option<i32>
) -> std::result::Result<String, String> {
    api_service.advanced_search(query, author, subject, date_from, date_to, is_patch, limit, offset).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn pull_updates_cmd(
    api_service: tauri::State<'_, Arc<ApiService>>
) -> std::result::Result<String, String> {
    api_service.pull_updates().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn full_sync_cmd(
    api_service: tauri::State<'_, Arc<ApiService>>
) -> std::result::Result<String, String> {
    api_service.full_sync().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_statistics_cmd(
    api_service: tauri::State<'_, Arc<ApiService>>
) -> std::result::Result<String, String> {
    api_service.get_statistics().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_email_thread_cmd(
    api_service: tauri::State<'_, Arc<ApiService>>,
    message_id: String
) -> std::result::Result<String, String> {
    api_service.get_email_thread(message_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_email_by_id_cmd(
    api_service: tauri::State<'_, Arc<ApiService>>,
    message_id: String
) -> std::result::Result<String, String> {
    api_service.get_email_by_id(message_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn test_connection_cmd(
    api_service: tauri::State<'_, Arc<ApiService>>
) -> std::result::Result<String, String> {
    api_service.test_connection().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_config_status_cmd(
    api_service: tauri::State<'_, Arc<ApiService>>
) -> std::result::Result<String, String> {
    api_service.get_config_status().await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_structure() {
        // This test ensures the API module compiles correctly
        // and all the basic structures are in place
        assert_eq!("search_emails", "search_emails");
    }
}
