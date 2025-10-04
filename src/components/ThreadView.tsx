import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./ThreadView.css";

interface ThreadSummary {
  thread_id: number;
  root_subject: string;
  root_author: string;
  reply_count: number;
  participant_count: number;
  created_at: string;
  last_activity: string;
  root_patch_id: number;
}

interface ThreadNode {
  patch_id: number;
  subject: string;
  author_name: string;
  author_email: string;
  sent_at: string;
  depth: number;
  message_id: string;
  body_preview: string;
  is_reply: boolean;
  is_series: boolean;
  series_info: string | null;
  has_diff: boolean;
  reply_count: number;
  commit_hash: string | null;
  children: ThreadNode[];
}

interface ThreadTree {
  thread_id: number;
  summary: ThreadSummary;
  root: ThreadNode;
}

interface ThreadBuildStats {
  total_threads: number;
  total_replies: number;
  orphaned_messages: number;
  max_depth: number;
  processing_time_ms: number;
}

export default function ThreadView() {
  const [threads, setThreads] = useState<ThreadSummary[]>([]);
  const [selectedThread, setSelectedThread] = useState<ThreadTree | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [buildStats, setBuildStats] = useState<ThreadBuildStats | null>(null);
  const [showBuildStats, setShowBuildStats] = useState(false);
  const [collapsedNodes, setCollapsedNodes] = useState<Set<number>>(new Set());
  const [expandedDiffs, setExpandedDiffs] = useState<Set<number>>(new Set());
  const [diffContents, setDiffContents] = useState<Map<number, string>>(new Map());
  const [expandedBodies, setExpandedBodies] = useState<Set<number>>(new Set());
  const [currentPage, setCurrentPage] = useState(1);
  const [pageSize, setPageSize] = useState(50);
  const [sortBy, setSortBy] = useState("recent"); // recent, oldest, newest, most_replies, most_participants

  useEffect(() => {
    // Add a small delay to prevent loading conflicts when switching tabs quickly
    let isMounted = true;
    const timer = setTimeout(() => {
      if (isMounted) {
        loadThreads();
      }
    }, 100);
    
    return () => {
      isMounted = false;
      clearTimeout(timer);
    };
  }, [currentPage, pageSize, sortBy]);

  async function buildThreads() {
    setLoading(true);
    setError("");
    setShowBuildStats(false);

    try {
      const stats: ThreadBuildStats = await invoke("build_threads");
      setBuildStats(stats);
      setShowBuildStats(true);
      await loadThreads();
    } catch (err) {
      setError(`Failed to build threads: ${err}`);
    } finally {
      setLoading(false);
    }
  }

  async function loadThreads() {
    setLoading(true);
    setError("");

    try {
      const offset = (currentPage - 1) * pageSize;
      const threadList: ThreadSummary[] = await invoke("get_threads", {
        limit: pageSize,
        offset: offset,
        sortBy: sortBy,
      });
      setThreads(threadList);
    } catch (err) {
      setError(`Failed to load threads: ${err}`);
    } finally {
      setLoading(false);
    }
  }

  async function loadThreadTree(threadId: number) {
    setLoading(true);
    setError("");
    setCollapsedNodes(new Set());

    try {
      const tree: ThreadTree = await invoke("get_thread_tree", {
        threadId: threadId,
      });
      setSelectedThread(tree);
    } catch (err) {
      setError(`Failed to load thread tree: ${err}`);
    } finally {
      setLoading(false);
    }
  }

  async function searchThreads() {
    if (!searchQuery.trim()) {
      setCurrentPage(1);
      await loadThreads();
      return;
    }

    setLoading(true);
    setError("");

    try {
      const results: ThreadSummary[] = await invoke("search_threads", {
        keyword: searchQuery,
        limit: pageSize,
      });
      setThreads(results);
      setCurrentPage(1); // Reset to first page on search
    } catch (err) {
      setError(`Failed to search threads: ${err}`);
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

  function formatRelativeDate(dateString: string): string {
    try {
      const date = new Date(dateString);
      const now = new Date();
      const diffMs = now.getTime() - date.getTime();
      const diffMins = Math.floor(diffMs / 60000);
      const diffHours = Math.floor(diffMs / 3600000);
      const diffDays = Math.floor(diffMs / 86400000);

      if (diffMins < 60) return `${diffMins}m ago`;
      if (diffHours < 24) return `${diffHours}h ago`;
      if (diffDays < 30) return `${diffDays}d ago`;
      return date.toLocaleDateString();
    } catch {
      return dateString;
    }
  }

  function toggleNodeCollapse(patchId: number) {
    const newCollapsed = new Set(collapsedNodes);
    if (newCollapsed.has(patchId)) {
      newCollapsed.delete(patchId);
    } else {
      newCollapsed.add(patchId);
    }
    setCollapsedNodes(newCollapsed);
  }

  function toggleBodyExpanded(patchId: number) {
    const newExpanded = new Set(expandedBodies);
    if (newExpanded.has(patchId)) {
      newExpanded.delete(patchId);
    } else {
      newExpanded.add(patchId);
    }
    setExpandedBodies(newExpanded);
  }

  async function toggleDiffView(patchId: number) {
    const newExpanded = new Set(expandedDiffs);
    
    if (newExpanded.has(patchId)) {
      // Collapse diff
      newExpanded.delete(patchId);
      setExpandedDiffs(newExpanded);
    } else {
      // Expand diff - load content if not already loaded
      if (!diffContents.has(patchId)) {
        try {
          const body: string | null = await invoke("get_patch_body", { patchId });
          if (body) {
            const newDiffs = new Map(diffContents);
            newDiffs.set(patchId, body);
            setDiffContents(newDiffs);
          }
        } catch (err) {
          setError(`Failed to load diff: ${err}`);
          return;
        }
      }
      newExpanded.add(patchId);
      setExpandedDiffs(newExpanded);
    }
  }

  function renderThreadNode(node: ThreadNode) {
    const isCollapsed = collapsedNodes.has(node.patch_id);
    const hasChildren = node.children.length > 0;
    const indent = node.depth > 0 ? 24 : 0; // Fixed 24px indentation for all replies
    const isDiffExpanded = expandedDiffs.has(node.patch_id);
    const diffContent = diffContents.get(node.patch_id);
    const isBodyExpanded = expandedBodies.has(node.patch_id);

    const isPatch = node.has_diff;
    
    // Check if body preview is truncated (longer than 800 chars)
    const bodyPreview = node.body_preview || "(no preview)";
    const isTruncated = bodyPreview.length > 800;
    const displayBody = (!isTruncated || isBodyExpanded) ? bodyPreview : bodyPreview.substring(0, 800);

    return (
      <div key={node.patch_id} className="thread-item" style={{ marginLeft: `${indent}px` }}>
        <div className="thread-item-content">
          <div className="thread-item-header">
            {hasChildren ? (
              <button
                className="collapse-btn"
                onClick={() => toggleNodeCollapse(node.patch_id)}
                title={isCollapsed ? "Expand" : "Collapse"}
              >
                {isCollapsed ? "[+]" : "[−]"}
              </button>
            ) : (
              node.depth > 0 && <span className="collapse-spacer">[=]</span>
            )}
            
            <div className="thread-item-meta">
              <span className="meta-author">{node.author_name}</span>
              <span className="meta-date">{formatRelativeDate(node.sent_at)}</span>
              {isPatch && <span className="meta-badge">PATCH</span>}
              {node.is_series && node.series_info && (
                <span className="meta-series">[{node.series_info}]</span>
              )}
              {node.commit_hash && (
                <span className="meta-commit" title="Git commit hash">
                  {node.commit_hash.substring(0, 8)}
                </span>
              )}
            </div>
          </div>

          {!isCollapsed && (
            <>
              {isPatch && (
                <div className="thread-item-subject">{node.subject}</div>
              )}
              <pre className="thread-item-body">{displayBody}</pre>
              
              {isTruncated && (
                <button
                  className="show-more-toggle"
                  onClick={() => toggleBodyExpanded(node.patch_id)}
                >
                  {isBodyExpanded ? "show less" : "show more"}
                </button>
              )}
              
              {isPatch && (
                <button
                  className="diff-toggle"
                  onClick={() => toggleDiffView(node.patch_id)}
                >
                  {isDiffExpanded ? "hide diff" : "view diff"}
                </button>
              )}

              {isDiffExpanded && diffContent && (
                <pre className="thread-item-diff">{diffContent}</pre>
              )}
            </>
          )}

          {isCollapsed && hasChildren && (
            <span className="collapsed-count">
              [{node.children.length} more]
            </span>
          )}
        </div>

        {!isCollapsed && hasChildren && (
          <div className="thread-children">
            {node.children.map((child) => renderThreadNode(child))}
          </div>
        )}
      </div>
    );
  }

  // Count total nodes in thread tree
  function countNodes(node: ThreadNode): number {
    return 1 + node.children.reduce((sum, child) => sum + countNodes(child), 0);
  }

  return (
    <div className="thread-view-container">
      {error && (
        <div className="error-banner">
          <strong>Error:</strong> {error}
        </div>
      )}

      {/* Build Threads Section */}
      {!selectedThread && (
        <div className="thread-controls">
          <div className="control-group">
            <button onClick={buildThreads} disabled={loading} className="build-btn">
              {loading ? "Building..." : "Build Thread Relationships"}
            </button>
            <button onClick={() => loadThreads()} disabled={loading} className="refresh-btn">
              Refresh
            </button>
          </div>

          {showBuildStats && buildStats && (
            <div className="build-stats">
              <h3>Thread Build Results</h3>
              <div className="stats-grid">
                <div className="stat">
                  <span className="stat-value">{buildStats.total_threads}</span>
                  <span className="stat-label">Threads</span>
                </div>
                <div className="stat">
                  <span className="stat-value">{buildStats.total_replies}</span>
                  <span className="stat-label">Replies</span>
                </div>
                <div className="stat">
                  <span className="stat-value">{buildStats.orphaned_messages}</span>
                  <span className="stat-label">Orphaned</span>
                </div>
                <div className="stat">
                  <span className="stat-value">{buildStats.max_depth}</span>
                  <span className="stat-label">Max Depth</span>
                </div>
                <div className="stat">
                  <span className="stat-value">{buildStats.processing_time_ms}ms</span>
                  <span className="stat-label">Processing Time</span>
                </div>
              </div>
            </div>
          )}

          <div className="search-section">
            <input
              type="text"
              className="search-input"
              placeholder="Search threads by subject..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && searchThreads()}
            />
            <button onClick={searchThreads} disabled={loading} className="search-btn">
              Search
            </button>
          </div>

          <div className="filter-section">
            <div className="filter-group">
              <label>Sort by:</label>
              <select 
                value={sortBy} 
                onChange={(e) => { setSortBy(e.target.value); setCurrentPage(1); }}
                className="sort-select"
              >
                <option value="recent">Most Recent Activity</option>
                <option value="newest">Newest First</option>
                <option value="oldest">Oldest First</option>
                <option value="most_replies">Most Replies</option>
                <option value="most_participants">Most Participants</option>
              </select>
            </div>
            <div className="filter-group">
              <label>Per page:</label>
              <select 
                value={pageSize} 
                onChange={(e) => { setPageSize(Number(e.target.value)); setCurrentPage(1); }}
                className="pagesize-select"
              >
                <option value="25">25</option>
                <option value="50">50</option>
                <option value="100">100</option>
                <option value="200">200</option>
              </select>
            </div>
          </div>
        </div>
      )}

      {/* Thread List View */}
      {!selectedThread && (
        <div className="threads-panel">
          <div className="panel-header">
            <h2>Email Threads</h2>
            <span className="thread-count-badge">{threads.length}</span>
          </div>

          {loading && <div className="loading-spinner">Loading threads...</div>}

          {!loading && threads.length === 0 && (
            <div className="empty-state">
              <p>No threads found.</p>
              <p>Click "Build Thread Relationships" to analyze patch emails.</p>
            </div>
          )}

          <div className="pagination-controls">
            <button 
              onClick={() => setCurrentPage(p => Math.max(1, p - 1))} 
              disabled={currentPage === 1 || loading}
              className="page-btn"
            >
              ← Previous
            </button>
            <span className="page-info">
              Page {currentPage}
            </span>
            <button 
              onClick={() => setCurrentPage(p => p + 1)} 
              disabled={threads.length < pageSize || loading}
              className="page-btn"
            >
              Next →
            </button>
          </div>

          <div className="threads-list">
            {threads.map((thread) => {
              const isSeriesThread = thread.root_subject.match(/\[.*?\d+\/\d+.*?\]/);
              
              return (
                <div
                  key={thread.thread_id}
                  className="thread-card"
                  onClick={() => loadThreadTree(thread.thread_id)}
                >
                  <div className="thread-header">
                    {isSeriesThread && (
                      <span className="series-badge" title="Patch Series">
                        SERIES
                      </span>
                    )}
                    <h3 className="thread-subject">{thread.root_subject}</h3>
                  </div>
                  <div className="thread-info">
                    <span className="thread-author">{thread.root_author}</span>
                    <span className="separator">•</span>
                    <span className="thread-replies">
                      {thread.reply_count} {thread.reply_count === 1 ? "reply" : "replies"}
                    </span>
                    <span className="separator">•</span>
                    <span className="thread-participants">
                      {thread.participant_count} {thread.participant_count === 1 ? "participant" : "participants"}
                    </span>
                  </div>
                  <div className="thread-dates">
                    <span className="thread-created" title={formatDate(thread.created_at)}>
                      Started {formatRelativeDate(thread.created_at)}
                    </span>
                    <span className="separator">•</span>
                    <span className="thread-activity" title={formatDate(thread.last_activity)}>
                      Last activity {formatRelativeDate(thread.last_activity)}
                    </span>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* Thread Detail View */}
      {selectedThread && (
        <div className="thread-detail-view">
          <div className="thread-detail-header">
            <button onClick={() => setSelectedThread(null)} className="back-btn">
              ← Back to Threads
            </button>
            <div className="thread-detail-stats">
              <span className="stat-item">
                {selectedThread.summary.reply_count} replies
              </span>
              <span className="separator">•</span>
              <span className="stat-item">
                {selectedThread.summary.participant_count} participants
              </span>
              <span className="separator">•</span>
              <span className="stat-item">
                {countNodes(selectedThread.root)} total messages
              </span>
            </div>
            <button 
              onClick={() => setCollapsedNodes(new Set())} 
              className="expand-all-btn"
              title="Expand all"
            >
              Expand All
            </button>
            <button 
              onClick={() => {
                const allIds = new Set<number>();
                const collectIds = (node: ThreadNode) => {
                  if (node.children.length > 0) {
                    allIds.add(node.patch_id);
                    node.children.forEach(collectIds);
                  }
                };
                collectIds(selectedThread.root);
                setCollapsedNodes(allIds);
              }} 
              className="collapse-all-btn"
              title="Collapse all"
            >
              Collapse All
            </button>
          </div>

          <div className="thread-detail-content">
            {renderThreadNode(selectedThread.root)}
          </div>
        </div>
      )}
    </div>
  );
}

