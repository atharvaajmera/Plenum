import React, { useState, useEffect, useRef, useMemo } from "react";
import { useNavigate } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { downloadDir } from "@tauri-apps/api/path";
import {
  Copy,
  Check,
  CheckCircle2,
  X,
  Smartphone,
  AlertCircle,
  Radio,
  Share2,
} from "lucide-react";
import { QRCodeSVG } from "qrcode.react";
import { PlenumEventEnvelope, TransferEvent, ReceiveUnifiedRequest, TransferSummary, IceServer, TransferUiPhase } from "../types/rust";
import { useSettings } from "../context/SettingsContext";
import { addHistoryEntry, getHistory } from "../services/history";
import { formatBytes, formatDuration } from "../utils/format";
import { isStaleSession, abandonSession } from "../utils/session";
import { createTransferMetrics, updateTransferMetrics, TransferMetricsState } from "../utils/transferMetrics";
import { encodeUnifiedQr } from "../utils/qrPayload";
import TransferAcceptDialog, { IncomingTransfer } from "../components/TransferAcceptDialog";
import { RELAY_SERVER_URL, DEFAULT_ICE_SERVERS } from "../config";

type LogEvent = { level: string; message: string };

const logToConsole = (_log: LogEvent) => undefined;

const STATE_LABELS: Record<string, string> = {
  Discovering: "Searching...",
  Listening: "Ready to receive files",
  Connecting: "Connecting to device...",
  SignalingConnected: "Connecting to device...",
  NegotiatingIce: "Establishing connection...",
  Connected: "Connected to device...",
};

const friendlyState = (state: string): string => STATE_LABELS[state] ?? "Connecting to device...";

const formatTimeAgo = (isoString: string): string => {
  try {
    const diffMs = Date.now() - new Date(isoString).getTime();
    const diffMins = Math.floor(diffMs / 60000);
    if (diffMins < 1) return "Just now";
    if (diffMins < 60) return `${diffMins}m ago`;
    const diffHours = Math.floor(diffMins / 60);
    if (diffHours < 24) return `${diffHours}h ago`;
    const diffDays = Math.floor(diffHours / 24);
    if (diffDays === 1) return "Yesterday";
    if (diffDays < 30) return `${diffDays} days ago`;
    const diffMonths = Math.floor(diffDays / 30);
    return `${diffMonths} mo ago`;
  } catch {
    return "Recently";
  }
};

const ReceivePage: React.FC = () => {
  const navigate = useNavigate();
  const [phase, setPhase] = useState<TransferUiPhase>("idle");
  const [deviceName, setDeviceName] = useState<string | null>("Loading...");
  const [localIp, setLocalIp] = useState<string | null>(null);
  const [localIps, setLocalIps] = useState<string[]>([]);
  const [username, setUsername] = useState<string | null>(null);
  const [status, setStatus] = useState<string>("Ready to receive files...");
  const [, setProgress] = useState<{ transferred: number, total: number } | null>(null);
  const [currentFile, setCurrentFile] = useState<{ name: string; totalBytes: number } | null>(null);
  const [activePeer, setActivePeer] = useState<{ name: string; address?: string } | null>(null);
  const [pin, setPin] = useState<string | null>(null);
  const [port, setPort] = useState<number | null>(null);
  const [roomCode, setRoomCode] = useState<string | null>(null);
  const [roomCodeCopied, setRoomCodeCopied] = useState(false);
  const [, setSelectedPathBadge] = useState<string | null>(null);
  const [copyError, setCopyError] = useState<string | null>(null);
  const [isQrModalOpen, setIsQrModalOpen] = useState(false);
  const [incoming, setIncoming] = useState<IncomingTransfer | null>(null);
  const [savedPath, setSavedPath] = useState<string | null>(null);
  const [speedText, setSpeedText] = useState<string | null>(null);
  const [etaText, setEtaText] = useState<string | null>(null);
  const [technicalDetails, setTechnicalDetails] = useState<string | null>(null);
  const [transferFailed, setTransferFailed] = useState(false);
  const successResetRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const metricsRef = useRef<TransferMetricsState | null>(null);
  const terminalEventRef = useRef(false);
  const activeSessionRef = useRef(0);
  const sessionFloorRef = useRef(0);
  const peerIdRef = useRef<string>("");
  const teardownRef = useRef<Promise<void>>(Promise.resolve());
  const { settings } = useSettings();
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const outputDirRef = useRef<string>("");

  const handleCopyRoomCode = async () => {
    if (!roomCode) return;
    try {
      await navigator.clipboard.writeText(roomCode);
      setCopyError(null);
      setRoomCodeCopied(true);
      setTimeout(() => setRoomCodeCopied(false), 2000);
    } catch (err) {
      console.error("Clipboard write failed:", err);
      setCopyError("Couldn't copy to clipboard — select and copy the code manually.");
    }
  };

  const handleEndSession = async () => {
    try {
      if (activeSessionRef.current) {
        await invoke("cancel_session_command", { sessionId: activeSessionRef.current });
      }
    } catch (err) {
      console.error("Failed to cancel session:", err);
    }
    navigate("/");
  };

  const handleAcceptResponse = (accept: boolean) => {
    const sessionId = activeSessionRef.current;
    setIncoming(null);
    invoke("respond_to_incoming_command", { sessionId, accept }).catch(() => setStatus("Could not respond to the transfer request. Please try again."));
    if (!accept) {
      terminalEventRef.current = true;
      setPhase("failed");
      setStatus("Transfer declined");
    }
  };

  const resetReceiveUi = () => {
    if (successResetRef.current) {
      clearTimeout(successResetRef.current);
      successResetRef.current = null;
    }
    setSavedPath(null);
    setProgress(null);
    setCurrentFile(null);
    setActivePeer(null);
    setSpeedText(null);
    setEtaText(null);
    metricsRef.current = null;
    setTransferFailed(false);
    setTechnicalDetails(null);
    setSelectedPathBadge(null);
    setPhase("listening");
    setStatus("Ready to receive files (Local Wi-Fi & Internet active)");
  };

  const recentPeers = useMemo(() => {
    const history = getHistory();
    const unique = new Map<string, { peerName: string; timestamp: string }>();
    for (const h of history) {
      if (h.peerName && h.peerName !== "Unknown peer" && !unique.has(h.peerName)) {
        unique.set(h.peerName, { peerName: h.peerName, timestamp: h.timestamp });
        if (unique.size >= 3) break;
      }
    }
    return Array.from(unique.values());
  }, [phase]);

  const handleTransferEvent = (trans: TransferEvent) => {
    if (terminalEventRef.current) {
      return;
    }
    if ("StateChanged" in trans) {
      if (trans.StateChanged.state === "Listening") {
        setPhase("listening");
        setStatus("Ready to receive files (Local Wi-Fi & Internet active)");
      } else if (trans.StateChanged.state !== "Closed") {
        setPhase("connecting");
        setStatus(friendlyState(trans.StateChanged.state));
      }
    } else if ("PathSelected" in trans) {
      if ("Local" in trans.PathSelected.path) {
        setSelectedPathBadge(`Local Wi-Fi (${trans.PathSelected.path.Local.address})`);
      } else {
        const relayed = trans.PathSelected.path.Internet.is_relayed ? " (Relayed)" : " (Direct P2P)";
        setSelectedPathBadge(`Internet${relayed}`);
      }
    } else if ("IncomingRequest" in trans) {
      setIncoming({
        fileName: trans.IncomingRequest.file_name,
        totalBytes: trans.IncomingRequest.total_bytes,
        senderName: trans.IncomingRequest.sender_name,
        peer: trans.IncomingRequest.peer,
      });
      setCurrentFile({
        name: trans.IncomingRequest.file_name,
        totalBytes: trans.IncomingRequest.total_bytes,
      });
      if (trans.IncomingRequest.sender_name || trans.IncomingRequest.peer) {
        setActivePeer({
          name: trans.IncomingRequest.sender_name || "Sender Device",
          address: trans.IncomingRequest.peer,
        });
      }
    } else if ("ConnectionEstablished" in trans) {
      const modeDesc = trans.ConnectionEstablished.mode === "Relay" ? "relay" : trans.ConnectionEstablished.mode === "Direct" ? "direct connection" : "local network";
      setStatus(`Connected via ${modeDesc}`);
    } else if ("Cancelled" in trans) {
      terminalEventRef.current = true;
      setPhase("cancelled");
      setStatus("Transfer cancelled");
      setProgress(null);
      setSpeedText(null);
      setEtaText(null);
      metricsRef.current = null;
      setIncoming(null);
    } else if ("Declined" in trans) {
      terminalEventRef.current = true;
      setPhase("failed");
      setTransferFailed(true);
      setStatus("Transfer declined");
      setProgress(null);
      setSpeedText(null);
      setEtaText(null);
      metricsRef.current = null;
      setIncoming(null);
      setTimeout(() => {
        resetReceiveUi();
      }, 2000);
    } else if ("Failed" in trans) {
      terminalEventRef.current = true;
      setPhase("failed");
      setTransferFailed(true);
      setStatus(trans.Failed.message);
      setProgress(null);
      setSpeedText(null);
      setEtaText(null);
      metricsRef.current = null;
    } else if ("Started" in trans) {
      if (successResetRef.current) {
        clearTimeout(successResetRef.current);
        successResetRef.current = null;
      }
      terminalEventRef.current = false;
      setPhase("transferring");
      setTransferFailed(false);
      setTechnicalDetails(null);
      setIncoming(null);
      setSavedPath(null);
      setCurrentFile({
        name: trans.Started.file_name,
        totalBytes: trans.Started.total_bytes,
      });
      setStatus(trans.Started.resumed_bytes > 0
        ? `Resuming ${trans.Started.file_name} from ${formatBytes(trans.Started.resumed_bytes)}...`
        : `Receiving ${trans.Started.file_name}...`);
      metricsRef.current = createTransferMetrics(trans.Started.total_bytes, trans.Started.resumed_bytes);
      setProgress({ transferred: trans.Started.resumed_bytes, total: trans.Started.total_bytes });
      setSpeedText(null);
      setEtaText(null);
    } else if ("Resumed" in trans) {
      setPhase("transferring");
      setStatus(`Resuming receive from ${formatBytes(trans.Resumed.resumed_bytes)}...`);
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
    } else if ("Completed" in trans) {
      terminalEventRef.current = true;
      setPhase("succeeded");
      setTransferFailed(false);
      setTechnicalDetails(null);
      const summary: TransferSummary = trans.Completed;
      setCurrentFile({
        name: summary.file_name,
        totalBytes: summary.total_bytes,
      });
      if (summary.peer_name || summary.peer) {
        setActivePeer({
          name: summary.peer_name || "Sender Device",
          address: summary.peer,
        });
      }
      const resumedBytes = summary.resumed_bytes ?? 0;
      const sessionBytes = Math.max(0, summary.total_bytes - resumedBytes);
      const path = outputDirRef.current
        ? `${outputDirRef.current}${outputDirRef.current.endsWith("\\") || outputDirRef.current.endsWith("/") ? "" : "\\"}${summary.file_name}`
        : null;
      addHistoryEntry({
        direction: "receive",
        fileName: summary.file_name,
        size: summary.total_bytes,
        resumedBytes,
        sessionBytes,
        peerName: summary.peer_name || summary.peer || "Unknown peer",
        durationMs: summary.elapsed_ms,
        mode: summary.mode,
        path: path ?? undefined,
        timestamp: new Date().toISOString(),
      });
      setSavedPath(path);
      const peerLabel = summary.peer_name ? `${summary.peer_name} (${summary.peer ?? ""})` : summary.peer ?? "peer";
      const resumedNote = resumedBytes > 0 ? ` (Resumed from ${formatBytes(resumedBytes)})` : "";
      setStatus(`Received ${summary.file_name} from ${peerLabel} in ${formatDuration(summary.elapsed_ms)}${resumedNote}`);
      setProgress(null);
      setSpeedText(null);
      setEtaText(null);
      metricsRef.current = null;
      successResetRef.current = setTimeout(() => {
        resetReceiveUi();
      }, 5000);
    }
  };

  const handleTransferEventRef = useRef(handleTransferEvent);
  handleTransferEventRef.current = handleTransferEvent;
  const handleLogEvent = (log: LogEvent) => {
    logToConsole(log);
    if (log.level === "Error") {
      setTechnicalDetails(log.message);
    }
  };
  const handleLogEventRef = useRef(handleLogEvent);
  handleLogEventRef.current = handleLogEvent;

  useEffect(() => {
    invoke<string | null>("get_device_name").then(setDeviceName).catch(() => setDeviceName(null));
    invoke<string | null>("get_local_ip").then(setLocalIp).catch(() => setLocalIp(null));
    invoke<string[]>("get_local_ips_command").then(setLocalIps).catch(() => setLocalIps([]));
    invoke<string | null>("get_username").then(setUsername).catch(() => setUsername(null));
  }, []);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;
    const prior = teardownRef.current;
    let markDone: () => void = () => {};
    teardownRef.current = new Promise<void>((resolve) => { markDone = resolve; });

    const setupUnifiedReceiver = async () => {
      await prior;
      if (cancelled) return;

      const fn = await listen<PlenumEventEnvelope>("plenum-event", (event) => {
        const { session_id, event: payload } = event.payload;
        if (isStaleSession(session_id, activeSessionRef, sessionFloorRef)) return;
        if ("Discovery" in payload) {
          const disc = payload.Discovery;
          if (typeof disc === "object" && "BroadcastStarted" in disc) {
            setPort(disc.BroadcastStarted.port);
            if (settingsRef.current.receive.requirePin) {
              setPin(disc.BroadcastStarted.token);
            }
          }
        } else if ("Transfer" in payload) {
          handleTransferEventRef.current(payload.Transfer);
        } else if ("Log" in payload) {
          handleLogEventRef.current(payload.Log);
        }
      });

      if (cancelled) { fn(); return; }
      unlisten = fn;

      const [code, myPeerId, ips] = await Promise.all([
        invoke<string>("generate_room_code_command"),
        invoke<string>("generate_peer_id_command"),
        invoke<string[]>("get_local_ips_command").catch(() => []),
      ]);

      if (cancelled) return;
      setRoomCode(code);
      peerIdRef.current = myPeerId;
      if (ips && ips.length > 0) {
        setLocalIps(ips);
      }

      const downloadsPath = await downloadDir();
      outputDirRef.current = downloadsPath;

      const iceServers: IceServer[] = [...DEFAULT_ICE_SERVERS];
      const turn = await invoke<IceServer | null>("fetch_turn_credentials_command", {
        relayServerUrl: RELAY_SERVER_URL,
        peerId: myPeerId,
      }).catch(() => null);
      if (turn) iceServers.push(turn);

      const req: ReceiveUnifiedRequest = {
        output_dir: downloadsPath,
        relay_server_url: RELAY_SERVER_URL,
        session_id: code,
        my_peer_id: myPeerId,
        ice_servers: iceServers,
        connect_timeout_secs: 600,
        port: 0,
        require_pin: settings.receive.requirePin,
        auto_accept: settings.receive.autoAccept,
        device_name: settings.deviceName || undefined,
        permissions: { local_network: true, file_system_read: true, file_system_write: true, background_transfer: false },
        options: { chunk_size: 32768, window_size: 128, timeout_ticks: 15000 }
      };

      while (!cancelled) {
        terminalEventRef.current = false;
        setPhase("listening");
        setStatus("Ready to receive files");
        try {
          const result = await invoke<TransferSummary>("receive_file_unified_command", { request: req });
          void result;
          if (!cancelled) await new Promise(resolve => setTimeout(resolve, 1500));
        } catch (err) {
          if (!cancelled && !terminalEventRef.current) {
            terminalEventRef.current = true;
            setPhase("failed");
            setStatus(`Could not receive the file: ${err instanceof Error ? err.message : String(err)}`);
          }
          break;
        }
      }
    };

    setupUnifiedReceiver().finally(() => markDone());

    return () => {
      cancelled = true;
      if (successResetRef.current) clearTimeout(successResetRef.current);
      abandonSession(activeSessionRef, sessionFloorRef);
      if (unlisten) unlisten();
      invoke("cancel_session_command", { sessionId: activeSessionRef.current }).catch(console.error);
    };
  }, [settings.receive.requirePin, settings.receive.autoAccept, settings.deviceName]);

  const isFailed = phase === "failed" || transferFailed;

  const unifiedQr = roomCode ? encodeUnifiedQr({
    version: 1,
    room_code: roomCode,
    lan_ips: localIps.length > 0 ? localIps : (localIp ? [localIp] : []),
    port: port || 0,
    pin: pin || undefined,
    peer_id: peerIdRef.current || undefined,
    device_name: settings.deviceName || deviceName || undefined,
  }) : null;

  return (
    <>
      {incoming && <TransferAcceptDialog incoming={incoming} onRespond={handleAcceptResponse} />}

      {isQrModalOpen && unifiedQr && (
        <div
          onClick={() => setIsQrModalOpen(false)}
          style={{
            position: "fixed",
            top: 0,
            left: 0,
            right: 0,
            bottom: 0,
            backgroundColor: "rgba(0, 0, 0, 0.75)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            zIndex: 9999,
            backdropFilter: "blur(8px)",
          }}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            style={{
              backgroundColor: "var(--bg-card)",
              borderRadius: "20px",
              padding: "24px 28px",
              border: "1px solid var(--border-color)",
              display: "flex",
              flexDirection: "column",
              alignItems: "center",
              gap: "18px",
              boxShadow: "0 16px 40px rgba(0, 0, 0, 0.6)",
              maxWidth: "360px",
              width: "90%",
            }}
          >
            <div style={{ display: "flex", justifyContent: "space-between", width: "100%", alignItems: "center" }}>
              <span style={{ fontWeight: 600, color: "var(--text-primary)", fontSize: "16px" }}>
                Scan QR to Receive
              </span>
              <button
                onClick={() => setIsQrModalOpen(false)}
                style={{
                  background: "transparent",
                  border: "none",
                  cursor: "pointer",
                  color: "var(--text-secondary)",
                  padding: "4px",
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                }}
              >
                <X size={18} />
              </button>
            </div>
            <div
              className="receive-modal-qr-box"
              style={{
                position: "relative",
                padding: "16px",
                backgroundColor: "#ffffff",
                borderRadius: "16px",
                boxShadow: "0 6px 20px rgba(0, 0, 0, 0.25), 0 0 20px rgba(89, 178, 134, 0.15)",
              }}
            >
              <div className="qr-corner qr-corner-tl" style={{ top: "-4px", left: "-4px", borderRadius: "18px 0 0 0" }} />
              <div className="qr-corner qr-corner-tr" style={{ top: "-4px", right: "-4px", borderRadius: "0 18px 0 0" }} />
              <div className="qr-corner qr-corner-bl" style={{ bottom: "-4px", left: "-4px", borderRadius: "0 0 0 18px" }} />
              <div className="qr-corner qr-corner-br" style={{ bottom: "-4px", right: "-4px", borderRadius: "0 0 18px 0" }} />
              <QRCodeSVG
                value={unifiedQr}
                size={240}
                fgColor="#000000"
                bgColor="#ffffff"
                level="M"
                marginSize={1}
              />
            </div>
            <div style={{ fontSize: "13px", color: "var(--text-secondary)", textAlign: "center" }}>
              Code: <span style={{ fontWeight: 600, color: "var(--text-primary)", fontFamily: "monospace" }}>{roomCode}</span>
              {pin && <> • PIN: <span style={{ fontWeight: 600, color: "var(--accent-primary)" }}>{pin}</span></>}
            </div>
          </div>
        </div>
      )}

      <div className="home-container receive-container" data-phase={phase}>
        <div className="ring-wrapper">
          <div className="segmented-ring"></div>
          <div className="core-circle"></div>
        </div>
        
        <h1 className="device-name">{settings.deviceName || deviceName || <span style={{ fontStyle: "italic", opacity: 0.6 }}>Unknown device</span>}</h1>
        <div className="device-id">
          {localIp ? <>{localIp}{port ? `:${port}` : ""}</> : <span style={{ fontStyle: "italic", opacity: 0.6 }}>Network address unavailable</span>}
          {username ? ` • ${username}` : ''}
        </div>

        {/* Where nav-buttons-container starts on HomePage, place receive utilities */}
        <div className="receive-utilities-container">
          <div className="receive-page-header">
            <h2 className="receive-page-title">Share the code</h2>
            <p className="receive-page-subtitle">Scan the QR or share the code with the sender to continue.</p>
          </div>

          <div className="receive-hero-grid">
            {/* Left Column: Bold QR Code Card */}
            <div
              className="receive-qr-card"
              onClick={() => setIsQrModalOpen(true)}
              title="Click to view full-size QR code"
            >
              {/* Viewfinder corner brackets */}
              <div className="qr-corner qr-corner-tl" />
              <div className="qr-corner qr-corner-tr" />
              <div className="qr-corner qr-corner-bl" />
              <div className="qr-corner qr-corner-br" />

              <div className="qr-svg-wrapper">
                {unifiedQr ? (
                  <QRCodeSVG
                    value={unifiedQr}
                    size={156}
                    fgColor="#000000"
                    bgColor="#ffffff"
                    level="M"
                    marginSize={0}
                  />
                ) : (
                  <div style={{ display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", color: "#64748b", height: 156 }}>
                    <Radio size={28} />
                    <span style={{ fontSize: "11px", marginTop: "6px" }}>Generating code...</span>
                  </div>
                )}
              </div>
            </div>

            {/* Right Column: Status, Code Bar, Devices */}
            <div className="receive-hero-details">
              {/* Status Row */}
              <div className="receive-status-row">
                <div className={`receive-status-icon-box ${phase === "listening" || phase === "transferring" ? "active" : ""}`}>
                  {phase === "succeeded" ? (
                    <CheckCircle2 size={18} color="var(--accent-primary)" />
                  ) : isFailed ? (
                    <AlertCircle size={18} color="#e5484d" />
                  ) : (
                    <Share2 size={18} />
                  )}
                </div>
                <div className="receive-status-text">
                  <div className="receive-status-headline">
                    {phase === "transferring"
                      ? (currentFile ? `Receiving ${currentFile.name}...` : "Receiving file...")
                      : phase === "succeeded"
                      ? "Transfer completed"
                      : isFailed
                      ? "Transfer interrupted"
                      : phase === "connecting"
                      ? "Connecting to device..."
                      : "Waiting for someone to join"}
                  </div>
                  <div className="receive-status-subline">
                    {phase === "transferring"
                      ? (speedText ? `${speedText}${etaText ? ` • ${etaText}` : ""}` : "Transferring data blocks...")
                      : phase === "succeeded"
                      ? (savedPath ? `Saved to ${savedPath}` : "File received successfully")
                      : isFailed
                      ? status
                      : "Closing the app cancels the transfer."}
                  </div>
                </div>
              </div>

              {/* Share Code Bar */}
              <div className="receive-code-bar">
                <div className="receive-code-content">
                  <div className="receive-code-string" title={roomCode || "Generating code..."}>
                    {roomCode || "•••• •••• ••••"}
                  </div>
                  {pin && <span className="receive-pin-badge">PIN: {pin}</span>}
                </div>
                <div className="receive-code-actions">
                  <button
                    className="receive-code-btn"
                    onClick={handleCopyRoomCode}
                    title={roomCodeCopied ? "Copied!" : "Copy code"}
                  >
                    {roomCodeCopied ? <Check size={16} color="var(--accent-primary)" /> : <Copy size={16} />}
                  </button>
                </div>
              </div>
              {copyError && (
                <div style={{ fontSize: "11px", color: "#e5484d", marginTop: "-4px" }}>
                  {copyError}
                </div>
              )}

              {/* Devices Section */}
              <div className="receive-devices-section">
                <div className="receive-section-label">Device</div>
                {activePeer ? (
                  <div className="receive-device-item">
                    <div className="receive-device-left">
                      <div className="receive-device-icon-box">
                        <Smartphone size={16} />
                      </div>
                      <div className="receive-device-info">
                        <div className="receive-device-name">
                          {activePeer.name}
                        </div>
                        <div className="receive-device-sub">
                          {activePeer.address ? `${activePeer.address} • ` : ""}
                          {phase === "transferring" ? "Transferring file" : phase === "succeeded" ? "Transfer completed" : "Connected"}
                        </div>
                      </div>
                    </div>
                    <div>
                      <div className="receive-device-pill">
                        {phase === "transferring" ? "Receiving" : phase === "succeeded" ? "Completed" : "Connected"}
                      </div>
                    </div>
                  </div>
                ) : recentPeers.length > 0 ? (
                  recentPeers.slice(0, 1).map((peer) => (
                    <div className="receive-device-item" key={peer.peerName}>
                      <div className="receive-device-left">
                        <div className="receive-device-icon-box">
                          <Smartphone size={16} />
                        </div>
                        <div className="receive-device-info">
                          <div className="receive-device-name">
                            {peer.peerName}
                          </div>
                          <div className="receive-device-sub">
                            {formatTimeAgo(peer.timestamp)}
                          </div>
                        </div>
                      </div>
                      <div>
                        <div className="receive-device-pill" style={{ opacity: 0.9 }}>
                          Known
                        </div>
                      </div>
                    </div>
                  ))
                ) : (
                  <div className="receive-device-item" style={{ opacity: 0.75, borderStyle: "dashed" }}>
                    <div className="receive-device-left">
                      <div className="receive-device-icon-box">
                        <Smartphone size={16} />
                      </div>
                      <div className="receive-device-info">
                        <div className="receive-device-name" style={{ fontSize: "13px" }}>
                          No sender connected
                        </div>
                        <div className="receive-device-sub">
                          Nearby devices scanning your QR will appear here
                        </div>
                      </div>
                    </div>
                    <div>
                      <div className="receive-device-pill" style={{ backgroundColor: "rgba(255, 255, 255, 0.08)", color: "var(--text-secondary)" }}>
                        Waiting
                      </div>
                    </div>
                  </div>
                )}
              </div>
            </div>
          </div>

          {/* Footer Actions */}
          <div className="receive-footer-actions">
            <button className="receive-end-session-btn" onClick={handleEndSession}>
              End session
            </button>
          </div>
        </div>

        {/* Error details if dev */}
        {import.meta.env.DEV && isFailed && technicalDetails && (
          <details className="technical-details" style={{ marginTop: "16px" }}>
            <summary>Technical details</summary>
            <p>{technicalDetails}</p>
          </details>
        )}
      </div>
    </>
  );
};

export default ReceivePage;
