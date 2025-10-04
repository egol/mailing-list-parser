// Module declarations
mod config;
mod models;
mod connection;
mod schema;
mod authors;
mod patches;
mod threading;
mod population;
pub mod merges;

// Re-export public types
pub use config::DatabaseConfig;
pub use models::{
    Author, 
    AuthorEmail, 
    Patch, 
    DatabaseSetupResult, 
    DatabasePopulationResult, 
    ThreadBuildStats
};

use sqlx::{Pool, Postgres};

/// Main database manager for handling PostgreSQL connections and operations
///
/// This struct provides a high-level interface for:
/// - Database connection management with connection pooling
/// - Schema setup and database initialization
/// - Author and patch data management
/// - Optimized batch processing for large datasets
/// - Progress reporting during data population
///
/// # Example
/// ```rust
/// use mailing_list_parser::database::{DatabaseManager, DatabaseConfig};
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let config = DatabaseConfig::from_env();
///     let mut db_manager = DatabaseManager::new(config);
///
///     // Setup database schema
///     let setup_result = db_manager.setup_database().await?;
///     println!("Database setup: {}", setup_result.message);
///
///     // Test connection
///     if db_manager.test_connection().await? {
///         println!("Database connection successful");
///     }
///
///     Ok(())
/// }
/// ```
pub struct DatabaseManager {
    pool: Option<Pool<Postgres>>,
    config: DatabaseConfig,
}

impl DatabaseManager {
    /// Create a new DatabaseManager instance
    pub fn new(config: DatabaseConfig) -> Self {
        Self {
            pool: None,
            config,
        }
    }
}

