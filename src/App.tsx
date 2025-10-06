import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import "./App.css";
import AuthorPatches from "./components/AuthorPatches";
import ThreadView from "./components/ThreadView";

interface ParseError {
  message: string;
}

interface DatabaseSetupResult {
  success: boolean;
  message: string;
  tables_created: string[];
}

interface DatabasePopulationResult {
  success: boolean;
  total_processed: number;
  total_authors_inserted: number;
  total_emails_inserted: number;
  errors: string[];
}

interface DatabaseStats {
  authors?: number;
  patches?: number;
  total_authors?: number;
  total_patches?: number;
  total_emails?: number;
  unique_authors?: number;
  unique_threads?: number;
}

interface GitConfig {
  repo_path: string;
  clone_url: string;
}

interface GitSyncResult {
  success: boolean;
  stdout: string;
  stderr: string;
  combined_output: string;
}

interface DatabaseConfig {
  host: string;
  port: number;
  user: string;
  password: string;
  database: string;
}

function App() {
  const [error, setError] = useState<string>("");
  const [loading, setLoading] = useState<boolean>(false);
  const [currentView, setCurrentView] = useState<'database' | 'authors' | 'threads'>('database');

  // Database related state
  const [databaseConnected, setDatabaseConnected] = useState<boolean>(false);
  const [databaseStats, setDatabaseStats] = useState<DatabaseStats>({});
  const [databaseSetupLoading, setDatabaseSetupLoading] = useState<boolean>(false);
  const [databasePopulationLoading, setDatabasePopulationLoading] = useState<boolean>(false);
  const [populationResult, setPopulationResult] = useState<DatabasePopulationResult | null>(null);

  // Commit parsing state
  const [commitLimit, setCommitLimit] = useState<number>(1000);
  const [maxCommits, setMaxCommits] = useState<number>(0);
  const [commitLimitLoading, setCommitLimitLoading] = useState<boolean>(false);

  // Population progress state
  const [populationProgress, setPopulationProgress] = useState<{current: number, total: number, commit_hash: string} | null>(null);

  // Git sync state
  const [syncLoading, setSyncLoading] = useState<boolean>(false);
  const [syncResult, setSyncResult] = useState<GitSyncResult | null>(null);
  
  // Git configuration state
  const [gitConfig, setGitConfig] = useState<GitConfig>({ repo_path: "", clone_url: "" });
  const [repoExists, setRepoExists] = useState<boolean>(false);
  const [showGitConfig, setShowGitConfig] = useState<boolean>(false);
  const [cloneLoading, setCloneLoading] = useState<boolean>(false);
  const [cloneResult, setCloneResult] = useState<GitSyncResult | null>(null);
  const [configSaving, setConfigSaving] = useState<boolean>(false);
  const [configSaveMessage, setConfigSaveMessage] = useState<string | null>(null);

  // Database connection state
  const [dbConfig, setDbConfig] = useState<DatabaseConfig>({
    host: "localhost",
    port: 5432,
    user: "postgres",
    password: "mysecretpassword",
    database: "postgres",
  });
  const [showDbConfig, setShowDbConfig] = useState<boolean>(false);
  const [dbConnecting, setDbConnecting] = useState<boolean>(false);
  const [dbConnectionError, setDbConnectionError] = useState<string | null>(null);

  // Reset database confirmation modal
  const [showResetConfirm, setShowResetConfirm] = useState<boolean>(false);
  const [resetLoading, setResetLoading] = useState<boolean>(false);

  useEffect(() => {
    loadInitialData();
    loadGitConfig();
    checkDatabaseConnection();
  }, []);

  useEffect(() => {
    const unlisten = listen('populate-progress', (event: any) => {
      setPopulationProgress(event.payload);
    });

    return () => {
      unlisten.then(fn => fn());
    };
  }, []);

  // Lazy-load max commits count only when user goes to database view
  useEffect(() => {
    if (currentView === 'database' && maxCommits === 0 && !commitLimitLoading) {
      loadMaxCommits();
    }
  }, [currentView]);

  async function checkDatabaseConnection() {
    try {
      const connected: boolean = await invoke("is_database_connected");
      setDatabaseConnected(connected);
      
      if (connected) {
        // If connected, load stats
        await loadDatabaseStats();
      }
    } catch (err) {
      console.error("Failed to check database connection:", err);
      setDatabaseConnected(false);
    }
  }

  async function loadInitialData() {
    setLoading(true);
    setError("");

    try {
      // Check if database is already connected (but don't auto-connect)
      await checkDatabaseConnection();

      // Set default view to database since that's our primary interface now
      setCurrentView('database');

    } catch (err) {
      const errorMessage = err as ParseError;
      setError(errorMessage.message);
    } finally {
      setLoading(false);
    }
  }

  async function connectToDatabase() {
    setDbConnecting(true);
    setDbConnectionError(null);
    setError("");

    try {
      const message: string = await invoke("connect_database", {
        host: dbConfig.host,
        port: dbConfig.port,
        user: dbConfig.user,
        password: dbConfig.password,
        database: dbConfig.database,
      });
      
      setDatabaseConnected(true);
      setShowDbConfig(false);
      
      // Load database stats after connection
      await loadDatabaseStats();
      
      console.log(message);
    } catch (err) {
      const errorMessage = err as string;
      setDbConnectionError(errorMessage);
      setDatabaseConnected(false);
    } finally {
      setDbConnecting(false);
    }
  }

  async function disconnectFromDatabase() {
    try {
      const message: string = await invoke("disconnect_database");
      setDatabaseConnected(false);
      setDatabaseStats({});
      console.log(message);
    } catch (err) {
      const errorMessage = err as string;
      setError(errorMessage);
    }
  }




  // Database functions
  async function loadDatabaseStats() {
    try {
      const stats: DatabaseStats = await invoke("get_database_stats");
      setDatabaseStats(stats);
      // Don't override connection status here - it's set by connection test
    } catch (err) {
      console.error("Failed to load database stats:", err);
      // Log but don't set disconnected - stats might fail for other reasons
    }
  }

  async function loadMaxCommits() {
    setCommitLimitLoading(true);
    try {
      const count: number = await invoke("get_total_git_commits");
      setMaxCommits(count);
    } catch (err) {
      console.error("Failed to load max commits count:", err);
      setMaxCommits(0);
    } finally {
      setCommitLimitLoading(false);
    }
  }

  async function setupDatabase() {
    setDatabaseSetupLoading(true);
    setError("");

    try {
      const result: DatabaseSetupResult = await invoke("setup_database");
      if (result.success) {
        // Test connection after setup
        const connected: boolean = await invoke("test_database_connection");
        setDatabaseConnected(connected);
        
        if (connected) {
          await loadDatabaseStats();
        }
        console.log(`Database setup successful! Created tables: ${result.tables_created.join(", ")}`);
      } else {
        setError(result.message);
      }
    } catch (err) {
      const errorMessage = err as string;
      setError(errorMessage);
    } finally {
      setDatabaseSetupLoading(false);
    }
  }

  async function confirmResetDatabase() {
    setResetLoading(true);
    setError("");

    try {
      const message: string = await invoke("reset_database");
      await loadDatabaseStats();
      setShowResetConfirm(false);
      // Show success message in the error area (it's not really an error)
      console.log(message);
    } catch (err) {
      const errorMessage = err as string;
      setError(errorMessage);
      setShowResetConfirm(false);
    } finally {
      setResetLoading(false);
    }
  }

  async function populateDatabase() {
    setDatabasePopulationLoading(true);
    setError("");
    setPopulationResult(null);
    setPopulationProgress(null);

    try {
      const result: DatabasePopulationResult = await invoke("populate_database", { limit: commitLimit || 1000 });
      setPopulationResult(result);

      if (result.success) {
        await loadDatabaseStats();

        // Switch to authors view - AuthorPatches component will load data automatically
        setCurrentView('authors');
      } else {
        setError(`Population completed with errors. Check console for details.`);
        console.error("Population errors:", result.errors);
      }
    } catch (err) {
      const errorMessage = err as string;
      setError(errorMessage);
    } finally {
      setDatabasePopulationLoading(false);
      setPopulationProgress(null);
    }
  }

  async function loadGitConfig() {
    try {
      const config: GitConfig = await invoke("get_git_config");
      setGitConfig(config);
      
      const exists: boolean = await invoke("check_git_repo_exists");
      setRepoExists(exists);
    } catch (err) {
      console.error("Failed to load git config:", err);
    }
  }

  async function browseForFolder() {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Select Repository Location",
      });
      
      if (selected && typeof selected === 'string') {
        setGitConfig({ ...gitConfig, repo_path: selected });
      }
    } catch (err) {
      console.error("Failed to open folder picker:", err);
    }
  }

  async function saveGitConfig() {
    if (!gitConfig.clone_url || !gitConfig.repo_path) {
      setError("Please provide both clone URL and repository path");
      return;
    }

    setConfigSaving(true);
    setError("");
    setConfigSaveMessage(null);

    try {
      const updatedConfig: GitConfig = await invoke("update_git_config", {
        repoPath: gitConfig.repo_path,
        cloneUrl: gitConfig.clone_url,
      });
      
      setGitConfig(updatedConfig);
      setConfigSaveMessage("Configuration saved successfully!");
      setShowGitConfig(false);
      
      // Check if repo exists with new path
      const exists: boolean = await invoke("check_git_repo_exists", { path: updatedConfig.repo_path });
      setRepoExists(exists);
      
      // Clear success message after 3 seconds
      setTimeout(() => setConfigSaveMessage(null), 3000);
    } catch (err) {
      const errorMessage = err as string;
      setError(errorMessage);
    } finally {
      setConfigSaving(false);
    }
  }

  async function syncGitRepository() {
    setSyncLoading(true);
    setError("");
    setSyncResult(null);

    try {
      const result: GitSyncResult = await invoke("sync_git_repository", { repoPath: gitConfig.repo_path || null });
      setSyncResult(result);
      
      // Reload max commits count after sync
      await loadMaxCommits();
    } catch (err) {
      const errorMessage = err as string;
      setError(errorMessage);
    } finally {
      setSyncLoading(false);
    }
  }

  async function cloneRepository() {
    if (!gitConfig.clone_url || !gitConfig.repo_path) {
      setError("Please provide both clone URL and target path");
      return;
    }

    setCloneLoading(true);
    setError("");
    setCloneResult(null);

    try {
      const result: GitSyncResult = await invoke("clone_git_repository", {
        cloneUrl: gitConfig.clone_url,
        targetPath: gitConfig.repo_path,
        bare: true // Clone as bare repository for BPF mailing list
      });
      
      setCloneResult(result);
      
      if (result.success) {
        setRepoExists(true);
        setShowGitConfig(false);
        // Reload max commits count after clone
        await loadMaxCommits();
      }
    } catch (err) {
      const errorMessage = err as string;
      setError(errorMessage);
    } finally {
      setCloneLoading(false);
    }
  }

  return (
    <main className="app">
      <header className="app-header">
        <h1>BPF Mailing List Parser</h1>
        <div className="stats">
          {databaseConnected ? (
            <>
              <span style={{ color: '#4caf50', fontWeight: 'bold' }}>● Connected</span>
              <span> | Patches: {databaseStats.total_patches || 0}</span>
              <span> | Authors: {databaseStats.total_authors || 0}</span>
              <span> | Emails: {databaseStats.total_emails || 0}</span>
            </>
          ) : (
            <span style={{ color: '#ff9800', fontWeight: 'bold' }}>○ Not Connected</span>
          )}
        </div>
        <div className="nav-tabs">
          <button
            className={currentView === 'database' ? 'active' : ''}
            onClick={() => setCurrentView('database')}
          >
            Database
          </button>
          <button
            className={currentView === 'authors' ? 'active' : ''}
            onClick={() => setCurrentView('authors')}
          >
            Authors & Patches
          </button>
          <button
            className={currentView === 'threads' ? 'active' : ''}
            onClick={() => setCurrentView('threads')}
          >
            Threads
          </button>
        </div>
      </header>

      <div className="main-content">

        {error && (
          <div className="error-message">
            <strong>Error:</strong> {error}
          </div>
        )}


        {currentView === 'authors' && (
          <AuthorPatches />
        )}

        {currentView === 'threads' && (
          <ThreadView />
        )}

        {currentView === 'database' && (
          <div className="database-section">
            <h2>Database Management</h2>

            {databaseConnected ? (
              <>
                <div className="database-stats">
                  <h3>Database Statistics</h3>
                  <div className="stats-grid">
                    <div className="stat-item">
                      <span className="stat-label">Total Patches:</span>
                      <span className="stat-value">{databaseStats.total_patches || 0}</span>
                    </div>
                    <div className="stat-item">
                      <span className="stat-label">Total Authors:</span>
                      <span className="stat-value">{databaseStats.total_authors || 0}</span>
                    </div>
                    <div className="stat-item">
                      <span className="stat-label">Email Addresses:</span>
                      <span className="stat-value">{databaseStats.total_emails || 0}</span>
                    </div>
                    <div className="stat-item">
                      <span className="stat-label">Raw Tables:</span>
                      <span className="stat-value">
                        Authors: {databaseStats.authors || 0} |
                        Patches: {databaseStats.patches || 0}
                      </span>
                    </div>
                  </div>
                </div>

                <div className="database-actions">
                  <div className="action-section">
                    <h3>Database Connection</h3>
                    <div className="repo-status success">
                      <strong>✓ Connected to database</strong>
                      <p>{dbConfig.user}@{dbConfig.host}:{dbConfig.port}/{dbConfig.database}</p>
                    </div>
                    <button
                      onClick={disconnectFromDatabase}
                      className="reset-btn"
                      style={{ backgroundColor: '#f44336' }}
                    >
                      Disconnect
                    </button>
                  </div>

                  <div className="action-section">
                    <h3>Git Repository</h3>
                    
                    {configSaveMessage && (
                      <div className="config-save-message success">
                        ✓ {configSaveMessage}
                      </div>
                    )}

                    {!repoExists ? (
                      <>
                        <div className="repo-status warning">
                          <strong>⚠ Repository not found</strong>
                          <p>No repository found at: {gitConfig.repo_path}</p>
                        </div>
                        
                        <button
                          onClick={() => setShowGitConfig(!showGitConfig)}
                          className="config-btn"
                        >
                          {showGitConfig ? 'Hide Configuration' : 'Configure & Clone Repository'}
                        </button>

                        {showGitConfig && (
                          <div className="git-config">
                            <div className="config-field">
                              <label htmlFor="clone-url">Clone URL:</label>
                              <input
                                id="clone-url"
                                type="text"
                                value={gitConfig.clone_url}
                                onChange={(e) => setGitConfig({ ...gitConfig, clone_url: e.target.value })}
                                placeholder="https://lore.kernel.org/bpf/0"
                              />
                              <small>Default: BPF Mailing List (https://lore.kernel.org/bpf/0)</small>
                            </div>

                            <div className="config-field">
                              <label htmlFor="repo-path">Local Path:</label>
                              <div className="input-with-button">
                              <input
                                id="repo-path"
                                type="text"
                                value={gitConfig.repo_path}
                                onChange={(e) => setGitConfig({ ...gitConfig, repo_path: e.target.value })}
                                placeholder="/path/to/repository"
                              />
                                <button
                                  onClick={browseForFolder}
                                  className="browse-btn"
                                  type="button"
                                >
                                  Browse...
                                </button>
                              </div>
                              <small>Where to clone the repository on your system</small>
                            </div>

                            <div className="button-group">
                              <button
                                onClick={saveGitConfig}
                                disabled={configSaving || !gitConfig.clone_url || !gitConfig.repo_path}
                                className="save-config-btn"
                              >
                                {configSaving ? 'Saving...' : 'Save Configuration'}
                              </button>

                              <button
                                onClick={cloneRepository}
                                disabled={cloneLoading || !gitConfig.clone_url || !gitConfig.repo_path}
                                className="clone-btn"
                              >
                                {cloneLoading ? 'Cloning...' : 'Clone Repository'}
                              </button>
                            </div>

                            {cloneResult && (
                              <div className={`git-output ${cloneResult.success ? 'success' : 'error'}`}>
                                <strong>{cloneResult.success ? '✓ Clone Successful' : '✗ Clone Failed'}</strong>
                                <pre>{cloneResult.combined_output}</pre>
                              </div>
                            )}
                          </div>
                        )}
                      </>
                    ) : (
                      <>
                        <div className="repo-status success">
                          <strong>✓ Repository configured</strong>
                          <p>{gitConfig.repo_path}</p>
                        </div>

                        <button
                          onClick={syncGitRepository}
                          disabled={syncLoading}
                          className="sync-btn"
                        >
                          {syncLoading ? 'Syncing...' : 'Sync Repository (git fetch)'}
                        </button>

                        {syncResult && (
                          <div className={`git-output ${syncResult.success ? 'success' : 'error'}`}>
                            <strong>{syncResult.success ? '✓ Sync Complete' : '✗ Sync Failed'}</strong>
                            <pre>{syncResult.combined_output}</pre>
                          </div>
                        )}

                        <button
                          onClick={() => setShowGitConfig(!showGitConfig)}
                          className="config-toggle-btn"
                        >
                          {showGitConfig ? 'Hide' : 'Edit'} Configuration
                        </button>

                        {showGitConfig && (
                          <div className="git-config">
                            <div className="config-field">
                              <label htmlFor="edit-clone-url">Clone URL:</label>
                              <input
                                id="edit-clone-url"
                                type="text"
                                value={gitConfig.clone_url}
                                onChange={(e) => setGitConfig({ ...gitConfig, clone_url: e.target.value })}
                                placeholder="https://lore.kernel.org/bpf/0"
                              />
                            </div>

                            <div className="config-field">
                              <label htmlFor="edit-repo-path">Local Path:</label>
                              <div className="input-with-button">
                              <input
                                id="edit-repo-path"
                                type="text"
                                value={gitConfig.repo_path}
                                onChange={(e) => setGitConfig({ ...gitConfig, repo_path: e.target.value })}
                                placeholder="/path/to/repository"
                              />
                                <button
                                  onClick={browseForFolder}
                                  className="browse-btn"
                                  type="button"
                                >
                                  Browse...
                                </button>
                              </div>
                            </div>

                            <div className="button-group">
                              <button
                                onClick={saveGitConfig}
                                disabled={configSaving || !gitConfig.clone_url || !gitConfig.repo_path}
                                className="save-config-btn"
                              >
                                {configSaving ? 'Saving...' : 'Save Changes'}
                              </button>
                              
                              <button
                                onClick={() => {
                                  setShowGitConfig(false);
                                  loadGitConfig(); // Reload to reset changes
                                }}
                                className="cancel-btn"
                              >
                                Cancel
                              </button>
                            </div>
                            
                            <small className="config-hint">
                              Configuration is saved in your application data folder
                            </small>
                          </div>
                        )}
                      </>
                    )}
                  </div>

                  <div className="action-section">
                    <h3>Setup Database</h3>
                    <p>Create the necessary tables and views for the mailing list data.</p>
                    <button
                      onClick={setupDatabase}
                      disabled={databaseSetupLoading}
                      className="setup-btn"
                    >
                      {databaseSetupLoading ? 'Setting up...' : 'Setup Database'}
                    </button>
                  </div>

                  <div className="action-section danger-zone">
                    <h3>Danger Zone</h3>
                    <p>Reset the database by dropping all tables. This cannot be undone!</p>
                    <button
                      onClick={() => setShowResetConfirm(true)}
                      disabled={resetLoading}
                      className="reset-btn"
                    >
                      Reset Database (Drop All Tables)
                    </button>
                  </div>

                  <div className="action-section">
                    <h3>Populate Database</h3>
                    <p>Parse and store email data from the git repository.</p>

                    <div className="populate-config">
                      <div className="commit-limit-selector">
                        <label htmlFor="commit-limit">Number of commits to parse:</label>
                        <input
                          id="commit-limit"
                          type="number"
                          value={commitLimit || ""}
                          onChange={(e) => {
                            const value = e.target.value;
                            if (value === "") {
                              setCommitLimit(0);
                            } else {
                              const parsed = parseInt(value);
                              if (!isNaN(parsed) && parsed > 0) {
                                setCommitLimit(parsed);
                              }
                            }
                          }}
                          onBlur={(e) => {
                            const value = e.target.value;
                            if (value === "" || parseInt(value) === 0) {
                              setCommitLimit(1000);
                            }
                          }}
                          disabled={databasePopulationLoading}
                          min="1"
                          max={maxCommits > 0 ? maxCommits : 50000}
                          placeholder="1000"
                        />
                      </div>
                      {commitLimitLoading ? (
                        <div className="loading-small">Loading commit count...</div>
                      ) : maxCommits > 0 && (
                        <div className="commit-stats">
                          <small>Total available commits: {maxCommits.toLocaleString()}</small>
                        </div>
                      )}
                    </div>

                    {databasePopulationLoading && populationProgress && (
                      <div className="progress-container">
                        <div className="progress-bar">
                          <div
                            className="progress-fill"
                            style={{ width: `${(populationProgress.current / populationProgress.total) * 100}%` }}
                          ></div>
                        </div>
                        <div className="progress-text">
                          Processing commit {populationProgress.current} of {populationProgress.total}
                        </div>
                        <div className="progress-commit">
                          {populationProgress.commit_hash.substring(0, 8)}...
                        </div>
                      </div>
                    )}

                    <button
                      onClick={populateDatabase}
                      disabled={databasePopulationLoading}
                      className="populate-btn"
                    >
                      {databasePopulationLoading ? 'Populating...' : 'Populate Database'}
                    </button>

                    {populationResult && (
                      <div className={`population-result ${populationResult.success ? 'success' : 'error'}`}>
                        <h4>Population Results:</h4>
                        <p>Processed: {populationResult.total_processed} commits</p>
                        <p>Authors: {populationResult.total_authors_inserted}</p>
                        <p>Patches: {populationResult.total_emails_inserted}</p>
                        {!populationResult.success && populationResult.errors.length > 0 && (
                          <div className="error-details">
                            <p className="error-text">Errors: {populationResult.errors.length}</p>
                            <details>
                              <summary>Show Error Details</summary>
                              <div className="error-list">
                                {populationResult.errors.map((error, index) => (
                                  <div key={index} className="error-item">
                                    <strong>Error {index + 1}:</strong> {error}
                                  </div>
                                ))}
                              </div>
                            </details>
                          </div>
                        )}
                      </div>
                    )}
                  </div>
                </div>
              </>
            ) : (
              <div className="database-setup">
                <h3>Database Connection</h3>
                <p>Connect to your PostgreSQL database to manage mailing list data.</p>
                
                {dbConnectionError && (
                  <div className="error-message">
                    <strong>Connection Error:</strong> {dbConnectionError}
                  </div>
                )}

                <button
                  onClick={() => setShowDbConfig(!showDbConfig)}
                  className="config-btn"
                >
                  {showDbConfig ? 'Hide Connection Settings' : 'Configure & Connect'}
                </button>

                {showDbConfig && (
                  <div className="git-config">
                    <div className="config-field">
                      <label htmlFor="db-host">Host:</label>
                      <input
                        id="db-host"
                        type="text"
                        value={dbConfig.host}
                        onChange={(e) => setDbConfig({ ...dbConfig, host: e.target.value })}
                        placeholder="localhost"
                      />
                      <small>Database server hostname or IP address</small>
                    </div>

                    <div className="config-field">
                      <label htmlFor="db-port">Port:</label>
                      <input
                        id="db-port"
                        type="number"
                        value={dbConfig.port}
                        onChange={(e) => setDbConfig({ ...dbConfig, port: parseInt(e.target.value) || 5432 })}
                        placeholder="5432"
                      />
                      <small>PostgreSQL port (default: 5432)</small>
                    </div>

                    <div className="config-field">
                      <label htmlFor="db-user">Username:</label>
                      <input
                        id="db-user"
                        type="text"
                        value={dbConfig.user}
                        onChange={(e) => setDbConfig({ ...dbConfig, user: e.target.value })}
                        placeholder="postgres"
                      />
                      <small>Database username</small>
                    </div>

                    <div className="config-field">
                      <label htmlFor="db-password">Password:</label>
                      <input
                        id="db-password"
                        type="password"
                        value={dbConfig.password}
                        onChange={(e) => setDbConfig({ ...dbConfig, password: e.target.value })}
                        placeholder="mysecretpassword"
                      />
                      <small>Database password</small>
                    </div>

                    <div className="config-field">
                      <label htmlFor="db-database">Database Name:</label>
                      <input
                        id="db-database"
                        type="text"
                        value={dbConfig.database}
                        onChange={(e) => setDbConfig({ ...dbConfig, database: e.target.value })}
                        placeholder="postgres"
                      />
                      <small>Name of the database to use</small>
                    </div>

                    <div className="button-group">
                      <button
                        onClick={connectToDatabase}
                        disabled={dbConnecting || !dbConfig.host || !dbConfig.user || !dbConfig.database}
                        className="clone-btn"
                      >
                        {dbConnecting ? 'Connecting...' : 'Connect to Database'}
                      </button>

                      <button
                        onClick={() => {
                          setShowDbConfig(false);
                          setDbConnectionError(null);
                        }}
                        className="cancel-btn"
                      >
                        Cancel
                      </button>
                    </div>
                  </div>
                )}

                <div className="connection-hint" style={{ marginTop: '1rem' }}>
                  <strong>Note:</strong> Make sure PostgreSQL is running before connecting.
                </div>
              </div>
            )}
          </div>
        )}

        {loading && (
          <div className="loading">
            <div className="spinner"></div>
            <span>Loading...</span>
          </div>
        )}

        {/* Reset Database Confirmation Modal */}
        {showResetConfirm && (
          <div className="modal-overlay" onClick={() => !resetLoading && setShowResetConfirm(false)}>
            <div className="modal-content" onClick={(e) => e.stopPropagation()}>
              <h2 style={{ color: '#f44336' }}>⚠️ Warning: Reset Database</h2>
              <p style={{ fontSize: '1.1em', marginBottom: '1rem' }}>
                This will <strong>permanently delete ALL</strong> database tables and data!
              </p>
              <p style={{ marginBottom: '1.5rem' }}>
                This action <strong>CANNOT be undone</strong>. Are you absolutely sure?
              </p>
              
              <div className="button-group" style={{ justifyContent: 'flex-end', gap: '1rem' }}>
                <button
                  onClick={() => setShowResetConfirm(false)}
                  disabled={resetLoading}
                  className="cancel-btn"
                  style={{ padding: '0.75rem 1.5rem' }}
                >
                  Cancel
                </button>
                <button
                  onClick={confirmResetDatabase}
                  disabled={resetLoading}
                  className="reset-btn"
                  style={{ padding: '0.75rem 1.5rem', backgroundColor: '#f44336' }}
                >
                  {resetLoading ? 'Resetting...' : 'Yes, Reset Database'}
                </button>
              </div>
            </div>
          </div>
        )}
      </div>
    </main>
  );
}

export default App;
