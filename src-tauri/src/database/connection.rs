use sqlx::postgres::PgPoolOptions;
use sqlx::{Pool, Postgres};
use crate::database::config::*;
use crate::database::DatabaseManager;

impl DatabaseManager {
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
    pub async fn ensure_connected(&mut self) -> Result<(), sqlx::Error> {
        if self.pool.is_none() {
            self.connect().await?;
        }
        Ok(())
    }

    /// Get a reference to the connection pool if connected
    pub fn get_pool(&self) -> Result<&Pool<Postgres>, sqlx::Error> {
        self.pool.as_ref().ok_or_else(|| sqlx::Error::Configuration("Not connected to database".into()))
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

    /// Close the database connection pool
    pub async fn close(&mut self) {
        if let Some(pool) = self.pool.take() {
            pool.close().await;
        }
    }
}

