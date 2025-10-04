/// Database threading test - Tests that reply information can be correctly stored and retrieved from database
/// This test should produce the same thread tree output as test_threading_full.rs but using the database
use std::collections::HashMap;
use crate::git_parser::{get_email_content, get_all_commits_with_limit};
use crate::mail_parser::{parse_email_headers, parse_email_from_content};
use crate::database::DatabaseManager;

#[derive(Debug, Clone)]
struct DbPatch {
    patch_id: i64,
    commit_hash: String,
    message_id: String,
    subject: String,
    author: String,
    is_reply: bool,
    is_series: bool,
    series_number: Option<i32>,
    series_total: Option<i32>,
}

#[derive(Debug, Clone)]
struct DbThreadNode {
    patch: DbPatch,
    depth: i32,
    parent_patch_id: Option<i64>,
    children: Vec<DbThreadNode>,
}

/// Parse and insert a specific set of commits to the database
async fn setup_test_commits(
    db: &mut DatabaseManager,
    target_commit: &str,
    search_limit: usize
) -> Result<Vec<DbPatch>, Box<dyn std::error::Error>> {
    println!("\n=== Setting Up Test Database ===");
    
    // Parse target commit to get its message ID and references
    let target_content = get_email_content(target_commit)?;
    let target_headers = parse_email_headers(&target_content);
    let target_metadata = crate::git_parser::get_single_commit_metadata(target_commit)?;
    
    let target_msg_id = target_headers.get("message-id")
        .ok_or("Target commit has no message-id")?
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_string();
    
    println!("Target: {} ({})", target_commit, &target_metadata.subject);
    println!("Message-ID: {}", target_msg_id);
    
    // Get references from target
    let ref_msg_ids: Vec<String> = target_headers.get("references")
        .map(|refs| {
            refs.split_whitespace()
                .map(|id| id.trim_start_matches('<').trim_end_matches('>').to_string())
                .collect()
        })
        .unwrap_or_default();
    
    let in_reply_to_id = target_headers.get("in-reply-to")
        .map(|id| id.trim_start_matches('<').trim_end_matches('>').to_string());
    
    println!("\nIn-Reply-To: {:?}", in_reply_to_id);
    println!("References ({}):", ref_msg_ids.len());
    for (i, ref_id) in ref_msg_ids.iter().enumerate() {
        println!("  [{}] {}", i, ref_id);
    }
    
    // Build set of all message IDs we're looking for
    let mut target_msg_ids: Vec<String> = ref_msg_ids.clone();
    if let Some(id) = in_reply_to_id.clone() {
        if !target_msg_ids.contains(&id) {
            target_msg_ids.push(id);
        }
    }
    target_msg_ids.push(target_msg_id.clone());
    
    println!("\n=== Searching for {} Related Commits ===", target_msg_ids.len());
    
    // Search through commits to find related ones
    let all_commits = get_all_commits_with_limit(Some(search_limit))?;
    let mut commits_to_insert = Vec::new();
    let mut found_emails = Vec::new();
    
    for commit in all_commits {
        let content = match get_email_content(&commit) {
            Ok(c) => c,
            Err(_) => continue,
        };
        
        let headers = parse_email_headers(&content);
        if let Some(msg_id) = headers.get("message-id") {
            let clean_id = msg_id.trim_start_matches('<').trim_end_matches('>').to_string();
            
            if target_msg_ids.contains(&clean_id) {
                let metadata = crate::git_parser::get_single_commit_metadata(&commit)?;
                let email_info = parse_email_from_content(&commit, &content, &metadata)?;
                
                println!("  ✓ Found: {} ({})", &commit[..12], &metadata.subject[..60.min(metadata.subject.len())]);
                
                commits_to_insert.push(commit.clone());
                found_emails.push((commit.clone(), email_info));
            }
        }
    }
    
    println!("\nFound {} commits to insert into database", found_emails.len());
    
    // Clear existing data and setup fresh database
    db.ensure_connected().await?;
    db.setup_database().await?;
    
    // Clear all data for fresh test
    println!("\nClearing existing data...");
    {
        let pool = db.get_pool()?;
        sqlx::query("TRUNCATE TABLE patch_replies CASCADE").execute(pool).await?;
        sqlx::query("TRUNCATE TABLE patch_threads CASCADE").execute(pool).await?;
        sqlx::query("TRUNCATE TABLE patches CASCADE").execute(pool).await?;
        sqlx::query("TRUNCATE TABLE author_emails CASCADE").execute(pool).await?;
        sqlx::query("TRUNCATE TABLE authors CASCADE").execute(pool).await?;
    }
    
    // Insert the patches using populate_database
    // Since we have EmailInfo tuples, we can insert them manually via SQL
    println!("\nInserting {} patches...", found_emails.len());
    
    {
        let pool = db.get_pool()?;
        
        // Collect authors
        let mut author_map: HashMap<(String, Option<String>), Vec<String>> = HashMap::new();
        for (_, email_info) in &found_emails {
            let key = (email_info.author_first_name.clone(), email_info.author_last_name.clone());
            author_map.entry(key)
                .or_insert_with(Vec::new)
                .push(email_info.author_email.clone());
        }
        
        // Insert authors
        for ((first_name, last_name), emails) in &author_map {
            let display_name = if let Some(ln) = last_name {
                format!("{} {}", first_name, ln)
            } else {
                first_name.clone()
            };
            
            sqlx::query(
                "INSERT INTO authors (first_name, last_name, display_name) 
                 VALUES ($1, $2, $3) 
                 ON CONFLICT (first_name, last_name) DO NOTHING"
            )
            .bind(first_name)
            .bind(last_name)
            .bind(&display_name)
            .execute(pool)
            .await?;
            
            // Get author_id
            let author_row = sqlx::query(
                "SELECT author_id FROM authors WHERE first_name = $1 AND (last_name = $2 OR (last_name IS NULL AND $2 IS NULL))"
            )
            .bind(first_name)
            .bind(last_name)
            .fetch_one(pool)
            .await?;
            
            use sqlx::Row;
            let author_id: i64 = author_row.get(0);
            
            // Insert emails
            for email in emails {
                sqlx::query(
                    "INSERT INTO author_emails (author_id, email) 
                     VALUES ($1, $2) 
                     ON CONFLICT (email) DO NOTHING"
                )
                .bind(author_id)
                .bind(email)
                .execute(pool)
                .await?;
            }
        }
        
        // Insert patches
        for (commit_hash, email_info) in &found_emails {
            // Get author_id
            let author_row = sqlx::query(
                "SELECT author_id FROM authors WHERE first_name = $1 AND (last_name = $2 OR (last_name IS NULL AND $2 IS NULL))"
            )
            .bind(&email_info.author_first_name)
            .bind(&email_info.author_last_name)
            .fetch_one(pool)
            .await?;
            
            use sqlx::Row;
            let author_id: i64 = author_row.get(0);
            
            // Get email_id
            let email_row = sqlx::query(
                "SELECT email_id FROM author_emails WHERE email = $1"
            )
            .bind(&email_info.author_email)
            .fetch_one(pool)
            .await?;
            
            let email_id: i64 = email_row.get(0);
            
            // Parse date
            let sent_at = chrono::DateTime::parse_from_rfc2822(&email_info.date)
                .or_else(|_| chrono::DateTime::parse_from_rfc3339(&email_info.date))
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now());
            
            // Detect patch series
            let series_regex = regex::Regex::new(r"\[.*?(\d+)/(\d+)\]").unwrap();
            let (is_series, series_number, series_total) = if let Some(captures) = series_regex.captures(&email_info.subject) {
                let num: i32 = captures.get(1).unwrap().as_str().parse().unwrap_or(0);
                let total: i32 = captures.get(2).unwrap().as_str().parse().unwrap_or(0);
                (true, Some(num), Some(total))
            } else {
                (false, None, None)
            };
            
            sqlx::query(
                "INSERT INTO patches (author_id, email_id, message_id, subject, sent_at, commit_hash, body_text, 
                                      is_series, series_number, series_total, in_reply_to, thread_references, is_reply)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
                 ON CONFLICT (message_id) DO NOTHING"
            )
            .bind(author_id)
            .bind(email_id)
            .bind(&email_info.message_id)
            .bind(&email_info.subject)
            .bind(sent_at)
            .bind(commit_hash)
            .bind(&email_info.body)
            .bind(is_series)
            .bind(series_number)
            .bind(series_total)
            .bind(&email_info.in_reply_to)
            .bind(&email_info.references)
            .bind(email_info.is_reply)
            .execute(pool)
            .await?;
        }
        
        println!("Inserted authors and patches");
    }
    
    // Diagnostic: Check what In-Reply-To values we have
    println!("\n=== Diagnostic: Checking Threading Information ===");
    {
        let pool = db.get_pool()?;
        let diag_rows = sqlx::query(
            "SELECT subject, message_id, in_reply_to, is_reply, is_series 
             FROM patches 
             ORDER BY sent_at ASC"
        )
        .fetch_all(pool)
        .await?;
        
        use sqlx::Row;
        for row in diag_rows {
            let subject: String = row.get(0);
            let msg_id: String = row.get(1);
            let in_reply_to: Option<String> = row.get(2);
            let is_reply: bool = row.get(3);
            let is_series: bool = row.get(4);
            
            println!("\n{}", &subject[..60.min(subject.len())]);
            println!("  Message-ID: {}", &msg_id[..40.min(msg_id.len())]);
            if let Some(irt) = in_reply_to {
                println!("  In-Reply-To: {}", &irt[..40.min(irt.len())]);
            }
            println!("  Is Reply: {}, Is Series: {}", is_reply, is_series);
        }
        
        // Check if all In-Reply-To references can be resolved
        println!("\n=== Checking Reference Resolution ===");
        let unresolved = sqlx::query(
            "SELECT p1.subject, p1.in_reply_to 
             FROM patches p1 
             WHERE p1.in_reply_to IS NOT NULL 
               AND NOT EXISTS (
                 SELECT 1 FROM patches p2 WHERE p2.message_id = p1.in_reply_to
               )"
        )
        .fetch_all(pool)
        .await?;
        
        if unresolved.is_empty() {
            println!("✓ All In-Reply-To references resolved!");
        } else {
            println!("✗ {} patches have unresolved In-Reply-To references:", unresolved.len());
            for row in unresolved {
                let subject: String = row.get(0);
                let in_reply_to: Option<String> = row.get(1);
                println!("  - {}: In-Reply-To {:?}", &subject[..40.min(subject.len())], in_reply_to);
            }
        }
    }
    
    // Build thread relationships
    println!("\n=== Building Thread Relationships ===");
    let stats = db.build_thread_relationships().await?;
    println!("Built {} threads with {} replies, max depth: {}", 
             stats.total_threads, stats.total_replies, stats.max_depth);
    
    // Query patches from database
    let patches = {
        let pool = db.get_pool()?;
        let patch_rows = sqlx::query(
            "SELECT p.patch_id, p.commit_hash, p.message_id, p.subject, 
                    a.display_name, p.is_reply, p.is_series, p.series_number, p.series_total
             FROM patches p
             JOIN authors a ON p.author_id = a.author_id
             ORDER BY p.sent_at ASC"
        )
        .fetch_all(pool)
        .await?;
        
        let mut patches = Vec::new();
        for row in patch_rows {
            use sqlx::Row;
            patches.push(DbPatch {
                patch_id: row.get(0),
                commit_hash: row.get(1),
                message_id: row.get(2),
                subject: row.get(3),
                author: row.get(4),
                is_reply: row.get(5),
                is_series: row.get(6),
                series_number: row.try_get(7).ok(),
                series_total: row.try_get(8).ok(),
            });
        }
        patches
    };
    
    Ok(patches)
}

/// Query and build thread tree from database
async fn query_thread_tree_from_db(
    db: &mut DatabaseManager,
    target_commit: &str,
    patches: &[DbPatch]
) -> Result<Option<DbThreadNode>, Box<dyn std::error::Error>> {
    db.ensure_connected().await?;
    let pool = db.get_pool()?;
    
    // Find target patch
    let target_patch = patches.iter()
        .find(|p| p.commit_hash == target_commit)
        .ok_or("Target commit not found in database")?;
    
    // Find thread containing this patch
    use sqlx::Row;
    let thread_row: Option<(i64,)> = sqlx::query_as(
        "SELECT thread_id FROM patch_replies WHERE patch_id = $1"
    )
    .bind(target_patch.patch_id)
    .fetch_optional(pool)
    .await?;
    
    let thread_id = match thread_row {
        Some((id,)) => id,
        None => {
            println!("WARNING: Target patch not found in any thread!");
            return Ok(None);
        }
    };
    
    println!("\nFound thread_id: {}", thread_id);
    
    // Query all patches in this thread
    let reply_rows = sqlx::query(
        "SELECT pr.patch_id, pr.parent_patch_id, pr.depth_level, pr.position_in_thread
         FROM patch_replies pr
         WHERE pr.thread_id = $1
         ORDER BY pr.position_in_thread ASC"
    )
    .bind(thread_id)
    .fetch_all(pool)
    .await?;
    
    println!("Found {} messages in thread", reply_rows.len());
    
    // Build patch map
    let patch_map: HashMap<i64, DbPatch> = patches.iter()
        .map(|p| (p.patch_id, p.clone()))
        .collect();
    
    // Build parent -> children map
    let mut children_map: HashMap<i64, Vec<(i64, i32)>> = HashMap::new();
    let mut root_id = None;
    let mut depth_map: HashMap<i64, i32> = HashMap::new();
    
    for row in &reply_rows {
        let patch_id: i64 = row.get(0);
        let parent_id: Option<i64> = row.get(1);
        let depth: i32 = row.get(2);
        
        depth_map.insert(patch_id, depth);
        
        if parent_id.is_none() {
            root_id = Some(patch_id);
        } else if let Some(parent) = parent_id {
            children_map.entry(parent)
                .or_insert_with(Vec::new)
                .push((patch_id, depth));
        }
    }
    
    // Build tree recursively
    fn build_node(
        patch_id: i64,
        patch_map: &HashMap<i64, DbPatch>,
        children_map: &HashMap<i64, Vec<(i64, i32)>>,
        depth_map: &HashMap<i64, i32>
    ) -> Option<DbThreadNode> {
        let patch = patch_map.get(&patch_id)?.clone();
        let depth = depth_map.get(&patch_id).copied().unwrap_or(0);
        
        let mut children = Vec::new();
        if let Some(child_ids) = children_map.get(&patch_id) {
            for (child_id, _) in child_ids {
                if let Some(child_node) = build_node(*child_id, patch_map, children_map, depth_map) {
                    children.push(child_node);
                }
            }
        }
        
        // Find parent from children_map
        let parent_patch_id = children_map.iter()
            .find(|(_, children)| children.iter().any(|(id, _)| *id == patch_id))
            .map(|(parent_id, _)| *parent_id);
        
        Some(DbThreadNode {
            patch,
            depth,
            parent_patch_id,
            children,
        })
    }
    
    if let Some(root) = root_id {
        Ok(build_node(root, &patch_map, &children_map, &depth_map))
    } else {
        Ok(None)
    }
}

/// Display thread tree from database (matching format of git-only test)
fn display_db_thread_tree(root: &DbThreadNode, target_commit: &str) {
    println!("\n=== Database Thread Tree Structure ===\n");
    print_db_node(root, target_commit);
    
    // Calculate statistics
    let (total, reply_count, max_depth) = count_nodes(root);
    
    println!("\n=== Database Thread Statistics ===");
    println!("Total messages: {}", total);
    println!("Max depth: {}", max_depth);
    println!("Replies: {}", reply_count);
    
    if let Some(target_depth) = find_depth(root, target_commit) {
        println!("\nTarget commit depth: {}", target_depth);
        if target_depth >= 2 {
            println!("✓ Properly nested reply structure detected!");
        }
    }
}

fn print_db_node(node: &DbThreadNode, target_commit: &str) {
    let indent = "│   ".repeat(node.depth as usize);
    let connector = if node.depth == 0 { "" } else { "├── " };
    let is_target = node.patch.commit_hash == target_commit;
    let marker = if is_target { " ← TARGET" } else { "" };
    
    println!("{}{}[depth={}] {}", indent, connector, node.depth, node.patch.subject);
    println!("{}    Author: {}", indent, node.patch.author);
    println!("{}    Commit: {}{}", indent, &node.patch.commit_hash[..12], marker);
    
    if node.patch.is_series {
        if let (Some(num), Some(total)) = (node.patch.series_number, node.patch.series_total) {
            println!("{}    Series: {}/{}", indent, num, total);
        }
    }
    
    println!("{}    Is Reply: {}", indent, node.patch.is_reply);
    
    if !node.children.is_empty() {
        println!("{}    └─ {} replies", indent, node.children.len());
        for child in &node.children {
            print_db_node(child, target_commit);
        }
    }
    println!();
}

fn count_nodes(node: &DbThreadNode) -> (usize, usize, i32) {
    let mut total = 1;
    let mut reply_count = if node.patch.is_reply { 1 } else { 0 };
    let mut max_depth = node.depth;
    
    for child in &node.children {
        let (child_total, child_replies, child_depth) = count_nodes(child);
        total += child_total;
        reply_count += child_replies;
        max_depth = max_depth.max(child_depth);
    }
    
    (total, reply_count, max_depth)
}

fn find_depth(node: &DbThreadNode, target_commit: &str) -> Option<i32> {
    if node.patch.commit_hash == target_commit {
        return Some(node.depth);
    }
    
    for child in &node.children {
        if let Some(depth) = find_depth(child, target_commit) {
            return Some(depth);
        }
    }
    
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_database_threading() {
        let target_commit = "776c1383cea5ea53c33dafa7391dfe4ad1c4fd19";
        let search_depth = 2000;
        
        println!("\n{}", "=".repeat(60));
        println!("Testing Database Threading with Nested Reply");
        println!("{}", "=".repeat(60));
        println!("Target: {}", target_commit);
        println!("Search depth: {} commits\n", search_depth);
        
        // Setup database
        let config = crate::database::DatabaseConfig::from_env();
        let mut db = DatabaseManager::new(config);
        
        match setup_test_commits(&mut db, target_commit, search_depth).await {
            Ok(patches) => {
                println!("\n{}", "=".repeat(60));
                println!("Database populated successfully");
                println!("Total patches in DB: {}", patches.len());
                
                // Query and display thread tree
                match query_thread_tree_from_db(&mut db, target_commit, &patches).await {
                    Ok(Some(root)) => {
                        display_db_thread_tree(&root, target_commit);
                        
                        println!("\n{}", "=".repeat(60));
                        println!("Test Complete!");
                        println!("{}", "=".repeat(60));
                    }
                    Ok(None) => {
                        eprintln!("✗ No thread tree found!");
                    }
                    Err(e) => {
                        eprintln!("✗ Error querying thread tree: {}", e);
                    }
                }
            }
            Err(e) => {
                eprintln!("✗ Error setting up database: {}", e);
            }
        }
    }
}

