import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./AuthorPatches.css";

interface Author {
  author_id: number;
  display_name: string;
  first_name: string;
  last_name: string | null;
  emails: string[];
  first_seen: string | null;
  patch_count: number;
}

interface Patch {
  patch_id: number;
  author_id: number;
  message_id: string;
  subject: string;
  sent_at: string;
  commit_hash: string | null;
  body_text: string | null;
  is_series: boolean | null;
  series_number: number | null;
  series_total: number | null;
  created_at: string | null;
}

export default function AuthorPatches() {
  const [authors, setAuthors] = useState<Author[]>([]);
  const [selectedAuthor, setSelectedAuthor] = useState<Author | null>(null);
  const [patches, setPatches] = useState<Patch[]>([]);
  const [selectedPatch, setSelectedPatch] = useState<Patch | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    // Add a small delay to prevent loading conflicts when switching tabs quickly
    let isMounted = true;
    const timer = setTimeout(() => {
      if (isMounted) {
        loadAuthors();
      }
    }, 100);
    
    return () => {
      isMounted = false;
      clearTimeout(timer);
    };
  }, []);

  async function loadAuthors() {
    setLoading(true);
    setError("");

    try {
      const authorList: Author[] = await invoke("get_authors");
      setAuthors(authorList);
    } catch (err) {
      setError(`Failed to load authors: ${err}`);
    } finally {
      setLoading(false);
    }
  }

  async function loadPatches(author: Author) {
    setLoading(true);
    setError("");
    setSelectedAuthor(author);
    setSelectedPatch(null);

    try {
      const patchList: Patch[] = await invoke("get_patches_by_author", {
        authorId: author.author_id,
      });
      setPatches(patchList);
    } catch (err) {
      setError(`Failed to load patches: ${err}`);
    } finally {
      setLoading(false);
    }
  }

  function formatDate(dateString: string): string {
    try {
      return new Date(dateString).toLocaleString();
    } catch {
      return dateString;
    }
  }

  return (
    <div className="author-patches-container">
      {error && (
        <div className="error-banner">
          <strong>Error:</strong> {error}
        </div>
      )}

      <div className="three-column-layout">
        {/* Authors List */}
        <div className="authors-panel">
          <div className="panel-header">
            <h2>Authors ({authors.length})</h2>
            <button onClick={loadAuthors} className="refresh-btn" title="Refresh">
              ↻
            </button>
          </div>

          {loading && !selectedAuthor && <div className="loading-spinner">Loading...</div>}

          <div className="authors-list">
            {authors.map((author) => (
              <div
                key={author.author_id}
                className={`author-card ${selectedAuthor?.author_id === author.author_id ? "selected" : ""}`}
                onClick={() => loadPatches(author)}
              >
                <div className="author-name">
                  {author.display_name}
                </div>
                <div className="author-email">{author.emails[0] || "No email"}</div>
                <div className="author-stats">
                  <span className="patch-count">{author.patch_count} patches</span>
                </div>
              </div>
            ))}
          </div>
        </div>

        {/* Patches List */}
        <div className="patches-panel">
          <div className="panel-header">
            <h2>
              {selectedAuthor
                ? `Patches by ${selectedAuthor.display_name}`
                : "Select an author"}
            </h2>
            {selectedAuthor && (
              <span className="patch-count-badge">{patches.length}</span>
            )}
          </div>

          {loading && selectedAuthor && <div className="loading-spinner">Loading...</div>}

          {selectedAuthor && !loading && (
            <div className="patches-list">
              {patches.map((patch) => (
                <div
                  key={patch.patch_id}
                  className={`patch-card ${selectedPatch?.patch_id === patch.patch_id ? "selected" : ""}`}
                  onClick={() => setSelectedPatch(patch)}
                >
                  <div className="patch-subject">{patch.subject}</div>
                  <div className="patch-meta">
                    <span className="patch-date">{formatDate(patch.sent_at)}</span>
                    {patch.is_series && (
                      <span className="series-badge">
                        [{patch.series_number}/{patch.series_total}]
                      </span>
                    )}
                  </div>
                </div>
              ))}

              {patches.length === 0 && (
                <div className="empty-state">No patches found for this author</div>
              )}
            </div>
          )}

          {!selectedAuthor && (
            <div className="empty-state">
              Select an author to view their patches
            </div>
          )}
        </div>

        {/* Patch Detail */}
        <div className="patch-detail-panel">
          <div className="panel-header">
            <h2>{selectedPatch ? "Patch Details" : "Select a patch"}</h2>
          </div>

          {selectedPatch && (
            <div className="patch-detail">
              <div className="detail-section">
                <h3>{selectedPatch.subject}</h3>
              </div>

              <div className="detail-section">
                <div className="detail-row">
                  <span className="detail-label">Sent:</span>
                  <span className="detail-value">{formatDate(selectedPatch.sent_at)}</span>
                </div>
                <div className="detail-row">
                  <span className="detail-label">Message ID:</span>
                  <span className="detail-value mono">{selectedPatch.message_id}</span>
                </div>
                {selectedPatch.commit_hash && (
                  <div className="detail-row">
                    <span className="detail-label">Commit:</span>
                    <span className="detail-value mono">{selectedPatch.commit_hash}</span>
                  </div>
                )}
                {selectedPatch.is_series && (
                  <div className="detail-row">
                    <span className="detail-label">Series:</span>
                    <span className="detail-value">
                      Part {selectedPatch.series_number} of {selectedPatch.series_total}
                    </span>
                  </div>
                )}
              </div>

              <div className="detail-section">
                <h4>Patch Content</h4>
                <div className="patch-body">
                  {selectedPatch.body_text?.split('\n').map((line, index) => (
                    <div key={index} className="body-line">
                      {line}
                    </div>
                  ))}
                </div>
              </div>
            </div>
          )}

          {!selectedPatch && (
            <div className="empty-state">
              Select a patch to view its details
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
