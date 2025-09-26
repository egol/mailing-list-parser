use crate::models::{SearchCriteria, SearchResults, Email, Result};
use crate::database::Database;
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// Search service for querying the mailing list database
pub struct SearchService {
    database: Arc<Database>,
}

impl SearchService {
    /// Create a new search service
    pub fn new(database: Arc<Database>) -> Self {
        SearchService { database }
    }

    /// Search emails based on criteria
    pub async fn search(&self, criteria: SearchCriteria) -> Result<SearchResults> {
        // Get total count for pagination info
        let total_count = self.database.get_email_count(&criteria).await?;

        // Get the actual email results
        let emails = self.database.search_emails(&criteria).await?;

        // Determine if there are more results
        let _limit = criteria.limit.unwrap_or(50) as i64;
        let offset = criteria.offset.unwrap_or(0) as i64;
        let has_more = offset + (emails.len() as i64) < total_count;

        Ok(SearchResults {
            emails,
            total_count,
            has_more,
        })
    }

    /// Advanced search with multiple filters
    pub async fn advanced_search(&self, query: Option<String>, author: Option<String>,
                                subject: Option<String>, date_from: Option<DateTime<Utc>>,
                                date_to: Option<DateTime<Utc>>, is_patch: Option<bool>,
                                limit: Option<i32>, offset: Option<i32>) -> Result<SearchResults> {
        let criteria = SearchCriteria {
            query,
            author,
            subject_contains: subject,
            date_from,
            date_to,
            is_patch,
            patch_series: None,
            limit,
            offset,
        };

        self.search(criteria).await
    }

    /// Search for patches by series
    pub async fn search_by_patch_series(&self, series_id: &str) -> Result<Vec<Email>> {
        let criteria = SearchCriteria {
            query: Some(series_id.to_string()),
            author: None,
            subject_contains: None,
            date_from: None,
            date_to: None,
            is_patch: Some(true),
            patch_series: Some(series_id.to_string()),
            limit: Some(100),
            offset: Some(0),
        };

        Ok(self.database.search_emails(&criteria).await?)
    }

    /// Get recent emails
    pub async fn get_recent(&self, limit: i32) -> Result<Vec<Email>> {
        let criteria = SearchCriteria {
            query: None,
            author: None,
            subject_contains: None,
            date_from: None,
            date_to: None,
            is_patch: None,
            patch_series: None,
            limit: Some(limit),
            offset: Some(0),
        };

        Ok(self.database.search_emails(&criteria).await?)
    }

    /// Get emails by author
    pub async fn get_by_author(&self, author: &str, limit: Option<i32>) -> Result<Vec<Email>> {
        let criteria = SearchCriteria {
            query: None,
            author: Some(author.to_string()),
            subject_contains: None,
            date_from: None,
            date_to: None,
            is_patch: None,
            patch_series: None,
            limit,
            offset: Some(0),
        };

        Ok(self.database.search_emails(&criteria).await?)
    }

    /// Get patch emails only
    pub async fn get_patches(&self, limit: Option<i32>) -> Result<Vec<Email>> {
        let criteria = SearchCriteria {
            query: None,
            author: None,
            subject_contains: None,
            date_from: None,
            date_to: None,
            is_patch: Some(true),
            patch_series: None,
            limit,
            offset: Some(0),
        };

        Ok(self.database.search_emails(&criteria).await?)
    }

    /// Search with text relevance scoring
    pub async fn search_with_relevance(&self, query: &str, limit: Option<i32>) -> Result<Vec<(Email, f64)>> {
        let criteria = SearchCriteria {
            query: Some(query.to_string()),
            author: None,
            subject_contains: None,
            date_from: None,
            date_to: None,
            is_patch: None,
            patch_series: None,
            limit: Some(limit.unwrap_or(50)),
            offset: Some(0),
        };

        let emails = self.database.search_emails(&criteria).await?;

        // Simple relevance scoring based on position and frequency
        let mut scored_emails = Vec::new();
        for email in emails {
            let score = self.calculate_relevance_score(&email, query);
            scored_emails.push((email, score));
        }

        // Sort by relevance score (descending)
        scored_emails.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        Ok(scored_emails)
    }

    /// Calculate relevance score for search results
    fn calculate_relevance_score(&self, email: &Email, query: &str) -> f64 {
        let query_lower = query.to_lowercase();
        let mut score = 0.0;

        // Subject match gets higher weight
        if email.subject.to_lowercase().contains(&query_lower) {
            score += 10.0;
            // Bonus for exact matches in subject
            if email.subject.to_lowercase() == query_lower {
                score += 5.0;
            }
        }

        // Body matches
        let body_lower = email.body.to_lowercase();
        let query_count = body_lower.matches(&query_lower).count() as f64;
        score += query_count * 2.0;

        // Boost for patches if searching for patches
        if query_lower.contains("patch") && email.is_patch {
            score += 3.0;
        }

        // Boost recent emails
        let days_since = (Utc::now() - email.date).num_days();
        if days_since < 7 {
            score += 2.0;
        } else if days_since < 30 {
            score += 1.0;
        }

        // Penalize very old emails
        if days_since > 365 {
            score *= 0.5;
        }

        score
    }

    /// Get thread for a specific email
    pub async fn get_email_thread(&self, message_id: &str) -> Result<Option<crate::models::Thread>> {
        self.database.get_thread(message_id).await
    }

    /// Get statistics about the mailing list
    pub async fn get_statistics(&self) -> Result<MailListStats> {
        let total_emails = self.database.get_email_count(&SearchCriteria {
            query: None,
            author: None,
            subject_contains: None,
            date_from: None,
            date_to: None,
            is_patch: None,
            patch_series: None,
            limit: None,
            offset: None,
        }).await?;

        let patch_emails = self.database.get_email_count(&SearchCriteria {
            query: None,
            author: None,
            subject_contains: None,
            date_from: None,
            date_to: None,
            is_patch: Some(true),
            patch_series: None,
            limit: None,
            offset: None,
        }).await?;

        let recent_emails = self.database.get_email_count(&SearchCriteria {
            query: None,
            author: None,
            subject_contains: None,
            date_from: Some(Utc::now() - chrono::Duration::days(30)),
            date_to: Some(Utc::now()),
            is_patch: None,
            patch_series: None,
            limit: None,
            offset: None,
        }).await?;

        Ok(MailListStats {
            total_emails,
            patch_emails,
            recent_emails,
        })
    }
}

/// Statistics about the mailing list
#[derive(Debug, Clone, serde::Serialize)]
pub struct MailListStats {
    pub total_emails: i64,
    pub patch_emails: i64,
    pub recent_emails: i64,
}

#[cfg(test)]
mod tests {
}
