import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri expects a fixed port. Bound to 127.0.0.1 explicitly: Vite otherwise binds only ::1 on this
// machine, and `tauri dev` waits forever probing 127.0.0.1.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1430,
    strictPort: true,
    host: "127.0.0.1",
    watch: { ignored: ["**/src-tauri/**"] },
  },
});
