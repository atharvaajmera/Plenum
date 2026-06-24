import React, { useState } from "react";
import { File, Folder, AlignLeft, ClipboardPaste, RefreshCcw, Monitor, Heart, Settings } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";

const SendPage: React.FC = () => {
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [selectionType, setSelectionType] = useState<"file" | "folder" | "text" | "paste" | null>(null);

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
    <div>
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
            <RefreshCcw size={16} color="var(--accent-primary)" style={{ cursor: "pointer" }} />
            <Monitor size={16} style={{ cursor: "pointer", color: "var(--text-secondary)" }} />
            <Heart size={16} style={{ cursor: "pointer", color: "var(--text-secondary)" }} />
            <Settings size={16} style={{ cursor: "pointer", color: "var(--text-secondary)" }} />
          </div>
        </div>
        
        {/* Discovered Device Card Example */}
        <div style={{ 
          backgroundColor: "var(--bg-card)", 
          padding: "24px", 
          borderRadius: "12px", 
          display: "flex", 
          alignItems: "center", 
          gap: "16px",
          marginTop: "16px"
        }}>
          <Monitor size={40} color="var(--text-secondary)" />
          <div style={{ display: "flex", flexDirection: "column", gap: "8px" }}>
            <div style={{ width: "80px", height: "16px", backgroundColor: "var(--bg-sidebar)", borderRadius: "4px" }}></div>
            <div style={{ width: "120px", height: "16px", backgroundColor: "var(--bg-sidebar)", borderRadius: "4px" }}></div>
          </div>
        </div>

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
