use std::collections::{HashMap, HashSet};
use sqlx::{Pool, Postgres, Row};
use chrono::{DateTime, Utc, NaiveDateTime};
use regex::Regex;
use crate::mail_parser::EmailInfo;
use crate::database::models::PatchData;

/// Static helper methods for patch operations
pub(crate) struct PatchOps;

impl PatchOps {
    /// Check which commit hashes already exist in the database using batch queries
    pub async fn get_existing_commit_hashes(
        commit_hashes: &[String],
        pool: &Pool<Postgres>
    ) -> Result<HashSet<String>, Box<dyn std::error::Error>> {
        if commit_hashes.is_empty() {
            return Ok(HashSet::new());
        }

        let mut existing_set = HashSet::new();

        // Process in batches to avoid SQL parameter limits (1000 at a time)
        for batch in commit_hashes.chunks(1000) {
            let placeholders: Vec<String> = (1..=batch.len()).map(|i| format!("${}", i)).collect();
            let query_str = format!("SELECT commit_hash FROM patches WHERE commit_hash IN ({})", placeholders.join(","));

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

    /// Collect unique author identities from email data
    /// Returns: HashMap<(first_name, last_name), Vec<email>>
    /// Filters out entries with invalid/empty data
    pub fn collect_unique_author_identities(emails: &[(String, EmailInfo)]) -> HashMap<(String, Option<String>), Vec<String>> {
        let mut author_identities: HashMap<(String, Option<String>), Vec<String>> = HashMap::new();
        let mut skipped_count = 0;

        for (_, email_info) in emails {
            // Validate email address (skip invalid entries)
            let email = email_info.author_email.trim();
            if email.is_empty() {
                eprintln!("Warning: Skipping entry with empty email for author: {} {}", 
                          email_info.author_first_name, 
                          email_info.author_last_name.as_ref().unwrap_or(&String::new()));
                skipped_count += 1;
                continue;
            }
            
            // Basic email validation
            if !email.contains('@') || email.starts_with('@') || email.ends_with('@') {
                eprintln!("Warning: Skipping entry with invalid email '{}' for author: {} {}", 
                          email,
                          email_info.author_first_name, 
                          email_info.author_last_name.as_ref().unwrap_or(&String::new()));
                skipped_count += 1;
                continue;
            }
            
            // Validate author name (skip if first name is empty, but allow "Unknown")
            // We need to keep "Unknown" authors because patches still need author records
            if email_info.author_first_name.trim().is_empty() {
                eprintln!("Warning: Skipping entry with empty name for email: {}", email);
                skipped_count += 1;
                continue;
            }
            
            let name_key = (email_info.author_first_name.clone(), email_info.author_last_name.clone());
            
            author_identities
                .entry(name_key)
                .or_insert_with(Vec::new)
                .push(email.to_string());
        }

        if skipped_count > 0 {
            eprintln!("Skipped {} entries with invalid email or name data during author collection", skipped_count);
        }
        
        eprintln!("Collected {} unique author identities from {} emails", 
                  author_identities.len(), emails.len());

        // Deduplicate emails for each author
        for emails_list in author_identities.values_mut() {
            emails_list.sort();
            emails_list.dedup();
        }

        author_identities
    }

    /// Upsert authors and their emails, return mappings
    /// Returns: (email -> author_id, email -> email_id)
    pub async fn upsert_authors_and_emails(
        author_identities: &HashMap<(String, Option<String>), Vec<String>>,
        pool: &Pool<Postgres>
    ) -> Result<(HashMap<String, i64>, HashMap<String, i64>), Box<dyn std::error::Error>> {
        if author_identities.is_empty() {
            return Ok((HashMap::new(), HashMap::new()));
        }

        // Step 1: Insert authors (by name)
        let mut sorted_authors: Vec<_> = author_identities.keys().collect();
        sorted_authors.sort();

        if !sorted_authors.is_empty() {
            let mut insert_query = String::from("INSERT INTO authors (first_name, last_name, display_name) VALUES ");
            let mut param_count = 1;

            for (i, _) in sorted_authors.iter().enumerate() {
                if i > 0 {
                    insert_query.push(',');
                }
                insert_query.push_str(&format!("(${}, ${}, ${})", param_count, param_count + 1, param_count + 2));
                param_count += 3;
            }

            insert_query.push_str(" ON CONFLICT (first_name, last_name) DO NOTHING");

            let mut query = sqlx::query(&insert_query);
            for (first_name, last_name) in &sorted_authors {
                let display_name = if let Some(ln) = last_name {
                    format!("{} {}", first_name, ln)
                } else {
                    first_name.to_string()
                };
                query = query.bind(first_name).bind(last_name).bind(display_name);
            }

            query.execute(pool).await?;
        }

        // Step 2: Get author IDs for all names
        let mut author_id_by_name: HashMap<(String, Option<String>), i64> = HashMap::new();
        for (first_name, last_name) in &sorted_authors {
            let row = sqlx::query("SELECT author_id FROM authors WHERE first_name = $1 AND (last_name = $2 OR (last_name IS NULL AND $2 IS NULL))")
                .bind(first_name)
                .bind(last_name)
                .fetch_one(pool)
                .await?;
            let author_id: i64 = row.get(0);
            author_id_by_name.insert((first_name.to_string(), last_name.clone()), author_id);
        }

        // Step 3: Insert author_emails
        let mut all_emails_to_insert = Vec::new();
        for ((first_name, last_name), emails) in author_identities {
            let author_id = author_id_by_name.get(&(first_name.clone(), last_name.clone())).unwrap();
            for email in emails {
                all_emails_to_insert.push((*author_id, email.clone()));
            }
        }

        if !all_emails_to_insert.is_empty() {
            all_emails_to_insert.sort_by(|a, b| a.1.cmp(&b.1));
            
            let mut insert_query = String::from("INSERT INTO author_emails (author_id, email) VALUES ");
            let mut param_count = 1;

            for (i, _) in all_emails_to_insert.iter().enumerate() {
                if i > 0 {
                    insert_query.push(',');
                }
                insert_query.push_str(&format!("(${}, ${})", param_count, param_count + 1));
                param_count += 2;
            }

            insert_query.push_str(" ON CONFLICT (email) DO NOTHING");

            let mut query = sqlx::query(&insert_query);
            for (author_id, email) in &all_emails_to_insert {
                query = query.bind(author_id).bind(email);
            }

            query.execute(pool).await?;
        }

        // Step 4: Get email IDs for all emails
        let all_emails: Vec<&String> = all_emails_to_insert.iter().map(|(_, email)| email).collect();
        let mut email_to_author_id = HashMap::new();
        let mut email_to_email_id = HashMap::new();

        if !all_emails.is_empty() {
            let placeholders: Vec<String> = (1..=all_emails.len()).map(|i| format!("${}", i)).collect();
            let select_query = format!(
                "SELECT email_id, author_id, email FROM author_emails WHERE email IN ({})",
                placeholders.join(",")
            );

            let mut select = sqlx::query(&select_query);
            for email in &all_emails {
                select = select.bind(*email);
            }

            let rows = select.fetch_all(pool).await?;
            for row in rows {
                let email_id: i64 = row.get(0);
                let author_id: i64 = row.get(1);
                let email: String = row.get::<String, _>(2).to_lowercase();
                email_to_author_id.insert(email.clone(), author_id);
                email_to_email_id.insert(email, email_id);
            }
        }

        Ok((email_to_author_id, email_to_email_id))
    }

    /// Prepare patch data for insertion with email IDs
    fn prepare_patches_with_email_ids(
        emails: &[(String, EmailInfo)],
        email_to_author_id: &HashMap<String, i64>,
        email_to_email_id: &HashMap<String, i64>
    ) -> Result<Vec<PatchData>, Box<dyn std::error::Error>> {
        let mut patches_data = Vec::new();

        for (commit_hash, email_info) in emails {
            let email = &email_info.author_email;
            
            // Try to get IDs from the provided maps
            let author_id = match email_to_author_id.get(email) {
                Some(&id) => id,
                None => {
                    // This should not happen if upsert was done correctly
                    return Err(format!("Author not found for email: {}", email).into());
                }
            };
            
            let email_id = match email_to_email_id.get(email) {
                Some(&id) => id,
                None => {
                    return Err(format!("Email ID not found for email: {}", email).into());
                }
            };

            // Parse date with multiple format fallbacks
            let parsed_date = Self::parse_email_date(&email_info.date)?;

            // Detect if it's a patch series
            let (is_series, series_number, series_total) = Self::detect_patch_series(&email_info.subject);
            
            // Detect and parse merge notification
            let (is_merge, merge_info) = crate::mail_parser::detect_and_parse_merge(email_info);

            patches_data.push(PatchData {
                author_id,
                email_id,
                message_id: email_info.message_id.clone(),
                subject: email_info.subject.clone(),
                sent_at: parsed_date,
                commit_hash: commit_hash.clone(),
                body_text: Some(email_info.body.clone()),
                is_series,
                series_number,
                series_total,
                in_reply_to: email_info.in_reply_to.clone(),
                references: email_info.references.clone(),
                is_reply: email_info.is_reply,
                // Merge notification fields
                is_merge_notification: is_merge,
                merge_info,
            });
        }

        Ok(patches_data)
    }

    /// Insert patches with email IDs in optimized batches
    async fn insert_patches_with_email_ids(
        emails: &[(String, EmailInfo)],
        email_to_author_id: &HashMap<String, i64>,
        email_to_email_id: &HashMap<String, i64>,
        pool: &Pool<Postgres>
    ) -> Result<u32, Box<dyn std::error::Error>> {
        // First, augment the maps with any missing emails from the database
        let mut complete_email_to_author_id = email_to_author_id.clone();
        let mut complete_email_to_email_id = email_to_email_id.clone();
        
        // Find emails that are missing from our maps
        let missing_emails: Vec<&String> = emails.iter()
            .map(|(_, info)| &info.author_email)
            .filter(|email| !complete_email_to_email_id.contains_key(*email))
            .collect();
        
        // Query database for missing email mappings
        if !missing_emails.is_empty() {
            let unique_missing: std::collections::HashSet<&String> = missing_emails.into_iter().collect();
            
            eprintln!("Looking up {} missing emails from database", unique_missing.len());
            
            let placeholders: Vec<String> = (1..=unique_missing.len()).map(|i| format!("${}", i)).collect();
            let query_str = format!(
                "SELECT ae.email, ae.email_id, ae.author_id 
                 FROM author_emails ae 
                 WHERE ae.email IN ({})",
                placeholders.join(",")
            );
            
            let mut query = sqlx::query(&query_str);
            for email in &unique_missing {
                query = query.bind(*email);
            }
            
            let rows = query.fetch_all(pool).await?;
            eprintln!("Found {} existing emails in database", rows.len());
            
            for row in rows {
                let email: String = row.get(0);
                let email_id: i64 = row.get(1);
                let author_id: i64 = row.get(2);
                complete_email_to_email_id.insert(email.clone(), email_id);
                complete_email_to_author_id.insert(email, author_id);
            }
            
            // Check if any emails are still missing after database lookup
            let still_missing: Vec<&String> = emails.iter()
                .map(|(_, info)| &info.author_email)
                .filter(|email| !complete_email_to_email_id.contains_key(*email))
                .collect();
            
            if !still_missing.is_empty() {
                eprintln!("WARNING: {} emails not found in batch or database:", still_missing.len());
                for email in &still_missing {
                    eprintln!("  - {}", email);
                }
                return Err(format!(
                    "Missing authors for {} emails. First missing: {}. This may indicate emails were skipped during collection.",
                    still_missing.len(),
                    still_missing.first().unwrap()
                ).into());
            }
        }
        
        let patches_data = Self::prepare_patches_with_email_ids(emails, &complete_email_to_author_id, &complete_email_to_email_id)?;

        if patches_data.is_empty() {
            return Ok(0);
        }

        // PostgreSQL has a parameter limit of ~65535
        // With 18 params per patch (including merge fields), we can do ~3640 patches per query
        // Use 3500 to be safe
        const MAX_PATCHES_PER_QUERY: usize = 3500;

        let mut inserted_patches = 0u32;

        // Insert in large batches for maximum throughput
        for patch_batch in patches_data.chunks(MAX_PATCHES_PER_QUERY) {
            let batch_count = Self::execute_patch_batch_insert(patch_batch, pool).await?;
            inserted_patches += batch_count;
        }

        Ok(inserted_patches)
    }

    /// Execute batch insert for a chunk of patches
    async fn execute_patch_batch_insert(patch_batch: &[PatchData], pool: &Pool<Postgres>) -> Result<u32, Box<dyn std::error::Error>> {
        let mut query = String::from("INSERT INTO patches (author_id, email_id, message_id, subject, sent_at, commit_hash, body_text, is_series, series_number, series_total, in_reply_to, thread_references, is_reply, is_merge_notification, merge_repository, merge_branch, merge_applied_by, merge_commit_links) VALUES ");
        let mut param_count = 1;

        for (i, _) in patch_batch.iter().enumerate() {
            if i > 0 {
                query.push(',');
            }
            query.push_str(&format!("(${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${})",
                                   param_count, param_count + 1, param_count + 2, param_count + 3,
                                   param_count + 4, param_count + 5, param_count + 6, param_count + 7,
                                   param_count + 8, param_count + 9, param_count + 10, param_count + 11,
                                   param_count + 12, param_count + 13, param_count + 14, param_count + 15,
                                   param_count + 16, param_count + 17));
            param_count += 18;
        }

        query.push_str(" ON CONFLICT (message_id) DO NOTHING");

        let mut insert_query = sqlx::query(&query);

        for patch_data in patch_batch {
            // Extract merge fields if present
            let (merge_repo, merge_branch, merge_applied_by, merge_commit_links) = 
                if let Some(ref merge_info) = patch_data.merge_info {
                    (
                        Some(merge_info.repository.clone()),
                        Some(merge_info.branch.clone()),
                        Some(merge_info.applied_by.clone()),
                        Some(merge_info.commit_links.clone())
                    )
                } else {
                    (None, None, None, None)
                };
            
            insert_query = insert_query
                .bind(patch_data.author_id)
                .bind(patch_data.email_id)
                .bind(&patch_data.message_id)
                .bind(&patch_data.subject)
                .bind(&patch_data.sent_at)
                .bind(&patch_data.commit_hash)
                .bind(&patch_data.body_text)
                .bind(&patch_data.is_series)
                .bind(&patch_data.series_number)
                .bind(&patch_data.series_total)
                .bind(&patch_data.in_reply_to)
                .bind(&patch_data.references)
                .bind(&patch_data.is_reply)
                .bind(&patch_data.is_merge_notification)
                .bind(merge_repo)
                .bind(merge_branch)
                .bind(merge_applied_by)
                .bind(merge_commit_links);
        }

        insert_query.execute(pool).await?;
        Ok(patch_batch.len() as u32)
    }

    /// Parse email date with multiple format support
    fn parse_email_date(date_str: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
        DateTime::parse_from_rfc2822(date_str)
            .or_else(|_| DateTime::parse_from_rfc3339(date_str))
            .map(|dt| dt.with_timezone(&Utc))
            .or_else(|_| {
                NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S")
                    .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc))
            })
    }

    /// Detect if email subject indicates a patch series
    fn detect_patch_series(subject: &str) -> (bool, Option<i32>, Option<i32>) {
        let series_regex = Regex::new(r"\[.*?(\d+)/(\d+)\]").unwrap();
        if let Some(captures) = series_regex.captures(subject) {
            let num: i32 = captures.get(1).unwrap().as_str().parse().unwrap_or(0);
            let total: i32 = captures.get(2).unwrap().as_str().parse().unwrap_or(0);
            (true, Some(num), Some(total))
        } else {
            (false, None, None)
        }
    }

    /// Insert batch to database (main entry point)
    pub async fn insert_batch_to_db(
        emails: &[(String, EmailInfo)], 
        pool: &Pool<Postgres>
    ) -> Result<(u32, u32), Box<dyn std::error::Error>> {
        if emails.is_empty() {
            return Ok((0, 0));
        }

        // Collect unique author identities (name -> emails mapping)
        let author_identities = Self::collect_unique_author_identities(emails);
        let author_count = author_identities.len() as u32;

        // Upsert authors and their emails, get ID mappings
        let (email_to_author_id, email_to_email_id) = Self::upsert_authors_and_emails(&author_identities, pool).await?;

        // Insert patches using the ID mappings
        let inserted_patches = Self::insert_patches_with_email_ids(emails, &email_to_author_id, &email_to_email_id, pool).await?;

        Ok((author_count, inserted_patches))
    }
}

