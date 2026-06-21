import React from "react";
import { useNavigate } from "react-router-dom";
import { Send, Wifi, Settings } from "lucide-react";

const HomePage: React.FC = () => {
  const navigate = useNavigate();

  return (
    <div className="home-container">
      <div className="ring-wrapper">
        <div className="segmented-ring"></div>
        <div className="core-circle"></div>
      </div>
      
      <h1 className="device-name">Quantum Leopard</h1>
      <div className="device-id">#A3 #B9</div>

      <div className="nav-buttons-container">
        <button className="big-nav-btn" onClick={() => navigate("/send")}>
          <Send size={24} />
          <span>Send</span>
        </button>
        <button className="big-nav-btn" onClick={() => navigate("/receive")}>
          <Wifi size={24} />
          <span>Receive</span>
        </button>
        <button className="big-nav-btn" onClick={() => navigate("/settings")}>
          <Settings size={24} />
          <span>Settings</span>
        </button>
      </div>
    </div>
  );
};

export default HomePage;
