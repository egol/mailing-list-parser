use std::fs;
use std::path::Path;
use sqlx::Row;
use crate::database::{DatabaseManager, DatabaseSetupResult};

impl DatabaseManager {
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
}

