use serde::{Deserialize, Serialize};
use gix::Repository;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CommitMetadata {
    pub commit_hash: String,
    pub author_name: String,
    pub author_email: String,
    pub subject: String,
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

impl From<gix::open::Error> for ParseError {
    fn from(error: gix::open::Error) -> Self {
        ParseError {
            message: format!("Git repository error: {}", error),
        }
    }
}

/// Open the git repository at the configured path
/// TODO: Make this configurable instead of hardcoded
fn open_repository() -> Result<Repository, ParseError> {
    let repo_path = "E:/bpf/git/0.git";
    let repo = gix::open(repo_path)?;
    Ok(repo)
}

/// Validate and sanitize email address
/// Returns a valid email or generates a placeholder for invalid/empty emails
fn validate_email(email: &str, commit_hash: &str) -> String {
    let trimmed = email.trim();
    
    // Check if email is empty or invalid
    if trimmed.is_empty() {
        eprintln!("Warning: Empty email for commit {}, using placeholder", commit_hash);
        return format!("unknown-{}@placeholder.local", &commit_hash[..8.min(commit_hash.len())]);
    }
    
    // Basic email validation: must contain @ and have text before/after it
    if !trimmed.contains('@') || trimmed.starts_with('@') || trimmed.ends_with('@') {
        eprintln!("Warning: Invalid email '{}' for commit {}, using placeholder", trimmed, commit_hash);
        return format!("invalid-{}@placeholder.local", &commit_hash[..8.min(commit_hash.len())]);
    }
    
    // Email looks valid, return lowercase version
    trimmed.to_lowercase()
}


/// Get all commit hashes from the BPF mailing list repository
/// Returns a vector of commit hashes in chronological order (oldest first)
/// Limited to first `limit` commits (default: 10)
pub fn get_all_commits_with_limit(limit: Option<usize>) -> Result<Vec<String>, ParseError> {
    let repo = open_repository()?;
    let limit = limit.unwrap_or(10);
    
    let head = repo.head_id().map_err(|e| ParseError {
        message: format!("Failed to get HEAD: {}", e),
    })?;
    
    let mut commits = Vec::new();
    let commit_iter = head.ancestors().all().map_err(|e| ParseError {
        message: format!("Failed to create commit iterator: {}", e),
    })?;
    
    for commit_result in commit_iter.take(limit) {
        let commit_info = commit_result.map_err(|e| ParseError {
            message: format!("Failed to iterate commits: {}", e),
        })?;
        commits.push(commit_info.id.to_string());
    }
    
    Ok(commits)
}

/// Get all commit hashes with default limit of 10
pub fn get_all_commits() -> Result<Vec<String>, ParseError> {
    get_all_commits_with_limit(None)
}

/// Get email content for multiple commit hashes using efficient batching
/// This retrieves raw email content for multiple commits using git cat-file --batch
pub fn get_multiple_email_content(commit_hashes: &[String]) -> Result<Vec<(String, String)>, ParseError> {
    if commit_hashes.is_empty() {
        return Ok(Vec::new());
    }

    // Always use batch mode for 2+ commits (much faster than individual git show calls)
    if commit_hashes.len() >= 2 {
        return get_batch_email_content(commit_hashes);
    }

    // For single commit, use direct call
    if let Some(commit_hash) = commit_hashes.first() {
        match get_single_email_content(commit_hash) {
            Ok(content) => Ok(vec![(commit_hash.clone(), content)]),
            Err(e) => Err(e),
        }
    } else {
        Ok(Vec::new())
    }
}

/// Efficiently retrieve email content for multiple commits using gix
fn get_batch_email_content(commit_hashes: &[String]) -> Result<Vec<(String, String)>, ParseError> {
    let repo = open_repository()?;
    let mut results = Vec::new();
    
    for commit_hash in commit_hashes {
        // Parse the commit hash into an ObjectId
        let commit_id = gix::ObjectId::from_hex(commit_hash.as_bytes()).map_err(|e| ParseError {
            message: format!("Invalid commit hash {}: {}", commit_hash, e),
        })?;
        
        // Get the commit object
        let commit = repo.find_object(commit_id).map_err(|e| ParseError {
            message: format!("Failed to find commit {}: {}", commit_hash, e),
        })?;
        
        let commit = commit.try_into_commit().map_err(|e| ParseError {
            message: format!("Object {} is not a commit: {}", commit_hash, e),
        })?;
        
        // Get the tree from the commit
        let tree_id = commit.tree_id().map_err(|e| ParseError {
            message: format!("Failed to get tree for commit {}: {}", commit_hash, e),
        })?;
        
        let tree = repo.find_object(tree_id).map_err(|e| ParseError {
            message: format!("Failed to find tree for commit {}: {}", commit_hash, e),
        })?;
        
        let tree = tree.try_into_tree().map_err(|e| ParseError {
            message: format!("Object is not a tree for commit {}: {}", commit_hash, e),
        })?;
        
        // Look for the "m" file in the tree
        let tree_ref = tree.decode().map_err(|e| ParseError {
            message: format!("Failed to decode tree for commit {}: {}", commit_hash, e),
        })?;
        
        // Find the entry named "m"
        let m_entry = tree_ref.entries.iter().find(|entry| {
            entry.filename.as_ref() as &[u8] == b"m"
        }).ok_or_else(|| ParseError {
            message: format!("No 'm' file found in commit {}", commit_hash),
        })?;
        
        // Get the blob content
        let blob = repo.find_object(m_entry.oid).map_err(|e| ParseError {
            message: format!("Failed to find blob 'm' for commit {}: {}", commit_hash, e),
        })?;
        
        let blob_data = blob.data.clone();
        
        // Convert to string and sanitize
        let content = String::from_utf8_lossy(&blob_data).to_string();
        let sanitized_content = content.replace('\0', "");
        
        results.push((commit_hash.clone(), sanitized_content));
    }
    
    Ok(results)
}

/// Get email content for a single commit hash
fn get_single_email_content(commit_hash: &str) -> Result<String, ParseError> {
    // Reuse the batch function for consistency
    let results = get_batch_email_content(&[commit_hash.to_string()])?;
    results.into_iter().next()
        .map(|(_, content)| content)
        .ok_or_else(|| ParseError {
            message: format!("Failed to get email content for commit {}", commit_hash),
        })
}

/// Get email content for a specific commit hash
/// This retrieves the raw email content stored in the "m" file of the commit
pub fn get_email_content(commit_hash: &str) -> Result<String, ParseError> {
    let results = get_multiple_email_content(&[commit_hash.to_string()])?;
    Ok(results.into_iter().next().unwrap().1)
}


/// Get the total number of emails in the repository
pub fn get_email_count() -> Result<usize, ParseError> {
    let commits = get_all_commits()?;
    Ok(commits.len())
}

/// Get the total count of all commits in the git repository
pub fn get_total_git_commits() -> Result<usize, ParseError> {
    let repo = open_repository()?;
    
    let head = repo.head_id().map_err(|e| ParseError {
        message: format!("Failed to get HEAD: {}", e),
    })?;
    
    let commit_iter = head.ancestors().all().map_err(|e| ParseError {
        message: format!("Failed to create commit iterator: {}", e),
    })?;
    
    let count = commit_iter.count();
    Ok(count)
}

/// Get commit metadata (author name, email, subject) for multiple commits
/// Returns a vector of CommitMetadata structs in the same order as input
pub fn get_commit_metadata(commit_hashes: &[String]) -> Result<Vec<CommitMetadata>, ParseError> {
    if commit_hashes.is_empty() {
        return Ok(Vec::new());
    }

    // With gix, we no longer have command-line length limits
    // However, we can still batch for better error handling and progress tracking
    const BATCH_SIZE: usize = 500; // Increased from 100 since no CLI limits
    
    let mut all_results = Vec::new();
    
    // Process commits in batches
    for chunk in commit_hashes.chunks(BATCH_SIZE) {
        let batch_results = get_commit_metadata_batch(chunk)?;
        all_results.extend(batch_results);
    }
    
    Ok(all_results)
}

/// Internal function to get metadata for a batch of commits
fn get_commit_metadata_batch(commit_hashes: &[String]) -> Result<Vec<CommitMetadata>, ParseError> {
    if commit_hashes.is_empty() {
        return Ok(Vec::new());
    }

    let repo = open_repository()?;
    let mut results = Vec::new();
    
    for commit_hash in commit_hashes {
        // Parse the commit hash into an ObjectId
        let commit_id = gix::ObjectId::from_hex(commit_hash.as_bytes()).map_err(|e| ParseError {
            message: format!("Invalid commit hash {}: {}", commit_hash, e),
        })?;
        
        // Get the commit object
        let commit = repo.find_object(commit_id).map_err(|e| ParseError {
            message: format!("Failed to find commit {}: {}", commit_hash, e),
        })?;
        
        let commit = commit.try_into_commit().map_err(|e| ParseError {
            message: format!("Object {} is not a commit: {}", commit_hash, e),
        })?;
        
        let commit_ref = commit.decode().map_err(|e| ParseError {
            message: format!("Failed to decode commit {}: {}", commit_hash, e),
        })?;
        
        // Extract metadata
        let author = &commit_ref.author;
        let author_name = String::from_utf8_lossy(author.name.as_ref()).to_string();
        let raw_email = String::from_utf8_lossy(author.email.as_ref());
        
        // Validate and sanitize email (handles empty/invalid emails)
        let author_email = validate_email(&raw_email, commit_hash);
        
        // Get subject (first line of message)
        let message = String::from_utf8_lossy(commit_ref.message.as_ref());
        let subject = message.lines().next().unwrap_or("").to_string();
        
        results.push(CommitMetadata {
            commit_hash: commit_hash.clone(),
            author_name,
            author_email,
            subject,
        });
    }
    
    Ok(results)
}

/// Get commit metadata for a single commit
pub fn get_single_commit_metadata(commit_hash: &str) -> Result<CommitMetadata, ParseError> {
    let results = get_commit_metadata(&[commit_hash.to_string()])?;
    results.into_iter().next().ok_or_else(|| ParseError {
        message: format!("Failed to get metadata for commit {}", commit_hash),
    })
}
