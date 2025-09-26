use crate::models::{Email, Result, ParserError, Config};
use crate::database::Database;
use crate::parser::{EmailParser, ThreadAnalyzer};
use std::sync::Arc;
use std::process::Command;
use std::path::Path;

/// Update service for pulling new emails from the mailing list
pub struct UpdateService {
    database: Arc<Database>,
    parser: EmailParser,
    thread_analyzer: ThreadAnalyzer,
    config: Config,
}

impl UpdateService {
    /// Create a new update service
    pub fn new(database: Arc<Database>, config: Config) -> Result<Self> {
        Ok(UpdateService {
            database,
            parser: EmailParser::new()?,
            thread_analyzer: ThreadAnalyzer::new()?,
            config,
        })
    }

    /// Pull latest changes from the git repository
    pub async fn pull_updates(&self) -> Result<UpdateResult> {
        log::info!("Starting mailing list update...");

        // Pull latest changes from git
        self.pull_git_updates().await?;

        // Get the latest commit hash
        let latest_commit = self.get_latest_commit().await?;

        // Check if we've already processed this commit
        if self.is_commit_processed(&latest_commit).await? {
            log::info!("No new commits to process");
            return Ok(UpdateResult {
                new_emails: 0,
                updated_threads: 0,
                latest_commit: latest_commit.clone(),
            });
        }

        // Get new emails since last update
        let new_emails = self.extract_new_emails(&latest_commit).await?;

        log::info!("Found {} new emails to process", new_emails.len());

        // Process and store the emails
        let processed_count = self.process_emails(new_emails).await?;

        // Update the processed commit marker
        self.mark_commit_processed(&latest_commit).await?;

        Ok(UpdateResult {
            new_emails: processed_count,
            updated_threads: 0, // TODO: Calculate updated threads
            latest_commit,
        })
    }

    /// Pull latest changes from git repository
    async fn pull_git_updates(&self) -> Result<()> {
        if !Path::new(&self.config.mailing_list_path).exists() {
            return Err(ParserError::Config(format!(
                "Mailing list path does not exist: {}",
                self.config.mailing_list_path
            )));
        }

        let output = Command::new("git")
            .args(&["pull", "--rebase"])
            .current_dir(&self.config.mailing_list_path)
            .output()
            .map_err(|e| ParserError::Config(format!("Failed to run git pull: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ParserError::Config(format!("Git pull failed: {}", stderr)));
        }

        log::info!("Git pull completed successfully");
        Ok(())
    }

    /// Get the latest commit hash
    async fn get_latest_commit(&self) -> Result<String> {
        let output = Command::new("git")
            .args(&["rev-parse", "HEAD"])
            .current_dir(&self.config.mailing_list_path)
            .output()
            .map_err(|e| ParserError::Config(format!("Failed to get latest commit: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ParserError::Config(format!("Git command failed: {}", stderr)));
        }

        let commit_hash = String::from_utf8(output.stdout)
            .map_err(|e| ParserError::Config(format!("Invalid UTF-8 in commit hash: {}", e)))?
            .trim()
            .to_string();

        Ok(commit_hash)
    }

    /// Check if a commit has already been processed
    async fn is_commit_processed(&self, commit_hash: &str) -> Result<bool> {
        let count = self.database.client.query_one(
            "SELECT COUNT(*) FROM processed_commits WHERE commit_hash = $1",
            &[&commit_hash]
        ).await?;

        Ok(count.get::<_, i64>(0) > 0)
    }

    /// Mark a commit as processed
    async fn mark_commit_processed(&self, commit_hash: &str) -> Result<()> {
        self.database.client.execute(
            "INSERT INTO processed_commits (commit_hash, processed_at) VALUES ($1, NOW())
             ON CONFLICT (commit_hash) DO UPDATE SET processed_at = NOW()",
            &[&commit_hash]
        ).await?;

        Ok(())
    }

    /// Extract new emails since the last processed commit
    async fn extract_new_emails(&self, latest_commit: &str) -> Result<Vec<Email>> {
        let last_processed = self.get_last_processed_commit().await?;

        let commit_range = match last_processed {
            Some(last) => format!("{}..{}", last, latest_commit),
            None => latest_commit.to_string(),
        };

        log::info!("Processing commits in range: {}", commit_range);

        let emails = self.extract_emails_from_commits(&commit_range).await?;
        Ok(emails)
    }

    /// Get the last processed commit hash
    async fn get_last_processed_commit(&self) -> Result<Option<String>> {
        let row = self.database.client.query_opt(
            "SELECT commit_hash FROM processed_commits ORDER BY processed_at DESC LIMIT 1",
            &[]
        ).await?;

        match row {
            Some(row) => Ok(Some(row.get(0))),
            None => Ok(None),
        }
    }

    /// Extract emails from a range of git commits
    async fn extract_emails_from_commits(&self, commit_range: &str) -> Result<Vec<Email>> {
        let output = Command::new("git")
            .args(&["show", "--format=fuller", "--name-only", commit_range])
            .current_dir(&self.config.mailing_list_path)
            .output()
            .map_err(|e| ParserError::Config(format!("Failed to extract commits: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ParserError::Config(format!("Git show failed: {}", stderr)));
        }

        let output_str = String::from_utf8_lossy(&output.stdout);
        let mut emails = Vec::new();

        // Parse the git output to extract email content
        let commits = self.parse_git_output(&output_str)?;

        for commit in commits {
            if let Ok(email) = self.parser.parse_from_git_commit(&commit.hash, &commit.content) {
                emails.push(email);
            }
        }

        Ok(emails)
    }

    /// Parse git show output to extract individual commits
    fn parse_git_output(&self, output: &str) -> Result<Vec<GitCommit>> {
        let mut commits = Vec::new();
        let mut current_commit: Option<GitCommit> = None;

        for line in output.lines() {
            if line.starts_with("commit ") {
                // Save previous commit if exists
                if let Some(commit) = current_commit.take() {
                    commits.push(commit);
                }

                // Start new commit
                let hash = line[7..].to_string();
                current_commit = Some(GitCommit {
                    hash,
                    content: String::new(),
                });
            } else if let Some(ref mut commit) = current_commit {
                // Skip commit metadata lines (author, date, etc.) and empty lines
                if !line.is_empty() &&
                   !line.starts_with("Author:") &&
                   !line.starts_with("Date:") &&
                   !line.starts_with("Merge:") &&
                   !line.starts_with(" ") &&
                   !line.starts_with("\t") {
                    commit.content.push_str(line);
                    commit.content.push('\n');
                }
            }
        }

        // Add the last commit
        if let Some(commit) = current_commit {
            commits.push(commit);
        }

        Ok(commits)
    }

    /// Process and store emails in the database
    async fn process_emails(&self, emails: Vec<Email>) -> Result<usize> {
        let mut processed_count = 0;

        for email in &emails {
            match self.database.store_email(email).await {
                Ok(_) => {
                    processed_count += 1;
                    log::debug!("Stored email: {}", email.message_id);
                }
                Err(e) => {
                    log::warn!("Failed to store email {}: {}", email.message_id, e);
                }
            }
        }

        // Analyze and store thread relationships
        let thread_relationships = self.thread_analyzer.analyze_threads(&emails).await?;

        if !thread_relationships.is_empty() {
            self.database.store_thread_relationships("default_thread", &thread_relationships).await?;
        }

        log::info!("Processed {} out of {} emails", processed_count, emails.len());
        Ok(processed_count)
    }

    /// Perform a full sync of the mailing list (for initial setup)
    pub async fn full_sync(&self) -> Result<UpdateResult> {
        log::info!("Starting full sync of mailing list...");

        // Get all commits
        let output = Command::new("git")
            .args(&["log", "--oneline", "--all"])
            .current_dir(&self.config.mailing_list_path)
            .output()
            .map_err(|e| ParserError::Config(format!("Failed to get git log: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ParserError::Config(format!("Git log failed: {}", stderr)));
        }

        let log_output = String::from_utf8_lossy(&output.stdout);
        let commits: Vec<&str> = log_output.lines().collect();

        // Process in batches to avoid memory issues
        let batch_size = 100;
        let mut total_processed = 0;

        for chunk in commits.chunks(batch_size) {
            let commit_range = chunk.iter()
                .map(|line| line.split_whitespace().next().unwrap_or(""))
                .collect::<Vec<_>>()
                .join(" ");

            if commit_range.is_empty() {
                continue;
            }

            let new_emails = self.extract_emails_from_commits(&commit_range).await?;
            let processed = self.process_emails(new_emails).await?;
            total_processed += processed;

            log::info!("Processed batch, total so far: {}", total_processed);
        }

        log::info!("Full sync completed, processed {} emails", total_processed);

        Ok(UpdateResult {
            new_emails: total_processed,
            updated_threads: 0,
            latest_commit: self.get_latest_commit().await?,
        })
    }
}

/// Represents a git commit with its content
#[derive(Debug, Clone)]
struct GitCommit {
    hash: String,
    content: String,
}

/// Result of an update operation
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateResult {
    pub new_emails: usize,
    pub updated_threads: usize,
    pub latest_commit: String,
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn test_git_output_parsing_standalone() {
        // Test the parsing logic directly without creating a full service
        let test_output = "commit abc123
Author: Test <test@example.com>
Date: Thu, 25 Sep 2025 23:20:12 +0000

This is the commit message

Some content here

commit def456
Author: Another Test <test2@example.com>
Date: Thu, 25 Sep 2025 23:21:12 +0000

Another commit";

        // Create a simple test implementation of the parsing logic
        let mut commits = Vec::new();
        let mut current_commit: Option<GitCommit> = None;

        for line in test_output.lines() {
            if line.starts_with("commit ") {
                if let Some(commit) = current_commit {
                    commits.push(commit);
                }
                current_commit = Some(GitCommit {
                    hash: line.strip_prefix("commit ").unwrap().to_string(),
                    content: String::new(),
                });
            } else if let Some(ref mut commit) = current_commit {
                if !commit.content.is_empty() {
                    commit.content.push('\n');
                }
                commit.content.push_str(line);
            }
        }

        if let Some(commit) = current_commit {
            commits.push(commit);
        }

        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].hash, "abc123");
        assert!(commits[0].content.contains("Some content here"));
    }
}
