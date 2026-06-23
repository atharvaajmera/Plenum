import React, { useState, useEffect } from "react";
import { useSettings } from "../context/SettingsContext";

const SettingsPage: React.FC = () => {
  const { settings, updateSettings, saveSettings } = useSettings();

  // Local state for the UI before saving
  const [generalSettings, setGeneralSettings] = useState(settings.general);
  const [receiveSettings, setReceiveSettings] = useState(settings.receive);
  const [themeIndex, setThemeIndex] = useState(settings.themeIndex);
  const [colorIndex, setColorIndex] = useState(settings.colorIndex);
  const [showSaved, setShowSaved] = useState(false);

  const themes = ["System", "Dark", "Light"];
  const colors = ["Aether", "Ocean", "Forest"];

  const toggleGeneral = (key: keyof typeof generalSettings) => {
    setGeneralSettings(prev => ({ ...prev, [key]: !prev[key] }));
  };

  const toggleReceive = (key: keyof typeof receiveSettings) => {
    setReceiveSettings(prev => ({ ...prev, [key]: !prev[key] }));
  };

  const Toggle = ({ on, onClick }: { on: boolean; onClick: () => void }) => (
    <div className={`toggle ${on ? "on" : ""}`} onClick={onClick}>
      <div className="toggle-knob"></div>
    </div>
  );

  const handleSave = async () => {
    updateSettings({
      themeIndex,
      colorIndex,
      general: generalSettings,
      receive: receiveSettings,
    });
    // Give context a tick to update state before saving (or just directly pass to saveSettings if we implemented it to take args, but useEffect in context handles it, wait no.
    // Actually, saveSettings in context saves the current state. So we need to call updateSettings, then saveSettings.
    // However, setState is async. The best way is to let the user save, wait, and call context.saveSettings()
  };

  useEffect(() => {
    // If settings change in context, we sync them (though typically not needed if this is the only modifier)
  }, [settings]);

  const handleSaveClick = async () => {
    updateSettings({
      themeIndex,
      colorIndex,
      general: generalSettings,
      receive: receiveSettings,
    });
    // Slight hack: we save the object directly to localstorage here so it's synchronous
    const newSettings = {
      themeIndex,
      colorIndex,
      general: generalSettings,
      receive: receiveSettings,
    };
    localStorage.setItem("aether-settings", JSON.stringify(newSettings));
    
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
