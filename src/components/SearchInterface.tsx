import { useState } from "react";
import { Email, SearchCriteria } from "../types";

interface SearchInterfaceProps {
  onSearch: (query: string) => void;
  emails: Email[];
  onEmailSelect: (email: Email) => void;
  isLoading: boolean;
}

export default function SearchInterface({
  onSearch,
  emails,
  onEmailSelect,
  isLoading,
}: SearchInterfaceProps) {
  const [searchQuery, setSearchQuery] = useState("");
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [searchCriteria, setSearchCriteria] = useState<SearchCriteria>({
    query: "",
    author: "",
    subject_contains: "",
    is_patch: undefined,
    limit: 50,
    offset: 0,
  });

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (searchQuery.trim()) {
      onSearch(searchQuery.trim());
    }
  };

  const handleEmailClick = (email: Email) => {
    onEmailSelect(email);
  };

  const formatDate = (dateString: string) => {
    try {
      return new Date(dateString).toLocaleDateString();
    } catch {
      return "Unknown date";
    }
  };

  const truncateText = (text: string, maxLength: number = 100) => {
    if (text.length <= maxLength) return text;
    return text.substring(0, maxLength) + "...";
  };

  return (
    <div className="search-interface">
      <div className="search-panel">
        <form onSubmit={handleSubmit} className="search-form">
          <div className="search-input-group">
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="Search emails (subject, body, author)..."
              className="search-input"
              disabled={isLoading}
            />
            <button type="submit" disabled={isLoading || !searchQuery.trim()}>
              {isLoading ? "Searching..." : "Search"}
            </button>
          </div>

          <div className="search-options">
            <button
              type="button"
              onClick={() => setShowAdvanced(!showAdvanced)}
              className="advanced-toggle"
            >
              {showAdvanced ? "Hide" : "Show"} Advanced Options
            </button>
          </div>

          {showAdvanced && (
            <div className="advanced-search">
              <div className="form-row">
                <label>
                  Author:
                  <input
                    type="text"
                    value={searchCriteria.author || ""}
                    onChange={(e) =>
                      setSearchCriteria({
                        ...searchCriteria,
                        author: e.target.value,
                      })
                    }
                    placeholder="Filter by author"
                  />
                </label>
                <label>
                  Subject contains:
                  <input
                    type="text"
                    value={searchCriteria.subject_contains || ""}
                    onChange={(e) =>
                      setSearchCriteria({
                        ...searchCriteria,
                        subject_contains: e.target.value,
                      })
                    }
                    placeholder="Filter by subject content"
                  />
                </label>
              </div>
              <div className="form-row">
                <label>
                  <input
                    type="checkbox"
                    checked={searchCriteria.is_patch === true}
                    onChange={(e) =>
                      setSearchCriteria({
                        ...searchCriteria,
                        is_patch: e.target.checked ? true : undefined,
                      })
                    }
                  />
                  Patches only
                </label>
                <label>
                  <input
                    type="checkbox"
                    checked={searchCriteria.is_patch === false}
                    onChange={(e) =>
                      setSearchCriteria({
                        ...searchCriteria,
                        is_patch: e.target.checked ? false : undefined,
                      })
                    }
                  />
                  Non-patches only
                </label>
              </div>
            </div>
          )}
        </form>
      </div>

      <div className="results-panel">
        <div className="results-header">
          <h2>Search Results ({emails.length} emails)</h2>
        </div>

        {isLoading && <div className="loading">Loading...</div>}

        {!isLoading && emails.length === 0 && (
          <div className="no-results">
            <p>No emails found. Try searching for something.</p>
          </div>
        )}

        {!isLoading && emails.length > 0 && (
          <div className="email-list">
            {emails.map((email) => (
              <div
                key={email.id}
                className={`email-item ${email.is_patch ? "patch-email" : ""}`}
                onClick={() => handleEmailClick(email)}
              >
                <div className="email-header">
                  <div className="email-meta">
                    <span className="email-subject">{email.subject}</span>
                    <span className="email-author">{email.from}</span>
                    <span className="email-date">{formatDate(email.date)}</span>
                  </div>
                  <div className="email-flags">
                    {email.is_patch && (
                      <span className="patch-flag">PATCH</span>
                    )}
                    {email.patch_number && (
                      <span className="patch-number">#{email.patch_number}</span>
                    )}
                  </div>
                </div>
                <div className="email-body-preview">
                  {truncateText(email.body, 200)}
                </div>
                {email.references.length > 0 && (
                  <div className="email-references">
                    <small>
                      {email.references.length} reference(s)
                    </small>
                  </div>
                )}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
