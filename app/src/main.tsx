import React, { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import {
  Activity,
  CheckCircle2,
  Download,
  FileUp,
  FolderDown,
  HardDrive,
  LayoutDashboard,
  Pause,
  Play,
  Plus,
  Radio,
  RotateCw,
  Send,
  Settings,
  Trash2,
  UploadCloud,
} from "lucide-react";
import "./styles.css";

type View = "dashboard" | "send" | "download" | "settings";

type Peer = {
  device_id: string;
  hostname: string;
  ip: string;
  version: string;
  last_seen_secs: number;
};

type DownloadTask = {
  id: string;
  status: string;
  target: string;
  progress: number;
  total_bytes: number;
  completed_bytes: number;
  download_speed: number;
  eta_seconds: number | null;
};

type TransferTask = {
  id: string;
  peer_ip: string;
  file_path: string;
  status: string;
  bytes_transferred: number;
  total_bytes: number;
  bytes_per_second: number;
  eta_seconds: number | null;
  error: string | null;
};

type AppSettings = {
  aria2_endpoint: string;
  aria2_secret: string;
};

const navItems = [
  { id: "dashboard" as const, label: "Dashboard", icon: LayoutDashboard },
  { id: "send" as const, label: "Send", icon: Send },
  { id: "download" as const, label: "Download", icon: Download },
  { id: "settings" as const, label: "Settings", icon: Settings },
];

function App() {
  const [activeView, setActiveView] = useState<View>("dashboard");
  const [peers, setPeers] = useState<Peer[]>([]);
  const [downloads, setDownloads] = useState<DownloadTask[]>([]);
  const [downloadUrl, setDownloadUrl] = useState("");
  const [dragActive, setDragActive] = useState(false);
  const [selectedPeerId, setSelectedPeerId] = useState<string | null>(null);
  const [sendFilePaths, setSendFilePaths] = useState<string[]>([]);
  const [transfers, setTransfers] = useState<TransferTask[]>([]);
  const [aria2Online, setAria2Online] = useState(false);
  const [pausedTransferIds, setPausedTransferIds] = useState<Set<string>>(new Set());
  const [status, setStatus] = useState("Ready");
  const [settings, setSettings] = useState<AppSettings>({
    aria2_endpoint: "http://127.0.0.1:6800/jsonrpc",
    aria2_secret: "",
  });

  useEffect(() => {
    void invoke<AppSettings>("get_settings")
      .then(setSettings)
      .catch(() => {});
    void refreshData();
    const timer = window.setInterval(refreshData, 2500);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    const unlistenProgress = listen<TransferTask>("transfer-progress", (event) => {
      setTransfers((current) => upsertTransfer(current, event.payload));
    });
    const unlistenReceived = listen<TransferTask>("transfer-received", (event) => {
      setTransfers((current) => upsertTransfer(current, event.payload));
      if (event.payload.status === "received") {
        setStatus(`Received file into ${event.payload.file_path}`);
      }
    });

    return () => {
      void unlistenProgress.then((unlisten) => unlisten());
      void unlistenReceived.then((unlisten) => unlisten());
    };
  }, []);

  async function refreshData() {
    try {
      const [peerResult, taskResult, historyResult, aria2Result] = await Promise.all([
        invoke<Peer[]>("list_peers"),
        invoke<DownloadTask[]>("download_tasks"),
        invoke<Array<{ id: string; status: string; target: string }>>("list_transfer_history"),
        invoke<boolean>("check_aria2_status").catch(() => false),
      ]);
      setPeers(peerResult);
      setDownloads(taskResult);
      setAria2Online(aria2Result);
      // Seed transfers from persisted history (real-time events will override active entries)
      setTransfers((current) => {
        let merged = [...current];
        for (const item of historyResult) {
          if (!merged.find((t) => t.id === item.id)) {
            merged = [
              {
                id: item.id,
                peer_ip: "",
                file_path: item.target,
                status: item.status,
                bytes_transferred: item.status === "completed" ? 1 : 0,
                total_bytes: item.status === "completed" ? 1 : 0,
                bytes_per_second: 0,
                eta_seconds: null,
                error: null,
              },
              ...merged,
            ];
          }
        }
        return merged;
      });
    } catch {
      setStatus("Backend commands are waiting for the Tauri runtime");
    }
  }

  async function addDownload() {
    if (!downloadUrl.trim()) {
      return;
    }

    setStatus("Adding download...");
    try {
      await invoke<string>("add_download", { url: downloadUrl.trim() });
      setDownloadUrl("");
      await refreshData();
      setStatus("Download added");
    } catch (error) {
      setStatus(String(error));
    }
  }

  async function pauseDownload(gid: string) {
    setStatus("Pausing download...");
    try {
      await invoke("pause_download", { gid });
      await refreshData();
      setStatus("Download paused");
    } catch (error) {
      setStatus(String(error));
    }
  }

  async function resumeDownload(gid: string) {
    setStatus("Resuming download...");
    try {
      await invoke("resume_download", { gid });
      await refreshData();
      setStatus("Download resumed");
    } catch (error) {
      setStatus(String(error));
    }
  }

  async function removeDownload(gid: string) {
    setStatus("Removing download...");
    try {
      await invoke("remove_download", { gid });
      await refreshData();
      setStatus("Download removed");
    } catch (error) {
      setStatus(String(error));
    }
  }

  async function browseForFile() {
    try {
      const selection = await openFileDialog({ multiple: true, directory: false });
      if (Array.isArray(selection) && selection.length > 0) {
        setSendFilePaths(selection);
        setStatus(`${selection.length} file(s) selected`);
      } else if (typeof selection === "string") {
        setSendFilePaths([selection]);
        setStatus("File selected");
      }
    } catch (error) {
      setStatus(String(error));
    }
  }

  async function launchAria2() {
    setStatus("Starting aria2...");
    try {
      const ok = await invoke<boolean>("launch_aria2");
      setAria2Online(ok);
      setStatus(ok ? "aria2 started" : "aria2 failed to start — is it installed?");
    } catch (error) {
      setStatus(String(error));
    }
  }

  async function sendSelectedFile() {
    const peer = peers.find((candidate) => candidate.device_id === selectedPeerId);
    if (!peer) {
      setStatus("Select a peer before sending");
      return;
    }
    if (sendFilePaths.length === 0) {
      setStatus("Choose at least one file before sending");
      return;
    }

    setStatus(`Sending ${sendFilePaths.length} file(s)...`);
    try {
      await invoke("send_file_to_peer", {
        peerIp: peer.ip,
        filePaths: sendFilePaths,
      });
      setSendFilePaths([]);
    } catch (error) {
      setStatus(String(error));
    }
  }

  async function pauseTransfer(transferId: string) {
    try {
      await invoke("pause_transfer", { transferId });
      setPausedTransferIds((current) => new Set(current).add(transferId));
      setStatus("Transfer paused");
    } catch (error) {
      setStatus(String(error));
    }
  }

  async function resumeTransfer(transferId: string) {
    try {
      await invoke("resume_transfer", { transferId });
      setPausedTransferIds((current) => {
        const next = new Set(current);
        next.delete(transferId);
        return next;
      });
      setStatus("Transfer resumed");
    } catch (error) {
      setStatus(String(error));
    }
  }

  async function retryTransfer(transferId: string) {
    setStatus("Retrying transfer...");
    try {
      await invoke("retry_transfer", { transferId });
      setPausedTransferIds((current) => {
        const next = new Set(current);
        next.delete(transferId);
        return next;
      });
      setStatus("Transfer retried");
    } catch (error) {
      setStatus(String(error));
    }
  }

  const stats = useMemo(
    () => [
      { label: "Peers", value: peers.length.toString(), icon: Radio },
      { label: "Transfers", value: transfers.length.toString(), icon: UploadCloud },
      { label: "Downloads", value: downloads.length.toString(), icon: FolderDown },
      { label: "Storage", value: "SQLite", icon: HardDrive },
    ],
    [downloads.length, peers.length, transfers.length],
  );

  return (
    <main className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark">F</div>
          <div>
            <strong>FlowHub</strong>
            <span>v0.1 MVP</span>
          </div>
        </div>

        <nav className="nav-list" aria-label="Primary">
          {navItems.map((item) => {
            const Icon = item.icon;
            return (
              <button
                key={item.id}
                className={activeView === item.id ? "nav-item active" : "nav-item"}
                onClick={() => setActiveView(item.id)}
                type="button"
              >
                <Icon size={18} />
                <span>{item.label}</span>
              </button>
            );
          })}
        </nav>

        <div className="sidebar-status">
          <Activity size={16} />
          <span>{status}</span>
        </div>
      </aside>

      <section className="content">
        {activeView === "dashboard" && (
          <Dashboard
            stats={stats}
            downloads={downloads}
            peers={peers}
            aria2Online={aria2Online}
            onPauseDownload={pauseDownload}
            onRemoveDownload={removeDownload}
            onResumeDownload={resumeDownload}
            onLaunchAria2={launchAria2}
          />
        )}
        {activeView === "send" && (
          <SendView
            dragActive={dragActive}
            filePaths={sendFilePaths}
            peers={peers}
            selectedPeerId={selectedPeerId}
            transfers={transfers}
            onBrowseFile={browseForFile}
            onDragActive={setDragActive}
            onFilePaths={setSendFilePaths}
            pausedTransferIds={pausedTransferIds}
            onPauseTransfer={pauseTransfer}
            onResumeTransfer={resumeTransfer}
            onRetryTransfer={retryTransfer}
            onSelectedPeerId={setSelectedPeerId}
            onSendFile={sendSelectedFile}
            onStatus={setStatus}
          />
        )}
        {activeView === "download" && (
          <DownloadView
            downloadUrl={downloadUrl}
            downloads={downloads}
            onAddDownload={addDownload}
            onDownloadUrl={setDownloadUrl}
            onPauseDownload={pauseDownload}
            onRemoveDownload={removeDownload}
            onResumeDownload={resumeDownload}
          />
        )}
        {activeView === "settings" && (
          <SettingsView
            settings={settings}
            onSave={async (next) => {
              try {
                await invoke("save_settings", { settings: next });
                setSettings(next);
                setStatus("Settings saved — restart to apply aria2 changes");
              } catch (error) {
                setStatus(String(error));
              }
            }}
          />
        )}
      </section>
    </main>
  );
}

function Dashboard({
  stats,
  peers,
  downloads,
  aria2Online,
  onPauseDownload,
  onRemoveDownload,
  onResumeDownload,
  onLaunchAria2,
}: {
  stats: Array<{ label: string; value: string; icon: React.ElementType }>;
  peers: Peer[];
  downloads: DownloadTask[];
  aria2Online: boolean;
  onPauseDownload: (gid: string) => void;
  onRemoveDownload: (gid: string) => void;
  onResumeDownload: (gid: string) => void;
  onLaunchAria2: () => void;
}) {
  return (
    <div className="view">
      <header className="view-header">
        <h1>Dashboard</h1>
        <p>Local discovery, file sending, aria2 downloads, and metadata storage in one workspace.</p>
      </header>
      <div className="stat-grid">
        {stats.map((stat) => {
          const Icon = stat.icon;
          return (
            <article className="stat-card" key={stat.label}>
              <Icon size={20} />
              <span>{stat.label}</span>
              <strong>{stat.value}</strong>
            </article>
          );
        })}
      </div>
      <div className="aria2-bar">
        <span className={aria2Online ? "dot dot-online" : "dot dot-offline"} />
        <span>aria2 {aria2Online ? "已连接" : "未连接"}</span>
        {!aria2Online && (
          <button className="secondary-button" onClick={onLaunchAria2} type="button">
            <Play size={14} />
            <span>启动 aria2</span>
          </button>
        )}
      </div>
      <div className="two-column">
        <Panel title="Nearby Devices">
          <PeerList peers={peers} />
        </Panel>
        <Panel title="Download Queue">
          <DownloadList
            downloads={downloads}
            onPauseDownload={onPauseDownload}
            onRemoveDownload={onRemoveDownload}
            onResumeDownload={onResumeDownload}
          />
        </Panel>
      </div>
    </div>
  );
}

function SendView({
  dragActive,
  filePaths,
  peers,
  selectedPeerId,
  transfers,
  onBrowseFile,
  onDragActive,
  onFilePaths,
  pausedTransferIds,
  onPauseTransfer,
  onResumeTransfer,
  onRetryTransfer,
  onSelectedPeerId,
  onSendFile,
  onStatus,
}: {
  dragActive: boolean;
  filePaths: string[];
  peers: Peer[];
  selectedPeerId: string | null;
  transfers: TransferTask[];
  pausedTransferIds: Set<string>;
  onBrowseFile: () => void;
  onDragActive: (active: boolean) => void;
  onFilePaths: (paths: string[]) => void;
  onPauseTransfer: (transferId: string) => void;
  onResumeTransfer: (transferId: string) => void;
  onRetryTransfer: (transferId: string) => void;
  onSelectedPeerId: (id: string) => void;
  onSendFile: () => void;
  onStatus: (status: string) => void;
}) {
  return (
    <div className="view">
      <header className="view-header">
        <h1>Send</h1>
        <p>Discover LAN devices and prepare peer-to-peer file transfers.</p>
      </header>
      <div className="two-column wide-left">
        <section
          className={dragActive ? "drop-zone active" : "drop-zone"}
          onDragEnter={(event) => {
            event.preventDefault();
            onDragActive(true);
          }}
          onDragOver={(event) => event.preventDefault()}
          onDragLeave={() => onDragActive(false)}
          onDrop={(event) => {
            event.preventDefault();
            onDragActive(false);
            const paths = Array.from(event.dataTransfer.files)
              .map((file) => extractDroppedPath(file))
              .filter((path): path is string => Boolean(path));
            if (paths.length > 0) {
              onFilePaths(paths);
              onStatus(`${paths.length} 个文件已就绪`);
            } else {
              onStatus("拖放未能获取文件路径，请手动输入");
            }
          }}
        >
          <UploadCloud size={42} />
          <strong>Drop files here</strong>
          <span>支持多文件拖放，或点击 Browse 选择文件。</span>
        </section>
        <Panel title="Discovered Devices">
          <PeerList peers={peers} selectedPeerId={selectedPeerId} onSelectPeer={onSelectedPeerId} />
        </Panel>
      </div>

      {/* 已选文件列表 */}
      {filePaths.length > 0 && (
        <div className="file-list-bar">
          {filePaths.map((p) => (
            <div className="file-chip" key={p}>
              <span title={p}>{p.split("/").pop()}</span>
              <button
                aria-label="Remove file"
                onClick={() => onFilePaths(filePaths.filter((x) => x !== p))}
                type="button"
              >
                ×
              </button>
            </div>
          ))}
        </div>
      )}

      <div className="send-bar">
        <button className="secondary-button" onClick={onBrowseFile} type="button">
          <FolderDown size={18} />
          <span>Browse…</span>
        </button>
        <button
          className="primary-button"
          onClick={onSendFile}
          disabled={filePaths.length === 0}
          type="button"
        >
          <FileUp size={18} />
          <span>{filePaths.length > 1 ? `Send ${filePaths.length} files` : "Send"}</span>
        </button>
      </div>
      <Panel title="Transfer Progress">
        <TransferList
          transfers={transfers}
          pausedTransferIds={pausedTransferIds}
          onPauseTransfer={onPauseTransfer}
          onResumeTransfer={onResumeTransfer}
          onRetryTransfer={onRetryTransfer}
        />
      </Panel>
    </div>
  );
}

function DownloadView({
  downloadUrl,
  downloads,
  onAddDownload,
  onDownloadUrl,
  onPauseDownload,
  onRemoveDownload,
  onResumeDownload,
}: {
  downloadUrl: string;
  downloads: DownloadTask[];
  onAddDownload: () => void;
  onDownloadUrl: (url: string) => void;
  onPauseDownload: (gid: string) => void;
  onRemoveDownload: (gid: string) => void;
  onResumeDownload: (gid: string) => void;
}) {
  return (
    <div className="view">
      <header className="view-header">
        <h1>Download</h1>
        <p>Add HTTP, HTTPS, FTP, and Magnet tasks through aria2 RPC.</p>
      </header>
      <div className="download-bar">
        <input
          value={downloadUrl}
          onChange={(event) => onDownloadUrl(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              void onAddDownload();
            }
          }}
          placeholder="https://example.com/file.iso or magnet:?xt=..."
        />
        <button className="primary-button" onClick={onAddDownload} type="button">
          <Plus size={18} />
          <span>Add</span>
        </button>
      </div>
      <Panel title="Tasks">
        <DownloadList
          downloads={downloads}
          onPauseDownload={onPauseDownload}
          onRemoveDownload={onRemoveDownload}
          onResumeDownload={onResumeDownload}
        />
      </Panel>
    </div>
  );
}

function SettingsView({
  settings,
  onSave,
}: {
  settings: AppSettings;
  onSave: (next: AppSettings) => Promise<void>;
}) {
  const [draft, setDraft] = useState<AppSettings>(settings);
  const [saving, setSaving] = useState(false);

  // Sync if parent settings change (e.g. loaded after mount)
  useEffect(() => {
    setDraft(settings);
  }, [settings]);

  async function handleSave() {
    setSaving(true);
    await onSave(draft);
    setSaving(false);
  }

  return (
    <div className="view">
      <header className="view-header">
        <h1>Settings</h1>
        <p>Configure discovery, transfer, download, and storage defaults.</p>
      </header>
      <div className="settings-grid">
        <label>
          Discovery port
          <input value="47321" readOnly />
        </label>
        <label>
          aria2 RPC endpoint
          <input
            value={draft.aria2_endpoint}
            onChange={(e) => setDraft((d) => ({ ...d, aria2_endpoint: e.target.value }))}
            placeholder="http://127.0.0.1:6800/jsonrpc"
          />
        </label>
        <label>
          aria2 RPC secret (optional)
          <input
            type="password"
            value={draft.aria2_secret}
            onChange={(e) => setDraft((d) => ({ ...d, aria2_secret: e.target.value }))}
            placeholder="leave blank if not set"
          />
        </label>
        <label>
          Database
          <input value="flowhub.db" readOnly />
        </label>
        <button className="primary-button" onClick={handleSave} disabled={saving} type="button">
          {saving ? "Saving…" : "Save settings"}
        </button>
      </div>
    </div>
  );
}

function Panel({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="panel">
      <div className="panel-title">{title}</div>
      {children}
    </section>
  );
}

function PeerList({
  peers,
  selectedPeerId,
  onSelectPeer,
}: {
  peers: Peer[];
  selectedPeerId?: string | null;
  onSelectPeer?: (id: string) => void;
}) {
  if (peers.length === 0) {
    return <div className="empty-row">No LAN peers discovered yet.</div>;
  }

  return (
    <div className="list">
      {peers.map((peer) => {
        const isRecent = peer.last_seen_secs <= 6;
        return (
          <button
            className={selectedPeerId === peer.device_id ? "list-row peer-row selected" : "list-row peer-row"}
            disabled={!onSelectPeer}
            key={peer.device_id}
            onClick={() => onSelectPeer?.(peer.device_id)}
            type="button"
          >
            <span className={isRecent ? "dot dot-online" : "dot dot-offline"} title={`${peer.last_seen_secs}s ago`} />
            <div>
              <strong>{peer.hostname}</strong>
              <span>{peer.ip}</span>
            </div>
            <small>{peer.version}</small>
          </button>
        );
      })}
    </div>
  );
}

function TransferList({
  transfers,
  pausedTransferIds,
  onPauseTransfer,
  onResumeTransfer,
  onRetryTransfer,
}: {
  transfers: TransferTask[];
  pausedTransferIds?: Set<string>;
  onPauseTransfer?: (transferId: string) => void;
  onResumeTransfer?: (transferId: string) => void;
  onRetryTransfer?: (transferId: string) => void;
}) {
  if (transfers.length === 0) {
    return <div className="empty-row">No active transfers yet.</div>;
  }

  return (
    <div className="list">
      {transfers.map((transfer) => {
        const progress =
          transfer.total_bytes > 0
            ? Math.min((transfer.bytes_transferred / transfer.total_bytes) * 100, 100)
            : 0;
        const isOutgoing = transfer.peer_ip !== "incoming";
        const isPaused = pausedTransferIds?.has(transfer.id) ?? false;
        const isActive = transfer.status === "sending";
        const isFailed = transfer.status === "error";
        return (
          <div className="download-row" key={transfer.id}>
            <div className="download-main">
              <strong>{transfer.file_path}</strong>
              <span>
                {transfer.peer_ip} · {formatBytes(transfer.bytes_transferred)}
                {transfer.total_bytes > 0 ? ` / ${formatBytes(transfer.total_bytes)}` : ""} ·{" "}
                {formatBytes(transfer.bytes_per_second)}/s · ETA {formatEta(transfer.eta_seconds)}
              </span>
              {transfer.error && <span className="error-text">{transfer.error}</span>}
              <div className="progress-track">
                <div className="progress-fill" style={{ width: `${progress}%` }} />
              </div>
            </div>
            <span className={`status-pill ${transfer.status}`}>{transfer.status}</span>
            {transfer.status === "completed" || transfer.status === "received" ? (
              <CheckCircle2 className="success-icon" size={20} />
            ) : null}
            {isOutgoing && (isActive || isFailed) && (
              <div className="icon-actions">
                {isActive &&
                  (isPaused ? (
                    <button
                      aria-label="Resume transfer"
                      onClick={() => onResumeTransfer?.(transfer.id)}
                      type="button"
                    >
                      <Play size={16} />
                    </button>
                  ) : (
                    <button
                      aria-label="Pause transfer"
                      onClick={() => onPauseTransfer?.(transfer.id)}
                      type="button"
                    >
                      <Pause size={16} />
                    </button>
                  ))}
                {isFailed && (
                  <button
                    aria-label="Retry transfer"
                    onClick={() => onRetryTransfer?.(transfer.id)}
                    type="button"
                  >
                    <RotateCw size={16} />
                  </button>
                )}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

function DownloadList({
  downloads,
  onPauseDownload,
  onRemoveDownload,
  onResumeDownload,
}: {
  downloads: DownloadTask[];
  onPauseDownload: (gid: string) => void;
  onRemoveDownload: (gid: string) => void;
  onResumeDownload: (gid: string) => void;
}) {
  if (downloads.length === 0) {
    return <div className="empty-row">No download tasks yet.</div>;
  }

  return (
    <div className="list">
      {downloads.map((task) => (
        <div className="download-row" key={task.id}>
          <div className="download-main">
            <strong>{task.target}</strong>
            <span>
              {formatBytes(task.completed_bytes)}
              {task.total_bytes > 0 ? ` / ${formatBytes(task.total_bytes)}` : ""} ·{" "}
              {formatBytes(task.download_speed)}/s · ETA {formatEta(task.eta_seconds)}
            </span>
            <div className="progress-track">
              <div className="progress-fill" style={{ width: `${Math.min(task.progress, 100)}%` }} />
            </div>
          </div>
          <span className={`status-pill ${task.status}`}>{task.status}</span>
          <div className="icon-actions">
            {task.status === "paused" ? (
              <button aria-label="Resume download" onClick={() => onResumeDownload(task.id)} type="button">
                <Play size={16} />
              </button>
            ) : (
              <button aria-label="Pause download" onClick={() => onPauseDownload(task.id)} type="button">
                <Pause size={16} />
              </button>
            )}
            <button aria-label="Remove download" onClick={() => onRemoveDownload(task.id)} type="button">
              <Trash2 size={16} />
            </button>
          </div>
        </div>
      ))}
    </div>
  );
}

function formatBytes(bytes: number) {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return "0 B";
  }

  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }

  return `${value >= 10 || unit === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[unit]}`;
}

function formatEta(seconds: number | null) {
  if (seconds === null || !Number.isFinite(seconds)) {
    return "--";
  }

  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const remainingSeconds = Math.floor(seconds % 60);
  if (hours > 0) {
    return `${hours}h ${minutes}m`;
  }
  if (minutes > 0) {
    return `${minutes}m ${remainingSeconds}s`;
  }
  return `${remainingSeconds}s`;
}

function extractDroppedPath(file: File) {
  const tauriFile = file as File & { path?: string };
  return tauriFile.path || file.webkitRelativePath || "";
}

function upsertTransfer(current: TransferTask[], next: TransferTask) {
  const index = current.findIndex((transfer) => transfer.id === next.id);
  if (index === -1) {
    return [next, ...current];
  }

  return current.map((transfer, currentIndex) => (currentIndex === index ? next : transfer));
}

createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
