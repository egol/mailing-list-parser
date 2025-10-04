/// Test script for threading logic with deeply nested replies
use std::collections::HashMap;
use mailparse::parse_mail;

// Re-export the functions we need to test
use crate::git_parser::{get_email_content, get_single_commit_metadata};
use crate::mail_parser::parse_email_from_content;

/// Parse quote depth from body text
/// Returns a map of quote_depth -> content
pub fn analyze_quote_structure(body: &str) -> Vec<(usize, String)> {
    let mut quote_levels = Vec::new();
    
    for line in body.lines() {
        let trimmed = line.trim_start();
        let mut depth = 0;
        let mut content = trimmed;
        
        // Count leading '>' characters
        while content.starts_with('>') {
            depth += 1;
            content = content[1..].trim_start();
        }
        
        if !content.is_empty() {
            quote_levels.push((depth, content.to_string()));
        }
    }
    
    quote_levels
}

/// Extract the reply chain from quoted text
/// Returns parent message IDs or subjects in order from oldest to newest
pub fn extract_reply_chain_from_quotes(body: &str) -> Vec<String> {
    let quote_levels = analyze_quote_structure(body);
    let mut chain = Vec::new();
    let mut seen_subjects = std::collections::HashSet::new();
    
    // Look for subject lines in quotes (format: "On <date>, <author> wrote:")
    // or message-id references
    for (depth, content) in quote_levels {
        // Look for "On ... wrote:" pattern which indicates a reply context
        if content.contains("wrote:") && content.contains("On ") {
            if !seen_subjects.contains(&content) {
                chain.push(format!("depth={}: {}", depth, content));
                seen_subjects.insert(content.clone());
            }
        }
    }
    
    chain
}

/// Analyze a specific commit's threading structure
pub async fn analyze_commit_threading(commit_hash: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== Analyzing commit {} ===\n", commit_hash);
    
    // 1. Get raw email content
    let email_content = get_email_content(commit_hash)?;
    println!("Email content length: {} bytes\n", email_content.len());
    
    // 2. Parse email using mailparse
    let parsed = parse_mail(email_content.as_bytes())?;
    
    // Extract headers
    println!("=== Headers ===");
    for header in parsed.headers.iter() {
        let key = header.get_key().to_lowercase();
        if key == "subject" || key == "in-reply-to" || key == "references" || key == "message-id" {
            println!("{}: {}", key, header.get_value());
        }
    }
    
    // 3. Get metadata
    let metadata = get_single_commit_metadata(commit_hash)?;
    println!("\n=== Metadata ===");
    println!("Author: {} <{}>", metadata.author_name, metadata.author_email);
    println!("Subject: {}", metadata.subject);
    
    // 4. Parse full email
    let email_info = parse_email_from_content(commit_hash, &email_content, &metadata)?;
    println!("\n=== Parsed Email ===");
    println!("Is Reply: {}", email_info.is_reply);
    println!("In-Reply-To: {:?}", email_info.in_reply_to);
    println!("References: {:?}", email_info.references);
    
    // 5. Analyze body quote structure
    let body = parsed.get_body()?;
    println!("\n=== Quote Structure Analysis ===");
    let quote_levels = analyze_quote_structure(&body);
    
    // Group by depth
    let mut depth_map: HashMap<usize, Vec<String>> = HashMap::new();
    for (depth, content) in quote_levels {
        depth_map.entry(depth).or_insert_with(Vec::new).push(content);
    }
    
    println!("Quote depth distribution:");
    let mut depths: Vec<_> = depth_map.keys().collect();
    depths.sort();
    for depth in depths {
        let lines = &depth_map[depth];
        println!("  Depth {}: {} lines", depth, lines.len());
        
        // Show first few lines as sample
        for (i, line) in lines.iter().take(3).enumerate() {
            let preview = if line.len() > 80 {
                format!("{}...", &line[..77])
            } else {
                line.clone()
            };
            println!("    [{}] {}", i, preview);
        }
    }
    
    // 6. Extract reply chain from quotes
    println!("\n=== Reply Chain Extraction ===");
    let reply_chain = extract_reply_chain_from_quotes(&body);
    println!("Found {} context markers:", reply_chain.len());
    for (i, context) in reply_chain.iter().enumerate() {
        println!("  [{}] {}", i, context);
    }
    
    // 7. Show body preview
    println!("\n=== Body Preview (first 500 chars) ===");
    let preview = body.chars().take(500).collect::<String>();
    println!("{}", preview);
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_deeply_nested_reply() {
        let commit_hash = "776c1383cea5ea53c33dafa7391dfe4ad1c4fd19";
        match analyze_commit_threading(commit_hash).await {
            Ok(_) => println!("\nAnalysis complete!"),
            Err(e) => eprintln!("Error: {}", e),
        }
    }
}

