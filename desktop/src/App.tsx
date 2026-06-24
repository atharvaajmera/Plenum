
import { BrowserRouter, Routes, Route } from "react-router-dom";
import Layout from "./components/Layout";
import HomePage from "./pages/HomePage";
import ReceivePage from "./pages/ReceivePage";
import SendPage from "./pages/SendPage";
import SettingsPage from "./pages/SettingsPage";
import { SettingsProvider } from "./context/SettingsContext";
import "./styles/index.css";

function App() {
  return (
    <SettingsProvider>
      <BrowserRouter>
        <Routes>
          <Route path="/" element={<Layout />}>
            <Route index element={<HomePage />} />
            <Route path="receive" element={<ReceivePage />} />
            <Route path="send" element={<SendPage />} />
            <Route path="settings" element={<SettingsPage />} />
          </Route>
        </Routes>
      </BrowserRouter>
    </SettingsProvider>
  );
}

export default App;
