use std::fs;
use std::path::Path;
use std::collections::{HashSet, HashMap};
use std::sync::Arc;
use std::time::Duration;
use serde::Serialize;
use regex::Regex;
use chrono::{DateTime, Utc, NaiveDateTime};
use sqlx::postgres::PgPoolOptions;
use sqlx::{Pool, Postgres, FromRow, Row};
use tokio::time::interval;
use tokio::sync::Semaphore;
use futures::future;
use crate::git_parser::get_all_commits_with_limit;
use crate::mail_parser::{EmailInfo, parse_emails_parallel};

// Database configuration constants
const DEFAULT_HOST: &str = "localhost";
const DEFAULT_PORT: u16 = 5432;
const DEFAULT_USER: &str = "postgres";
const DEFAULT_PASSWORD: &str = "mysecretpassword";
const DEFAULT_DATABASE: &str = "postgres";

// Connection pool configuration
const MAX_CONNECTIONS: u32 = 500; // Increased for concurrent batch inserts
const MIN_CONNECTIONS: u32 = 50;
const MAX_LIFETIME_SECS: u64 = 300;
const IDLE_TIMEOUT_SECS: u64 = 60;

// Configuration constants
const PATCH_BATCH_SIZE: usize = 1000; // Send to DB every 1000 patches
const PARALLEL_CHUNK_SIZE: usize = 5000; // Process 5000 commits in parallel (I/O bound)
const MAX_CONCURRENT_DB_INSERTS: usize = 10; // Limit concurrent DB operations
const PROGRESS_UPDATE_INTERVAL_MS: u64 = 100; // Update progress every 0.1 seconds
const PATCH_INSERT_PARAMS_PER_ROW: usize = 9; // Number of parameters per patch row in INSERT
const AUTHOR_INSERT_PARAMS_PER_ROW: usize = 2; // Number of parameters per author row in INSERT
const HASH_CHECK_BATCH_SIZE: usize = 1000; // Check 1000 hashes at a time for existence


// Standard table names
const AUTHORS_TABLE: &str = "authors";
const PATCHES_TABLE: &str = "patches";

#[derive(Debug, Serialize, Clone, FromRow)]
pub struct Author {
    pub author_id: i64,
    pub name: Option<String>,
    pub email: String,
    pub first_seen: Option<DateTime<Utc>>,
    pub patch_count: i32,
}

#[derive(Debug, Serialize, Clone, FromRow)]
pub struct Patch {
    pub patch_id: i64,
    pub author_id: i64,
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

#[derive(Debug, Serialize)]
pub struct DatabaseSetupResult {
    pub success: bool,
    pub message: String,
    pub tables_created: Vec<String>,
}


#[derive(Debug)]
struct PatchData {
    pub author_id: i64,
    pub message_id: String,
    pub subject: String,
    pub sent_at: DateTime<Utc>,
    pub commit_hash: String,
    pub body_text: Option<String>,
    pub is_series: bool,
    pub series_number: Option<i32>,
    pub series_total: Option<i32>,
}

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

    /// Establish database connection with optimized pool settings
    pub async fn connect(&mut self) -> Result<(), sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(MAX_CONNECTIONS)
            .min_connections(MIN_CONNECTIONS)
            .max_lifetime(std::time::Duration::from_secs(MAX_LIFETIME_SECS))
            .idle_timeout(std::time::Duration::from_secs(IDLE_TIMEOUT_SECS))
            .connect(&self.config.connection_string())
            .await?;

        self.pool = Some(pool);
        Ok(())
    }

    /// Ensure database connection is established, connecting if necessary
    async fn ensure_connected(&mut self) -> Result<(), sqlx::Error> {
        if self.pool.is_none() {
            self.connect().await?;
        }
        Ok(())
    }

    /// Get a reference to the connection pool if connected
    fn get_pool(&self) -> Result<&Pool<Postgres>, sqlx::Error> {
        self.pool.as_ref().ok_or_else(|| sqlx::Error::Configuration("Not connected to database".into()))
    }

    /// Execute SQL commands from a file
    pub async fn execute_sql_file<P: AsRef<Path>>(&mut self, file_path: P) -> Result<(), Box<dyn std::error::Error>> {
        let sql_content = fs::read_to_string(file_path)?;
        self.ensure_connected().await?;

        let pool = self.get_pool()?;
        println!("Executing SQL file with batch execute...");
        sqlx::raw_sql(&sql_content).execute(pool).await?;
        println!("SQL file executed successfully");

        Ok(())
    }

    /// Reset database by dropping all user-defined tables
    pub async fn reset_database(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        self.ensure_connected().await?;

        let pool = self.get_pool()?;
        println!("Dropping all tables...");

        // Get all user-defined tables in the current database
        let table_rows = sqlx::query(
            "SELECT table_name FROM information_schema.tables
             WHERE table_schema = 'public'
             AND table_type = 'BASE TABLE'
             AND table_name NOT IN ('spatial_ref_sys', 'geography_columns', 'geometry_columns', 'raster_columns', 'raster_overviews')"
        )
        .fetch_all(pool)
        .await?;

        let table_count = table_rows.len();

        // Drop each table with CASCADE to handle dependencies
        for row in table_rows {
            let table_name: String = row.get("table_name");
            println!("Dropping table: {}", table_name);

            sqlx::query(&format!("DROP TABLE IF EXISTS {} CASCADE", table_name))
                .execute(pool)
                .await?;
        }

        println!("All tables dropped successfully");

        Ok(format!("Database reset successful. Dropped {} tables.", table_count))
    }

    /// Initialize database schema from SQL files
    pub async fn setup_database(&mut self) -> Result<DatabaseSetupResult, Box<dyn std::error::Error>> {
        self.ensure_connected().await
            .map_err(|e| format!("Failed to connect to database during setup: {}", e))?;

        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let sql_dir = Path::new(manifest_dir).join("sql");
        let mut tables_created = Vec::new();

        let sql_files = ["00_schema.sql"];

        for sql_file in &sql_files {
            let file_path = sql_dir.join(sql_file);
            if file_path.exists() {
                println!("Executing SQL file: {}", sql_file);
                self.execute_sql_file(&file_path).await
                    .map_err(|e| format!("Failed to execute SQL file '{}': {}", sql_file, e))?;
                tables_created.push(sql_file.to_string());
            } else {
                return Err(format!("SQL schema file not found: {}", file_path.display()).into());
            }
        }

        Ok(DatabaseSetupResult {
            success: true,
            message: format!("Database setup completed successfully. Created {} tables/views.", tables_created.len()),
            tables_created,
        })
    }

    /// Test database connection
    pub async fn test_connection(&mut self) -> Result<bool, sqlx::Error> {
        self.ensure_connected().await?;

        let pool = self.get_pool()?;
        let result: (i32,) = sqlx::query_as("SELECT 1")
            .fetch_one(pool)
            .await?;

        Ok(result.0 == 1)
    }

    /// Get comprehensive database statistics
    pub async fn get_database_stats(&mut self) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        self.ensure_connected().await?;

        let pool = self.get_pool()?;
        let mut stats = serde_json::Map::new();

        // Basic counts using constants
        let tables = [AUTHORS_TABLE, PATCHES_TABLE];
        for table in &tables {
            let count: (i64,) = sqlx::query_as(&format!("SELECT COUNT(*) FROM {}", table))
                .fetch_one(pool)
                .await
                .unwrap_or((0,));

            stats.insert((*table).to_string(), serde_json::Value::Number(count.0.into()));
        }

        // Calculate derived statistics
        let total_emails: (i64,) = sqlx::query_as(&format!("SELECT COUNT(*) FROM {}", PATCHES_TABLE))
            .fetch_one(pool)
            .await
            .unwrap_or((0,));

        let unique_authors: (i64,) = sqlx::query_as(&format!("SELECT COUNT(DISTINCT author_id) FROM {}", PATCHES_TABLE))
            .fetch_one(pool)
            .await
            .unwrap_or((0,));

        // Use consistent naming
        stats.insert("total_authors".to_string(), serde_json::Value::Number(unique_authors.0.into()));
        stats.insert("total_patches".to_string(), serde_json::Value::Number(total_emails.0.into()));

        Ok(serde_json::Value::Object(stats))
    }
    
    /// Get all authors with their patch counts, ordered by contribution
    pub async fn get_authors(&mut self) -> Result<Vec<Author>, Box<dyn std::error::Error>> {
        self.ensure_connected().await?;

        let pool = self.get_pool()?;
        let authors = sqlx::query_as::<_, Author>(
            &format!("SELECT author_id, name, email, first_seen, patch_count FROM {} ORDER BY patch_count DESC", AUTHORS_TABLE)
        )
        .fetch_all(pool)
        .await?;

        Ok(authors)
    }

    /// Check which commit hashes already exist in the database using batch queries
    async fn get_existing_commit_hashes(&self, commit_hashes: &[String]) -> Result<HashSet<String>, Box<dyn std::error::Error>> {
        if commit_hashes.is_empty() {
            return Ok(HashSet::new());
        }

        let pool = self.get_pool()?;
        let mut existing_set = HashSet::new();

        // Process in batches to avoid SQL parameter limits
        for batch in commit_hashes.chunks(HASH_CHECK_BATCH_SIZE) {
            let placeholders: Vec<String> = (1..=batch.len()).map(|i| format!("${}", i)).collect();
            let query_str = format!("SELECT commit_hash FROM {} WHERE commit_hash IN ({})", PATCHES_TABLE, placeholders.join(","));

            let mut query = sqlx::query(&query_str);
            for commit_hash in batch {
                query = query.bind(commit_hash);
            }

            let existing_rows = query.fetch_all(pool).await.unwrap_or_default();
            for row in existing_rows {
                existing_set.insert(row.get::<String, _>(0));
            }
        }

        Ok(existing_set)
    }

    /// Search patches by author name or email with author info
    pub async fn search_patches_by_author(&mut self, author_pattern: &str, limit: Option<usize>) -> Result<Vec<(Patch, Author)>, Box<dyn std::error::Error>> {
        self.ensure_connected().await?;

        let pool = self.get_pool()?;
        let search_pattern = format!("%{}%", author_pattern.to_lowercase());

        let limit_clause = limit.map_or(String::new(), |l| format!(" LIMIT {}", l));
        let results = sqlx::query(&format!(
            "SELECT p.patch_id, p.author_id, p.message_id, p.subject, p.sent_at, p.commit_hash, p.body_text, p.is_series, p.series_number, p.series_total, p.created_at,
                    a.author_id, a.name, a.email, a.first_seen, a.patch_count
             FROM {} p
             JOIN {} a ON p.author_id = a.author_id
             WHERE LOWER(a.name) LIKE $1 OR LOWER(a.email) LIKE $1
             ORDER BY p.sent_at DESC{}",
            PATCHES_TABLE, AUTHORS_TABLE, limit_clause
        ))
        .bind(&search_pattern)
        .fetch_all(pool)
        .await?;

        let mut patches_with_authors = Vec::new();
        for row in results {
            let patch = Patch {
                patch_id: row.get(0),
                author_id: row.get(1),
                message_id: row.get(2),
                subject: row.get(3),
                sent_at: row.get(4),
                commit_hash: row.get(5),
                body_text: row.get(6),
                is_series: row.get(7),
                series_number: row.get(8),
                series_total: row.get(9),
                created_at: row.get(10),
            };

            let author = Author {
                author_id: row.get(11),
                name: row.get(12),
                email: row.get(13),
                first_seen: row.get(14),
                patch_count: row.get(15),
            };

            patches_with_authors.push((patch, author));
        }

        Ok(patches_with_authors)
    }

    /// Get patches by author ID, ordered by date
    pub async fn get_patches_by_author(&mut self, author_id: i64) -> Result<Vec<Patch>, Box<dyn std::error::Error>> {
        self.ensure_connected().await?;

        let pool = self.get_pool()?;
        let patches = sqlx::query_as::<_, Patch>(
            &format!("SELECT patch_id, author_id, message_id, subject, sent_at, commit_hash, body_text, is_series, series_number, series_total, created_at
                     FROM {}
                     WHERE author_id = $1
                     ORDER BY sent_at DESC", PATCHES_TABLE)
        )
        .bind(author_id)
        .fetch_all(pool)
        .await?;

        Ok(patches)
    }

    /// Close the database connection pool
    pub async fn close(&mut self) {
        if let Some(pool) = self.pool.take() {
            pool.close().await;
        }
    }

    /// Populate database with author/patch data using optimized parallel batch processing
    ///
    /// This method performs a complete data population cycle:
    /// 1. Retrieves commits from the mail parser
    /// 2. Filters out already processed commits for efficiency
    /// 3. Processes emails in parallel batches with fallback to individual parsing
    /// 4. Inserts authors and patches in optimized batches
    /// 5. Reports progress through the provided callback based on actual database counts
    ///
    /// # Arguments
    /// * `limit` - Optional limit on number of commits to process
    /// * `progress_callback` - Optional callback function for progress reporting
    ///   The callback receives: (current_count, total_commits, status_message)
    ///
    /// # Returns
    /// * `DatabasePopulationResult` containing statistics and any errors encountered
    ///
    /// # Example
    /// ```rust
    /// use mailing_list_parser::database::DatabaseManager;
    ///
    /// async fn populate_with_progress(db: &mut DatabaseManager) -> Result<(), Box<dyn std::error::Error>> {
    ///     let result = db.populate_database(
    ///         Some(1000), // Limit to 1000 commits
    ///         Some(|current, total, status| {
    ///             println!("Progress: {}/{} - {}", current, total, status);
    ///         })
    ///     ).await?;
    ///
    ///     println!("Population complete: {} authors, {} patches",
    ///              result.total_authors_inserted, result.total_emails_inserted);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn populate_database<F>(&mut self, limit: Option<usize>, progress_callback: Option<F>) -> Result<DatabasePopulationResult, Box<dyn std::error::Error>>
    where
        F: Fn(u32, u32, String) + Send + Sync + 'static,
    {
        self.ensure_connected().await?;
        self.setup_database().await?;

        let commits = get_all_commits_with_limit(limit)?;
        let total_commits = commits.len() as u32;

        println!("Starting optimized database population with {} commits", total_commits);

        // Get initial patch count
        let initial_patch_count = self.get_patch_count().await.unwrap_or(0);

        // Start background progress reporter if callback provided
        let pool = self.pool.clone();
        let progress_reporter_handle = if let Some(callback) = progress_callback {
            Some(self.start_progress_reporter(
                total_commits,
                initial_patch_count,
                pool.clone().unwrap(),
                callback
            ).await)
        } else {
            None
        };

        let result = self.process_commit_batches(&commits, total_commits).await;

        // Stop progress reporter
        if let Some(reporter) = progress_reporter_handle {
            reporter.abort();
        }

        println!("Database population completed: {} processed, {} authors, {} patches",
                 result.total_processed, result.total_authors_inserted, result.total_emails_inserted);

        Ok(result)
    }

    /// Process commits in batches with parallel parsing and incremental database insertion
    async fn process_commit_batches(
        &mut self,
        commits: &[String],
        _total_commits: u32
    ) -> DatabasePopulationResult
    {
        let mut processed = 0u32;
        let mut inserted_authors = 0u32;
        let mut inserted_patches = 0u32;
        let mut errors = Vec::new();

        // Filter out commits that already exist in the database
        println!("Checking for existing commits in database...");
        let existing_commits = match self.get_existing_commit_hashes(commits).await {
            Ok(existing) => existing,
            Err(e) => {
                errors.push(format!("Error checking existing commits: {}", e));
                HashSet::new() // Continue with empty set but log the error
            }
        };
        
        let new_commits: Vec<String> = commits.iter()
            .filter(|commit_hash| !existing_commits.contains(*commit_hash))
            .cloned()
            .collect();

        let skipped_count = commits.len() - new_commits.len();
        if skipped_count > 0 {
            println!("Skipping {} existing commits, processing {} new commits", skipped_count, new_commits.len());
        } else {
            println!("No existing commits found, processing all {} commits", new_commits.len());
        }

        if new_commits.is_empty() {
            println!("All commits already exist in database - nothing to process");
            return DatabasePopulationResult {
                success: true,
                total_processed: commits.len() as u32,
                total_authors_inserted: 0,
                total_emails_inserted: 0,
                total_replies_inserted: 0,
                total_commits_inserted: 0,
                errors: vec![],
            };
        }

        // Process all batches concurrently - parse AND insert with limited DB concurrency
        let total_batches = (new_commits.len() + PATCH_BATCH_SIZE - 1) / PATCH_BATCH_SIZE;
        let pool = self.pool.clone().expect("Pool must exist");
        let db_semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_DB_INSERTS));
        let mut batch_handles = Vec::new();
        
        println!("Starting concurrent processing of {} batches (max {} concurrent DB inserts)", 
                 total_batches, MAX_CONCURRENT_DB_INSERTS);
        
        // Spawn tasks that parse AND insert concurrently for each batch
        for (batch_idx, commit_batch) in new_commits.chunks(PATCH_BATCH_SIZE).enumerate() {
            let commit_batch_vec = commit_batch.to_vec();
            let pool_clone = pool.clone();
            let db_sem = Arc::clone(&db_semaphore);
            
            let handle = tokio::spawn(async move {
                let batch_size = commit_batch_vec.len();
                
                // Fetch all commits for this batch using git cat-file (fast batch operation)
                println!("Batch {} fetching {} commits", batch_idx + 1, batch_size);
                let email_contents = match tokio::task::spawn_blocking(move || {
                    crate::git_parser::get_multiple_email_content(&commit_batch_vec)
                }).await {
                    Ok(Ok(contents)) => contents,
                    Ok(Err(e)) => {
                        let err_msg = format!("Failed to fetch batch {}: {}", batch_idx + 1, e);
                        return Err(vec![err_msg]);
                    }
                    Err(e) => {
                        let err_msg = format!("Task error fetching batch {}: {}", batch_idx + 1, e);
                        return Err(vec![err_msg]);
                    }
                };
                
                // Parse all emails immediately after fetching
                println!("Batch {} parsing {} emails", batch_idx + 1, email_contents.len());
                let (parsed_emails, parse_errors) = parse_emails_parallel(email_contents).await;
                
                println!("Batch {} parsed: {} emails, {} errors", batch_idx + 1, parsed_emails.len(), parse_errors.len());
                
                if parsed_emails.is_empty() {
                    return Ok((batch_idx, batch_size, 0, 0, parse_errors));
                }
                
                // Wait for a DB slot (limits concurrent DB operations to avoid overwhelming the pool)
                let _permit = db_sem.acquire().await.expect("Semaphore closed");
                println!("Batch {} got DB slot, inserting {} emails", batch_idx + 1, parsed_emails.len());
                
                // Insert to database
                let result = match Self::insert_batch_to_db(&parsed_emails, &pool_clone).await {
                    Ok((authors_count, patches_count)) => {
                        println!("Batch {} DONE: {} authors, {} patches", batch_idx + 1, authors_count, patches_count);
                        Ok((batch_idx, batch_size, authors_count, patches_count, parse_errors))
                    }
                    Err(e) => {
                        let mut errors = parse_errors;
                        for (commit_hash, _) in &parsed_emails {
                            errors.push(format!("Error inserting commit {}: {}", commit_hash, e));
                        }
                        Err(errors)
                    }
                };
                
                // Permit automatically released when dropped
                result
            });
            
            batch_handles.push(handle);
        }
        
        // Wait for all batches to complete (parsing + insertion happening in parallel)
        let results = future::join_all(batch_handles).await;
        
        for result in results {
            match result {
                Ok(Ok((_, batch_size, authors_count, patches_count, batch_errors))) => {
                    processed += batch_size as u32;
                    inserted_authors += authors_count;
                    inserted_patches += patches_count;
                    errors.extend(batch_errors);
                }
                Ok(Err(batch_errors)) => {
                    errors.extend(batch_errors);
                }
                Err(e) => {
                    errors.push(format!("Batch task failed: {}", e));
                }
            }
        }

        println!("Processing complete: {} processed, {} authors, {} patches", 
                 processed, inserted_authors, inserted_patches);

        DatabasePopulationResult {
            success: errors.is_empty(),
            total_processed: processed,
            total_authors_inserted: inserted_authors,
            total_emails_inserted: inserted_patches,
            total_replies_inserted: 0,
            total_commits_inserted: 0,
            errors,
        }
    }




    /// Get current patch count from database
    async fn get_patch_count(&self) -> Result<u32, Box<dyn std::error::Error>> {
        let pool = self.get_pool()?;
        let count: (i64,) = sqlx::query_as(&format!("SELECT COUNT(*) FROM {}", PATCHES_TABLE))
            .fetch_one(pool)
            .await?;
        Ok(count.0 as u32)
    }

    /// Start a background progress reporter that polls the database for actual progress
    async fn start_progress_reporter<F>(
        &self,
        total_commits: u32,
        initial_count: u32,
        pool: Pool<Postgres>,
        callback: F
    ) -> tokio::task::JoinHandle<()>
    where
        F: Fn(u32, u32, String) + Send + Sync + 'static,
    {
        let callback = Arc::new(callback);

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_millis(PROGRESS_UPDATE_INTERVAL_MS));

            loop {
                interval.tick().await;

                // Poll database for current patch count
                let current_count = match sqlx::query_as::<_, (i64,)>(&format!("SELECT COUNT(*) FROM {}", PATCHES_TABLE))
                    .fetch_one(&pool)
                    .await
                {
                    Ok((count,)) => count as u32,
                    Err(_) => continue, // Skip this tick if database query fails
                };

                // Calculate patches added since start
                let patches_processed = current_count.saturating_sub(initial_count);

                // Report progress
                callback(patches_processed, total_commits, format!("processing ({} patches)", current_count));

                // Stop if we've processed all commits (with some buffer)
                if patches_processed >= total_commits {
                    break;
                }
            }
        })
    }
    
    /// Static method to insert batch to database (used in concurrent tasks)
    async fn insert_batch_to_db(emails: &[(String, EmailInfo)], pool: &Pool<Postgres>) -> Result<(u32, u32), Box<dyn std::error::Error>> {
        if emails.is_empty() {
            return Ok((0, 0));
        }

        // Process authors and patches in optimized batches
        let (existing_authors, authors_to_insert) = Self::collect_authors_static(emails, pool).await?;
        let mut all_authors = existing_authors;
        let new_authors = Self::insert_new_authors_static(&authors_to_insert, pool).await?;

        // Merge new authors into the existing authors map
        all_authors.extend(new_authors);

        let inserted_patches = Self::insert_patches_batch_static(emails, &all_authors, pool).await?;

        Ok((authors_to_insert.len() as u32, inserted_patches))
    }

    /// Batch insert multiple patches and their authors efficiently
    async fn batch_insert_patches(&mut self, emails: &[(String, EmailInfo)]) -> Result<(u32, u32), Box<dyn std::error::Error>> {
        let pool = self.get_pool()?;
        Self::insert_batch_to_db(emails, pool).await
    }

    /// Static method to collect and categorize authors from email data
    async fn collect_authors_static(emails: &[(String, EmailInfo)], pool: &Pool<Postgres>) -> Result<(HashMap<String, i64>, HashMap<String, String>), Box<dyn std::error::Error>> {
        let mut authors_to_insert = HashMap::new();
        let mut existing_authors = HashMap::new();

        for (_, email_info) in emails {
            let email = Self::extract_email_static(&email_info.from);
            let name = Self::extract_name_static(&email_info.from);

            if !existing_authors.contains_key(&email) && !authors_to_insert.contains_key(&email) {
                // Check if author already exists
                let existing: Option<(i64,)> = sqlx::query_as(&format!("SELECT author_id FROM {} WHERE email = $1", AUTHORS_TABLE))
                    .bind(&email)
                    .fetch_optional(pool)
                    .await?;

                if let Some((id,)) = existing {
                    existing_authors.insert(email.clone(), id);
                } else {
                    authors_to_insert.insert(email.clone(), name.to_string());
                }
            }
        }

        Ok((existing_authors, authors_to_insert))
    }

    /// Collect and categorize authors from email data
    async fn collect_authors(&self, emails: &[(String, EmailInfo)], pool: &Pool<Postgres>) -> Result<(HashMap<String, i64>, HashMap<String, String>), Box<dyn std::error::Error>> {
        Self::collect_authors_static(emails, pool).await
    }

    /// Static method to insert new authors in batch with conflict resolution
    async fn insert_new_authors_static(authors_to_insert: &HashMap<String, String>, pool: &Pool<Postgres>) -> Result<HashMap<String, i64>, Box<dyn std::error::Error>> {
        if authors_to_insert.is_empty() {
            return Ok(HashMap::new());
        }

        // Use batch insert with ON CONFLICT for better performance
        let mut query = format!("INSERT INTO {} (name, email) VALUES ", AUTHORS_TABLE);
        let mut values = Vec::new();
        let mut param_count = 1;

        for (i, (email, name)) in authors_to_insert.iter().enumerate() {
            if i > 0 {
                query.push(',');
            }
            query.push_str(&format!("(${}, ${})", param_count, param_count + 1));
            values.push(name.as_str());
            values.push(email.as_str());
            param_count += AUTHOR_INSERT_PARAMS_PER_ROW;
        }

        query.push_str(" ON CONFLICT (email) DO UPDATE SET name = EXCLUDED.name RETURNING author_id, email");

        let mut insert_query = sqlx::query(&query);
        for value in values {
            insert_query = insert_query.bind(value);
        }

        let new_author_rows = insert_query.fetch_all(pool).await?;
        let mut new_authors = HashMap::new();

        for (i, row) in new_author_rows.iter().enumerate() {
            let email = authors_to_insert.keys().nth(i).unwrap();
            let author_id: i64 = row.get(0);
            new_authors.insert(email.clone(), author_id);
        }

        Ok(new_authors)
    }

    /// Insert new authors in batch with conflict resolution
    async fn insert_new_authors(&self, authors_to_insert: &HashMap<String, String>, pool: &Pool<Postgres>) -> Result<HashMap<String, i64>, Box<dyn std::error::Error>> {
        Self::insert_new_authors_static(authors_to_insert, pool).await
    }

    /// Static method to insert patches in optimized batches
    async fn insert_patches_batch_static(emails: &[(String, EmailInfo)], existing_authors: &HashMap<String, i64>, pool: &Pool<Postgres>) -> Result<u32, Box<dyn std::error::Error>> {
        let patches_data = Self::prepare_patches_data_static(emails, existing_authors)?;

        if patches_data.is_empty() {
            return Ok(0);
        }

        let mut inserted_patches = 0u32;

        // Use batch insert for patches (optimized for large datasets)
        for patch_batch in patches_data.chunks(PATCH_BATCH_SIZE) {
            let batch_count = Self::execute_patch_batch_insert_static(patch_batch, pool).await?;
            inserted_patches += batch_count;
        }

        Ok(inserted_patches)
    }

    /// Insert patches in optimized batches
    async fn insert_patches_batch(&self, emails: &[(String, EmailInfo)], existing_authors: &HashMap<String, i64>, pool: &Pool<Postgres>) -> Result<u32, Box<dyn std::error::Error>> {
        Self::insert_patches_batch_static(emails, existing_authors, pool).await
    }

    /// Static method to prepare patch data for insertion
    fn prepare_patches_data_static(emails: &[(String, EmailInfo)], existing_authors: &HashMap<String, i64>) -> Result<Vec<PatchData>, Box<dyn std::error::Error>> {
        let mut patches_data = Vec::new();

        for (commit_hash, email_info) in emails {
            let email = Self::extract_email_static(&email_info.from);
            let author_id = *existing_authors.get(&email)
                .ok_or_else(|| format!("Author not found for email: {}", email))?;

            // Parse date with multiple format fallbacks
            let parsed_date = Self::parse_email_date_static(&email_info.date)?;

            // Detect if it's a patch series
            let (is_series, series_number, series_total) = Self::detect_patch_series_static(&email_info.subject);

            patches_data.push(PatchData {
                author_id,
                message_id: email_info.message_id.clone(),
                subject: email_info.subject.clone(),
                sent_at: parsed_date,
                commit_hash: commit_hash.clone(),
                body_text: Some(email_info.body.clone()),
                is_series,
                series_number,
                series_total,
            });
        }

        Ok(patches_data)
    }

    /// Prepare patch data for insertion
    fn prepare_patches_data(&self, emails: &[(String, EmailInfo)], existing_authors: &HashMap<String, i64>) -> Result<Vec<PatchData>, Box<dyn std::error::Error>> {
        Self::prepare_patches_data_static(emails, existing_authors)
    }

    /// Static method to execute batch insert for a chunk of patches
    async fn execute_patch_batch_insert_static(patch_batch: &[PatchData], pool: &Pool<Postgres>) -> Result<u32, Box<dyn std::error::Error>> {
        let mut query = format!("INSERT INTO {} (author_id, message_id, subject, sent_at, commit_hash, body_text, is_series, series_number, series_total) VALUES ", PATCHES_TABLE);
        let mut param_count = 1;

        for (i, _) in patch_batch.iter().enumerate() {
            if i > 0 {
                query.push(',');
            }
            query.push_str(&format!("(${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${})",
                                   param_count, param_count + 1, param_count + 2, param_count + 3,
                                   param_count + 4, param_count + 5, param_count + 6, param_count + 7, param_count + 8));
            param_count += PATCH_INSERT_PARAMS_PER_ROW;
        }

        query.push_str(" ON CONFLICT (message_id) DO NOTHING");

        let mut insert_query = sqlx::query(&query);

        for patch_data in patch_batch {
            insert_query = insert_query
                .bind(patch_data.author_id)
                .bind(&patch_data.message_id)
                .bind(&patch_data.subject)
                .bind(&patch_data.sent_at)
                .bind(&patch_data.commit_hash)
                .bind(&patch_data.body_text)
                .bind(&patch_data.is_series)
                .bind(&patch_data.series_number)
                .bind(&patch_data.series_total);
        }

        insert_query.execute(pool).await?;
        Ok(patch_batch.len() as u32)
    }

    /// Execute batch insert for a chunk of patches
    async fn execute_patch_batch_insert(&self, patch_batch: &[PatchData], pool: &Pool<Postgres>) -> Result<u32, Box<dyn std::error::Error>> {
        Self::execute_patch_batch_insert_static(patch_batch, pool).await
    }

    /// Static method to extract email address from email header
    fn extract_email_static(from_header: &str) -> String {
        let email_regex = Regex::new(r"<([^>]+)>").unwrap();
        if let Some(captures) = email_regex.captures(from_header) {
            captures.get(1).unwrap().as_str().to_string()
        } else {
            from_header.to_string()
        }
    }

    /// Static method to extract name from email header
    fn extract_name_static(from_header: &str) -> String {
        from_header.split('<').next().unwrap_or(from_header).trim().to_string()
    }

    /// Static method to parse email date with multiple format support
    fn parse_email_date_static(date_str: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
        DateTime::parse_from_rfc2822(date_str)
            .or_else(|_| DateTime::parse_from_rfc3339(date_str))
            .map(|dt| dt.with_timezone(&Utc))
            .or_else(|_| {
                NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S")
                    .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc))
            })
    }

    /// Static method to detect if email subject indicates a patch series
    fn detect_patch_series_static(subject: &str) -> (bool, Option<i32>, Option<i32>) {
        let series_regex = Regex::new(r"\[.*?(\d+)/(\d+)\]").unwrap();
        if let Some(captures) = series_regex.captures(subject) {
            let num: i32 = captures.get(1).unwrap().as_str().parse().unwrap_or(0);
            let total: i32 = captures.get(2).unwrap().as_str().parse().unwrap_or(0);
            (true, Some(num), Some(total))
        } else {
            (false, None, None)
        }
    }

    /// Extract email address from email header
    fn extract_email(&self, from_header: &str) -> String {
        Self::extract_email_static(from_header)
    }

    /// Extract name from email header
    fn extract_name(&self, from_header: &str) -> String {
        Self::extract_name_static(from_header)
    }

    /// Parse email date with multiple format support
    fn parse_email_date(&self, date_str: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
        Self::parse_email_date_static(date_str)
    }

    /// Detect if email subject indicates a patch series
    fn detect_patch_series(&self, subject: &str) -> (bool, Option<i32>, Option<i32>) {
        Self::detect_patch_series_static(subject)
    }

}

#[derive(Debug, Serialize)]
pub struct DatabasePopulationResult {
    pub success: bool,
    pub total_processed: u32,
    pub total_authors_inserted: u32,
    pub total_emails_inserted: u32,
    pub total_replies_inserted: u32,
    pub total_commits_inserted: u32,
    pub errors: Vec<String>,
}