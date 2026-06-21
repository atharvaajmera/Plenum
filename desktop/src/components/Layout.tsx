import React from "react";
import { Outlet, useNavigate, useLocation } from "react-router-dom";
import { ArrowLeft } from "lucide-react";
import "../styles/index.css";

const Layout: React.FC = () => {
  const navigate = useNavigate();
  const location = useLocation();

  return (
    <div className="app-layout">
      {/* Top Navigation Bar */}
      <header className="top-nav">
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
      </header>

      {/* Main Content Area */}
      <main className="main-content">
        <Outlet />
      </main>
    </div>
  );
};

export default Layout;
