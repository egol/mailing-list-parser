import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";
import AuthorPatches from "./components/AuthorPatches";

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
  total_replies_inserted: number;
  total_commits_inserted: number;
  errors: string[];
}

interface DatabaseStats {
  authors?: number;
  emails?: number;
  email_replies?: number;
  email_commits?: number;
  total_emails?: number;
  unique_authors?: number;
  unique_threads?: number;
}

interface Author {
  author_id: number;
  name?: string;
  email: string;
  first_seen?: string;
  patch_count: number;
}

interface Patch {
  patch_id: number;
  author_id: number;
  message_id: string;
  subject: string;
  sent_at: string;
  commit_hash?: string;
  body_text?: string;
  is_series?: boolean;
  series_number?: number;
  series_total?: number;
  created_at?: string;
}

function App() {
  const [error, setError] = useState<string>("");
  const [loading, setLoading] = useState<boolean>(false);
  const [currentView, setCurrentView] = useState<'database' | 'authors'>('database');

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

  useEffect(() => {
    loadInitialData();
  }, []);

  useEffect(() => {
    const unlisten = listen('populate-progress', (event: any) => {
      setPopulationProgress(event.payload);
    });

    return () => {
      unlisten.then(fn => fn());
    };
  }, []);

  async function loadInitialData() {
    setLoading(true);
    setError("");

    try {
      // Check if database is already set up and populated
      await loadDatabaseStats();

      // Load max commits count
      await loadMaxCommits();

      // Set default view to database since that's our primary interface now
      setCurrentView('database');

    } catch (err) {
      const errorMessage = err as ParseError;
      setError(errorMessage.message);
    } finally {
      setLoading(false);
    }
  }




  // Database functions
  async function testDatabaseConnection() {
    setLoading(true);
    setError("");

    try {
      const connected: boolean = await invoke("test_database_connection");
      setDatabaseConnected(connected);
      if (connected) {
        await loadDatabaseStats();
      }
    } catch (err) {
      const errorMessage = err as string;
      setError(errorMessage);
      setDatabaseConnected(false);
    } finally {
      setLoading(false);
    }
  }

  async function loadDatabaseStats() {
    try {
      const stats: DatabaseStats = await invoke("get_database_stats");
      setDatabaseStats(stats);
    } catch (err) {
      console.error("Failed to load database stats:", err);
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
        setDatabaseConnected(true);
        await loadDatabaseStats();
        alert(`Database setup successful! Created tables: ${result.tables_created.join(", ")}`);
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

  async function resetDatabase() {
    const confirmed = confirm(
      "⚠️ WARNING: This will drop ALL database tables and data!\n\n" +
      "Are you absolutely sure you want to reset the database?"
    );

    if (!confirmed) {
      return;
    }

    setLoading(true);
    setError("");

    try {
      const message: string = await invoke("reset_database");
      await loadDatabaseStats();
      alert(`✅ ${message}`);
    } catch (err) {
      const errorMessage = err as string;
      setError(errorMessage);
    } finally {
      setLoading(false);
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

  return (
    <main className="app">
      <header className="app-header">
        <h1>BPF Mailing List Parser</h1>
        <div className="stats">
          {databaseConnected && (
            <>
              <span>DB Emails: {databaseStats.total_emails || 0}</span>
              <span> | Authors: {databaseStats.unique_authors || 0}</span>
              <span> | Threads: {databaseStats.unique_threads || 0}</span>
            </>
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

        {currentView === 'database' && (
          <div className="database-section">
            <h2>Database Management</h2>

            <div className="database-status">
              <div className="status-item">
                <span className="status-label">Connection Status:</span>
                <span className={`status-value ${databaseConnected ? 'connected' : 'disconnected'}`}>
                  {databaseConnected ? 'Connected' : 'Not Connected'}
                </span>
                {!databaseConnected && (
                  <button onClick={testDatabaseConnection} disabled={loading} className="test-btn">
                    Test Connection
                  </button>
                )}
              </div>
            </div>

            {databaseConnected && (
              <>
                <div className="database-stats">
                  <h3>Database Statistics</h3>
                  <div className="stats-grid">
                    <div className="stat-item">
                      <span className="stat-label">Total Emails:</span>
                      <span className="stat-value">{databaseStats.total_emails || 0}</span>
                    </div>
                    <div className="stat-item">
                      <span className="stat-label">Unique Authors:</span>
                      <span className="stat-value">{databaseStats.unique_authors || 0}</span>
                    </div>
                    <div className="stat-item">
                      <span className="stat-label">Email Threads:</span>
                      <span className="stat-value">{databaseStats.unique_threads || 0}</span>
                    </div>
                    <div className="stat-item">
                      <span className="stat-label">Raw Tables:</span>
                      <span className="stat-value">
                        Authors: {databaseStats.authors || 0} |
                        Emails: {databaseStats.emails || 0} |
                        Replies: {databaseStats.email_replies || 0} |
                        Commits: {databaseStats.email_commits || 0}
                      </span>
                    </div>
                  </div>
                </div>

                <div className="database-actions">
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
                    <h3>⚠️ Danger Zone</h3>
                    <p>Reset the database by dropping all tables. This cannot be undone!</p>
                    <button
                      onClick={resetDatabase}
                      disabled={loading}
                      className="reset-btn"
                    >
                      {loading ? 'Resetting...' : 'Reset Database (Drop All Tables)'}
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
                        <p>Processed: {populationResult.total_processed} emails</p>
                        <p>Authors: {populationResult.total_authors_inserted}</p>
                        <p>Emails: {populationResult.total_emails_inserted}</p>
                        <p>Replies: {populationResult.total_replies_inserted}</p>
                        <p>Commits: {populationResult.total_commits_inserted}</p>
                        {!populationResult.success && (
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
            )}

            {!databaseConnected && (
              <div className="database-setup">
                <h3>Database Setup Required</h3>
                <p>Set up your PostgreSQL database connection and initialize the schema.</p>
                <button onClick={testDatabaseConnection} disabled={loading} className="test-btn">
                  Test Database Connection
                </button>
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
      </div>
    </main>
  );
}

export default App;
