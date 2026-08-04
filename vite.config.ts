import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

export default defineConfig({
  base: "./",
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes("node_modules")) return undefined;
          if (
            id.includes("naive-ui")
            || id.includes("css-render")
            || id.includes("@vicons")
            || id.includes("vue")
          ) return "ui";
          if (id.includes("marked") || id.includes("dompurify")) return "markdown";
          if (id.includes("html-to-image")) return "image-export";
          return "vendor";
        },
      },
    },
  },
});
