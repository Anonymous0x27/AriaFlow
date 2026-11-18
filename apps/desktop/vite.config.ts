import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// Tauri 2 expects a fixed dev port and relative asset base.
export default defineConfig({
  plugins: [vue()],
  base: "./",
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: "127.0.0.1",
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    target: "es2021",
    outDir: "dist",
    emptyOutDir: true,
  },
});
