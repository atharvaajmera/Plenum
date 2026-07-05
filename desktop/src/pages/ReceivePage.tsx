import React, { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { downloadDir } from "@tauri-apps/api/path";
import { Copy, Check } from "lucide-react";
import { PlenumEvent, ReceiveRequest, TransferSummary } from "../types/rust";
import { useSettings } from "../context/SettingsContext";

const ReceivePage: React.FC = () => {
  const [deviceName, setDeviceName] = useState<string>("Loading...");
  const [localIp, setLocalIp] = useState<string>("");
  const [username, setUsername] = useState<string>("");
  const [status, setStatus] = useState<string>("Ready to receive files");
  const [progress, setProgress] = useState<{ transferred: number, total: number } | null>(null);
  const [pin, setPin] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const { settings } = useSettings();

  const handleCopyPin = () => {
    if (pin) {
      navigator.clipboard.writeText(pin);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  };

  useEffect(() => {
    invoke<string>("get_device_name").then(setDeviceName).catch(console.error);
    invoke<string>("get_local_ip").then(setLocalIp).catch(console.error);
    invoke<string>("get_username").then(setUsername).catch(console.error);

    let unlisten: UnlistenFn | undefined;

    const setupReceiver = async () => {
      // 1. Listen for events
      unlisten = await listen<PlenumEvent>("plenum-event", (event) => {
        const payload = event.payload;
        if ("Discovery" in payload) {
           const disc = payload.Discovery;
           if (typeof disc === "object" && "BroadcastStarted" in disc) {
             console.log("Broadcast started on port:", disc.BroadcastStarted.port);
             if (settings.receive.requirePin) {
               setPin(disc.BroadcastStarted.token);
             }
           }
        } else if ("Transfer" in payload) {
           const trans = payload.Transfer;
            if ("StateChanged" in trans) {
              if (trans.StateChanged.state !== "Closed") {
                if (trans.StateChanged.state === "Listening") {
                  setStatus("Ready to receive files");
                } else if (trans.StateChanged.state === "Connected") {
                  setStatus("Connected to device...");
                } else {
                  setStatus(trans.StateChanged.state);
                }
              }
           } else if ("Started" in trans) {
             setStatus(`Receiving ${trans.Started.file_name}...`);
             setProgress({ transferred: 0, total: trans.Started.total_bytes });
           } else if ("Progress" in trans) {
             setProgress({ transferred: trans.Progress.transferred_bytes, total: trans.Progress.total_bytes });
           } else if ("Completed" in trans) {
             setStatus(`Received ${trans.Completed.file_name} successfully!`);
             setProgress(null);
           }
        }
      });

      // 2. Resolve the real system Downloads directory
      const downloadsPath = await downloadDir();

      const req: ReceiveRequest = {
        port: 0, // auto-assign; firewall allows the whole exe
        output_dir: downloadsPath,
        announce_on_lan: true,
        permissions: { local_network: true, file_system_read: true, file_system_write: true, background_transfer: false },
        options: { chunk_size: 32768, window_size: 128, timeout_ticks: 1000 }
      };

      try {
        const result = await invoke<TransferSummary>("receive_file_command", { request: req });
        console.log("Receive completed:", result);
      } catch (err) {
        console.error("Receive error:", err);
        setStatus("Error: " + err);
      }
    };

    setupReceiver();

    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  return (
    <div className="receive-container">
      <div className="ring-wrapper">
        <div className="segmented-ring"></div>
        <div className="core-circle"></div>
      </div>
      
      <div style={{ display: "flex", flexDirection: "column", alignItems: "center", marginTop: "20px" }}>
        <h1 className="device-name" style={{ textAlign: "center" }}>{deviceName}</h1>
        <div className="device-id" style={{ textAlign: "center" }}>{localIp} {username ? `• ${username}` : ''}</div>
        
        <div style={{ marginTop: "20px", fontSize: "14px", color: "var(--text-secondary)", textAlign: "center" }}>
          {status}
        </div>
        
        {pin && (
          <div style={{ marginTop: "16px", padding: "12px 24px", backgroundColor: "var(--bg-card)", borderRadius: "8px", border: "1px dashed var(--accent-primary)", display: "flex", flexDirection: "column", alignItems: "center" }}>
            <div style={{ fontSize: "12px", color: "var(--text-secondary)", textAlign: "center", marginBottom: "4px" }}>PIN Required</div>
            <div style={{ display: "flex", alignItems: "center", gap: "12px" }}>
              <div style={{ fontSize: "24px", fontWeight: "bold", color: "var(--accent-primary)", letterSpacing: "4px" }}>{pin}</div>
              <div onClick={handleCopyPin} style={{ cursor: "pointer", padding: "4px", backgroundColor: "var(--bg-sidebar)", borderRadius: "4px" }}>
                {copied ? <Check size={16} color="var(--accent-primary)" /> : <Copy size={16} color="var(--text-secondary)" />}
              </div>
            </div>
          </div>
        )}
        
        {progress && (
          <div style={{ marginTop: "16px", width: "80%", maxWidth: "300px" }}>
            <div style={{ width: "100%", backgroundColor: "var(--bg-sidebar)", height: "8px", borderRadius: "4px", overflow: "hidden" }}>
              <div style={{ width: `${(progress.transferred / progress.total) * 100}%`, backgroundColor: "var(--accent-primary)", height: "100%" }} />
            </div>
            <div style={{ fontSize: "12px", color: "var(--text-secondary)", marginTop: "8px", textAlign: "center" }}>
              {Math.round(progress.transferred / 1024 / 1024)} MB / {Math.round(progress.total / 1024 / 1024)} MB
            </div>
          </div>
        )}
      </div>


    </div>
  );
};

export default ReceivePage;
