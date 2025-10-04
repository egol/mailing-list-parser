/// Database configuration constants

// Database configuration
pub const DEFAULT_HOST: &str = "localhost";
pub const DEFAULT_PORT: u16 = 5432;
pub const DEFAULT_USER: &str = "postgres";
pub const DEFAULT_PASSWORD: &str = "mysecretpassword";
pub const DEFAULT_DATABASE: &str = "postgres";

// Connection pool
pub const MAX_CONNECTIONS: u32 = 500;
pub const MIN_CONNECTIONS: u32 = 50;
pub const MAX_LIFETIME_SECS: u64 = 300;
pub const IDLE_TIMEOUT_SECS: u64 = 60;

// Batch processing
pub const PARSE_BATCH_SIZE: usize = 1000;
pub const DB_INSERT_BATCH_SIZE: usize = 5000;
pub const PROGRESS_UPDATE_INTERVAL_MS: u64 = 100;
pub const CHANNEL_BUFFER_SIZE: usize = 100;

/// Configuration for PostgreSQL database connection
///
/// This struct holds all necessary connection parameters for establishing
/// a database connection. It supports both programmatic configuration
/// and environment variable loading.
///
/// # Environment Variables
/// The following environment variables are supported for configuration:
/// - `DB_HOST`: Database host (default: "localhost")
/// - `DB_PORT`: Database port (default: 5432)
/// - `DB_USER`: Database username (default: "postgres")
/// - `DB_PASSWORD`: Database password (default: "mysecretpassword")
/// - `DB_NAME`: Database name (default: "postgres")
///
/// # Example
/// ```rust
/// use mailing_list_parser::database::DatabaseConfig;
///
/// // From environment variables
/// let config = DatabaseConfig::from_env();
///
/// // Programmatic configuration
/// let config = DatabaseConfig {
///     host: "localhost".to_string(),
///     port: 5432,
///     user: "myuser".to_string(),
///     password: "mypass".to_string(),
///     database: "mydb".to_string(),
/// };
///
/// // Get connection string for debugging
/// println!("Connection string: {}", config.connection_string());
/// ```
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub database: String,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            host: DEFAULT_HOST.to_string(),
            port: DEFAULT_PORT,
            user: DEFAULT_USER.to_string(),
            password: DEFAULT_PASSWORD.to_string(),
            database: DEFAULT_DATABASE.to_string(),
        }
    }
}

impl DatabaseConfig {
    pub fn from_env() -> Self {
        Self {
            host: std::env::var("DB_HOST").unwrap_or_else(|_| DEFAULT_HOST.to_string()),
            port: std::env::var("DB_PORT")
                .unwrap_or_else(|_| DEFAULT_PORT.to_string())
                .parse()
                .unwrap_or(DEFAULT_PORT),
            user: std::env::var("DB_USER").unwrap_or_else(|_| DEFAULT_USER.to_string()),
            password: std::env::var("DB_PASSWORD").unwrap_or_else(|_| DEFAULT_PASSWORD.to_string()),
            database: std::env::var("DB_NAME").unwrap_or_else(|_| DEFAULT_DATABASE.to_string()),
        }
    }

    pub fn connection_string(&self) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}",
            self.user, self.password, self.host, self.port, self.database
        )
    }
}

