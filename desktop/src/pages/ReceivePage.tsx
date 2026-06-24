import React, { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

const ReceivePage: React.FC = () => {
  const [deviceName, setDeviceName] = useState<string>("Loading...");
  const [localIp, setLocalIp] = useState<string>("");
  const [username, setUsername] = useState<string>("");

  useEffect(() => {
    invoke<string>("get_device_name").then(setDeviceName).catch(console.error);
    invoke<string>("get_local_ip").then(setLocalIp).catch(console.error);
    invoke<string>("get_username").then(setUsername).catch(console.error);
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
      </div>


    </div>
  );
};

export default ReceivePage;
