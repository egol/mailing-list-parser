use sqlx::Row;
use crate::database::{DatabaseManager, Author, Patch};

impl DatabaseManager {
    /// Get comprehensive database statistics
    pub async fn get_database_stats(&mut self) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        self.ensure_connected().await?;

        let pool = self.get_pool()?;
        let mut stats = serde_json::Map::new();

        // Basic counts
        let tables = ["authors", "patches"];
        for table in &tables {
            let count: (i64,) = sqlx::query_as(&format!("SELECT COUNT(*) FROM {}", table))
                .fetch_one(pool)
                .await
                .unwrap_or((0,));

            stats.insert((*table).to_string(), serde_json::Value::Number(count.0.into()));
        }

        // Calculate derived statistics
        let total_emails: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM patches")
            .fetch_one(pool)
            .await
            .unwrap_or((0,));

        let unique_authors: (i64,) = sqlx::query_as("SELECT COUNT(DISTINCT author_id) FROM patches")
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
            "SELECT author_id, first_name, last_name, display_name, first_seen, patch_count FROM authors ORDER BY patch_count DESC"
        )
        .fetch_all(pool)
        .await?;
        
        Ok(authors)
    }

    /// Search patches by author name or email with author info
    pub async fn search_patches_by_author(&mut self, author_pattern: &str, limit: Option<usize>) -> Result<Vec<(Patch, Author)>, Box<dyn std::error::Error>> {
        self.ensure_connected().await?;

        let pool = self.get_pool()?;
        let search_pattern = format!("%{}%", author_pattern.to_lowercase());

        let limit_clause = limit.map_or(String::new(), |l| format!(" LIMIT {}", l));
        let results = sqlx::query(&format!(
            "SELECT p.patch_id, p.author_id, p.email_id, p.message_id, p.subject, p.sent_at, p.commit_hash, p.body_text, p.is_series, p.series_number, p.series_total, p.created_at,
                    a.author_id, a.first_name, a.last_name, a.display_name, a.first_seen, a.patch_count
             FROM patches p
             JOIN authors a ON p.author_id = a.author_id
             LEFT JOIN author_emails e ON p.email_id = e.email_id
             WHERE LOWER(a.display_name) LIKE $1 OR LOWER(a.first_name) LIKE $1 OR LOWER(a.last_name) LIKE $1 OR LOWER(e.email) LIKE $1
             ORDER BY p.sent_at DESC{}",
            limit_clause
        ))
        .bind(&search_pattern)
        .fetch_all(pool)
        .await?;

        let mut patches_with_authors = Vec::new();
        for row in results {
            let patch = Patch {
                patch_id: row.get(0),
                author_id: row.get(1),
                email_id: row.get(2),
                message_id: row.get(3),
                subject: row.get(4),
                sent_at: row.get(5),
                commit_hash: row.get(6),
                body_text: row.get(7),
                is_series: row.get(8),
                series_number: row.get(9),
                series_total: row.get(10),
                created_at: row.get(11),
            };

            let author = Author {
                author_id: row.get(12),
                first_name: row.get(13),
                last_name: row.get(14),
                display_name: row.get(15),
                first_seen: row.get(16),
                patch_count: row.get(17),
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
            "SELECT patch_id, author_id, email_id, message_id, subject, sent_at, commit_hash, body_text, is_series, series_number, series_total, created_at
             FROM patches
             WHERE author_id = $1
             ORDER BY sent_at DESC"
        )
        .bind(author_id)
        .fetch_all(pool)
        .await?;

        Ok(patches)
    }

    /// Refresh patch_count for all authors (run after bulk insertion)
    pub(crate) async fn refresh_author_patch_counts(&self) -> Result<(), Box<dyn std::error::Error>> {
        let pool = self.get_pool()?;
        println!("Refreshing author patch counts...");
        
        sqlx::query(
            "UPDATE authors a SET patch_count = (SELECT COUNT(*) FROM patches p WHERE p.author_id = a.author_id)"
        )
        .execute(pool)
        .await?;
        
        println!("Author patch counts refreshed");
        Ok(())
    }
}

