use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use sqlx::Pool;
use tokio::sync::mpsc;
use tokio::time::interval;
use futures::future;
use crate::database::{DatabaseManager, DatabasePopulationResult};
use crate::database::config::*;
use crate::database::patches::PatchOps;
use crate::git_parser::get_all_commits_with_limit;
use crate::mail_parser::parse_emails_parallel;

impl DatabaseManager {
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

    /// Process commits with parallel parsing and sequential optimized DB insertion
    /// Architecture: Multiple parser tasks -> Channel -> Single DB inserter task
    async fn process_commit_batches(
        &mut self,
        commits: &[String],
        _total_commits: u32
    ) -> DatabasePopulationResult
    {
        let mut errors = Vec::new();

        // Filter out commits that already exist in the database
        println!("Checking for existing commits in database...");
        let pool = self.get_pool().unwrap();
        let existing_commits = match PatchOps::get_existing_commit_hashes(commits, pool).await {
            Ok(existing) => existing,
            Err(e) => {
                errors.push(format!("Error checking existing commits: {}", e));
                HashSet::new()
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
                errors: vec![],
            };
        }

        // Create channel for parsed emails
        let (tx, mut rx) = mpsc::channel::<(Vec<(String, crate::mail_parser::EmailInfo)>, Vec<String>)>(CHANNEL_BUFFER_SIZE);
        
        let total_batches = (new_commits.len() + PARSE_BATCH_SIZE - 1) / PARSE_BATCH_SIZE;
        println!("Starting parallel parsing of {} batches, sequential DB insertion", total_batches);
        
        // Spawn parallel parser tasks
        let mut parser_handles = Vec::new();
        for (batch_idx, commit_batch) in new_commits.chunks(PARSE_BATCH_SIZE).enumerate() {
            let commit_batch_vec = commit_batch.to_vec();
            let tx_clone = tx.clone();
            
            let handle = tokio::spawn(async move {
                // Fetch commits
                println!("Batch {} fetching {} commits", batch_idx + 1, commit_batch_vec.len());
                let (email_contents, metadata_list) = match tokio::task::spawn_blocking(move || {
                    // Fetch email contents
                    let contents = crate::git_parser::get_multiple_email_content(&commit_batch_vec)?;
                    // Extract commit hashes for metadata lookup
                    let commit_hashes: Vec<String> = contents.iter().map(|(hash, _)| hash.clone()).collect();
                    // Fetch commit metadata
                    let metadata = crate::git_parser::get_commit_metadata(&commit_hashes)?;
                    Ok::<_, crate::git_parser::ParseError>((contents, metadata))
                }).await {
                    Ok(Ok((contents, metadata))) => (contents, metadata),
                    Ok(Err(e)) => {
                        eprintln!("Failed to fetch batch {}: {}", batch_idx + 1, e);
                        return;
                    }
                    Err(e) => {
                        eprintln!("Task error fetching batch {}: {}", batch_idx + 1, e);
                        return;
                    }
                };
                
                // Combine email contents with metadata
                let emails_with_metadata: Vec<(String, String, crate::git_parser::CommitMetadata)> = email_contents
                    .into_iter()
                    .zip(metadata_list.into_iter())
                    .map(|((hash, content), metadata)| (hash, content, metadata))
                    .collect();
                
                // Parse emails
                println!("Batch {} parsing {} emails", batch_idx + 1, emails_with_metadata.len());
                let (parsed_emails, parse_errors) = parse_emails_parallel(emails_with_metadata).await;
                println!("Batch {} parsed: {} emails, {} errors", batch_idx + 1, parsed_emails.len(), parse_errors.len());
                
                // Send to DB inserter via channel
                if tx_clone.send((parsed_emails, parse_errors)).await.is_err() {
                    eprintln!("Batch {}: Channel closed, DB inserter stopped", batch_idx + 1);
                }
            });
            
            parser_handles.push(handle);
        }
        
        // Drop original sender so channel closes when all parsers finish
        drop(tx);
        
        // Spawn single DB inserter task (sequential, optimized batching)
        let pool = self.pool.clone().expect("Pool must exist");
        let db_handle = tokio::spawn(async move {
            let mut all_emails = Vec::new();
            let mut all_errors = Vec::new();
            let mut processed = 0u32;
            
            // Collect all parsed results from channel
            while let Some((parsed_emails, parse_errors)) = rx.recv().await {
                processed += parsed_emails.len() as u32;
                all_emails.extend(parsed_emails);
                all_errors.extend(parse_errors);
            }
            
            println!("All parsing complete. Inserting {} emails to database in optimized batches...", all_emails.len());
            
            let mut inserted_authors = 0u32;
            let mut inserted_patches = 0u32;
            
            // Insert in large optimized batches (sequential to avoid deadlocks)
            for (batch_num, batch) in all_emails.chunks(DB_INSERT_BATCH_SIZE).enumerate() {
                println!("Inserting batch {}: {} emails", batch_num + 1, batch.len());
                match PatchOps::insert_batch_to_db(batch, &pool).await {
                    Ok((authors_count, patches_count)) => {
                        inserted_authors += authors_count;
                        inserted_patches += patches_count;
                        println!("Batch {} inserted: {} authors, {} patches", batch_num + 1, authors_count, patches_count);
                    }
                    Err(e) => {
                        for (commit_hash, _) in batch {
                            all_errors.push(format!("Error inserting commit {}: {}", commit_hash, e));
                        }
                    }
                }
            }
            
            (processed, inserted_authors, inserted_patches, all_errors)
        });
        
        // Wait for all parsers to complete
        future::join_all(parser_handles).await;
        
        // Wait for DB inserter to complete
        let (processed, inserted_authors, inserted_patches, db_errors) = db_handle.await
            .unwrap_or((0, 0, 0, vec!["DB inserter task failed".to_string()]));
        
        errors.extend(db_errors);

        println!("Processing complete: {} processed, {} authors, {} patches", 
                 processed, inserted_authors, inserted_patches);

        // Refresh author patch counts after bulk insertion
        if let Err(e) = self.refresh_author_patch_counts().await {
            errors.push(format!("Failed to refresh author patch counts: {}", e));
        }

        DatabasePopulationResult {
            success: errors.is_empty(),
            total_processed: processed,
            total_authors_inserted: inserted_authors,
            total_emails_inserted: inserted_patches,
            errors,
        }
    }

    /// Get current patch count from database
    async fn get_patch_count(&self) -> Result<u32, Box<dyn std::error::Error>> {
        let pool = self.get_pool()?;
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM patches")
            .fetch_one(pool)
            .await?;
        Ok(count.0 as u32)
    }

    /// Start a background progress reporter that polls the database for actual progress
    async fn start_progress_reporter<F>(
        &self,
        total_commits: u32,
        initial_count: u32,
        pool: Pool<sqlx::Postgres>,
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
                let current_count = match sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM patches")
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
}

