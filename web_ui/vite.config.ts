import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
    plugins: [react(), tailwindcss()],
    server: {
        host: true,
        proxy: {
            "/api": {
                target: "http://localhost:8090",
                changeOrigin: true,
                rewrite: (path) => path.replace(/^\/api/, ""),
            },
            "/ws": { target: "ws://localhost:8090", ws: true },
        },
    },
});

