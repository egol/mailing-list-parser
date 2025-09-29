use git2::{Commit, Repository};
use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PatchInfo {
    pub subject: String,
    pub author: String,
    pub email: String,
    pub date: String,
    pub message_id: String,
    pub body: String,
    pub commit_hash: String,
    pub files_changed: Vec<String>,
    pub patch_type: String,
    pub thread_info: Option<String>,
    pub patch_content: String,
    pub related_patches: Vec<RelatedPatch>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RelatedPatch {
    pub subject: String,
    pub commit_hash: String,
    pub relation_type: String, // "parent", "child", "sibling", etc.
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ParseError {
    pub message: String,
}

impl From<git2::Error> for ParseError {
    fn from(error: git2::Error) -> Self {
        ParseError {
            message: format!("Git error: {}", error),
        }
    }
}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

fn parse_email_from_commit(commit: &Commit, repo: &Repository) -> Result<PatchInfo, ParseError> {
    let message = commit.message().unwrap_or("No message");
    let author = commit.author().name().unwrap_or("Unknown").to_string();
    let email = commit.author().email().unwrap_or("unknown@example.com").to_string();
    let date = commit.time().seconds().to_string();
    let commit_hash = commit.id().to_string();

    // For kernel mailing lists, the commit message is usually just the subject line
    // Extract the subject (first line) and use the rest as body
    let lines: Vec<&str> = message.lines().collect();
    let subject = if lines.is_empty() {
        "No subject".to_string()
    } else {
        lines[0].to_string()
    };

    // Get the actual patch content (diff)
    let patch_content = get_patch_content(commit, repo)?;

    // Everything after the first line is the body (if any)
    let body = if lines.len() > 1 {
        lines[1..].join("\n").trim().to_string()
    } else {
        // For kernel patches, if there's no body, show what we can determine
        if subject.contains("PATCH") {
            "This is a kernel patch with diff content shown below.".to_string()
        } else {
            "No additional commit message content.".to_string()
        }
    };

    // Get files changed in this commit
    let files_changed = get_files_changed(commit, repo)?;

    // Determine patch type based on subject
    let patch_type = if subject.contains("PATCH") {
        if subject.contains("RFC") {
            "RFC Patch".to_string()
        } else if subject.contains("pull request") || subject.contains("PULL") {
            "Pull Request".to_string()
        } else {
            "Patch".to_string()
        }
    } else if subject.starts_with("Re:") {
        "Reply".to_string()
    } else {
        "Other".to_string()
    };

    // Extract threading information from subject
    let thread_info = if subject.starts_with("Re:") {
        Some(subject.clone())
    } else if subject.contains("PATCH") {
        // Look for series information like "1/5" or "v2"
        if let Some(series_match) = subject.find("v") {
            Some(format!("Series: {}", &subject[series_match..]))
        } else if let Some(patch_num) = subject.find("PATCH") {
            Some(format!("Patch series starting at: {}", &subject[patch_num..]))
        } else {
            None
        }
    } else {
        None
    };

    // Find related patches in the same thread/series
    let related_patches = find_related_patches(&subject, repo)?;

    // Generate a synthetic Message-ID based on the commit hash
    let message_id = format!("commit-{}", commit_hash);

    Ok(PatchInfo {
        subject,
        author,
        email,
        date,
        message_id,
        body,
        commit_hash,
        files_changed,
        patch_type,
        thread_info,
        patch_content,
        related_patches,
    })
}

fn get_patch_content(commit: &Commit, _repo: &Repository) -> Result<String, ParseError> {
    // For now, just return a placeholder - getting the actual diff is complex with git2
    // In a real implementation, you'd use git show or git diff commands
    Ok(format!("Diff content for commit {} - would show actual patch here", commit.id()))
}

fn get_files_changed(commit: &Commit, _repo: &Repository) -> Result<Vec<String>, ParseError> {
    // For now, return a placeholder - getting files changed is complex with git2
    // In a real implementation, you'd use git show --name-only or similar
    Ok(vec![format!("Files changed in commit {} - would list actual files here", commit.id())])
}

fn find_related_patches(subject: &str, _repo: &Repository) -> Result<Vec<RelatedPatch>, ParseError> {
    let mut related = Vec::new();

    // For now, return a placeholder - finding related patches requires complex git operations
    // In a real implementation, you'd search through git log for related subjects
    if subject.contains("net-next") {
        related.push(RelatedPatch {
            subject: "Related net-next patch found".to_string(),
            commit_hash: "abc123".to_string(),
            relation_type: "series".to_string(),
        });
    }

    Ok(related)
}

fn expand_path(path: &str) -> PathBuf {
    if path.starts_with("~/") {
        let home = env::var("HOME").unwrap_or_else(|_| "/".to_string());
        PathBuf::from(home + &path[1..])
    } else {
        PathBuf::from(path)
    }
}

fn get_latest_patch_from_repo(repo_path: &str) -> Result<PatchInfo, ParseError> {
    let expanded_path = expand_path(repo_path);

    let repo = Repository::open(&expanded_path)
        .map_err(|e| ParseError {
            message: format!("Failed to open repository '{}': {}", expanded_path.display(), e)
        })?;

    // Get the latest commit on the main branch (usually master or main)
    let head = repo.head()
        .map_err(|e| ParseError {
            message: format!("Failed to get HEAD: {}", e)
        })?;

    let commit = head.peel_to_commit()
        .map_err(|e| ParseError {
            message: format!("Failed to peel to commit: {}", e)
        })?;

    parse_email_from_commit(&commit, &repo)
}

#[tauri::command]
fn get_latest_patch(repo_path: &str) -> Result<PatchInfo, ParseError> {
    get_latest_patch_from_repo(repo_path)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![greet, get_latest_patch])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
