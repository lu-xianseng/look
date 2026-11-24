import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    // 固定 IPv4：不指定 host 时 Node 22 会只绑 ::1，部分机器上 WebView2 走 localhost→127.0.0.1
    // 解析失败，dev 版 exe 就会白屏、看起来“启动没反应”。
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "chrome110",
    minify: "esbuild",
    sourcemap: false,
  },
});
