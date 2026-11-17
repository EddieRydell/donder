import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  build: {
    // The oldest webviews Donder supports: WKWebView on macOS 11 (Safari 14) and WebView2 on Windows.
    target: ["safari14", "chrome105"],
    // Monaco is bundled with the app, which loads it from disk.
    chunkSizeWarningLimit: 5000
  },
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1"
  },
  envPrefix: ["VITE_", "TAURI_"]
});
