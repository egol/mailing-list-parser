import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Email, Thread, ThreadNode } from "../types";

interface EmailThreadViewProps {
  email: Email;
  onBack: () => void;
}

interface ThreadEmail extends Email {
  thread_node?: ThreadNode;
}

export default function EmailThreadView({ email, onBack }: EmailThreadViewProps) {
  const [threadEmails, setThreadEmails] = useState<ThreadEmail[]>([]);
  const [expandedEmails, setExpandedEmails] = useState<Set<string>>(new Set());
  const [isLoading, setIsLoading] = useState(false);

  useEffect(() => {
    loadThread();
  }, [email]);

  async function loadThread() {
    try {
      setIsLoading(true);
      // For now, just show the current email
      // In a full implementation, we'd load the entire thread
      setThreadEmails([email]);
    } catch (error) {
      console.error("Failed to load thread:", error);
    } finally {
      setIsLoading(false);
    }
  }

  const toggleExpanded = (emailId: string) => {
    const newExpanded = new Set(expandedEmails);
    if (newExpanded.has(emailId)) {
      newExpanded.delete(emailId);
    } else {
      newExpanded.add(emailId);
    }
    setExpandedEmails(newExpanded);
  };

  const formatDate = (dateString: string) => {
    try {
      return new Date(dateString).toLocaleString();
    } catch {
      return "Unknown date";
    }
  };

  const formatEmailBody = (body: string) => {
    // Basic formatting for email content
    return body
      .split("\n")
      .map((line, index) => {
        // Handle quoted text (lines starting with >)
        if (line.startsWith(">")) {
          return (
            <div key={index} className="quoted-text">
              {line}
            </div>
          );
        }
        // Handle diff content
        if (line.startsWith("diff --git") || line.startsWith("@@")) {
          return (
            <div key={index} className="diff-content">
              {line}
            </div>
          );
        }
        // Handle signed-off-by and other metadata
        if (line.startsWith("Signed-off-by:") || line.startsWith("Reviewed-by:")) {
          return (
            <div key={index} className="email-metadata">
              {line}
            </div>
          );
        }
        // Regular content
        if (line.trim()) {
          return (
            <div key={index} className="email-content">
              {line}
            </div>
          );
        }
        return <br key={index} />;
      });
  };

  const getIndentationLevel = (email: ThreadEmail) => {
    // Simple indentation based on reply structure
    // In a full implementation, this would use the thread_node.depth
    return email.in_reply_to ? 1 : 0;
  };

  return (
    <div className="thread-view">
      <div className="thread-header">
        <button onClick={onBack} className="back-button">
          ← Back to Search
        </button>
        <h2>Thread: {email.subject}</h2>
      </div>

      {isLoading && <div className="loading">Loading thread...</div>}

      {!isLoading && (
        <div className="thread-content">
          {threadEmails.map((threadEmail, index) => {
            const isExpanded = expandedEmails.has(threadEmail.id);
            const indentation = getIndentationLevel(threadEmail);

            return (
              <div
                key={threadEmail.id}
                className={`thread-email ${indentation > 0 ? "reply" : "root"}`}
                style={{ marginLeft: `${indentation * 20}px` }}
              >
                <div className="thread-email-header">
                  <div className="email-info">
                    <span className="email-from">{threadEmail.from}</span>
                    <span className="email-date">{formatDate(threadEmail.date)}</span>
                    {threadEmail.is_patch && (
                      <span className="patch-indicator">PATCH</span>
                    )}
                  </div>
                  <button
                    onClick={() => toggleExpanded(threadEmail.id)}
                    className="expand-button"
                  >
                    {isExpanded ? "−" : "+"}
                  </button>
                </div>

                <div className="email-subject">
                  {threadEmail.subject}
                </div>

                {isExpanded && (
                  <div className="email-body">
                    {formatEmailBody(threadEmail.body)}

                    {threadEmail.references.length > 0 && (
                      <div className="email-references">
                        <strong>References:</strong>
                        <ul>
                          {threadEmail.references.map((ref, refIndex) => (
                            <li key={refIndex}>{ref}</li>
                          ))}
                        </ul>
                      </div>
                    )}

                    {threadEmail.to.length > 0 && (
                      <div className="email-to">
                        <strong>To:</strong> {threadEmail.to.join(", ")}
                      </div>
                    )}

                    {threadEmail.cc.length > 0 && (
                      <div className="email-cc">
                        <strong>CC:</strong> {threadEmail.cc.join(", ")}
                      </div>
                    )}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
