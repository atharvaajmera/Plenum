import React from "react";

const SettingsPage: React.FC = () => {
  return (
    <div className="settings-container">
      <h1 className="settings-title">Settings</h1>

      <div className="settings-card">
        <h3 style={{ padding: "16px 24px", fontSize: "14px", fontWeight: 600 }}>General</h3>
        <div className="settings-row">
          <span className="settings-label">Theme</span>
          <div className="pill-select">System ▼</div>
        </div>
        <div className="settings-row">
          <span className="settings-label">Color</span>
          <div className="pill-select">Aether ▼</div>
        </div>
        <div className="settings-row">
          <span className="settings-label">Language</span>
          <div className="pill-select">System ▼</div>
        </div>
        <div className="settings-row">
          <span className="settings-label">Minimize to the System Tray/Menu Bar when closing</span>
          <div className="toggle"><div className="toggle-knob"></div></div>
        </div>
        <div className="settings-row">
          <span className="settings-label">Autostart after login</span>
          <div className="toggle"><div className="toggle-knob"></div></div>
        </div>
        <div className="settings-row">
          <span className="settings-label">Animations</span>
          <div className="toggle on"><div className="toggle-knob"></div></div>
        </div>
      </div>

      <div className="settings-card">
        <h3 style={{ padding: "16px 24px", fontSize: "14px", fontWeight: 600 }}>Receive</h3>
        <div className="settings-row">
          <span className="settings-label">Quick Save</span>
          <div className="toggle"><div className="toggle-knob"></div></div>
        </div>
        <div className="settings-row">
          <span className="settings-label">Quick Save for "Favorites"</span>
          <div className="toggle"><div className="toggle-knob"></div></div>
        </div>
        <div className="settings-row">
          <span className="settings-label">Require PIN</span>
          <div className="toggle"><div className="toggle-knob"></div></div>
        </div>
      </div>
    </div>
  );
};

export default SettingsPage;
