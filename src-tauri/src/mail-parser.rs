use std::collections::HashMap;
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EmailInfo {
    pub commit_hash: String,
    pub subject: String,
    pub normalized_subject: String,  // Parser-normalized subject
    pub from: String,
    pub to: String,
    pub date: String,
    pub message_id: String,
    pub body: String,
    pub headers: HashMap<String, String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ParseError {
    pub message: String,
}

impl std::error::Error for ParseError {}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl From<std::io::Error> for ParseError {
    fn from(error: std::io::Error) -> Self {
        ParseError {
            message: format!("IO error: {}", error),
        }
    }
}

// Implement Send + Sync for Tauri compatibility
unsafe impl Send for ParseError {}
unsafe impl Sync for ParseError {}

/// Parse email headers from raw email content
/// Returns a HashMap of header field -> value
/// Handles multi-line headers (header folding)
pub fn parse_email_headers(email_content: &str) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    let header_regex = Regex::new(r"^([A-Za-z-]+):\s*(.+)$").unwrap();
    let mut current_header: Option<(String, String)> = None;

    for line in email_content.lines() {
        // Stop parsing headers when we hit an empty line
        if line.trim().is_empty() {
            // Save any pending header
            if let Some((key, value)) = current_header.take() {
                headers.insert(key, value);
            }
            break;
        }

        // Check if this is a continuation line (starts with whitespace)
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some((ref _key, ref mut value)) = current_header {
                // Append to current header value
                value.push(' ');
                value.push_str(line.trim());
            }
            continue;
        }

        // Save previous header if any
        if let Some((key, value)) = current_header.take() {
            headers.insert(key, value);
        }

        // Try to parse new header
        if let Some(captures) = header_regex.captures(line.trim()) {
            if let (Some(key), Some(value)) = (captures.get(1), captures.get(2)) {
                current_header = Some((
                    key.as_str().to_lowercase(),
                    value.as_str().trim().to_string()
                ));
            }
        }
    }

    // Save any remaining header
    if let Some((key, value)) = current_header {
        headers.insert(key, value);
    }

    headers
}

/// Extract the email body from raw email content
/// Everything after the headers until the end
pub fn extract_email_body(email_content: &str) -> String {
    let mut in_body = false;
    let mut body_lines = Vec::new();

    for line in email_content.lines() {
        if in_body {
            body_lines.push(line);
        } else if line.trim().is_empty() {
            in_body = true;
        }
    }

    body_lines.join("\n").trim().to_string()
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
    let whitespace_regex = Regex::new(r"\s+").unwrap();
    normalized = whitespace_regex.replace_all(&normalized, " ").to_string();

    normalized.trim().to_string()
}

/// Sanitize string for PostgreSQL - remove null bytes and invalid UTF-8
fn sanitize_string(s: &str) -> String {
    s.chars()
        .filter(|c| *c != '\0') // Remove null bytes
        .collect::<String>()
        .replace('\u{FFFD}', "") // Remove replacement characters
}

/// Parse complete email information from commit hash and email content
pub fn parse_email_from_content(commit_hash: &str, email_content: &str) -> Result<EmailInfo, ParseError> {
    let headers = parse_email_headers(email_content);
    let body = extract_email_body(email_content);

    let subject = headers.get("subject").cloned().unwrap_or_else(|| "No subject".to_string());
    let normalized_subject = normalize_subject(&subject);

    let email_info = EmailInfo {
        commit_hash: commit_hash.to_string(),
        subject: sanitize_string(&subject),
        normalized_subject: sanitize_string(&normalized_subject),
        from: sanitize_string(&headers.get("from").cloned().unwrap_or_else(|| "Unknown".to_string())),
        to: sanitize_string(&headers.get("to").cloned().unwrap_or_else(|| "Unknown".to_string())),
        date: sanitize_string(&headers.get("date").cloned().unwrap_or_else(|| "Unknown".to_string())),
        message_id: sanitize_string(&headers.get("message-id").cloned().unwrap_or_else(|| format!("commit-{}", commit_hash))),
        body: sanitize_string(&body),
        headers,
    };

    Ok(email_info)
}

/// Parse multiple emails in parallel from commit hash/content pairs
/// Returns (successful_emails, errors)
pub async fn parse_emails_parallel(emails: Vec<(String, String)>) -> (Vec<(String, EmailInfo)>, Vec<String>) {
    use futures::future;
    
    let mut parse_handles = Vec::new();
    
    for (commit_hash, email_content) in emails {
        let handle = tokio::spawn(async move {
            match parse_email_from_content(&commit_hash, &email_content) {
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