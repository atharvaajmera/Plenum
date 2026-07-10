import React, { useState, useEffect } from "react";
import { useSettings } from "../context/SettingsContext";

const SettingsPage: React.FC = () => {
  const { settings, updateSettings } = useSettings();

  // Local state for the UI before saving
  const [generalSettings, setGeneralSettings] = useState(settings.general);
  const [receiveSettings, setReceiveSettings] = useState(settings.receive);
  const [internetSettings, setInternetSettings] = useState(settings.internet);
  const [themeIndex, setThemeIndex] = useState(settings.themeIndex);
  const [colorIndex, setColorIndex] = useState(settings.colorIndex);
  const [showSaved, setShowSaved] = useState(false);

  const themes = ["System", "Dark", "Light"];
  const colors = ["Plenum", "Ocean", "Forest"];

  const toggleGeneral = (key: keyof typeof generalSettings) => {
    setGeneralSettings(prev => ({ ...prev, [key]: !prev[key] }));
  };

  const toggleReceive = (key: keyof typeof receiveSettings) => {
    setReceiveSettings(prev => ({ ...prev, [key]: !prev[key] }));
  };

  const updateIceServer = (index: number, field: "urls" | "username" | "credential", value: string) => {
    setInternetSettings(prev => ({
      ...prev,
      iceServers: prev.iceServers.map((s, i) => i === index ? { ...s, [field]: value } : s),
    }));
  };

  const removeIceServer = (index: number) => {
    setInternetSettings(prev => ({
      ...prev,
      iceServers: prev.iceServers.filter((_, i) => i !== index),
    }));
  };

  const addIceServer = () => {
    setInternetSettings(prev => ({
      ...prev,
      iceServers: [...prev.iceServers, { urls: "" }],
    }));
  };

  const Toggle = ({ on, onClick }: { on: boolean; onClick: () => void }) => (
    <div className={`toggle ${on ? "on" : ""}`} onClick={onClick}>
      <div className="toggle-knob"></div>
    </div>
  );

  useEffect(() => {
    // If settings change in context, we sync them (though typically not needed if this is the only modifier)
  }, [settings]);

  const handleSaveClick = async () => {
    updateSettings({
      themeIndex,
      colorIndex,
      general: generalSettings,
      receive: receiveSettings,
      internet: internetSettings,
    });
    // Slight hack: we save the object directly to localstorage here so it's synchronous
    const newSettings = {
      themeIndex,
      colorIndex,
      general: generalSettings,
      receive: receiveSettings,
      internet: internetSettings,
    };
    localStorage.setItem("plenum-settings", JSON.stringify(newSettings));
    
    // Call the standalone apply function from context
    import("../context/SettingsContext").then(({ applyThemeToDom }) => {
        applyThemeToDom(newSettings);
    });

    setShowSaved(true);
    setTimeout(() => setShowSaved(false), 3000);
  };

  return (
    <div className="settings-container">
      <h1 className="settings-title">Settings</h1>

      <div className="settings-card">
        <h3 style={{ padding: "16px 24px", fontSize: "14px", fontWeight: 600 }}>General</h3>
        <div className="settings-row">
          <span className="settings-label">Theme</span>
          <select className="pill-select" value={themeIndex} onChange={(e) => setThemeIndex(Number(e.target.value))}>
            {themes.map((t, i) => <option key={i} value={i}>{t}</option>)}
          </select>
        </div>
        <div className="settings-row">
          <span className="settings-label">Color</span>
          <select className="pill-select" value={colorIndex} onChange={(e) => setColorIndex(Number(e.target.value))}>
            {colors.map((c, i) => <option key={i} value={i}>{c}</option>)}
          </select>
        </div>
        <div className="settings-row">
          <span className="settings-label">Minimize to the System Tray/Menu Bar when closing</span>
          <Toggle on={generalSettings.minimizeToTray} onClick={() => toggleGeneral("minimizeToTray")} />
        </div>
        <div className="settings-row">
          <span className="settings-label">Autostart after login</span>
          <Toggle on={generalSettings.autostart} onClick={() => toggleGeneral("autostart")} />
        </div>
      </div>

      <div className="settings-card">
        <h3 style={{ padding: "16px 24px", fontSize: "14px", fontWeight: 600 }}>Receive</h3>
        <div className="settings-row">
          <span className="settings-label">Quick Save</span>
          <Toggle on={receiveSettings.quickSave} onClick={() => toggleReceive("quickSave")} />
        </div>
        <div className="settings-row">
          <span className="settings-label">Quick Save for "Favorites"</span>
          <Toggle on={receiveSettings.quickSaveFavorites} onClick={() => toggleReceive("quickSaveFavorites")} />
        </div>
        <div className="settings-row">
          <span className="settings-label">Require PIN</span>
          <Toggle on={receiveSettings.requirePin} onClick={() => toggleReceive("requirePin")} />
        </div>
      </div>

      <div className="settings-card">
        <h3 style={{ padding: "16px 24px", fontSize: "14px", fontWeight: 600 }}>Internet Transfers</h3>
        <div className="settings-row">
          <span className="settings-label">Relay Server URL</span>
          <input
            type="text"
            value={internetSettings.relayServerUrl}
            onChange={(e) => setInternetSettings(prev => ({ ...prev, relayServerUrl: e.target.value }))}
            placeholder="wss://your-relay.example.com/ws"
            style={{ flex: 1, marginLeft: "16px", padding: "8px 12px", borderRadius: "8px", border: "1px solid var(--border-color)", backgroundColor: "var(--bg-sidebar)", color: "var(--text-primary)", outline: "none", fontSize: "14px" }}
          />
        </div>

        <div style={{ padding: "8px 24px 16px" }}>
          <span className="settings-label">ICE Servers</span>
          <div style={{ display: "flex", flexDirection: "column", gap: "12px", marginTop: "12px" }}>
            {internetSettings.iceServers.map((server, i) => (
              <div key={i} style={{ display: "flex", gap: "8px", alignItems: "center" }}>
                <input
                  type="text"
                  value={server.urls}
                  onChange={(e) => updateIceServer(i, "urls", e.target.value)}
                  placeholder="stun:stun.l.google.com:19302 or turn:host:port"
                  style={{ flex: 2, padding: "8px 12px", borderRadius: "8px", border: "1px solid var(--border-color)", backgroundColor: "var(--bg-sidebar)", color: "var(--text-primary)", outline: "none", fontSize: "13px" }}
                />
                <input
                  type="text"
                  value={server.username ?? ""}
                  onChange={(e) => updateIceServer(i, "username", e.target.value)}
                  placeholder="username (optional)"
                  style={{ flex: 1, padding: "8px 12px", borderRadius: "8px", border: "1px solid var(--border-color)", backgroundColor: "var(--bg-sidebar)", color: "var(--text-primary)", outline: "none", fontSize: "13px" }}
                />
                <input
                  type="password"
                  value={server.credential ?? ""}
                  onChange={(e) => updateIceServer(i, "credential", e.target.value)}
                  placeholder="credential (optional)"
                  style={{ flex: 1, padding: "8px 12px", borderRadius: "8px", border: "1px solid var(--border-color)", backgroundColor: "var(--bg-sidebar)", color: "var(--text-primary)", outline: "none", fontSize: "13px" }}
                />
                <button
                  onClick={() => removeIceServer(i)}
                  style={{ padding: "8px 12px", borderRadius: "8px", border: "1px solid var(--border-color)", backgroundColor: "transparent", color: "var(--text-secondary)", cursor: "pointer" }}
                >
                  Remove
                </button>
              </div>
            ))}
          </div>
          <button
            onClick={addIceServer}
            style={{ marginTop: "12px", padding: "8px 16px", borderRadius: "8px", border: "1px solid var(--accent-primary)", backgroundColor: "transparent", color: "var(--accent-primary)", cursor: "pointer", fontWeight: 500 }}
          >
            Add ICE server
          </button>
          <p style={{ fontSize: "12px", color: "var(--text-secondary)", marginTop: "12px" }}>
            STUN alone works for some NAT types; add a TURN server for symmetric NAT (see relay-server deployment docs).
          </p>
        </div>
      </div>

      <div style={{ display: "flex", justifyContent: "center", marginTop: "32px", paddingBottom: "32px", position: "relative" }}>
        {showSaved && <div style={{ position: "absolute", top: "-30px", color: "var(--accent-primary)", fontWeight: 500 }}>Settings saved!</div>}
        <button className="big-nav-btn" onClick={handleSaveClick} style={{ padding: "12px 48px", backgroundColor: "var(--accent-primary)", color: "var(--bg-app)", border: "none" }}>
          Save Settings
        </button>
      </div>
    </div>
  );
};

export default SettingsPage;
