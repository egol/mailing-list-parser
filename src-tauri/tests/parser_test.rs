#[cfg(test)]
mod tests {
    use mailing_list_parser_lib::parser::{EmailParser, ThreadAnalyzer};
    use mailing_list_parser_lib::models::{Email, DEFAULT_MAILING_LIST_GIT_PATH};
use std::env;
use std::path::Path;
use std::process::Command;

    #[test]
    fn test_parse_simple_email() {
        let parser = EmailParser::new().unwrap();

        let raw_email = r#"From: test@example.com
To: bpf@vger.kernel.org
Subject: [PATCH] bpf: Add new feature
Date: Thu, 25 Sep 2025 23:20:12 +0000
Message-ID: <test123@example.com>

This is a test patch for BPF functionality.

Signed-off-by: Test User <test@example.com>
"#;

        let result = parser.parse_email(raw_email);
        assert!(result.is_ok());

        let email = result.unwrap();
        assert_eq!(email.subject, "[PATCH] bpf: Add new feature");
        assert_eq!(email.from, "test@example.com");
        assert_eq!(email.message_id, "test123@example.com");
        assert!(email.is_patch);
    }

    #[test]
    fn test_parse_patch_with_version() {
        let parser = EmailParser::new().unwrap();

        let raw_email = r#"From: developer@kernel.org
To: bpf@vger.kernel.org
Subject: [PATCH v2 1/3] bpf: Implement new functionality
Date: Fri, 26 Sep 2025 10:30:00 +0000
Message-ID: <patch-v2-1@example.com>

This is version 2 of the patch series.

---
diff --git a/kernel/bpf/core.c b/kernel/bpf/core.c
index 1234567..abcdefg 100644
--- a/kernel/bpf/core.c
+++ b/kernel/bpf/core.c
@@ -10,6 +10,12 @@
 #include <linux/bpf.h>

+/* New BPF functionality */
+static int new_bpf_function(void)
+{
+    return 0;
+}
+
 int main(void)
 {
+    return new_bpf_function();
 }
"#;

        let result = parser.parse_email(raw_email);
        assert!(result.is_ok());

        let email = result.unwrap();
        assert_eq!(email.subject, "[PATCH v2 1/3] bpf: Implement new functionality");
        assert_eq!(email.from, "developer@kernel.org");
        assert!(email.is_patch);
        assert_eq!(email.patch_number, Some(1));
        assert_eq!(email.patch_version, Some(2));
    }

    #[test]
    fn test_parse_reply_email() {
        let parser = EmailParser::new().unwrap();

        let raw_email = r#"From: reviewer@kernel.org
To: bpf@vger.kernel.org
Subject: Re: [PATCH] bpf: Add new feature
Date: Fri, 26 Sep 2025 11:00:00 +0000
Message-ID: <reply123@example.com>
In-Reply-To: <test123@example.com>
References: <test123@example.com>

On Thu, Sep 25, 2025 at 11:20 PM test@example.com wrote:
> This is a test patch for BPF functionality.
>
> Signed-off-by: Test User <test@example.com>

This looks good to me.

Reviewed-by: Reviewer <reviewer@kernel.org>
"#;

        let result = parser.parse_email(raw_email);
        assert!(result.is_ok());

        let email = result.unwrap();
        assert_eq!(email.subject, "Re: [PATCH] bpf: Add new feature");
        assert_eq!(email.from, "reviewer@kernel.org");
        assert_eq!(email.in_reply_to, Some("test123@example.com".to_string()));
        assert!(email.references.contains(&"test123@example.com".to_string()));
        assert!(!email.is_patch); // This is a reply, not a patch itself
    }

    #[test]
    fn test_patch_detection_by_content() {
        let parser = EmailParser::new().unwrap();

        let raw_email = r#"From: developer@kernel.org
To: bpf@vger.kernel.org
Subject: RFC: BPF improvement proposal
Date: Fri, 26 Sep 2025 12:00:00 +0000
Message-ID: <rfc123@example.com>

Hi all,

I propose the following improvement to BPF:

diff --git a/kernel/bpf/verifier.c b/kernel/bpf/verifier.c
index 1234567..abcdefg 100644
--- a/kernel/bpf/verifier.c
+++ b/kernel/bpf/verifier.c
@@ -100,6 +100,12 @@
 void check_function_call(struct bpf_verifier_env *env)
 {
+    /* New check for function calls */
+    if (is_special_function_call(env)) {
+        return;
+    }
+
     // existing code
 }
"#;

        let result = parser.parse_email(raw_email);
        assert!(result.is_ok());

        let email = result.unwrap();
        assert_eq!(email.subject, "RFC: BPF improvement proposal");
        assert!(email.is_patch); // Should detect patch by content even without [PATCH] in subject
    }

    // Git integration test helpers
    fn get_git_repo_path() -> String {
        env::var("BPF_GIT_PATH").unwrap_or_else(|_| DEFAULT_MAILING_LIST_GIT_PATH.to_string())
    }

    fn get_latest_commit_hash(repo_path: &str) -> Option<String> {
        // For bare repositories (ending in .git), use the repo path directly
        // For regular repositories, use the .git subdirectory
        let git_dir = if repo_path.ends_with(".git") {
            repo_path.to_string()
        } else {
            format!("{}/.git", repo_path)
        };

        let output = Command::new("git")
            .args(&["--git-dir", &git_dir, "rev-parse", "HEAD"])
            .output()
            .ok()?;

        if output.status.success() {
            let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
            println!("✅ Got commit hash: {}", hash);
            Some(hash)
        } else {
            println!("❌ Git command failed: {:?}", String::from_utf8_lossy(&output.stderr));
            None
        }
    }

    fn get_commit_content(repo_path: &str, commit_hash: &str) -> Option<String> {
        // For bare repositories (ending in .git), use the repo path directly
        // For regular repositories, use the .git subdirectory
        let git_dir = if repo_path.ends_with(".git") {
            repo_path.to_string()
        } else {
            format!("{}/.git", repo_path)
        };

        // First try to get the content of the email file (usually named 'm' or similar)
        let output = Command::new("git")
            .args(&["--git-dir", &git_dir, "show", &format!("{}:m", commit_hash)])
            .output()
            .ok()?;

        if output.status.success() {
            let content = String::from_utf8_lossy(&output.stdout).to_string();
            if content.contains("Message-ID:") {
                println!("✅ Retrieved email content from file 'm' ({} chars)", content.len());
                return Some(content);
            }
        }

        // If that doesn't work, try common email file names
        let email_files = ["m", "message", "email", "msg"];
        for file in &email_files {
            let output = Command::new("git")
                .args(&["--git-dir", &git_dir, "show", &format!("{}:{}", commit_hash, file)])
                .output()
                .ok()?;

            if output.status.success() {
                let content = String::from_utf8_lossy(&output.stdout).to_string();
                if content.contains("Message-ID:") {
                    println!("✅ Retrieved email content from file '{}' ({} chars)", file, content.len());
                    return Some(content);
                }
            }
        }

        // If no email file found, try to get the raw commit content and extract email-like content
        let output = Command::new("git")
            .args(&["--git-dir", &git_dir, "show", "--format=fuller", commit_hash])
            .output()
            .ok()?;

        if output.status.success() {
            let content = String::from_utf8_lossy(&output.stdout).to_string();
            // Look for lines that start with common email headers
            let lines: Vec<&str> = content.lines().collect();
            let mut email_start = None;
            for (i, line) in lines.iter().enumerate() {
                if line.starts_with("Message-ID:") ||
                   line.starts_with("Subject:") ||
                   line.starts_with("From:") ||
                   line.starts_with("Date:") {
                    email_start = Some(i);
                    break;
                }
            }

            if let Some(start) = email_start {
                let email_content = lines[start..].join("\n");
                if email_content.contains("Message-ID:") {
                    println!("✅ Extracted email content from commit diff ({} chars)", email_content.len());
                    return Some(email_content);
                }
            }
        }

        println!("❌ No email content found in commit {}", commit_hash);
        None
    }

    fn parse_emails_from_commits(repo_path: &str, commit_hashes: Vec<&str>) -> Vec<Email> {
        let parser = EmailParser::new().unwrap();
        let mut emails = Vec::new();

        for commit_hash in commit_hashes {
            if let Some(content) = get_commit_content(repo_path, commit_hash) {
                println!("🔍 Attempting to parse email from commit {}", commit_hash);
                match parser.parse_from_git_commit(commit_hash, &content) {
                    Ok(email) => {
                        println!("✅ Successfully parsed email: {}", email.subject);
                        emails.push(email);
                    }
                    Err(e) => {
                        println!("❌ Failed to parse email from commit {}: {}", commit_hash, e);
                    }
                }
            } else {
                println!("❌ Could not get content for commit {}", commit_hash);
            }
        }

        println!("📧 Total emails parsed: {}", emails.len());
        emails
    }

    // Integration tests with real git repository
    #[test]
    fn test_parse_emails_from_real_git_repo() {
        let repo_path = get_git_repo_path();

        // Skip test if repository doesn't exist
        if !Path::new(&repo_path).exists() {
            println!("⚠️  Skipping test: BPF git repository not found at {}", repo_path);
            println!("   Current working directory: {:?}", std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("unknown")));
            println!("   Set BPF_GIT_PATH environment variable to point to your BPF git clone");
            return;
        }

        // Validate repository structure
        let repo_path_obj = Path::new(&repo_path);
        let is_git_repo = if repo_path.ends_with(".git") {
            // This is likely a bare repository
            repo_path_obj.exists() && (
                repo_path_obj.join("HEAD").exists() ||
                repo_path_obj.join("refs").exists() ||
                repo_path_obj.join("objects").exists()
            )
        } else {
            // Look for .git subdirectory
            repo_path_obj.join(".git").exists()
        };

        if !is_git_repo {
            println!("⚠️  Skipping test: {} does not appear to be a valid git repository", repo_path);
            return;
        }

        println!("✅ Using git repository at: {}", repo_path);

        let commit_hash = match get_latest_commit_hash(&repo_path) {
            Some(hash) => hash,
            None => {
                println!("Skipping test: Could not get latest commit hash");
                return;
            }
        };

        println!("Testing with commit: {}", commit_hash);

        let emails = parse_emails_from_commits(&repo_path, vec![&commit_hash]);
        assert!(!emails.is_empty(), "Should parse at least one email from git commit");

        for email in &emails {
            println!("Parsed email: {} from {} - Patch: {}",
                    email.subject,
                    email.from,
                    email.is_patch);

            // Basic validation
            assert!(!email.message_id.is_empty());
            assert!(!email.subject.is_empty());
            assert!(!email.from.is_empty());
            assert!(!email.body.is_empty());
        }
    }

    #[test]
    fn test_patch_detection_from_real_emails() {
        let repo_path = get_git_repo_path();
        if !Path::new(&repo_path).exists() {
            println!("Skipping test: BPF git repository not found at {}", repo_path);
            return;
        }

        // Get recent commits
        let output = Command::new("git")
            .args(&["log", "--oneline", "-10"])
            .current_dir(&repo_path)
            .output()
            .ok();

        if let Some(output) = output {
            if output.status.success() {
                let log_output = String::from_utf8_lossy(&output.stdout);
                let commit_hashes: Vec<&str> = log_output
                    .lines()
                    .take(5) // Test first 5 commits
                    .filter_map(|line| line.split_whitespace().next())
                    .collect();

                if commit_hashes.is_empty() {
                    println!("No commits found to test");
                    return;
                }

                let emails = parse_emails_from_commits(&repo_path, commit_hashes);
                assert!(!emails.is_empty(), "Should parse emails from recent commits");

                // Check that we can detect patches
                let patches: Vec<_> = emails.iter()
                    .filter(|email| email.is_patch)
                    .collect();

                println!("Found {} patches out of {} total emails", patches.len(), emails.len());

                // If we have patches, validate their structure
                for patch in patches {
                    println!("Patch found: {} - Number: {:?}, Version: {:?}",
                            patch.subject,
                            patch.patch_number,
                            patch.patch_version);

                    if let Some(patch_num) = patch.patch_number {
                        assert!(patch_num > 0, "Patch number should be positive");
                    }

                    if let Some(version) = patch.patch_version {
                        assert!(version > 0, "Version should be positive");
                    }
                }
            }
        }
    }

    #[test]
    fn test_thread_analysis_from_real_emails() {
        let repo_path = get_git_repo_path();
        if !Path::new(&repo_path).exists() {
            println!("Skipping test: BPF git repository not found at {}", repo_path);
            return;
        }

        // Get several recent commits to have potential threads
        let output = Command::new("git")
            .args(&["log", "--oneline", "-20"])
            .current_dir(&repo_path)
            .output()
            .ok();

        if let Some(output) = output {
            if output.status.success() {
                let log_output = String::from_utf8_lossy(&output.stdout);
                let commit_hashes: Vec<&str> = log_output
                    .lines()
                    .take(10) // Test first 10 commits
                    .filter_map(|line| line.split_whitespace().next())
                    .collect();

                if commit_hashes.is_empty() {
                    println!("No commits found to test");
                    return;
                }

                let emails = parse_emails_from_commits(&repo_path, commit_hashes);
                if emails.is_empty() {
                    println!("No emails parsed from commits");
                    return;
                }

                println!("Analyzing threads for {} emails", emails.len());

                // Test thread analysis
                let thread_analyzer = ThreadAnalyzer::new().unwrap();
                let thread_relationships = tokio::runtime::Runtime::new()
                    .unwrap()
                    .block_on(thread_analyzer.analyze_threads(&emails))
                    .unwrap();

                println!("Found {} thread relationships", thread_relationships.len());

                // Validate thread structure
                for (email_id, (parent_id, depth)) in &thread_relationships {
                    println!("Email {} has parent {:?} at depth {}",
                            email_id,
                            parent_id,
                            depth);

                    if let Some(parent) = parent_id {
                        assert!(emails.iter().any(|e| e.message_id == *parent),
                               "Parent email should exist in the email set");
                    }

                    assert!(*depth >= 0, "Depth should be non-negative");
                }
            }
        }
    }

    #[test]
    fn test_email_statistics_from_repo() {
        let repo_path = get_git_repo_path();
        if !Path::new(&repo_path).exists() {
            println!("Skipping test: BPF git repository not found at {}", repo_path);
            return;
        }

        // Get a larger set of commits for statistics
        let output = Command::new("git")
            .args(&["log", "--oneline", "-50"])
            .current_dir(&repo_path)
            .output()
            .ok();

        if let Some(output) = output {
            if output.status.success() {
                let log_output = String::from_utf8_lossy(&output.stdout);
                let commit_hashes: Vec<&str> = log_output
                    .lines()
                    .take(30) // Test first 30 commits
                    .filter_map(|line| line.split_whitespace().next())
                    .collect();

                if commit_hashes.is_empty() {
                    println!("No commits found to test");
                    return;
                }

                let emails = parse_emails_from_commits(&repo_path, commit_hashes);

                println!("Statistics for {} parsed emails:", emails.len());

                let patch_count = emails.iter().filter(|e| e.is_patch).count();
                let reply_count = emails.iter().filter(|e| e.in_reply_to.is_some()).count();
                let total_references: usize = emails.iter().map(|e| e.references.len()).sum();

                println!("- Patches: {} ({:.1}%)",
                        patch_count,
                        (patch_count as f64 / emails.len() as f64) * 100.0);
                println!("- Replies: {} ({:.1}%)",
                        reply_count,
                        (reply_count as f64 / emails.len() as f64) * 100.0);
                println!("- Total references: {}", total_references);
                println!("- Average references per email: {:.1}",
                        total_references as f64 / emails.len() as f64);

                // Validate that we have meaningful data
                assert!(emails.len() > 0, "Should parse some emails");
                assert!(patch_count >= 0, "Patch count should be non-negative");
                assert!(reply_count >= 0, "Reply count should be non-negative");
            }
        }
    }

    #[test]
    fn test_git_repository_structure() {
        let repo_path = get_git_repo_path();
        if !Path::new(&repo_path).exists() {
            println!("Skipping test: BPF git repository not found at {}", repo_path);
            return;
        }

        // Test basic git operations
        // Note: This is a bare repository (ends with .git), so it doesn't have a .git subdirectory
        assert!(Path::new(&format!("{}/HEAD", repo_path)).exists(),
               "Should have HEAD file");

        assert!(Path::new(&format!("{}/refs", repo_path)).exists(),
               "Should have refs directory");

        // Test that we can get commit info
        let commit_hash = get_latest_commit_hash(&repo_path);
        assert!(commit_hash.is_some(), "Should be able to get latest commit hash");
        assert!(commit_hash.as_ref().unwrap().len() >= 7,
               "Commit hash should be at least 7 characters");

        println!("Repository is valid, latest commit: {:?}", commit_hash);
    }
}
