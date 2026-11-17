import { readFileSync } from "node:fs";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The version shown in the app, from package.json (bumped with Cargo.toml and tauri.conf.json at
// every release), so the sidebar never needs a round trip to the backend to say it.
const { version } = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8"));

// Tauri expects a fixed port. Bound to 127.0.0.1 explicitly: Vite otherwise binds only ::1 on this
// machine, and `tauri dev` waits forever probing 127.0.0.1.
export default defineConfig({
  plugins: [react()],
  define: { __APP_VERSION__: JSON.stringify(version) },
  clearScreen: false,
  server: {
    port: 1430,
    strictPort: true,
    host: "127.0.0.1",
    watch: { ignored: ["**/src-tauri/**"] },
  },
});
