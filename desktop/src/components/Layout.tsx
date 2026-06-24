import React, { useState } from "react";
import { Outlet, useNavigate, useLocation } from "react-router-dom";
import { ArrowLeft, Info } from "lucide-react";
import "../styles/index.css";

const Layout: React.FC = () => {
  const navigate = useNavigate();
  const location = useLocation();
  const [showInfo, setShowInfo] = useState(false);

  return (
    <div className="app-layout">
      {/* Top Navigation Bar */}
      <header className="top-nav">
        <div className="nav-left">
          {location.pathname !== "/" && (
            <button 
              className="icon-button" 
              onClick={() => navigate(-1)}
              style={{ marginRight: "8px", color: "var(--text-primary)" }}
            >
              <ArrowLeft size={24} />
            </button>
          )}
          <div className="nav-logo">
            <img src="/aether-logo.png" alt="Aether Logo" />
          </div>
          <span className="nav-title">Aether</span>
        </div>
        
        <div style={{ position: "relative" }}>
          <button className="icon-button" title="Info" onClick={() => setShowInfo(!showInfo)}>
            <Info size={20} />
          </button>
          {showInfo && (
            <div style={{
              position: "absolute",
              top: "100%",
              right: 0,
              marginTop: "8px",
              backgroundColor: "var(--bg-card)",
              border: "1px solid var(--border-color)",
              borderRadius: "8px",
              padding: "16px",
              minWidth: "280px",
              zIndex: 100,
              boxShadow: "0 4px 12px rgba(0,0,0,0.3)",
            }}>
              <div style={{ fontSize: "14px", fontWeight: 600, marginBottom: "12px", color: "var(--text-primary)" }}>How to use Aether?</div>
              <div style={{ fontSize: "13px", color: "var(--text-secondary)", lineHeight: 1.5 }}>
                <strong style={{ color: "var(--text-primary)" }}>To Send:</strong> Select a file or folder, then choose a nearby device to securely transfer it over your local network.
                <br /><br />
                <strong style={{ color: "var(--text-primary)" }}>To Receive:</strong> Stay on the Receive screen to be discoverable. Accept the incoming transfer when prompted.
                <br /> <br />
                <strong>Note:</strong> Ensure that both devices are connected to the same Wi-Fi!
              </div>
            </div>
          )}
        </div>
      </header>

      {/* Main Content Area */}
      <main className="main-content">
        <Outlet />
      </main>
    </div>
  );
};

export default Layout;
