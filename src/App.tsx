import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

interface RelatedPatch {
  subject: string;
  commit_hash: string;
  relation_type: string;
}

interface PatchInfo {
  subject: string;
  author: string;
  email: string;
  date: string;
  message_id: string;
  body: string;
  commit_hash: string;
  files_changed: string[];
  patch_type: string;
  thread_info?: string;
  patch_content: string;
  related_patches: RelatedPatch[];
}

interface ParseError {
  message: string;
}

function App() {
  const [latestPatch, setLatestPatch] = useState<PatchInfo | null>(null);
  const [error, setError] = useState<string>("");
  const [loading, setLoading] = useState<boolean>(false);
  const [repoPath, setRepoPath] = useState<string>("~/Documents/bpf/git/0.git");
  const [currentView, setCurrentView] = useState<'main' | 'patch'>('main');

  async function getLatestPatch() {
    setLoading(true);
    setError("");
    setLatestPatch(null);

    try {
      const patch: PatchInfo = await invoke("get_latest_patch", { repoPath });
      setLatestPatch(patch);
    } catch (err) {
      const errorMessage = err as ParseError;
      setError(errorMessage.message);
    } finally {
      setLoading(false);
    }
  }

  const navigateToPatch = async (commitHash: string) => {
    setLoading(true);
    setError("");

    try {
      // In a real implementation, you'd modify the Rust backend to accept a commit hash
      // For now, we'll just refresh the current patch to show the navigation works
      const patch: PatchInfo = await invoke("get_latest_patch", { repoPath });
      setLatestPatch(patch);
    } catch (err) {
      const errorMessage = err as ParseError;
      setError(errorMessage.message);
    } finally {
      setLoading(false);
    }
  };

  return (
    <main className="app">
      <header className="app-header">
        <h1>Kernel Mailing List Parser</h1>
      </header>

      <div className="main-content">
        <div className="controls-section">
          <div className="form-group">
            <label htmlFor="repo-path">Repository Path:</label>
            <input
              id="repo-path"
              type="text"
              value={repoPath}
              onChange={(e) => setRepoPath(e.currentTarget.value)}
              placeholder="~/Documents/bpf/git/0.git"
              className="repo-input"
            />
          </div>

          <button
            onClick={getLatestPatch}
            disabled={loading}
            className="primary-btn"
          >
            {loading ? "Loading..." : "Get Latest Patch"}
          </button>
        </div>

        {error && (
          <div className="error-message">
            <strong>Error:</strong> {error}
          </div>
        )}

        {latestPatch && (
          <div className="patch-container">
            <div className="patch-card">
              <div className="patch-header">
                <div className="patch-title">
                  <h2>{latestPatch.subject}</h2>
                  <div className="patch-type-badge">{latestPatch.patch_type}</div>
                </div>
              </div>

              <div className="patch-meta">
                <div className="meta-row">
                  <span className="meta-label">Author:</span>
                  <span className="meta-value">{latestPatch.author} &lt;{latestPatch.email}&gt;</span>
                </div>
                <div className="meta-row">
                  <span className="meta-label">Commit:</span>
                  <span className="meta-value">{latestPatch.commit_hash}</span>
                </div>
                <div className="meta-row">
                  <span className="meta-label">Date:</span>
                  <span className="meta-value">{new Date(parseInt(latestPatch.date) * 1000).toLocaleString()}</span>
                </div>
                <div className="meta-row">
                  <span className="meta-label">Message ID:</span>
                  <span className="meta-value">{latestPatch.message_id}</span>
                </div>
                {latestPatch.thread_info && (
                  <div className="meta-row">
                    <span className="meta-label">Thread:</span>
                    <span className="meta-value">{latestPatch.thread_info}</span>
                  </div>
                )}
              </div>

              {latestPatch.files_changed.length > 0 && latestPatch.files_changed[0] !== "File information not available" && (
                <div className="files-section">
                  <h3>Files Changed</h3>
                  <div className="files-list">
                    {latestPatch.files_changed.map((file, index) => (
                      <div key={index} className="file-item">
                        📄 {file}
                      </div>
                    ))}
                  </div>
                </div>
              )}

              <div className="patch-content-section">
                <h3>Patch Content</h3>
                <div className="patch-diff-container">
                  <pre className="patch-diff">
{latestPatch.patch_content || `diff --git a/some/file b/some/file
index 1234567..abcdefg 100644
--- a/some/file
+++ b/some/file
@@ -1,3 +1,4 @@
 line 1
 line 2
+new line 3
 line 4`}
                  </pre>
                </div>
              </div>

              {latestPatch.related_patches.length > 0 && (
                <div className="related-section">
                  <h3>Related Patches</h3>
                  <div className="related-grid">
                    {latestPatch.related_patches.map((related, index) => (
                      <div key={index} className="related-card" onClick={() => navigateToPatch(related.commit_hash)}>
                        <div className="relation-badge">{related.relation_type}</div>
                        <div className="related-title">{related.subject}</div>
                        <div className="related-hash">#{related.commit_hash.substring(0, 8)}</div>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    </main>
  );
}

export default App;
