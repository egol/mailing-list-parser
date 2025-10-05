use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::fs;
use std::io;

/// Git repository configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitConfig {
    pub repo_path: String,
    pub clone_url: String,
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            repo_path: String::new(),
            clone_url: "https://lore.kernel.org/bpf/0".to_string(),
        }
    }
}

impl GitConfig {
    /// Get the path to the configuration file
    fn get_config_file_path() -> Result<PathBuf, io::Error> {
        // Use app data directory for config file
        let config_dir = if cfg!(windows) {
            std::env::var("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("."))
        } else {
            std::env::var("HOME")
                .map(|h| PathBuf::from(h).join(".config"))
                .unwrap_or_else(|_| PathBuf::from("."))
        };
        
        let app_config_dir = config_dir.join("mailing-list-parser");
        
        // Create directory if it doesn't exist
        if !app_config_dir.exists() {
            fs::create_dir_all(&app_config_dir)?;
        }
        
        Ok(app_config_dir.join("git-config.json"))
    }

    /// Load configuration from file, falling back to environment variables, then defaults
    pub fn load() -> Self {
        // Try to load from file first
        if let Ok(config_path) = Self::get_config_file_path() {
            if config_path.exists() {
                if let Ok(contents) = fs::read_to_string(&config_path) {
                    if let Ok(config) = serde_json::from_str::<GitConfig>(&contents) {
                        return config;
                    }
                }
            }
        }
        
        // Fall back to environment variables
        Self::from_env()
    }

    /// Load configuration from environment variables with defaults
    pub fn from_env() -> Self {
        Self {
            repo_path: std::env::var("GIT_REPO_PATH")
                .unwrap_or_else(|_| Self::default().repo_path),
            clone_url: std::env::var("GIT_CLONE_URL")
                .unwrap_or_else(|_| Self::default().clone_url),
        }
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<(), String> {
        let config_path = Self::get_config_file_path()
            .map_err(|e| format!("Failed to get config path: {}", e))?;
        
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        
        fs::write(&config_path, json)
            .map_err(|e| format!("Failed to write config file: {}", e))?;
        
        Ok(())
    }

    /// Check if the repository exists at the configured path
    pub fn repo_exists(&self) -> bool {
        let path = PathBuf::from(&self.repo_path);
        path.exists() && path.is_dir()
    }

    /// Get the repository path as a PathBuf
    pub fn get_path(&self) -> PathBuf {
        PathBuf::from(&self.repo_path)
    }
}

/// Result of a git operation with detailed output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitOperationResult {
    pub success: bool,
    pub message: String,
    pub stdout: String,
    pub stderr: String,
}

impl GitOperationResult {
    pub fn success(stdout: String, stderr: String) -> Self {
        let message = if !stderr.is_empty() {
            stderr.clone()
        } else if !stdout.is_empty() {
            stdout.clone()
        } else {
            "Operation completed successfully".to_string()
        };

        Self {
            success: true,
            message,
            stdout,
            stderr,
        }
    }

    pub fn error(message: String) -> Self {
        Self {
            success: false,
            message: message.clone(),
            stdout: String::new(),
            stderr: message,
        }
    }
}

