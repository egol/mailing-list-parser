use std::collections::HashMap;
use regex::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use once_cell::sync::Lazy;
use mailparse::parse_mail;
use crate::git_parser::CommitMetadata;

// Lazy-compiled regexes for performance
static WHITESPACE_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());
static EMAIL_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"<([^>]+)>").unwrap());

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EmailInfo {
    pub commit_hash: String,
    pub subject: String,
    pub normalized_subject: String,
    pub from: String,
    // Normalized author info
    pub author_email: String,        // Normalized lowercase email
    pub author_first_name: String,   // Parsed first name
    pub author_last_name: Option<String>, // Parsed last name
    pub author_display_name: String, // "First Last" for display
    // Other fields
    pub to: String,
    pub date: String,
    pub message_id: String,
    pub body: String,
    pub headers: HashMap<String, String>,
    // Threading fields
    pub in_reply_to: Option<String>,    // Message-ID of parent
    pub references: Vec<String>,        // Full thread chain
    pub is_reply: bool,                 // Quick flag
}

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Mail parse error: {0}")]
    MailParse(#[from] mailparse::MailParseError),
    
    #[error("Parse error: {0}")]
    Parse(String),
}

/// Normalize subject line for threading/comparison
/// Removes Re:, Fwd:, etc., normalizes whitespace, and lowercases
pub fn normalize_subject(subject: &str) -> String {
    let mut normalized = subject.trim().to_lowercase();

    // Remove common reply/forward prefixes (case-insensitive)
    let prefixes = ["re:", "fwd:", "fw:", "aw:", "[patch]", "[rfc]"];
    loop {
        let mut changed = false;
        for prefix in &prefixes {
            if normalized.starts_with(prefix) {
                normalized = normalized[prefix.len()..].trim_start().to_string();
                changed = true;
                break;
            }
        }
        if !changed {
            break;
        }
    }

    // Normalize whitespace: replace multiple spaces with single space
    normalized = WHITESPACE_REGEX.replace_all(&normalized, " ").to_string();

    normalized.trim().to_string()
}

/// Extract email address from From/To header and normalize to lowercase
pub fn extract_email(from_header: &str) -> String {
    let email = if let Some(captures) = EMAIL_REGEX.captures(from_header) {
        captures.get(1).unwrap().as_str()
    } else {
        from_header
    };
    // Normalize to lowercase to match CITEXT column behavior
    email.trim().to_lowercase()
}

/// Extract and normalize name from From header
/// Removes quotes, extra whitespace, and special characters
pub fn extract_name(from_header: &str) -> String {
    let name = from_header.split('<').next().unwrap_or(from_header).trim();
    normalize_name(name)
}

/// Normalize a name string by removing quotes, extra whitespace, and unwanted symbols
pub fn normalize_name(name: &str) -> String {
    name
        .replace('"', "")           // Remove quotes
        .replace('\'', "")          // Remove single quotes
        .replace('`', "")           // Remove backticks
        .replace(['(', ')'], "")    // Remove parentheses
        .replace(['[', ']'], "")    // Remove brackets
        .trim()                     // Remove leading/trailing whitespace
        .split_whitespace()         // Split on any whitespace
        .collect::<Vec<_>>()
        .join(" ")                  // Join with single space
}

/// Parse name into first and last name components
/// Returns (first_name, last_name, display_name)
/// Handles company suffixes and malformed names
pub fn parse_name_components(full_name: &str) -> (String, Option<String>, String) {
    let normalized = normalize_name(full_name);
    let parts: Vec<&str> = normalized.split_whitespace().collect();

    if parts.is_empty() {
        // Fallback for empty names
        eprintln!("Warning: Empty name after normalization: '{}'", full_name);
        return ("Unknown".to_string(), None, "Unknown".to_string());
    }

    if parts.len() == 1 {
        // Single name (bot, username, etc.)
        (parts[0].to_string(), None, parts[0].to_string())
    } else {
        // Multiple parts: first is first_name, rest is last_name
        let first_name = parts[0].to_string();
        let last_name = parts[1..].join(" ");
        let display_name = format!("{} {}", first_name, last_name);
        (first_name, Some(last_name), display_name)
    }
}

/// Sanitize string for PostgreSQL - remove null bytes and invalid UTF-8
fn sanitize_string(s: &str) -> String {
    s.chars()
        .filter(|c| *c != '\0') // Remove null bytes
        .collect::<String>()
        .replace('\u{FFFD}', "") // Remove replacement characters
}

/// Clean up Message-ID (remove < > brackets)
fn sanitize_message_id(msg_id: &str) -> String {
    msg_id.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_string()
}

/// Parse threading headers from email
/// Returns (in_reply_to, references, is_reply)
fn parse_threading_info(headers: &HashMap<String, String>, subject: &str) -> (Option<String>, Vec<String>, bool) {
    // Get In-Reply-To header
    let in_reply_to = headers.get("in-reply-to")
        .map(|s| sanitize_message_id(s));
    
    // Parse References header (space-separated Message-IDs)
    let references: Vec<String> = headers.get("references")
        .map(|refs| {
            refs.split_whitespace()
                .map(|id| sanitize_message_id(id))
                .filter(|id| !id.is_empty())
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();
    
    // Determine is_reply based on Subject header (Re: prefix)
    // Patch series members have In-Reply-To but are NOT replies
    let is_reply = subject.trim().to_lowercase().starts_with("re:");
    
    (in_reply_to, references, is_reply)
}

/// Parse complete email information from commit hash and email content
/// Uses commit metadata for author and subject information (much more reliable)
/// Now uses mailparse crate for proper email parsing and decoding
pub fn parse_email_from_content(commit_hash: &str, email_content: &str, metadata: &CommitMetadata) -> Result<EmailInfo, ParseError> {
    // Use mailparse crate to properly parse the email
    let parsed = parse_mail(email_content.as_bytes())?;
    
    // Extract headers into HashMap
    let headers: HashMap<String, String> = parsed.headers
        .iter()
        .map(|h| (h.get_key().to_lowercase(), h.get_value()))
        .collect();
    
    // Get body - mailparse automatically decodes based on Content-Transfer-Encoding!
    let body = parsed.get_body().unwrap_or_default();

    // Use commit metadata for subject (much more reliable than email headers)
    let subject = &metadata.subject;
    let normalized_subject = normalize_subject(subject);

    // Use commit metadata for author information (most reliable source)
    let author_email = metadata.author_email.to_lowercase(); // Already normalized in CommitMetadata
    let author_name = &metadata.author_name;
    let (author_first_name, author_last_name, author_display_name) = parse_name_components(author_name);
    
    // Format from_header for compatibility
    let from_header = format!("{} <{}>", author_name, author_email);

    // Parse threading information
    let (in_reply_to, references, is_reply) = parse_threading_info(&headers, subject);

    let email_info = EmailInfo {
        commit_hash: commit_hash.to_string(),
        subject: sanitize_string(subject),
        normalized_subject: sanitize_string(&normalized_subject),
        from: sanitize_string(&from_header),
        // Normalized author info from commit metadata
        author_email,
        author_first_name,
        author_last_name,
        author_display_name,
        // Other fields from email headers
        to: sanitize_string(&headers.get("to").cloned().unwrap_or_else(|| "Unknown".to_string())),
        date: sanitize_string(&headers.get("date").cloned().unwrap_or_else(|| "Unknown".to_string())),
        message_id: sanitize_message_id(&headers.get("message-id").cloned().unwrap_or_else(|| format!("commit-{}", commit_hash))),
        body: sanitize_string(&body),
        headers: headers.clone(),
        // Threading fields
        in_reply_to,
        references,
        is_reply,
    };

    Ok(email_info)
}

/// Parse multiple emails in parallel from commit hash/content/metadata tuples
/// Returns (successful_emails, errors)
pub async fn parse_emails_parallel(emails: Vec<(String, String, CommitMetadata)>) -> (Vec<(String, EmailInfo)>, Vec<String>) {
    use futures::future;
    
    let mut parse_handles = Vec::new();
    
    for (commit_hash, email_content, metadata) in emails {
        let handle = tokio::spawn(async move {
            match parse_email_from_content(&commit_hash, &email_content, &metadata) {
                Ok(email_info) => Ok((commit_hash, email_info)),
                Err(e) => Err(format!("Error parsing commit {}: {}", commit_hash, e)),
            }
        });
        parse_handles.push(handle);
    }
    
    let results = future::join_all(parse_handles).await;
    let mut parsed_emails = Vec::new();
    let mut errors = Vec::new();
    
    for result in results {
        match result {
            Ok(Ok(email)) => parsed_emails.push(email),
            Ok(Err(e)) => errors.push(e),
            Err(e) => errors.push(format!("Task error: {}", e)),
        }
    }
    
    (parsed_emails, errors)
}
