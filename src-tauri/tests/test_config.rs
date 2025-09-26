#[cfg(test)]
mod test_config {
    use std::env;
use mailing_list_parser_lib::models::DEFAULT_MAILING_LIST_GIT_PATH;

    /// Test configuration for the mailing list parser
    pub struct TestConfig {
        pub git_repo_path: String,
        pub enable_integration_tests: bool,
        pub max_commits_to_test: usize,
    }

    impl TestConfig {
        /// Create a new test configuration
        pub fn new() -> Self {
            let git_repo_path = env::var("BPF_GIT_PATH").unwrap_or_else(|_| DEFAULT_MAILING_LIST_GIT_PATH.to_string());
            let enable_integration_tests = env::var("ENABLE_INTEGRATION_TESTS")
                .unwrap_or_else(|_| "true".to_string())
                .parse()
                .unwrap_or(true);

            Self {
                git_repo_path,
                enable_integration_tests,
                max_commits_to_test: env::var("MAX_TEST_COMMITS")
                    .unwrap_or_else(|_| "50".to_string())
                    .parse()
                    .unwrap_or(50),
            }
        }

        /// Get environment information for tests
        pub fn get_environment_info(&self) -> String {
            format!(
                "Test Environment:\n\
                 - Git Repository: {}\n\
                 - Integration Tests: {}\n\
                 - Max Commits to Test: {}\n\
                 - Current Working Directory: {}\n\
                 - BPF_GIT_PATH: {}\n\
                 - ENABLE_INTEGRATION_TESTS: {}\n\
                 - MAX_TEST_COMMITS: {}",
                self.git_repo_path,
                self.enable_integration_tests,
                self.max_commits_to_test,
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("unknown")).display(),
                env::var("BPF_GIT_PATH").unwrap_or_else(|_| "not set".to_string()),
                env::var("ENABLE_INTEGRATION_TESTS").unwrap_or_else(|_| "not set".to_string()),
                env::var("MAX_TEST_COMMITS").unwrap_or_else(|_| "not set".to_string())
            )
        }
    }

    #[test]
    fn test_configuration_loading() {
        let config = TestConfig::new();

        println!("{}", config.get_environment_info());

        // Basic validation
        assert!(!config.git_repo_path.is_empty());
        assert!(config.max_commits_to_test > 0);
        assert!(config.max_commits_to_test <= 1000); // Reasonable upper bound

        // Validate that the path is properly formatted
        assert!(config.git_repo_path.ends_with(".git") || config.git_repo_path.ends_with("0.git"),
               "Git repository path should end with .git: {}", config.git_repo_path);

        // Test that we can access the directory structure
        let path = std::path::Path::new(&config.git_repo_path);
        if path.exists() {
            println!("✅ Git repository found at: {}", config.git_repo_path);

            // Check if it's actually a git repository
            let git_dir = path.join(".git");
            if git_dir.exists() || path.ends_with(".git") {
                println!("✅ Valid git repository structure detected");
            } else {
                println!("⚠️  Warning: Git repository structure may be incomplete");
            }
        } else {
            println!("⚠️  Warning: Git repository path does not exist: {}", config.git_repo_path);
            println!("   You may need to clone the repository or set BPF_GIT_PATH environment variable");
        }
    }

    #[test]
    fn test_environment_variables() {
        // Test that environment variables are properly read
        let config = TestConfig::new();

        // These are just informational tests to show what values are being used
        println!("Using git repo path: {}", config.git_repo_path);
        println!("Integration tests enabled: {}", config.enable_integration_tests);
        println!("Max commits to test: {}", config.max_commits_to_test);

        // You can override these values for different test scenarios:
        // export BPF_GIT_PATH=/custom/path/to/bpf
        // export ENABLE_INTEGRATION_TESTS=false
        // export MAX_TEST_COMMITS=10
    }
}
