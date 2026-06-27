import React, { createContext, useContext, useState, useEffect } from "react";

export interface GeneralSettings {
  minimizeToTray: boolean;
  autostart: boolean;
}

export interface ReceiveSettings {
  quickSave: boolean;
  quickSaveFavorites: boolean;
  requirePin: boolean;
}

export interface SettingsState {
  themeIndex: number; // 0=System, 1=Dark, 2=Light
  colorIndex: number; // 0=Plenum, 1=Ocean, 2=Forest
  general: GeneralSettings;
  receive: ReceiveSettings;
}

export interface SettingsContextType {
  settings: SettingsState;
  updateSettings: (newSettings: Partial<SettingsState>) => void;
  saveSettings: () => void;
}

const defaultSettings: SettingsState = {
  themeIndex: 0,
  colorIndex: 0,
  general: {
    minimizeToTray: false,
    autostart: false,
  },
  receive: {
    quickSave: false,
    quickSaveFavorites: false,
    requirePin: false,
  },
};

const SettingsContext = createContext<SettingsContextType | null>(null);

export const useSettings = () => {
  const context = useContext(SettingsContext);
  if (!context) {
    throw new Error("useSettings must be used within a SettingsProvider");
  }
  return context;
};

export const applyThemeToDom = (settings: SettingsState) => {
  const doc = document.documentElement;

  // Apply Theme
  let isDark = true;
  if (settings.themeIndex === 2) isDark = false; // Light
  else if (settings.themeIndex === 0) { // System
    isDark = window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches;
  }
  doc.setAttribute("data-theme", isDark ? "dark" : "light");

  // Apply Color
  if (settings.colorIndex === 1) doc.setAttribute("data-color", "ocean");
  else if (settings.colorIndex === 2) doc.setAttribute("data-color", "forest");
  else doc.removeAttribute("data-color"); // Default Plenum
};

export const SettingsProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [settings, setSettings] = useState<SettingsState>(() => {
    try {
      const stored = localStorage.getItem("plenum-settings");
      return stored ? JSON.parse(stored) : defaultSettings;
    } catch {
      return defaultSettings;
    }
  });

  // Apply to DOM on initial mount
  useEffect(() => {
    applyThemeToDom(settings);
  }, []);

  const updateSettings = (newSettings: Partial<SettingsState>) => {
    setSettings((prev) => ({ ...prev, ...newSettings }));
  };

  const saveSettings = () => {
    localStorage.setItem("plenum-settings", JSON.stringify(settings));
    applyThemeToDom(settings);
  };

  return (
    <SettingsContext.Provider value={{ settings, updateSettings, saveSettings }}>
      {children}
    </SettingsContext.Provider>
  );
};
