use std::process::Command;
use serde::{Deserialize, Serialize};

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


/// Get all commit hashes from the BPF mailing list repository
/// Returns a vector of commit hashes in chronological order (oldest first)
/// Limited to first `limit` commits (default: 10)
pub fn get_all_commits_with_limit(limit: Option<usize>) -> Result<Vec<String>, ParseError> {
    let limit_str = limit.unwrap_or(10).to_string();
    let output = Command::new("git")
        .args(&[
            "--git-dir=E:/bpf/git/0.git",
            "--work-tree=E:/bpf",
            "log",
            "--format=%H",
            "-n", &limit_str
        ])
        .output()?;

    if !output.status.success() {
        return Err(ParseError {
            message: format!("Git command failed: {}", String::from_utf8_lossy(&output.stderr)),
        });
    }

    // Use lossy UTF-8 conversion to handle invalid sequences gracefully
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    let commits: Vec<String> = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .collect();

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

/// Efficiently retrieve email content for multiple commits using git cat-file --batch
fn get_batch_email_content(commit_hashes: &[String]) -> Result<Vec<(String, String)>, ParseError> {
    use std::io::Write;
    use std::process::{Stdio, Command};

    let mut child = Command::new("git")
        .args(&[
            "--git-dir=E:/bpf/git/0.git",
            "--work-tree=E:/bpf",
            "cat-file",
            "--batch"
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let mut stdin = child.stdin.take().ok_or_else(|| ParseError {
        message: "Failed to open stdin".to_string(),
    })?;

    // Write all object references to stdin
    for commit_hash in commit_hashes {
        writeln!(stdin, "{}:m", commit_hash).map_err(|e| ParseError {
            message: format!("Failed to write to git cat-file: {}", e),
        })?;
    }

    // Close stdin to signal end of input
    drop(stdin);

    // Read output
    let output = child.wait_with_output()?;

    if !output.status.success() {
        return Err(ParseError {
            message: format!("git cat-file failed: {}", String::from_utf8_lossy(&output.stderr)),
        });
    }

    // Parse batch output and sanitize
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut results = Vec::new();
    let mut lines = stdout.lines();
    let mut commit_idx = 0;

    while let Some(header_line) = lines.next() {
        // Header format: "<sha> <type> <size>"
        let parts: Vec<&str> = header_line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }

        let size: usize = parts[2].parse().unwrap_or(0);
        if size == 0 {
            commit_idx += 1;
            continue;
        }

        // Read the content (size bytes)
        let mut content_lines = Vec::new();
        let mut bytes_read = 0;
        
        while bytes_read < size {
            if let Some(line) = lines.next() {
                bytes_read += line.len() + 1; // +1 for newline
                content_lines.push(line);
                if bytes_read >= size {
                    break;
                }
            } else {
                break;
            }
        }

        let content = content_lines.join("\n");
        
        // Sanitize content - remove null bytes which cause PostgreSQL errors
        let sanitized_content = content.replace('\0', "");
        
        if commit_idx < commit_hashes.len() {
            results.push((commit_hashes[commit_idx].clone(), sanitized_content));
        }
        
        commit_idx += 1;
        
        // Skip the blank line between entries
        lines.next();
    }

    Ok(results)
}

/// Get email content for a single commit hash
fn get_single_email_content(commit_hash: &str) -> Result<String, ParseError> {
    let output = Command::new("git")
        .args(&[
            "--git-dir=E:/bpf/git/0.git",
            "--work-tree=E:/bpf",
            "show",
            &format!("{}:m", commit_hash)
        ])
        .output()?;

    if !output.status.success() {
        return Err(ParseError {
            message: format!("Failed to get email for commit {}: {}",
                commit_hash, String::from_utf8_lossy(&output.stderr)),
        });
    }

    // Use lossy UTF-8 conversion and remove null bytes
    let content = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(content.replace('\0', ""))
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
    let output = Command::new("git")
        .args(&[
            "--git-dir=E:/bpf/git/0.git",
            "--work-tree=E:/bpf",
            "rev-list",
            "--count",
            "HEAD"
        ])
        .output()?;

    if !output.status.success() {
        return Err(ParseError {
            message: format!("Git command failed: {}", String::from_utf8_lossy(&output.stderr)),
        });
    }

    // Parse the output as a number
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let count: usize = stdout.parse().map_err(|_| ParseError {
        message: format!("Failed to parse git rev-list output: '{}'", stdout),
    })?;

    Ok(count)
}
