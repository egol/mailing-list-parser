import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

interface ConfigPanelProps {
  isOpen: boolean;
  onClose: () => void;
}

interface ConfigStatus {
  database_connected: boolean;
  mailing_list_path: string;
}

export default function ConfigPanel({ isOpen, onClose }: ConfigPanelProps) {
  const [configStatus, setConfigStatus] = useState<ConfigStatus | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [directoryPath, setDirectoryPath] = useState("E:/bpf");

  useEffect(() => {
    if (isOpen) {
      loadConfigStatus();
    }
  }, [isOpen]);

  async function loadConfigStatus() {
    try {
      setIsLoading(true);
      const response = await invoke<string>("get_config_status_cmd");
      const status: ConfigStatus = JSON.parse(response);
      setConfigStatus(status);
      setDirectoryPath(status.mailing_list_path);
    } catch (error) {
      console.error("Failed to load config status:", error);
    } finally {
      setIsLoading(false);
    }
  }

  async function testConnection() {
    try {
      setIsLoading(true);
      await invoke<string>("test_connection_cmd");
      alert("Database connection successful!");
    } catch (error) {
      alert("Database connection failed: " + error);
    } finally {
      setIsLoading(false);
    }
  }

  async function updateDirectoryPath() {
    try {
      setIsLoading(true);
      // For now, we'll just store this locally
      // In a full implementation, this would update the backend config
      localStorage.setItem("mailing_list_path", directoryPath);
      alert("Directory path updated successfully!");
      await loadConfigStatus(); // Refresh the status
    } catch (error) {
      alert("Failed to update directory path: " + error);
    } finally {
      setIsLoading(false);
    }
  }

  if (!isOpen) return null;

  return (
    <div className="config-overlay">
      <div className="config-panel">
        <div className="config-header">
          <h2>Configuration</h2>
          <button onClick={onClose} className="close-button">
            ×
          </button>
        </div>

        <div className="config-content">
          <div className="config-section">
            <h3>Database Connection</h3>
            {configStatus && (
              <div className="connection-status">
                <div className={`status-indicator ${configStatus.database_connected ? 'connected' : 'disconnected'}`}>
                  {configStatus.database_connected ? '● Connected' : '● Disconnected'}
                </div>
                <button onClick={testConnection} disabled={isLoading}>
                  {isLoading ? 'Testing...' : 'Test Connection'}
                </button>
              </div>
            )}
          </div>

          <div className="config-section">
            <h3>Mailing List Directory</h3>
            <div className="directory-config">
              <label>
                Path:
                <input
                  type="text"
                  value={directoryPath}
                  onChange={(e) => setDirectoryPath(e.target.value)}
                  placeholder="Enter directory path"
                />
              </label>
              <button onClick={updateDirectoryPath} disabled={isLoading}>
                Update Path
              </button>
            </div>
            <small className="config-help">
              This should be the path to your local mailing list repository (e.g., E:/bpf)
            </small>
          </div>

          <div className="config-section">
            <h3>Backend Information</h3>
            <div className="backend-info">
              <div className="info-item">
                <strong>Current Path:</strong> {configStatus?.mailing_list_path || 'Not set'}
              </div>
              <div className="info-item">
                <strong>Status:</strong> Ready to parse emails
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
