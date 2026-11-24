import { existsSync } from "node:fs";
import { resolve } from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const root = process.cwd();
const input: Record<string, string> = {
  main: resolve(root, "index.html"),
};
if (existsSync(resolve(root, "qa/notch-showcase.html"))) {
  input["qa/notch-showcase"] = resolve(root, "qa/notch-showcase.html");
}
if (existsSync(resolve(root, "qa/reset.html"))) {
  input["qa/reset"] = resolve(root, "qa/reset.html");
}
if (existsSync(resolve(root, "qa/planner.html"))) {
  input["qa/planner"] = resolve(root, "qa/planner.html");
}
if (existsSync(resolve(root, "qa/workspace-showcase.html"))) {
  input["qa/workspace-showcase"] = resolve(root, "qa/workspace-showcase.html");
}
if (existsSync(resolve(root, "qa/areapick.html"))) {
  input["qa/areapick"] = resolve(root, "qa/areapick.html");
}

// Tauri expects a fixed dev port and `dist/index.html` as the primary entry.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    target: "es2022",
    minify: true,
    sourcemap: false,
    rollupOptions: { input },
  },
});
