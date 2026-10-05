import React, { useState, useEffect, useRef } from "react";
import { File, Folder, Plus, X, Trash2, CheckCircle2, ArrowRight } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { PlenumEventEnvelope, SendUnifiedRequest, TransferSummary, TransferEvent, IceServer, TransferUiPhase } from "../types/rust";
import { addHistoryEntry } from "../services/history";
import { formatBytes, formatDuration, progressPercent } from "../utils/format";
import { isStaleSession } from "../utils/session";
import { createTransferMetrics, updateTransferMetrics, TransferMetricsState } from "../utils/transferMetrics";
import { parseQrPayload } from "../utils/qrPayload";
import { useSettings } from "../context/SettingsContext";
import { RELAY_SERVER_URL, DEFAULT_ICE_SERVERS } from "../config";

type LogEvent = { level: string; message: string };

const logToConsole = (log: LogEvent) => {
  if (log.level === "Error") {
    console.error(log.message);
  } else if (log.level === "Warn") {
    console.warn(log.message);
  } else {
    console.info(log.message);
  }
};

const STATE_LABELS: Record<string, string> = {
  Discovering: "Searching...",
  Listening: "Ready to receive files",
  Connecting: "Establishing secure connection…",
  SignalingConnected: "Establishing secure connection…",
  NegotiatingIce: "Establishing secure connection…",
  Connected: "Establishing secure connection…",
};

const friendlyState = (state: string): string => STATE_LABELS[state] ?? "Establishing secure connection…";

const SendPage: React.FC = () => {
  const { settings } = useSettings();
  const [activePathBadge, setActivePathBadge] = useState<string | null>(null);
  const [phase, setPhase] = useState<TransferUiPhase>("idle");
  const [selectedPaths, setSelectedPaths] = useState<string[]>([]);
  const [transferStatus, setTransferStatus] = useState<string>("");
  const [progress, setProgress] = useState<{ transferred: number, total: number } | null>(null);
  const [isDragging, setIsDragging] = useState(false);
  const [roomCodeInput, setRoomCodeInput] = useState("");
  const [isConnectingRemote, setIsConnectingRemote] = useState(false);
  const [isTransferActive, setIsTransferActive] = useState(false);
  const [sendSuccess, setSendSuccess] = useState(false);
  const [speedText, setSpeedText] = useState<string | null>(null);
  const [etaText, setEtaText] = useState<string | null>(null);
  const autoResetRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const metricsRef = useRef<TransferMetricsState | null>(null);
  const terminalEventRef = useRef(false);
  const activeSessionRef = useRef(0);
  const sessionFloorRef = useRef(0);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;

    const setupListener = async () => {
      unlisten = await listen<PlenumEventEnvelope>("plenum-event", (event) => {
        const { session_id, event: payload } = event.payload;
        // Drop events from a superseded transfer so they can't mutate this one.
        if (isStaleSession(session_id, activeSessionRef, sessionFloorRef)) return;
        if ("Transfer" in payload) {
          const trans: TransferEvent = payload.Transfer;
          if (terminalEventRef.current) {
            return;
          }
          if ("StateChanged" in trans) {
            if (trans.StateChanged.state !== "Closed") {
              setPhase("connecting");
              setTransferStatus(friendlyState(trans.StateChanged.state));
            }
          } else if ("PathSelected" in trans) {
            if ("Local" in trans.PathSelected.path) {
              setActivePathBadge(`🟢 Local Wi-Fi (${trans.PathSelected.path.Local.address})`);
              setTransferStatus(`Path selected: Local Wi-Fi direct (${trans.PathSelected.path.Local.address})`);
            } else {
              const relayed = trans.PathSelected.path.Internet.is_relayed ? " (Relayed)" : " (Direct P2P)";
              setActivePathBadge(`🌐 Internet${relayed}`);
              setTransferStatus(`Path selected: Internet via room ${trans.PathSelected.path.Internet.room_code}`);
            }
          } else if ("Started" in trans) {
            if (autoResetRef.current) clearTimeout(autoResetRef.current);
            setPhase("transferring");
            setSendSuccess(false);
            setTransferStatus(trans.Started.resumed_bytes > 0
              ? `Resuming ${trans.Started.file_name} from ${formatBytes(trans.Started.resumed_bytes)}...`
              : `Sending ${trans.Started.file_name}...`);
            metricsRef.current = createTransferMetrics(trans.Started.total_bytes, trans.Started.resumed_bytes);
            setProgress({ transferred: trans.Started.resumed_bytes, total: trans.Started.total_bytes });
            setSpeedText(null);
            setEtaText(null);
            terminalEventRef.current = false;
          } else if ("ConnectionEstablished" in trans) {
            const connection = trans.ConnectionEstablished.mode === "Relay" ? "relay" : trans.ConnectionEstablished.mode === "Direct" ? "direct connection" : "local network";
            setTransferStatus(`Connected via ${connection}`);
          } else if ("AwaitingApproval" in trans) {
            setPhase("awaitingApproval");
            setTransferStatus("Waiting for receiver to accept...");
          } else if ("Resumed" in trans) {
            setPhase("transferring");
            setTransferStatus(`Resuming transfer from ${formatBytes(trans.Resumed.resumed_bytes)}...`);
            setProgress((current) => current ? { ...current, transferred: trans.Resumed.resumed_bytes } : current);
          } else if ("Progress" in trans) {
            setPhase("transferring");
            if (metricsRef.current) {
              const metrics = updateTransferMetrics(metricsRef.current, trans.Progress.transferred_bytes);
              setProgress({ transferred: trans.Progress.transferred_bytes, total: trans.Progress.total_bytes });
              if (metrics.speedBps != null) {
                setSpeedText(`${formatBytes(Math.round(metrics.speedBps))}/s`);
              }
              setEtaText(metrics.etaSeconds != null && metrics.etaSeconds > 0 ? `${metrics.etaSeconds}s left` : "");
            } else {
              setProgress({ transferred: trans.Progress.transferred_bytes, total: trans.Progress.total_bytes });
            }
          } else if ("Declined" in trans) {
            terminalEventRef.current = true;
            setPhase("failed");
            setTransferStatus(trans.Declined.reason === "timeout" ? "Receiver did not respond" : "Receiver declined the transfer");
            setProgress(null);
            setSpeedText(null);
            setEtaText(null);
            metricsRef.current = null;
            if (autoResetRef.current) clearTimeout(autoResetRef.current);
            autoResetRef.current = setTimeout(() => {
              setPhase("idle");
              setTransferStatus("");
              terminalEventRef.current = false;
            }, 2000);
          } else if ("Failed" in trans) {
            terminalEventRef.current = true;
            setPhase("failed");
            setTransferStatus(trans.Failed.message);
            setProgress(null);
            setSpeedText(null);
            setEtaText(null);
            metricsRef.current = null;
          } else if ("Cancelled" in trans) {
            terminalEventRef.current = true;
            setPhase("cancelled");
            setTransferStatus("Transfer cancelled\nThe partial file can be resumed later.");
            setProgress(null);
            setSpeedText(null);
            setEtaText(null);
            metricsRef.current = null;
            if (autoResetRef.current) clearTimeout(autoResetRef.current);
            autoResetRef.current = setTimeout(() => {
              setPhase("idle");
              setTransferStatus("");
              terminalEventRef.current = false;
            }, 2000);
          } else if ("Packaging" in trans) {
            const pkg = trans.Packaging;
            setTransferStatus(`Packaging files (${pkg.current_item}/${pkg.total_items}): ${pkg.current_name}`);
          } else if ("Completed" in trans) {
            terminalEventRef.current = true;
            setPhase("succeeded");
            const summary: TransferSummary = trans.Completed;
            const peerLabel = summary.peer_name ?? summary.peer ?? "device";
            const resumedBytes = summary.resumed_bytes ?? 0;
            const sessionBytes = Math.max(0, summary.total_bytes - resumedBytes);
            addHistoryEntry({
              direction: "send",
              fileName: summary.file_name,
              size: summary.total_bytes,
              resumedBytes,
              sessionBytes,
              peerName: peerLabel,
              durationMs: summary.elapsed_ms,
              mode: summary.mode,
              timestamp: new Date().toISOString(),
            });
            setSendSuccess(true);
            setRoomCodeInput("");
            setTransferStatus(
              summary.elapsed_ms != null
                ? (resumedBytes > 0
                    ? `Sent to ${peerLabel} in ${formatDuration(summary.elapsed_ms)} (Resumed from ${formatBytes(resumedBytes)})`
                    : `Sent to ${peerLabel} in ${formatDuration(summary.elapsed_ms)}`)
                : `Sent ${summary.file_name} successfully!`
            );
            setProgress(null);
            setSpeedText(null);
            setEtaText(null);
            metricsRef.current = null;
            if (autoResetRef.current) clearTimeout(autoResetRef.current);
            autoResetRef.current = setTimeout(() => {
              setPhase("idle");
              setSendSuccess(false);
              setTransferStatus("");
              terminalEventRef.current = false;
            }, 5000);
          }
        } else if ("Log" in payload) {
          if (!terminalEventRef.current) {
            logToConsole(payload.Log);
          }
        }
      });

      const unlistenDrop = await listen<{ paths: string[] }>('tauri://drag-drop', (event) => {
        if (event.payload.paths && event.payload.paths.length > 0) {
          setSelectedPaths((prev) => Array.from(new Set([...prev, ...event.payload.paths])));
        }
        setIsDragging(false);
      });

      const unlistenDragEnter = await listen('tauri://drag-enter', () => setIsDragging(true));
      const unlistenDragLeave = await listen('tauri://drag-leave', () => setIsDragging(false));

      return () => {
        if (unlisten) unlisten();
        unlistenDrop();
        unlistenDragEnter();
        unlistenDragLeave();
      };
    };

    let cleanupFn: (() => void) | undefined;
    setupListener().then(cleanup => {
      if (cancelled) cleanup();
      else cleanupFn = cleanup;
    });

    return () => {
      cancelled = true;
      if (cleanupFn) cleanupFn();
      if (autoResetRef.current) clearTimeout(autoResetRef.current);
    };
  }, []);

  const handleRoomCodeConnect = async () => {
    if (selectedPaths.length === 0) {
      setTransferStatus("Please select files or a folder first");
      return;
    }
    const inputVal = roomCodeInput.trim();
    if (inputVal === "") {
      setTransferStatus("Please enter a room code or paste a Plenum link");
      return;
    }

    const parsed = parseQrPayload(inputVal);
    if (!parsed || (parsed.type !== "unified" && parsed.type !== "room")) {
      setTransferStatus("Invalid code or link format. Please enter a 9-character code or a valid Plenum URL.");
      return;
    }

    if (isConnectingRemote || isTransferActive) return;

    setTransferStatus("Connecting to receiver…");
    terminalEventRef.current = false;
    setPhase("connecting");
    setIsConnectingRemote(true);
    setIsTransferActive(true);

    try {
      const myPeerId = await invoke<string>("generate_peer_id_command");
      const iceServers: IceServer[] = [...DEFAULT_ICE_SERVERS];
      const turn = await invoke<IceServer | null>("fetch_turn_credentials_command", {
        relayServerUrl: RELAY_SERVER_URL,
        peerId: myPeerId,
      });
      if (turn) iceServers.push(turn);

      const payload = parsed.payload ?? {
        version: 1,
        room_code: parsed.code.toUpperCase(),
        lan_ips: [],
        port: 0,
      };

      setTransferStatus("Checking best path (Local Wi-Fi or Internet)…");
      const req: SendUnifiedRequest = {
        file_path: selectedPaths[0] || "",
        file_paths: selectedPaths,
        payload,
        relay_server_url: RELAY_SERVER_URL,
        my_peer_id: myPeerId,
        ice_servers: iceServers,
        connect_timeout_secs: 30,
        device_name: settings.deviceName || undefined,
        permissions: { local_network: true, file_system_read: true, file_system_write: true, background_transfer: false },
        options: { chunk_size: 32768, window_size: 128, timeout_ticks: 1000 }
      };
      const result = await invoke<TransferSummary>("send_file_unified_command", { request: req });
      console.log("Unified send completed:", result);
    } catch (err) {
      console.error("Send error:", err);
      if (!terminalEventRef.current) {
        terminalEventRef.current = true;
        setPhase("failed");
        setTransferStatus(err instanceof Error ? err.message : "Error: " + err);
      }
      setProgress(null);
    } finally {
      setIsConnectingRemote(false);
      setIsTransferActive(false);
    }
  };

  const handleCancelTransfer = async () => {
    if (phase === "cancelling" || terminalEventRef.current) return;
    setPhase("cancelling");
    setTransferStatus("Cancelling transfer...");
    try {
      await invoke("cancel_session_command", { sessionId: activeSessionRef.current });
    } catch (err) {
      console.error("Cancel failed:", err);
    }
  };

  const handleSelectFiles = async () => {
    try {
      const selected = await open({ multiple: true, directory: false });
      if (selected) {
        const paths = Array.isArray(selected) ? selected : [selected];
        setSelectedPaths((prev) => Array.from(new Set([...prev, ...paths])));
      }
    } catch (err) {
      console.error(err);
    }
  };

  const handleSelectFolder = async () => {
    try {
      const selected = await open({ multiple: false, directory: true });
      if (selected && typeof selected === "string") {
        setSelectedPaths((prev) => Array.from(new Set([...prev, selected])));
      }
    } catch (err) {
      console.error(err);
    }
  };

  const handleRemovePath = (pathToRemove: string, e?: React.MouseEvent) => {
    e?.stopPropagation();
    setSelectedPaths((prev) => prev.filter((p) => p !== pathToRemove));
  };

  const handleClearPaths = (e?: React.MouseEvent) => {
    e?.stopPropagation();
    setSelectedPaths([]);
  };

  const isSuccess = phase === "succeeded" || sendSuccess;
  const isInFlight = (phase === "connecting" || phase === "awaitingApproval" || phase === "transferring" || phase === "cancelling" || isTransferActive || isConnectingRemote) && !isSuccess;

  return (
    <div style={{ position: "relative", height: "100%" }} data-phase={phase}>
      {isDragging && (
        <div style={{
          position: "absolute",
          top: 0, left: 0, right: 0, bottom: 0,
          backgroundColor: "rgba(0, 0, 0, 0.7)",
          zIndex: 100,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          borderRadius: "12px",
          border: "2px dashed var(--accent-primary)"
        }}>
          <h2 style={{ color: "var(--accent-primary)" }}>Drop files or folders here to send</h2>
        </div>
      )}

      <div className="card-section">
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "8px" }}>
          <h2 className="section-title" style={{ margin: 0 }}>Files &amp; Folders</h2>
          {selectedPaths.length > 0 && (
            <div style={{ display: "flex", gap: "8px" }}>
              <button
                type="button"
                onClick={handleSelectFiles}
                disabled={isInFlight}
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: "6px",
                  padding: "4px 10px",
                  borderRadius: "6px",
                  border: "1px solid var(--border-color)",
                  backgroundColor: "var(--bg-sidebar)",
                  color: "var(--text-primary)",
                  fontSize: "12px",
                  cursor: isInFlight ? "default" : "pointer",
                }}
              >
                <Plus size={14} /> Add Files
              </button>
              <button
                type="button"
                onClick={handleSelectFolder}
                disabled={isInFlight}
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: "6px",
                  padding: "4px 10px",
                  borderRadius: "6px",
                  border: "1px solid var(--border-color)",
                  backgroundColor: "var(--bg-sidebar)",
                  color: "var(--text-primary)",
                  fontSize: "12px",
                  cursor: isInFlight ? "default" : "pointer",
                }}
              >
                <Folder size={14} /> Add Folder
              </button>
              <button
                type="button"
                onClick={handleClearPaths}
                disabled={isInFlight}
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: "4px",
                  padding: "4px 8px",
                  borderRadius: "6px",
                  border: "none",
                  backgroundColor: "transparent",
                  color: "var(--text-secondary)",
                  fontSize: "12px",
                  cursor: isInFlight ? "default" : "pointer",
                }}
                title="Clear all"
              >
                <Trash2 size={14} />
              </button>
            </div>
          )}
        </div>

        {selectedPaths.length === 0 ? (
          <div
            className="action-card"
            style={{
              padding: "24px 16px",
              textAlign: "center",
              display: "flex",
              flexDirection: "column",
              alignItems: "center",
              gap: "10px",
              borderStyle: "dashed",
            }}
          >
            <div style={{ display: "flex", gap: "12px", color: "var(--text-secondary)" }}>
              <File size={28} />
              <Folder size={28} />
            </div>
            <div>
              <div style={{ fontWeight: 600, color: "var(--text-primary)", fontSize: "14px" }}>
                Select files or folders to send
              </div>
              <div style={{ fontSize: "12px", color: "var(--text-secondary)", marginTop: "2px" }}>
                or drag &amp; drop them anywhere
              </div>
            </div>
            <div style={{ display: "flex", gap: "10px", marginTop: "4px" }}>
              <button
                type="button"
                onClick={handleSelectFiles}
                disabled={isInFlight}
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: "6px",
                  padding: "8px 14px",
                  borderRadius: "8px",
                  backgroundColor: "var(--accent-primary)",
                  color: "white",
                  border: "none",
                  fontWeight: 600,
                  fontSize: "13px",
                  cursor: isInFlight ? "default" : "pointer",
                }}
              >
                <Plus size={15} /> Choose Files
              </button>
              <button
                type="button"
                onClick={handleSelectFolder}
                disabled={isInFlight}
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: "6px",
                  padding: "8px 14px",
                  borderRadius: "8px",
                  backgroundColor: "var(--bg-sidebar)",
                  border: "1px solid var(--border-color)",
                  color: "var(--text-primary)",
                  fontWeight: 600,
                  fontSize: "13px",
                  cursor: isInFlight ? "default" : "pointer",
                }}
              >
                <Folder size={15} /> Choose Folder
              </button>
            </div>
          </div>
        ) : (
          <div
            style={{
              backgroundColor: "var(--bg-card)",
              border: "1px solid var(--border-color)",
              borderRadius: "10px",
              overflow: "hidden",
            }}
          >
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
                padding: "8px 12px",
                backgroundColor: "var(--bg-sidebar)",
                borderBottom: "1px solid var(--border-color)",
                fontSize: "12px",
                fontWeight: 600,
                color: "var(--text-primary)",
              }}
            >
              <span>
                {selectedPaths.length} {selectedPaths.length === 1 ? "item" : "items"} selected
              </span>
              <span style={{ fontSize: "11px", color: "var(--accent-primary)" }}>
                {selectedPaths.length === 1 && !selectedPaths[0].includes(".")
                  ? "Folder (ZIP)"
                  : selectedPaths.length === 1
                  ? "Single File (Direct)"
                  : "Multi-file (ZIP)"}
              </span>
            </div>
            <div style={{ maxHeight: "140px", overflowY: "auto", padding: "6px" }}>
              {selectedPaths.map((itemPath) => {
                const baseName = itemPath.split(/[/\\]/).pop() || itemPath;
                const isFolderLike = !baseName.includes(".");
                return (
                  <div
                    key={itemPath}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                      padding: "6px 8px",
                      borderRadius: "6px",
                      backgroundColor: "var(--bg-card)",
                      marginBottom: "3px",
                      fontSize: "13px",
                    }}
                  >
                    <div style={{ display: "flex", alignItems: "center", gap: "8px", overflow: "hidden" }}>
                      {isFolderLike ? (
                        <Folder size={16} color="var(--accent-primary)" style={{ flexShrink: 0 }} />
                      ) : (
                        <File size={16} color="var(--text-secondary)" style={{ flexShrink: 0 }} />
                      )}
                      <span
                        style={{
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                          color: "var(--text-primary)",
                          fontWeight: 500,
                        }}
                        title={itemPath}
                      >
                        {baseName}
                      </span>
                    </div>
                    {!isInFlight && (
                      <button
                        type="button"
                        onClick={(e) => handleRemovePath(itemPath, e)}
                        style={{
                          background: "none",
                          border: "none",
                          color: "var(--text-secondary)",
                          cursor: "pointer",
                          padding: "2px",
                          display: "flex",
                          alignItems: "center",
                          borderRadius: "4px",
                        }}
                        title="Remove"
                      >
                        <X size={14} />
                      </button>
                    )}
                  </div>
                );
              })}
            </div>
          </div>
        )}
      </div>

      <div className="card-section">
        <div className="section-title">
          <span>Connect to Receiver</span>
        </div>
        <div style={{ display: "flex", gap: "12px" }}>
          <input
            type="text"
            value={roomCodeInput}
            onChange={(e) => setRoomCodeInput(e.target.value)}
            placeholder="Enter 9-character code or paste Plenum link"
            style={{ flex: 1, padding: "10px 12px", borderRadius: "8px", border: "1px solid var(--border-color)", backgroundColor: "var(--bg-sidebar)", color: "var(--text-primary)", outline: "none", fontSize: "14px", letterSpacing: "1px" }}
            onKeyDown={(e) => { if (e.key === "Enter") handleRoomCodeConnect(); }}
          />
          <button
            onClick={handleRoomCodeConnect}
            disabled={isConnectingRemote || isTransferActive}
            style={{ display: "inline-flex", alignItems: "center", gap: "6px", padding: "10px 20px", borderRadius: "8px", border: "none", backgroundColor: "var(--accent-primary)", color: "white", fontWeight: 600, cursor: (isConnectingRemote || isTransferActive) ? "default" : "pointer", opacity: (isConnectingRemote || isTransferActive) ? 0.6 : 1 }}
          >
            <span>Connect</span>
            <ArrowRight size={16} />
          </button>
        </div>

        {transferStatus && (
          <div style={{ marginTop: "24px", padding: "16px", backgroundColor: "var(--bg-card)", borderRadius: "8px", textAlign: "center", border: isSuccess ? "1px solid var(--accent-primary)" : "none" }}>
            {activePathBadge && (
              <div style={{ display: "inline-flex", alignItems: "center", gap: "8px", padding: "4px 12px", borderRadius: "12px", backgroundColor: "var(--bg-sidebar)", border: "1px solid var(--border-color)", marginBottom: "10px", fontSize: "12px", fontWeight: 600, color: "var(--text-primary)" }}>
                {activePathBadge}
              </div>
            )}
            <div style={{ display: "flex", alignItems: "center", justifyContent: "center", gap: "8px", fontSize: "14px", color: isSuccess ? "var(--text-primary)" : "var(--text-secondary)", fontWeight: isSuccess ? 600 : 400 }}>
              {isSuccess && <CheckCircle2 size={18} color="var(--accent-primary)" />}
              {transferStatus}
            </div>
            {progress && (
              <div style={{ marginTop: "12px", width: "100%" }}>
                <div style={{ width: "100%", backgroundColor: "var(--bg-sidebar)", height: "6px", borderRadius: "3px", overflow: "hidden" }}>
                  <div style={{ width: `${progressPercent(progress.transferred, progress.total)}%`, backgroundColor: "var(--accent-primary)", height: "100%" }} />
                </div>
                <div style={{ display: "flex", justifyContent: "space-between", marginTop: "8px", fontSize: "12px", color: "var(--text-secondary)" }}>
                  <span>{Math.round(progressPercent(progress.transferred, progress.total))}%  •  {formatBytes(progress.transferred)} / {formatBytes(progress.total)}</span>
                  {speedText && <span>{speedText}{etaText ? ` • ${etaText}` : ""}</span>}
                </div>
              </div>
            )}
            {isInFlight && (
              <div style={{ marginTop: "12px" }}>
                <button
                  onClick={handleCancelTransfer}
                  disabled={phase === "cancelling"}
                  style={{
                    padding: "6px 14px",
                    borderRadius: "6px",
                    border: "1px solid var(--border-color)",
                    backgroundColor: "transparent",
                    color: phase === "cancelling" ? "var(--text-secondary)" : "var(--accent-primary)",
                    fontSize: "12px",
                    cursor: phase === "cancelling" ? "not-allowed" : "pointer",
                    opacity: phase === "cancelling" ? 0.6 : 1,
                  }}
                >
                  {phase === "cancelling" ? "Cancelling..." : "Cancel transfer"}
                </button>
              </div>
            )}
          </div>
        )}

        <div style={{ marginTop: "32px", textAlign: "center" }}>
          <p style={{ fontSize: "13px", color: "var(--text-secondary)", lineHeight: "1.6" }}>
            Enter the 9-character room code or paste the link from the receiving device.<br />
            Plenum automatically prioritizes direct high-speed local Wi-Fi, falling back to Internet relay if needed.
          </p>
        </div>
      </div>
    </div>
  );
};

export default SendPage;
