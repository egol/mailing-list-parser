/// Full threading test - Tests threading logic without database
use std::collections::HashMap;
use crate::git_parser::{get_email_content, get_commit_metadata, get_all_commits_with_limit};
use crate::mail_parser::{parse_email_from_content, parse_email_headers};

#[derive(Debug, Clone)]
struct ParsedPatch {
    commit_hash: String,
    message_id: String,
    subject: String,
    author: String,
    in_reply_to: Option<String>,
    references: Vec<String>,
    is_reply: bool,
}

/// Find commits by their message IDs
pub async fn find_commits_by_message_ids(
    target_commit: &str,
    search_limit: usize
) -> Result<Vec<ParsedPatch>, Box<dyn std::error::Error>> {
    println!("\n=== Analyzing Target Commit ===");
    
    // Parse target commit first
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
    
    // Search through commits
    let all_commits = get_all_commits_with_limit(Some(search_limit))?;
    let mut found_patches = Vec::new();
    let mut msg_id_to_commit: HashMap<String, String> = HashMap::new();
    
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
                
                let in_reply_to = headers.get("in-reply-to")
                    .map(|id| id.trim_start_matches('<').trim_end_matches('>').to_string());
                
                let references: Vec<String> = headers.get("references")
                    .map(|refs| {
                        refs.split_whitespace()
                            .map(|id| id.trim_start_matches('<').trim_end_matches('>').to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                
                let is_reply = metadata.subject.trim().to_lowercase().starts_with("re:");
                
                let patch = ParsedPatch {
                    commit_hash: commit.clone(),
                    message_id: clean_id.clone(),
                    subject: metadata.subject.clone(),
                    author: metadata.author_name.clone(),
                    in_reply_to,
                    references,
                    is_reply,
                };
                
                println!("  ✓ Found: {} ({})", &commit[..12], &metadata.subject[..60.min(metadata.subject.len())]);
                
                msg_id_to_commit.insert(clean_id, commit);
                found_patches.push(patch);
            }
        }
    }
    
    println!("\nFound {} commits in thread", found_patches.len());
    Ok(found_patches)
}

/// Build threading structure from patches
pub fn build_thread_structure(patches: &[ParsedPatch]) -> (HashMap<String, Vec<String>>, HashMap<String, usize>) {
    // Build message_id -> patch mapping
    let mut msg_to_patch: HashMap<String, &ParsedPatch> = HashMap::new();
    for patch in patches {
        msg_to_patch.insert(patch.message_id.clone(), patch);
    }
    
    // Build parent -> children map
    let mut children_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut parent_map: HashMap<String, String> = HashMap::new();
    
    // Find all non-reply patches first (potential roots)
    let non_replies: Vec<&ParsedPatch> = patches.iter().filter(|p| !p.is_reply).collect();
    
    // Build parent-child relationships for ALL patches (not just replies)
    // Patch series members also need to be linked to their parent
    for patch in patches {
        // Skip if this patch has no references (it's a root)
        if patch.in_reply_to.is_none() && patch.references.is_empty() {
            continue;
        }
        
        // Try In-Reply-To first
        let mut parent_id = patch.in_reply_to.as_ref()
            .and_then(|id| msg_to_patch.get(id).map(|_| id.clone()));
        
        // Fall back to references (walk backwards to find most recent ancestor)
        if parent_id.is_none() && !patch.references.is_empty() {
            for ref_id in patch.references.iter().rev() {
                if msg_to_patch.contains_key(ref_id) {
                    parent_id = Some(ref_id.clone());
                    break;
                }
            }
        }
        
        if let Some(parent) = parent_id {
            children_map.entry(parent.clone())
                .or_insert_with(Vec::new)
                .push(patch.message_id.clone());
            parent_map.insert(patch.message_id.clone(), parent.clone());
            let patch_type = if patch.is_reply { "Reply" } else { "Patch" };
            println!("  {} {} -> parent {}", patch_type, &patch.subject[..40.min(patch.subject.len())], &parent[..30.min(parent.len())]);
        }
    }
    
    println!("\nChildren map has {} parents with children", children_map.len());
    
    // Find true root - the non-reply that doesn't reference any other message in our set
    // The root is the earliest message that starts the thread
    let mut root_id: Option<String> = None;
    
    // First, try to find a non-reply that doesn't reference anything in our set
    for patch in &non_replies {
        // Check if this patch references any other message in our set
        let references_our_set = patch.in_reply_to.as_ref()
            .map(|id| msg_to_patch.contains_key(id))
            .unwrap_or(false)
            || patch.references.iter().any(|ref_id| msg_to_patch.contains_key(ref_id));
        
        if !references_our_set {
            // This is a true root - doesn't reference anything in our set
            root_id = Some(patch.message_id.clone());
            println!("\n=== Thread Root ===");
            println!("Message-ID: {}", patch.message_id);
            println!("Subject: {}", patch.subject);
            println!("Author: {}", patch.author);
            break;
        }
    }
    
    // If still no root found, find the one that others reference most
    if root_id.is_none() {
        for patch in &non_replies {
            let is_referenced = patches.iter().any(|p| {
                p.in_reply_to.as_ref() == Some(&patch.message_id) ||
                p.references.contains(&patch.message_id)
            });
            
            if is_referenced {
                root_id = Some(patch.message_id.clone());
                println!("\n=== Thread Root (most referenced) ===");
                println!("Message-ID: {}", patch.message_id);
                println!("Subject: {}", patch.subject);
                println!("Author: {}", patch.author);
                break;
            }
        }
    }
    
    // If still no root found, use first non-reply
    if root_id.is_none() && !non_replies.is_empty() {
        root_id = Some(non_replies[0].message_id.clone());
    }
    
    // Calculate depths using BFS from root
    let mut depth_map: HashMap<String, usize> = HashMap::new();
    let mut queue: Vec<(String, usize)> = Vec::new();
    
    if let Some(root) = root_id {
        println!("\nBuilding depth map from root: {}", &root[..30.min(root.len())]);
        queue.push((root.clone(), 0));
        depth_map.insert(root, 0);
        
        while let Some((current_id, depth)) = queue.pop() {
            if let Some(children) = children_map.get(&current_id) {
                println!("  Node at depth {} has {} children", depth, children.len());
                for child_id in children {
                    depth_map.insert(child_id.clone(), depth + 1);
                    queue.push((child_id.clone(), depth + 1));
                }
            }
        }
        println!("Depth map has {} entries", depth_map.len());
    }
    
    // Handle orphaned messages (no parent in our set)
    for patch in patches {
        if !depth_map.contains_key(&patch.message_id) {
            depth_map.insert(patch.message_id.clone(), 0);
        }
    }
    
    (children_map, depth_map)
}

/// Display thread tree
pub fn display_thread_tree(
    patches: &[ParsedPatch],
    children_map: &HashMap<String, Vec<String>>,
    depth_map: &HashMap<String, usize>,
    target_msg_id: &str
) {
    // Find root - the one with depth 0
    let root = patches.iter()
        .find(|p| depth_map.get(&p.message_id).copied().unwrap_or(999) == 0);
    
    if let Some(root) = root {
        println!("\n=== Thread Tree Structure ===\n");
        print_tree_node(root, patches, children_map, depth_map, 0, target_msg_id);
        
        // Display statistics
        println!("\n=== Thread Statistics ===");
        println!("Total messages: {}", patches.len());
        println!("Max depth: {}", depth_map.values().max().unwrap_or(&0));
        
        let reply_count = patches.iter().filter(|p| p.is_reply).count();
        println!("Replies: {}", reply_count);
        
        if let Some(target_depth) = depth_map.get(target_msg_id) {
            println!("\nTarget commit depth: {}", target_depth);
            if *target_depth >= 2 {
                println!("✓ Properly nested reply structure detected!");
            }
        }
    }
}

fn print_tree_node(
    patch: &ParsedPatch,
    all_patches: &[ParsedPatch],
    children_map: &HashMap<String, Vec<String>>,
    depth_map: &HashMap<String, usize>,
    _depth: usize,  // Unused - we get depth from depth_map
    target_msg_id: &str
) {
    let actual_depth = depth_map.get(&patch.message_id).copied().unwrap_or(0);
    let indent = "│   ".repeat(actual_depth);
    let connector = if actual_depth == 0 { "" } else { "├── " };
    let is_target = patch.message_id == target_msg_id;
    let marker = if is_target { " ← TARGET" } else { "" };
    
    println!("{}{}[depth={}] {}", indent, connector, actual_depth, patch.subject);
    println!("{}    Author: {}", indent, patch.author);
    println!("{}    Commit: {}{}", indent, &patch.commit_hash[..12], marker);
    
    if let Some(in_reply_to) = &patch.in_reply_to {
        println!("{}    In-Reply-To: {}...", indent, &in_reply_to[..30.min(in_reply_to.len())]);
    }
    
    if !patch.references.is_empty() {
        println!("{}    References: {} message IDs", indent, patch.references.len());
    }
    
    // Print children
    if let Some(children) = children_map.get(&patch.message_id) {
        println!("{}    └─ {} replies", indent, children.len());
        for child_id in children {
            if let Some(child) = all_patches.iter().find(|p| &p.message_id == child_id) {
                print_tree_node(child, all_patches, children_map, depth_map, 0, target_msg_id);
            }
        }
    }
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_nested_reply_structure() {
        let target_commit = "776c1383cea5ea53c33dafa7391dfe4ad1c4fd19";
        let search_depth = 2000;
        
        println!("\n{}", "=".repeat(60));
        println!("Testing Threading Logic with Nested Reply");
        println!("{}", "=".repeat(60));
        println!("Target: {}", target_commit);
        println!("Search depth: {} commits\n", search_depth);
        
        match find_commits_by_message_ids(target_commit, search_depth).await {
            Ok(patches) => {
                if patches.is_empty() {
                    eprintln!("✗ No related commits found!");
                    return;
                }
                
                // Find target message ID
                let target_msg_id = patches.iter()
                    .find(|p| p.commit_hash == target_commit)
                    .map(|p| p.message_id.clone())
                    .unwrap_or_default();
                
                // Build thread structure
                let (children_map, depth_map) = build_thread_structure(&patches);
                
                // Display results
                display_thread_tree(&patches, &children_map, &depth_map, &target_msg_id);
                
                println!("\n{}", "=".repeat(60));
                println!("Test Complete!");
                println!("{}", "=".repeat(60));
            }
            Err(e) => {
                eprintln!("✗ Error: {}", e);
            }
        }
    }
}
