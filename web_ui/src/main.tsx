import { createRoot } from "react-dom/client";
import "./index.css";
import "./kwant-theme.css";
import App from "./App";
import { WebSocketProvider } from "./context/WebSocketContext";
import { ThemeProvider } from "./context/ThemeContext";
import { AuthProvider } from "./context/AuthContext";

createRoot(document.getElementById("root")!).render(
    <ThemeProvider>
        <AuthProvider>
            <WebSocketProvider>
                <App />
            </WebSocketProvider>
        </AuthProvider>
    </ThemeProvider>
);
