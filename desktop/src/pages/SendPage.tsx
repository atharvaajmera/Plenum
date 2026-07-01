import React, { useState, useEffect } from "react";
import { File, Folder, AlignLeft, ClipboardPaste, RefreshCcw, Monitor, Heart, Settings } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { PlenumEvent, DiscoverRequest, DiscoverySummary, SendRequest, TransferSummary } from "../types/rust";

const SendPage: React.FC = () => {
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [selectionType, setSelectionType] = useState<"file" | "folder" | "text" | "paste" | null>(null);
  const [peers, setPeers] = useState<DiscoverySummary[]>([]);
  const [isDiscovering, setIsDiscovering] = useState(false);
  const [transferStatus, setTransferStatus] = useState<string>("");
  const [progress, setProgress] = useState<{ transferred: number, total: number } | null>(null);
  const [isDragging, setIsDragging] = useState(false);
  const [pinInputPeer, setPinInputPeer] = useState<DiscoverySummary | null>(null);
  const [pinInput, setPinInput] = useState("");

  const startDiscovery = async () => {
    setIsDiscovering(true);
    setPeers([]);
    
    try {
      const req: DiscoverRequest = {
        timeout_secs: 10,
        permissions: { local_network: true, file_system_read: true, file_system_write: true, background_transfer: false }
      };
      await invoke<DiscoverySummary>("discover_peers_command", { request: req });
    } catch (err) {
      console.error("Discovery error:", err);
    } finally {
      setIsDiscovering(false);
    }
  };

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;

    const setupListener = async () => {
      unlisten = await listen<PlenumEvent>("plenum-event", (event) => {
        const payload = event.payload;
        if ("Discovery" in payload) {
           const disc = payload.Discovery;
           if (typeof disc === "object" && "PeerFound" in disc) {
             setPeers((prev) => {
               const exists = prev.find(p => p.token === disc.PeerFound.token);
               if (exists) return prev;
               return [...prev, disc.PeerFound];
             });
           }
        } else if ("Transfer" in payload) {
           const trans = payload.Transfer;
            if ("StateChanged" in trans) {
              if (trans.StateChanged.state !== "Closed") {
                setTransferStatus(trans.StateChanged.state === "Connected" ? "Connected to device..." : trans.StateChanged.state);
              }
           } else if ("Started" in trans) {
             setTransferStatus(`Sending ${trans.Started.file_name}...`);
             setProgress({ transferred: 0, total: trans.Started.total_bytes });
           } else if ("Progress" in trans) {
             setProgress({ transferred: trans.Progress.transferred_bytes, total: trans.Progress.total_bytes });
           } else if ("Completed" in trans) {
             setTransferStatus(`Sent ${trans.Completed.file_name} successfully!`);
             setProgress(null);
           }
        }
      });

      const unlistenDrop = await listen<{ paths: string[] }>('tauri://drag-drop', (event) => {
        if (event.payload.paths.length > 0) {
          setSelectedPath(event.payload.paths[0]);
          setSelectionType("file");
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
      cleanupFn = cleanup;
    });
    startDiscovery();

    return () => {
      if (cleanupFn) cleanupFn();
    };
  }, []);

  const handlePeerClick = (peer: DiscoverySummary) => {
    if (!selectedPath) {
      setTransferStatus("Please select a file or folder first");
      return;
    }
    setPinInputPeer(peer);
    setPinInput("");
  };

  const handlePinSubmit = async () => {
    if (!pinInputPeer) return;
    
    if (pinInput.trim() !== "") {
      if (pinInput.trim().toUpperCase() !== pinInputPeer.token.toUpperCase()) {
        setTransferStatus("Error: Incorrect PIN entered.");
        return;
      }
    }

    setTransferStatus("Connecting to device...");
    const peer = pinInputPeer;
    setPinInputPeer(null);
    
    try {
      const req: SendRequest = {
        file_path: selectedPath!,
        address: peer.address,
        discovery_token: peer.token,
        permissions: { local_network: true, file_system_read: true, file_system_write: true, background_transfer: false },
        options: { chunk_size: 32768, window_size: 128, timeout_ticks: 1000 }
      };
      const result = await invoke<TransferSummary>("send_file_command", { request: req });
      console.log("Send completed:", result);
    } catch (err) {
      console.error("Send error:", err);
      setTransferStatus("Error: " + err);
      setProgress(null);
    }
  };

  const handleSelectFile = async () => {
    try {
      const selected = await open({ multiple: false, directory: false });
      if (selected && !Array.isArray(selected)) {
        setSelectedPath(selected);
        setSelectionType("file");
      }
    } catch (err) {
      console.error(err);
    }
  };

  const handleSelectFolder = async () => {
    try {
      const selected = await open({ multiple: false, directory: true });
      if (selected && !Array.isArray(selected)) {
        setSelectedPath(selected);
        setSelectionType("folder");
      }
    } catch (err) {
      console.error(err);
    }
  };

  const handleSelectText = () => {
    setSelectionType("text");
    setSelectedPath("Text Input selected");
  };

  const handleSelectPaste = async () => {
    setSelectionType("paste");
    setSelectedPath("Clipboard Content selected");
  };
  return (
    <div style={{ position: "relative", height: "100%" }}>
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
          <h2 style={{ color: "var(--accent-primary)" }}>Drop file here to send</h2>
        </div>
      )}
      <div className="card-section">
        <h2 className="section-title">Selection</h2>
        <div className="card-grid">
          <div className="action-card" onClick={handleSelectFile} style={{ borderColor: selectionType === "file" ? "var(--accent-primary)" : "var(--border-color)" }}>
            <File size={28} />
            <span>File</span>
          </div>
          <div className="action-card" onClick={handleSelectFolder} style={{ borderColor: selectionType === "folder" ? "var(--accent-primary)" : "var(--border-color)" }}>
            <Folder size={28} />
            <span>Folder</span>
          </div>
          <div className="action-card" onClick={handleSelectText} style={{ borderColor: selectionType === "text" ? "var(--accent-primary)" : "var(--border-color)" }}>
            <AlignLeft size={28} />
            <span>Text</span>
          </div>
          <div className="action-card" onClick={handleSelectPaste} style={{ borderColor: selectionType === "paste" ? "var(--accent-primary)" : "var(--border-color)" }}>
            <ClipboardPaste size={28} />
            <span>Paste</span>
          </div>
        </div>
        {selectedPath && (
          <div style={{ marginTop: "12px", fontSize: "13px", color: "var(--text-secondary)" }}>
            Selected: <span style={{ color: "var(--text-primary)" }}>{selectedPath.split(/[/\\]/).pop()}</span>
          </div>
        )}
      </div>

      <div className="card-section">
        <div className="section-title" style={{ justifyContent: "space-between" }}>
          <div style={{ display: "flex", alignItems: "center", gap: "12px" }}>
            <span>Nearby devices</span>
            <RefreshCcw 
              size={16} 
              color={isDiscovering ? "var(--text-secondary)" : "var(--accent-primary)"} 
              style={{ cursor: "pointer", opacity: isDiscovering ? 0.5 : 1 }} 
              onClick={startDiscovery} 
            />
          </div>
        </div>
        {peers.length === 0 && !isDiscovering && (
           <div style={{ padding: "24px", textAlign: "center", color: "var(--text-secondary)", fontSize: "14px" }}>
             Ready to send files. Make sure the receiver is open on the other device.
           </div>
        )}
        
        {peers.length === 0 && isDiscovering && (
           <div style={{ padding: "24px", textAlign: "center", color: "var(--text-secondary)", fontSize: "14px" }}>
             Searching for nearby devices...
           </div>
        )}

        {peers.map((peer, i) => (
          <div key={i} style={{ marginTop: "16px" }}>
            <div onClick={() => handlePeerClick(peer)} style={{ 
              backgroundColor: "var(--bg-card)", 
              padding: "24px", 
              borderRadius: pinInputPeer?.address === peer.address ? "12px 12px 0 0" : "12px", 
              display: "flex", 
              alignItems: "center", 
              gap: "16px",
              cursor: "pointer",
              border: "1px solid transparent",
              borderBottom: pinInputPeer?.address === peer.address ? "1px solid var(--border-color)" : "1px solid transparent",
              transition: "all 0.2s ease"
            }}
            onMouseEnter={(e) => e.currentTarget.style.borderColor = "var(--accent-primary)"}
            onMouseLeave={(e) => { if (pinInputPeer?.address !== peer.address) e.currentTarget.style.borderColor = "transparent"; }}
            >
              <Monitor size={40} color="var(--accent-primary)" />
              <div style={{ display: "flex", flexDirection: "column", gap: "4px" }}>
                <div style={{ fontWeight: 600, color: "var(--text-primary)" }}>{peer.hostname}</div>
                <div style={{ fontSize: "12px", color: "var(--text-secondary)" }}>{peer.address}</div>
              </div>
            </div>
            
            {pinInputPeer?.address === peer.address && (
              <div style={{ backgroundColor: "var(--bg-card)", padding: "16px 24px", borderRadius: "0 0 12px 12px", display: "flex", flexDirection: "column", gap: "12px" }}>
                <div style={{ fontSize: "13px", color: "var(--text-secondary)" }}>
                  Enter PIN if required, otherwise leave blank:
                </div>
                <div style={{ display: "flex", gap: "12px" }}>
                  <input 
                    type="text" 
                    value={pinInput} 
                    onChange={(e) => setPinInput(e.target.value)} 
                    placeholder="PIN" 
                    maxLength={6}
                    style={{ flex: 1, padding: "10px 12px", borderRadius: "8px", border: "1px solid var(--border-color)", backgroundColor: "var(--bg-sidebar)", color: "var(--text-primary)", outline: "none", fontSize: "14px", letterSpacing: "2px", textTransform: "uppercase" }}
                    onKeyDown={(e) => { if (e.key === "Enter") handlePinSubmit(); }}
                  />
                  <button onClick={handlePinSubmit} style={{ padding: "10px 20px", borderRadius: "8px", border: "none", backgroundColor: "var(--accent-primary)", color: "white", fontWeight: 600, cursor: "pointer" }}>
                    Connect
                  </button>
                </div>
              </div>
            )}
          </div>
        ))}

        {transferStatus && (
          <div style={{ marginTop: "24px", padding: "16px", backgroundColor: "var(--bg-card)", borderRadius: "8px", textAlign: "center" }}>
            <div style={{ fontSize: "14px", color: "var(--text-secondary)" }}>
              {transferStatus}
            </div>
            {progress && (
              <div style={{ marginTop: "12px", width: "100%" }}>
                <div style={{ width: "100%", backgroundColor: "var(--bg-sidebar)", height: "6px", borderRadius: "3px", overflow: "hidden" }}>
                  <div style={{ width: `${(progress.transferred / progress.total) * 100}%`, backgroundColor: "var(--accent-primary)", height: "100%" }} />
                </div>
              </div>
            )}
          </div>
        )}

        <div style={{ marginTop: "40px", textAlign: "center" }}>
          <span style={{ fontSize: "14px", color: "var(--accent-primary)", cursor: "pointer", fontWeight: 500 }}>Troubleshoot</span>
          <p style={{ fontSize: "13px", color: "var(--text-secondary)", marginTop: "16px" }}>
            Please ensure that the desired target is also on the same Wi-Fi network.
          </p>
        </div>
      </div>
    </div>
  );
};

export default SendPage;
