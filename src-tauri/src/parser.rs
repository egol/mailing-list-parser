use crate::models::{Email, Result, ParserError};
use mailparse::{ParsedMail, MailHeaderMap};
use chrono::{DateTime, Utc};
use regex::Regex;
use std::collections::HashMap;

/// Email parser for mailing list messages
pub struct EmailParser {
    patch_regex: Regex,
    patch_version_regex: Regex,
    message_id_regex: Regex,
}

impl EmailParser {
    /// Create a new email parser
    pub fn new() -> Result<Self> {
        Ok(EmailParser {
            patch_regex: Regex::new(r"(?i)\[.*patch.*\]").map_err(|e| {
                ParserError::EmailParsing(format!("Failed to create patch regex: {}", e))
            })?,
            patch_version_regex: Regex::new(r"(?i)v(\d+)|patch v(\d+)|version (\d+)").map_err(|e| {
                ParserError::EmailParsing(format!("Failed to create version regex: {}", e))
            })?,
            message_id_regex: Regex::new(r"<([^>]+)>").map_err(|e| {
                ParserError::EmailParsing(format!("Failed to create message ID regex: {}", e))
            })?,
        })
    }

    /// Parse an email from raw text content
    pub fn parse_email(&self, raw_content: &str) -> Result<Email> {
        let parsed = mailparse::parse_mail(raw_content.as_bytes()).map_err(|e| {
            ParserError::EmailParsing(format!("Failed to parse email: {}", e))
        })?;

        let headers = parsed.get_headers();
        let message_id = self.extract_message_id(&headers)?;
        let subject = self.extract_subject(&headers)?;
        let from = self.extract_from(&headers)?;
        let (to, cc) = self.extract_recipients(&headers)?;
        let date = self.extract_date(&headers)?;
        let body = self.extract_body(&parsed)?;
        let (references, in_reply_to) = self.extract_references(&headers)?;
        let (is_patch, patch_info) = self.analyze_patch_status(&subject, &body)?;

        Ok(Email {
            id: message_id.clone(),
            message_id,
            subject,
            from,
            to,
            cc,
            date,
            body,
            references,
            in_reply_to,
            patch_number: patch_info.patch_number,
            patch_version: patch_info.version,
            is_patch,
            patch_filename: patch_info.filename,
            commit_hash: None, // Will be set when storing from git
        })
    }

    /// Parse email from git commit content
    pub fn parse_from_git_commit(&self, commit_hash: &str, raw_content: &str) -> Result<Email> {
        let mut email = self.parse_email(raw_content)?;
        email.commit_hash = Some(commit_hash.to_string());
        Ok(email)
    }

    /// Extract Message-ID from headers
    fn extract_message_id(&self, headers: &impl MailHeaderMap) -> Result<String> {
        let raw_message_id = headers.get_first_value("Message-ID")
            .or_else(|| headers.get_first_value("Message-Id"))
            .ok_or_else(|| ParserError::EmailParsing("No Message-ID found".to_string()))?;

        if raw_message_id.is_empty() {
            return Err(ParserError::EmailParsing("Empty Message-ID".to_string()));
        }

        // Use regex to extract the message ID from angle brackets if present
        if let Some(captures) = self.message_id_regex.captures(&raw_message_id) {
            if let Some(message_id) = captures.get(1) {
                return Ok(message_id.as_str().to_string());
            }
        }

        // If no angle brackets found, return as-is
        Ok(raw_message_id)
    }

    /// Extract Subject from headers
    fn extract_subject(&self, headers: &impl MailHeaderMap) -> Result<String> {
        let subject = headers.get_first_value("Subject")
            .ok_or_else(|| ParserError::EmailParsing("No Subject found".to_string()))?;

        if subject.is_empty() {
            return Err(ParserError::EmailParsing("Empty Subject".to_string()));
        }

        Ok(subject)
    }

    /// Extract From from headers
    fn extract_from(&self, headers: &impl MailHeaderMap) -> Result<String> {
        let from = headers.get_first_value("From")
            .ok_or_else(|| ParserError::EmailParsing("No From found".to_string()))?;

        if from.is_empty() {
            return Err(ParserError::EmailParsing("Empty From".to_string()));
        }

        Ok(from)
    }

    /// Extract To and CC recipients
    fn extract_recipients(&self, headers: &impl MailHeaderMap) -> Result<(Vec<String>, Vec<String>)> {
        let to = headers.get_all_values("To");

        let cc = headers.get_all_values("Cc");

        Ok((to, cc))
    }

    /// Extract date from headers
    fn extract_date(&self, headers: &impl MailHeaderMap) -> Result<DateTime<Utc>> {
        let date_str = headers.get_first_value("Date")
            .ok_or_else(|| ParserError::EmailParsing("No Date found".to_string()))?;

        if date_str.is_empty() {
            return Err(ParserError::EmailParsing("Empty Date".to_string()));
        }

        // Try to parse various date formats
        Ok(DateTime::parse_from_rfc2822(&date_str)
            .or_else(|_| DateTime::parse_from_rfc3339(&date_str))
            .map_err(|e| ParserError::EmailParsing(format!("Failed to parse date '{}': {}", date_str, e)))?
            .with_timezone(&Utc))
    }

    /// Extract References and In-Reply-To
    fn extract_references(&self, headers: &impl MailHeaderMap) -> Result<(Vec<String>, Option<String>)> {
        let references = headers.get_all_values("References")
            .into_iter()
            .flat_map(|ref_str: String| {
                self.message_id_regex.captures_iter(&ref_str)
                    .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
                    .collect::<Vec<_>>()
            })
            .collect();

        let in_reply_to = headers.get_first_value("In-Reply-To")
            .and_then(|irt| {
                self.message_id_regex.captures(&irt)
                    .and_then(|cap| cap.get(1).map(|m| m.as_str().to_string()))
            });

        Ok((references, in_reply_to))
    }

    /// Extract email body
    fn extract_body(&self, parsed: &ParsedMail) -> Result<String> {
        // Try to get the text/plain part first
        if let Some(body_part) = self.find_text_part(parsed) {
            return Ok(body_part.get_body().unwrap_or_else(|_| String::new()));
        }

        // Fall back to the main body
        Ok(parsed.get_body().unwrap_or_else(|_| String::new()))
    }

    /// Find the text/plain part of a multipart email
    fn find_text_part<'a>(&self, parsed: &'a ParsedMail<'a>) -> Option<&'a ParsedMail<'a>> {
        // For now, just return the parsed mail as-is
        // TODO: Implement proper multipart parsing
        Some(parsed)
    }

    /// Analyze if this is a patch and extract patch information
    fn analyze_patch_status(&self, subject: &str, body: &str) -> Result<(bool, PatchInfo)> {
        let mut is_patch = false;
        let mut patch_number = None;
        let mut version = None;
        let mut filename = None;

        // Check subject for patch indicators
        // Only consider it a patch if it doesn't start with "Re:" (replies)
        if !subject.to_lowercase().starts_with("re:") && self.patch_regex.is_match(subject) {
            is_patch = true;

            // Extract patch number from subject like [PATCH 1/3]
            if let Some(captures) = Regex::new(r"(?i)\[.*patch.*?(\d+)/(\d+).*?\]")
                .map_err(|e| ParserError::EmailParsing(format!("Regex error: {}", e)))?
                .captures(subject)
            {
                if captures.len() >= 3 {
                    patch_number = captures[1].parse().ok();
                }
            }

            // Extract version from subject
            if let Some(captures) = self.patch_version_regex.captures(subject) {
                for cap in captures.iter().skip(1) {
                    if let Some(m) = cap {
                        version = m.as_str().parse().ok();
                        break;
                    }
                }
            }

            // Extract filename from subject like [PATCH] file.patch
            if let Some(filename_match) = subject.split(']').nth(1) {
                let filename_part = filename_match.trim();
                if filename_part.contains('.') && !filename_part.contains(' ') {
                    filename = Some(filename_part.to_string());
                }
            }
        }

        // Also check body for patch content
        if body.contains("diff --git") || body.contains("@@") {
            is_patch = true;
        }

        Ok((is_patch, PatchInfo {
            patch_number,
            version,
            filename,
        }))
    }
}

/// Information about a patch extracted from an email
#[derive(Debug, Clone)]
struct PatchInfo {
    patch_number: Option<i32>,
    version: Option<i32>,
    filename: Option<String>,
}

/// Thread analyzer for building email thread relationships
pub struct ThreadAnalyzer {
    parser: EmailParser,
}

impl ThreadAnalyzer {
    /// Create a new thread analyzer
    pub fn new() -> Result<Self> {
        Ok(ThreadAnalyzer {
            parser: EmailParser::new()?,
        })
    }

    /// Analyze a collection of emails and build thread relationships
    pub async fn analyze_threads(&self, emails: &[Email]) -> Result<HashMap<String, (Option<String>, i32)>> {
        let mut thread_map = HashMap::new();
        let mut thread_roots = HashMap::new();

        // First pass: identify thread roots and build basic structure
        for email in emails {
            if let Some(parent_id) = &email.in_reply_to {
                // This is a reply, find the parent
                let parent_found = emails.iter()
                    .any(|e| e.message_id == *parent_id);

                if parent_found {
                    thread_map.insert(email.message_id.clone(), (Some(parent_id.clone()), 0));
                } else {
                    // Parent not in current batch, treat as potential root
                    thread_roots.insert(email.message_id.clone(), email);
                }
            } else {
                // No In-Reply-To, this is a thread root
                thread_roots.insert(email.message_id.clone(), email);
            }
        }

        // Second pass: calculate depths and handle references
        let mut updated_map = thread_map.clone();
        for email in emails {
            if let Some((parent_id, _)) = thread_map.get(&email.message_id) {
                // Calculate depth based on parent chain
                let depth = self.calculate_depth(&thread_map, &email.message_id, 0);
                updated_map.insert(email.message_id.clone(), (parent_id.clone(), depth));
            }
        }

        // Third pass: handle emails that reference others through References header
        for email in emails {
            if !updated_map.contains_key(&email.message_id) {
                // Check if this email references any other email in the batch
                for ref_id in &email.references {
                    if emails.iter().any(|e| e.message_id == *ref_id) {
                        let depth = self.calculate_depth(&updated_map, ref_id, 1);
                        updated_map.insert(email.message_id.clone(), (Some(ref_id.clone()), depth));
                        break;
                    }
                }
            }
        }

        Ok(updated_map)
    }

    /// Calculate the depth of an email in the thread
    fn calculate_depth(&self, thread_map: &HashMap<String, (Option<String>, i32)>, email_id: &str, current_depth: i32) -> i32 {
        if let Some((Some(parent_id), _)) = thread_map.get(email_id) {
            self.calculate_depth(thread_map, parent_id, current_depth + 1)
        } else {
            current_depth
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_patch_detection() {
        let parser = EmailParser::new().unwrap();

        let subject = "[PATCH 1/3] bpf: Add new feature";
        let body = "This is a patch description.\n\nSigned-off-by: Test User <test@example.com>";

        let (is_patch, patch_info) = parser.analyze_patch_status(subject, body).unwrap();

        assert!(is_patch);
        assert_eq!(patch_info.patch_number, Some(1));
        assert_eq!(patch_info.version, None);
    }

    #[test]
    fn test_version_extraction() {
        let parser = EmailParser::new().unwrap();

        let subject = "[PATCH v2 1/3] bpf: Add new feature";
        let body = "This is version 2 of the patch.";

        let (is_patch, patch_info) = parser.analyze_patch_status(subject, body).unwrap();

        assert!(is_patch);
        assert_eq!(patch_info.version, Some(2));
    }
}
