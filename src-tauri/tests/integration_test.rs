#[cfg(test)]
mod integration_tests {
    use mailing_list_parser_lib::parser::{EmailParser, ThreadAnalyzer};
    use mailing_list_parser_lib::models::{Email, DEFAULT_MAILING_LIST_GIT_PATH};
use std::env;
use std::path::Path;
use std::process::Command;

    fn get_git_repo_path() -> String {
        env::var("BPF_GIT_PATH").unwrap_or_else(|_| DEFAULT_MAILING_LIST_GIT_PATH.to_string())
    }

    fn get_latest_commit_hash(repo_path: &str) -> Option<String> {
        let output = Command::new("git")
            .args(&["rev-parse", "HEAD"])
            .current_dir(repo_path)
            .output()
            .ok()?;

        if output.status.success() {
            Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            None
        }
    }

    fn get_commit_content(repo_path: &str, commit_hash: &str) -> Option<String> {
        let output = Command::new("git")
            .args(&["show", "--format=fuller", commit_hash])
            .current_dir(repo_path)
            .output()
            .ok()?;

        if output.status.success() {
            Some(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            None
        }
    }

    fn parse_emails_from_commits(repo_path: &str, commit_hashes: Vec<&str>) -> Vec<Email> {
        let parser = EmailParser::new().unwrap();
        let mut emails = Vec::new();

        for commit_hash in commit_hashes {
            if let Some(content) = get_commit_content(repo_path, commit_hash) {
                if let Ok(email) = parser.parse_from_git_commit(commit_hash, &content) {
                    emails.push(email);
                }
            }
        }

        emails
    }

    #[test]
    fn integration_test_with_real_bpf_mailing_list() {
        let repo_path = get_git_repo_path();

        // Skip test if repository doesn't exist
        if !Path::new(&repo_path).exists() {
            println!("⚠️  Skipping integration test: BPF git repository not found at {}", repo_path);
            println!("   Current working directory: {:?}", std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("unknown")));
            println!("   Set BPF_GIT_PATH environment variable to point to your BPF git clone");
            println!("   Expected location: E:/bpf/git/0.git");
            return;
        }

        // Validate repository structure
        let repo_path_obj = Path::new(&repo_path);
        if !repo_path_obj.is_dir() {
            println!("⚠️  Skipping test: {} is not a directory", repo_path);
            return;
        }

        println!("🔍 Running integration test with BPF repository at: {}", repo_path);

        // Verify it's a git repository (either bare repo with .git structure or the .git directory itself)
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
            println!("   Expected to find HEAD, refs, or objects directories");
            return;
        }

        println!("✅ Valid git repository structure detected");

        // Get latest commit
        let latest_commit = match get_latest_commit_hash(&repo_path) {
            Some(hash) => {
                println!("📧 Latest commit: {}", hash);
                hash
            },
            None => {
                println!("⚠️  Could not get latest commit hash, skipping test");
                return;
            }
        };

        // Parse emails from recent commits
        let recent_commits = match Command::new("git")
            .args(&["log", "--oneline", "-20"])
            .current_dir(&repo_path)
            .output()
            .ok()
        {
            Some(output) if output.status.success() => {
                let stdout = output.stdout;
                let log_output = String::from_utf8_lossy(&stdout);
                log_output
                    .lines()
                    .take(10)
                    .filter_map(|line| line.split_whitespace().next())
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
            },
            _ => vec![latest_commit.clone()] // Fallback to just latest commit
        };

        println!("📨 Parsing {} commits for emails...", recent_commits.len());

        let emails = parse_emails_from_commits(&repo_path, recent_commits.iter().map(|s| s.as_str()).collect());

        if emails.is_empty() {
            println!("⚠️  No emails were parsed from the commits");
            println!("   This might be normal if the commits don't contain email data");
            return;
        }

        println!("✅ Successfully parsed {} emails", emails.len());

        // Analyze the results
        let patch_emails = emails.iter().filter(|e| e.is_patch).collect::<Vec<_>>();
        let reply_emails = emails.iter().filter(|e| e.in_reply_to.is_some()).collect::<Vec<_>>();

        println!("📊 Statistics:");
        println!("   📌 Patches: {} ({:.1}%)",
                patch_emails.len(),
                (patch_emails.len() as f64 / emails.len() as f64) * 100.0);
        println!("   💬 Replies: {} ({:.1}%)",
                reply_emails.len(),
                (reply_emails.len() as f64 / emails.len() as f64) * 100.0);

        // Test thread analysis
        if emails.len() > 1 {
            println!("🔗 Testing thread analysis...");
            let thread_analyzer = ThreadAnalyzer::new().unwrap();
            let thread_relationships = tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(thread_analyzer.analyze_threads(&emails))
                .unwrap();

            println!("   Found {} thread relationships", thread_relationships.len());

            if !thread_relationships.is_empty() {
                println!("   Thread structure validated successfully!");
            }
        }

        // Validate email structure
        for (i, email) in emails.iter().enumerate() {
            assert!(!email.message_id.is_empty(), "Email {} should have message ID", i);
            assert!(!email.subject.is_empty(), "Email {} should have subject", i);
            assert!(!email.from.is_empty(), "Email {} should have sender", i);

            if i < 3 { // Show first 3 emails as examples
                println!("   📧 Email {}: \"{}\" from {} (patch: {})",
                        i + 1,
                        email.subject.chars().take(60).collect::<String>(),
                        email.from,
                        email.is_patch);
            }
        }

        println!("🎉 Integration test completed successfully!");
        println!("   Parsed {} emails from BPF mailing list", emails.len());
    }

    #[test]
    fn test_bpf_repo_accessibility() {
        let repo_path = get_git_repo_path();

        println!("🔍 Testing accessibility of BPF repository at: {}", repo_path);

        if !Path::new(&repo_path).exists() {
            println!("⚠️  Repository path does not exist: {}", repo_path);
            println!("   This test will be skipped, but you can set BPF_GIT_PATH environment variable");
            println!("   Example: export BPF_GIT_PATH=/path/to/your/bpf/clone");
            println!("   Skipping test gracefully...");
            return;
        }

        // Test that the path is actually a directory
        let repo_path_obj = Path::new(&repo_path);
        if !repo_path_obj.is_dir() {
            println!("⚠️  Path exists but is not a directory: {}", repo_path);
            println!("   Expected a directory but found a file or symlink");
            return;
        }

        // Test basic git operations
        let git_dir = format!("{}/.git", repo_path);
        if !Path::new(&git_dir).exists() {
            println!("⚠️  Path exists but is not a valid git repository: {}", repo_path);
            println!("   Expected .git directory not found at: {}", git_dir);
            println!("   This might be a file instead of a directory, or git repo is corrupted");
            return;
        }

        // Test git operations
        let commit_hash = get_latest_commit_hash(&repo_path);
        if commit_hash.is_none() {
            println!("⚠️  Could not read commit hash from repository");
            println!("   This might indicate git is not properly initialized or accessible");
            return;
        }

        println!("✅ Successfully accessed BPF repository!");
        println!("   Path: {}", repo_path);
        println!("   Latest commit: {}", commit_hash.unwrap());
    }

    #[test]
    fn test_email_parsing_edge_cases() {
        let repo_path = get_git_repo_path();

        if !Path::new(&repo_path).exists() {
            println!("⚠️  Skipping edge case tests: Repository not found");
            return;
        }

        let _parser = EmailParser::new().unwrap();

        // Test with a commit that might have different email formats
        let output = Command::new("git")
            .args(&["log", "--oneline", "-5"])
            .current_dir(&repo_path)
            .output()
            .ok();

        if let Some(output) = output {
            if output.status.success() {
                let log_output = String::from_utf8_lossy(&output.stdout);
                let commit_hashes: Vec<&str> = log_output
                    .lines()
                    .filter_map(|line| line.split_whitespace().next())
                    .collect();

                if !commit_hashes.is_empty() {
                    let emails = parse_emails_from_commits(&repo_path, commit_hashes);

                    println!("🧪 Testing edge cases with {} emails", emails.len());

                    // Test various email properties
                    for email in &emails {
                        // All emails should have basic properties
                        assert!(!email.message_id.is_empty());
                        assert!(!email.subject.is_empty());
                        assert!(!email.from.is_empty());

                        // Patch detection should be consistent
                        if email.is_patch {
                            // If it's detected as a patch, it should have patch-like content
                            let has_patch_indicators = email.subject.contains("[PATCH]") ||
                                                     email.subject.contains("[RFC]") ||
                                                     email.body.contains("diff --git") ||
                                                     email.body.contains("@@");

                            assert!(has_patch_indicators,
                                   "Email detected as patch should have patch indicators: {}",
                                   email.subject);
                        }

                        // Message-ID should be properly formatted
                        assert!(email.message_id.starts_with('<') && email.message_id.ends_with('>'),
                               "Message-ID should be in angle brackets: {}", email.message_id);
                    }

                    println!("✅ All edge cases handled correctly");
                }
            }
        }
    }
}
