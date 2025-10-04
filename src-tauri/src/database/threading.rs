use std::collections::{HashMap, VecDeque};
use sqlx::{Pool, Postgres, Row};
use crate::database::{DatabaseManager, ThreadBuildStats};
use regex::Regex;

/// Metadata about a patch needed for threading
#[allow(dead_code)]
struct PatchThreadInfo {
    patch_id: i64,
    message_id: String,
    subject: String,
    normalized_subject: String,
    sent_at: chrono::DateTime<chrono::Utc>,
    is_reply: bool,  // Based on "Re:" prefix in subject
    is_series: bool,
    series_number: Option<i32>,
    series_total: Option<i32>,
}

/// Extract series identifier from subject line
/// Example: "[PATCH v3 net-next 03/12] ..." -> "v3 net-next/12"
/// This creates a unique key for each patch series
fn extract_series_identifier(subject: &str, series_total: i32) -> Option<String> {
    let re = Regex::new(r"\[PATCH\s+([^\]]*?)\s+\d+/\d+\]").ok()?;
    if let Some(caps) = re.captures(subject) {
        if let Some(identifier) = caps.get(1) {
            // Combine identifier with series_total to create unique key
            // This handles "v3" vs "v4" of the same patch series
            return Some(format!("{}/{}", identifier.as_str().trim(), series_total));
        }
    }
    None
}

impl DatabaseManager {
    /// Build thread relationships for all patches in database
    /// Improved approach: Handles patch series and nested replies correctly
    /// Uses In-Reply-To and References headers to build complete thread hierarchy
    pub async fn build_thread_relationships(&mut self) -> Result<ThreadBuildStats, Box<dyn std::error::Error>> {
        let start_time = std::time::Instant::now();
        
        self.ensure_connected().await?;
        let pool = self.get_pool()?;
        
        println!("Fetching all patches for thread building...");
        
        // Step 1: Fetch all patches with threading info and series metadata
        let patch_rows = sqlx::query(
            "SELECT patch_id, message_id, subject, sent_at, in_reply_to, thread_references,
                    is_series, series_number, series_total
             FROM patches 
             ORDER BY sent_at ASC"
        )
        .fetch_all(pool)
        .await?;
        
        println!("Processing {} patches...", patch_rows.len());
        
        // Step 2: Build message_id -> patch_id mapping
        let mut msg_id_to_patch_id: HashMap<String, i64> = HashMap::new();
        let mut patches_info: Vec<PatchThreadInfo> = Vec::new();
        
        for row in &patch_rows {
            let patch_id: i64 = row.get(0);
            let message_id: String = row.get(1);
            let subject: String = row.get(2);
            let sent_at: chrono::DateTime<chrono::Utc> = row.get(3);
            let is_series: bool = row.try_get(6).unwrap_or(false);
            let series_number: Option<i32> = row.try_get(7).ok();
            let series_total: Option<i32> = row.try_get(8).ok();
            
            msg_id_to_patch_id.insert(message_id.clone(), patch_id);
            
            let is_reply = subject.trim().to_lowercase().starts_with("re:");
            let normalized_subject = crate::mail_parser::normalize_subject(&subject);
            
            patches_info.push(PatchThreadInfo {
                patch_id,
                message_id: message_id.clone(),
                subject: subject.clone(),
                normalized_subject: normalized_subject.clone(),
                sent_at,
                is_reply,
                is_series,
                series_number,
                series_total,
            });
        }
        
        // Step 3: Build mapping from normalized subject to patch IDs (for fallback matching)
        let mut subject_to_patches: HashMap<String, Vec<i64>> = HashMap::new();
        for patch_info in &patches_info {
            subject_to_patches
                .entry(patch_info.normalized_subject.clone())
                .or_insert_with(Vec::new)
                .push(patch_info.patch_id);
        }
        
        // Step 3.5: Build series identifier mapping
        // Extract series identifier (e.g., "v3 net-next 12" from "[PATCH v3 net-next 03/12]")
        // and map to the earliest patch in that series
        let mut series_to_root: HashMap<String, i64> = HashMap::new();
        for patch_info in &patches_info {
            if patch_info.is_series && patch_info.series_total.is_some() {
                // Extract series identifier from subject
                // Pattern: [PATCH <identifier> N/M] where identifier might be "v3 net-next", "bpf-next", etc.
                if let Some(series_id) = extract_series_identifier(&patch_info.subject, patch_info.series_total.unwrap()) {
                    series_to_root.entry(series_id)
                        .and_modify(|root_id| {
                            // Keep the patch with lowest series_number (or earliest if numbers are same)
                            if let Some(existing_patch) = patches_info.iter().find(|p| p.patch_id == *root_id) {
                                let should_replace = match (existing_patch.series_number, patch_info.series_number) {
                                    (Some(existing_num), Some(new_num)) => new_num < existing_num,
                                    _ => patch_info.sent_at < existing_patch.sent_at,
                                };
                                if should_replace {
                                    *root_id = patch_info.patch_id;
                                }
                            }
                        })
                        .or_insert(patch_info.patch_id);
                }
            }
        }
        println!("Found {} patch series", series_to_root.len());
        
        // Step 4: Build parent-child relationships for ALL patches (not just "Re:" replies)
        // Patch series members also need to be linked to their parent
        let mut children_map: HashMap<i64, Vec<i64>> = HashMap::new();
        let mut patch_has_parent: HashMap<i64, bool> = HashMap::new();
        
        for row in &patch_rows {
            let patch_id: i64 = row.get(0);
            let subject: String = row.get(2);
            let in_reply_to: Option<String> = row.get(4);
            let references: Vec<String> = row.try_get(5).unwrap_or_default();
            
            // Skip patches with no references (potential roots)
            if in_reply_to.is_none() && references.is_empty() {
                continue;
            }
            
            // Strategy 1: Try In-Reply-To header (most direct parent)
            let mut parent_id = if let Some(parent_msg_id) = in_reply_to.as_ref() {
                msg_id_to_patch_id.get(parent_msg_id).copied()
            } else {
                None
            };
            
            // Strategy 2: Walk backwards through References to find closest ancestor
            if parent_id.is_none() && !references.is_empty() {
                for ref_id in references.iter().rev() {
                    if let Some(pid) = msg_id_to_patch_id.get(ref_id).copied() {
                        parent_id = Some(pid);
                        break;
                    }
                }
            }
            
            // Strategy 3: Fall back to subject-based matching
            // For patches/replies that reference messages not in our database
            if parent_id.is_none() {
                let normalized = crate::mail_parser::normalize_subject(&subject);
                if let Some(candidates) = subject_to_patches.get(&normalized) {
                    // Find the earliest patch with this subject (likely the root)
                    // that is not the current patch itself
                    parent_id = candidates.iter()
                        .filter(|&&pid| pid != patch_id)
                        .min()
                        .copied();
                }
            }
            
            // Strategy 4: For patch series members, link to the series root
            // This handles cases where the cover letter (00/N) is missing
            if parent_id.is_none() {
                if let Some(patch_info) = patches_info.iter().find(|p| p.patch_id == patch_id) {
                    if patch_info.is_series && patch_info.series_total.is_some() {
                        if let Some(series_id) = extract_series_identifier(&subject, patch_info.series_total.unwrap()) {
                            if let Some(&root_id) = series_to_root.get(&series_id) {
                                // Don't link to ourselves
                                if root_id != patch_id {
                                    parent_id = Some(root_id);
                                    println!("  Series: {} -> root {} (series: {})", patch_id, root_id, series_id);
                                }
                            }
                        }
                    }
                }
            }
            
            if let Some(parent) = parent_id {
                children_map.entry(parent).or_insert_with(Vec::new).push(patch_id);
                patch_has_parent.insert(patch_id, true);
            } else {
                // Debug: log patches that couldn't find a parent
                if in_reply_to.is_some() || !references.is_empty() {
                    // Safe string truncation at char boundaries
                    let truncated_subject = subject.chars().take(60).collect::<String>();
                    println!("  Orphan: {} (has refs but no parent) - {}", patch_id, truncated_subject);
                }
            }
        }
        
        println!("Built {} parent-child relationships", children_map.len());
        
        // Step 5: Find true roots - patches that don't reference anything in our set
        let mut root_patches: Vec<&PatchThreadInfo> = Vec::new();
        for patch_info in &patches_info {
            // A root is a patch that has no parent in our database
            if !patch_has_parent.contains_key(&patch_info.patch_id) {
                root_patches.push(patch_info);
                println!("  Root: {} ({})", patch_info.patch_id, &patch_info.subject[..60.min(patch_info.subject.len())]);
            }
        }
        
        println!("Found {} root patches", root_patches.len());
        
        // Step 6: Clear all old thread relationships before rebuilding
        // This prevents duplicate key errors when patches move between threads
        println!("Clearing old thread relationships...");
        sqlx::query("DELETE FROM patch_replies")
            .execute(pool)
            .await?;
        
        // Step 7: Build threads from each root (optimized with batch inserts)
        println!("Building {} threads with batch inserts...", root_patches.len());
        let (total_threads, total_replies, max_depth) = self.build_all_threads_batched(
            &root_patches,
            &children_map,
            pool
        ).await?;
        
        // Count orphaned patches (patches with references but no parent found)
        let orphaned = patches_info.len() - root_patches.len() - (total_replies as usize);
        
        println!("Thread building complete: {} threads, {} replies, {} orphaned", 
                 total_threads, total_replies, orphaned);
        
        let elapsed = start_time.elapsed();
        
        Ok(ThreadBuildStats {
            total_threads,
            total_replies,
            orphaned_messages: orphaned as u32,
            max_depth,
            processing_time_ms: elapsed.as_millis() as u64,
        })
    }
    
    /// Build all threads at once with optimized batch inserts
    /// Much faster than building one thread at a time
    async fn build_all_threads_batched(
        &self,
        root_patches: &[&PatchThreadInfo],
        children_map: &HashMap<i64, Vec<i64>>,
        pool: &Pool<Postgres>
    ) -> Result<(u32, u32, i32), Box<dyn std::error::Error>> {
        if root_patches.is_empty() {
            return Ok((0, 0, 0));
        }
        
        // Step 1: Batch insert all thread roots
        println!("Inserting {} thread roots...", root_patches.len());
        let mut thread_values = Vec::new();
        let mut param_count = 1;
        let mut query_str = String::from("INSERT INTO patch_threads (root_patch_id, root_message_id, subject_base) VALUES ");
        
        for (i, root) in root_patches.iter().enumerate() {
            if i > 0 {
                query_str.push(',');
            }
            query_str.push_str(&format!("(${}, ${}, ${})", param_count, param_count + 1, param_count + 2));
            param_count += 3;
            thread_values.push((root.patch_id, &root.message_id, &root.normalized_subject));
        }
        
        query_str.push_str(" ON CONFLICT (root_patch_id) DO UPDATE SET root_message_id = EXCLUDED.root_message_id, subject_base = EXCLUDED.subject_base");
        
        let mut query = sqlx::query(&query_str);
        for (patch_id, message_id, subject) in &thread_values {
            query = query.bind(patch_id).bind(*message_id).bind(*subject);
        }
        query.execute(pool).await?;
        
        // Step 2: Get all thread IDs
        let mut root_to_thread_id: HashMap<i64, i64> = HashMap::new();
        for root in root_patches {
            let row = sqlx::query("SELECT thread_id FROM patch_threads WHERE root_patch_id = $1")
                .bind(root.patch_id)
                .fetch_one(pool)
                .await?;
            let thread_id: i64 = row.get(0);
            root_to_thread_id.insert(root.patch_id, thread_id);
        }
        
        // Step 3: Build all patch_replies data in parallel
        println!("Building thread hierarchies...");
        let root_patch_ids: Vec<i64> = root_patches.iter().map(|r| r.patch_id).collect();
        
        let all_replies = tokio::task::spawn_blocking({
            let children_map = children_map.clone();
            let root_to_thread_id = root_to_thread_id.clone();
            
            move || {
                let mut all_replies = Vec::new();
                let mut max_depth = 0i32;
                
                for root_id in root_patch_ids {
                    let thread_id = *root_to_thread_id.get(&root_id).unwrap();
                    
                    // Add root
                    all_replies.push((thread_id, root_id, None, 0i32, vec![root_id]));
                    
                    // BFS through children
                    let mut queue = VecDeque::new();
                    queue.push_back((root_id, 0i32, vec![root_id]));
                    
                    while let Some((current_patch_id, depth, path)) = queue.pop_front() {
                        if let Some(children) = children_map.get(&current_patch_id) {
                            for &child_id in children {
                                let new_depth = depth + 1;
                                max_depth = max_depth.max(new_depth);
                                let mut new_path = path.clone();
                                new_path.push(child_id);
                                
                                all_replies.push((thread_id, child_id, Some(current_patch_id), new_depth, new_path.clone()));
                                queue.push_back((child_id, new_depth, new_path));
                            }
                        }
                    }
                }
                
                (all_replies, max_depth)
            }
        }).await?;
        
        let (all_replies, max_depth) = all_replies;
        
        // Step 4: Batch insert all patch_replies
        println!("Inserting {} patch replies in batches...", all_replies.len());
        const BATCH_SIZE: usize = 5000;
        
        for batch in all_replies.chunks(BATCH_SIZE) {
            let mut query_str = String::from("INSERT INTO patch_replies (thread_id, patch_id, parent_patch_id, depth_level, position_in_thread, thread_path) VALUES ");
            let mut param_count = 1;
            
            for (i, _) in batch.iter().enumerate() {
                if i > 0 {
                    query_str.push(',');
                }
                query_str.push_str(&format!("(${}, ${}, ${}, ${}, ${}, ${})", 
                    param_count, param_count + 1, param_count + 2, param_count + 3, param_count + 4, param_count + 5));
                param_count += 6;
            }
            
            let mut query = sqlx::query(&query_str);
            for (thread_id, patch_id, parent_patch_id, depth, path) in batch {
                query = query
                    .bind(thread_id)
                    .bind(patch_id)
                    .bind(parent_patch_id)
                    .bind(depth)
                    .bind(0i32) // position_in_thread - placeholder
                    .bind(path);
            }
            
            query.execute(pool).await?;
        }
        
        // Step 5: Calculate and update thread statistics in bulk
        println!("Calculating thread statistics...");
        
        // Use single query to update all thread statistics at once
        sqlx::query(
            "UPDATE patch_threads pt
             SET reply_count = subq.reply_count,
                 participant_count = subq.participant_count,
                 updated_at = NOW(),
                 last_activity_at = subq.last_activity
             FROM (
               SELECT 
                 pr.thread_id,
                 COUNT(*) FILTER (WHERE pr.parent_patch_id IS NOT NULL) as reply_count,
                 COUNT(DISTINCT p.author_id) as participant_count,
                 MAX(p.sent_at) as last_activity
               FROM patch_replies pr
               JOIN patches p ON pr.patch_id = p.patch_id
               GROUP BY pr.thread_id
             ) subq
             WHERE pt.thread_id = subq.thread_id"
        )
        .execute(pool)
        .await?;
        
        let total_threads = root_patches.len() as u32;
        let total_replies = all_replies.len() as u32 - total_threads; // Subtract roots
        
        Ok((total_threads, total_replies, max_depth))
    }
    
    /// Build a single thread starting from a root patch
    /// Uses proper BFS with children_map for O(1) lookups
    /// Includes all patches and replies that reference this thread
    #[allow(dead_code)]
    async fn build_single_thread(
        &self,
        root_patch_id: i64,
        root_message_id: &str,
        normalized_subject: &str,
        children_map: &HashMap<i64, Vec<i64>>,
        pool: &Pool<Postgres>
    ) -> Result<(u32, i32), Box<dyn std::error::Error>> {
        // Create thread entry
        let thread_row = sqlx::query(
            "INSERT INTO patch_threads (root_patch_id, root_message_id, subject_base)
             VALUES ($1, $2, $3)
             ON CONFLICT (root_patch_id) DO UPDATE 
             SET root_message_id = EXCLUDED.root_message_id,
                 subject_base = EXCLUDED.subject_base
             RETURNING thread_id"
        )
        .bind(root_patch_id)
        .bind(root_message_id)
        .bind(normalized_subject)
        .fetch_one(pool)
        .await?;
        
        let thread_id: i64 = thread_row.get(0);
        
        // Insert root as reply with depth 0
        sqlx::query(
            "INSERT INTO patch_replies (thread_id, patch_id, parent_patch_id, depth_level, position_in_thread, thread_path)
             VALUES ($1, $2, NULL, 0, 0, ARRAY[$2]::BIGINT[])"
        )
        .bind(thread_id)
        .bind(root_patch_id)
        .execute(pool)
        .await?;
        
        // Build a map of patch_id -> sent_at for all patches in this thread
        // This avoids N+1 queries during tree building
        let mut patch_times: HashMap<i64, chrono::DateTime<chrono::Utc>> = HashMap::new();
        
        // Collect all patch IDs that will be in this thread
        let mut all_patch_ids = vec![root_patch_id];
        let mut stack = vec![root_patch_id];
        while let Some(current_id) = stack.pop() {
            if let Some(children) = children_map.get(&current_id) {
                for &child_id in children {
                    all_patch_ids.push(child_id);
                    stack.push(child_id);
                }
            }
        }
        
        // Fetch all sent_at times in a single query
        if !all_patch_ids.is_empty() {
            let rows = sqlx::query("SELECT patch_id, sent_at FROM patches WHERE patch_id = ANY($1)")
                .bind(&all_patch_ids)
                .fetch_all(pool)
                .await?;
            for row in rows {
                let patch_id: i64 = row.get(0);
                let sent_at: chrono::DateTime<chrono::Utc> = row.get(1);
                patch_times.insert(patch_id, sent_at);
            }
        }
        
        // Build tree using proper BFS (VecDeque for O(1) pop_front)
        let mut queue = VecDeque::new();
        queue.push_back((root_patch_id, 0, vec![root_patch_id]));
        
        let mut position = 1;  // Root is position 0
        let mut reply_count = 0u32;
        let mut max_depth = 0i32;
        
        while let Some((current_id, depth, path)) = queue.pop_front() {
            // Get children from the pre-built children_map (O(1) lookup)
            if let Some(children) = children_map.get(&current_id) {
                // Sort children by sent_at for chronological order using pre-fetched times
                let mut children_with_time: Vec<(i64, chrono::DateTime<chrono::Utc>)> = Vec::new();
                for &child_id in children {
                    if let Some(&sent_at) = patch_times.get(&child_id) {
                        children_with_time.push((child_id, sent_at));
                    }
                }
                children_with_time.sort_by_key(|(_, time)| *time);
                
                for (child_id, _) in children_with_time {
                    let child_depth = depth + 1;
                    max_depth = max_depth.max(child_depth);
                    
                    let mut child_path = path.clone();
                    child_path.push(child_id);
                    
                    // Insert reply
                    sqlx::query(
                        "INSERT INTO patch_replies (thread_id, patch_id, parent_patch_id, depth_level, position_in_thread, thread_path)
                         VALUES ($1, $2, $3, $4, $5, $6)"
                    )
                    .bind(thread_id)
                    .bind(child_id)
                    .bind(current_id)
                    .bind(child_depth)
                    .bind(position)
                    .bind(&child_path)
                    .execute(pool)
                    .await?;
                    
                    position += 1;
                    reply_count += 1;
                    
                    // Add to queue for BFS traversal
                    queue.push_back((child_id, child_depth, child_path));
                }
            }
        }
        
        // Update thread statistics
        sqlx::query("SELECT update_thread_stats($1)")
            .bind(thread_id)
            .execute(pool)
            .await?;
        
        Ok((reply_count, max_depth))
    }
}
