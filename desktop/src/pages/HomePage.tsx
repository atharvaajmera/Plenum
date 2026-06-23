import React, { useState, useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { Send, Wifi, Settings } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";

const HomePage: React.FC = () => {
  const navigate = useNavigate();
  const [deviceName, setDeviceName] = useState<string>("Loading...");

  useEffect(() => {
    invoke<string>("get_device_name")
      .then((name) => setDeviceName(name))
      .catch(console.error);
  }, []);

  return (
    <div className="home-container">
      <div className="ring-wrapper">
        <div className="segmented-ring"></div>
        <div className="core-circle"></div>
      </div>
      
      <h1 className="device-name">{deviceName}</h1>
      <div className="device-id">#A3 #B9</div>

      <div className="nav-buttons-container">
        <div className="nav-buttons-row">
          <button className="big-nav-btn" onClick={() => navigate("/send")}>
            <Send size={24} />
            <span>Send</span>
          </button>
          <button className="big-nav-btn" onClick={() => navigate("/receive")}>
            <Wifi size={24} />
            <span>Receive</span>
          </button>
        </div>
        <button className="big-nav-btn" style={{ margin: "0 auto", width: "100%" }} onClick={() => navigate("/settings")}>
          <Settings size={24} />
          <span>Settings</span>
        </button>
      </div>
    </div>
  );
};

export default HomePage;
