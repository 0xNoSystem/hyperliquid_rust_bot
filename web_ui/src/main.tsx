import { createRoot } from "react-dom/client";
import "./index.css";
import App from "./App";
import { WebSocketProvider } from "./context/WebSocketContext";

createRoot(document.getElementById("root")!).render(
    <WebSocketProvider>
        <App />
    </WebSocketProvider>
);
