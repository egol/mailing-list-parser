import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";
import SearchInterface from "./components/SearchInterface";
import EmailThreadView from "./components/EmailThreadView";
import ConfigPanel from "./components/ConfigPanel";
import { Email, SearchResults } from "./types";

function App() {
  const [currentView, setCurrentView] = useState<"search" | "thread">("search");
  const [searchResults, setSearchResults] = useState<Email[]>([]);
  const [selectedEmail, setSelectedEmail] = useState<Email | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [showConfig, setShowConfig] = useState(false);

  // Test the backend connection on startup
  useEffect(() => {
    testBackendConnection();
  }, []);

  async function testBackendConnection() {
    try {
      setIsLoading(true);
      const response = await invoke<string>("test_parser_cmd");
      console.log("Backend connection successful:", response);
    } catch (error) {
      console.error("Backend connection failed:", error);
    } finally {
      setIsLoading(false);
    }
  }

  async function handleSearch(query: string) {
    try {
      setIsLoading(true);
      const response = await invoke<string>("search_emails_cmd", {
        query,
        limit: 50,
        offset: 0
      });
      const results: SearchResults = JSON.parse(response);
      setSearchResults(results.emails);
      setCurrentView("search");
    } catch (error) {
      console.error("Search failed:", error);
    } finally {
      setIsLoading(false);
    }
  }

  async function handleEmailSelect(email: Email) {
    setSelectedEmail(email);
    setCurrentView("thread");
  }

  async function handleLoadRecent() {
    try {
      setIsLoading(true);
      const response = await invoke<string>("get_recent_emails_cmd", {
        limit: 20
      });
      const emails: Email[] = JSON.parse(response);
      setSearchResults(emails);
      setCurrentView("search");
    } catch (error) {
      console.error("Failed to load recent emails:", error);
    } finally {
      setIsLoading(false);
    }
  }

  return (
    <div className="app">
      <header className="app-header">
        <h1>Mailing List Parser</h1>
        <nav className="nav-buttons">
          <button
            onClick={() => setCurrentView("search")}
            className={currentView === "search" ? "active" : ""}
          >
            Search
          </button>
          <button onClick={handleLoadRecent} disabled={isLoading}>
            Recent Emails
          </button>
          <button onClick={() => setShowConfig(true)} className="settings-button">
            ⚙️ Settings
          </button>
        </nav>
      </header>

      <main className="app-main">
        {currentView === "search" && (
          <SearchInterface
            onSearch={handleSearch}
            emails={searchResults}
            onEmailSelect={handleEmailSelect}
            isLoading={isLoading}
          />
        )}

        {currentView === "thread" && selectedEmail && (
          <EmailThreadView
            email={selectedEmail}
            onBack={() => setCurrentView("search")}
          />
        )}
      </main>

      <ConfigPanel
        isOpen={showConfig}
        onClose={() => setShowConfig(false)}
      />
    </div>
  );
}

export default App;
