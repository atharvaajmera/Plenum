import React, { useState } from "react";

const ReceivePage: React.FC = () => {

  return (
    <div className="receive-container">
      <div className="ring-wrapper">
        <div className="segmented-ring"></div>
        <div className="core-circle"></div>
      </div>
      
      {/* TODO: Fetch actual hostname and pairing token from Rust core */}
      <h1 className="device-name">Quantum Leopard</h1>
      <div className="device-id">#A3 #B9</div>


    </div>
  );
};

export default ReceivePage;
