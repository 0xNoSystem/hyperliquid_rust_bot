import { createRoot } from "react-dom/client";
import "./index.css";
import App from "./App";
import { WebSocketProvider } from "./context/WebSocketContext";
import { ThemeProvider } from "./context/ThemeContext";

createRoot(document.getElementById("root")!).render(
    <ThemeProvider>
        <WebSocketProvider>
            <App />
        </WebSocketProvider>
    </ThemeProvider>
);
