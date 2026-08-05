import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

export default defineConfig({
  plugins: [vue()],
  build: {
    lib: {
      entry: {
        index: "src/index.ts",
        tauri: "src/tauri.ts",
      },
      formats: ["es"],
      fileName: (_format, entryName) => `${entryName}.js`,
    },
    rollupOptions: {
      external: [
        "vue",
        "naive-ui",
        "@vicons/fa",
        "@vicons/fluent",
        "@tauri-apps/api/core",
        "@tauri-apps/plugin-dialog",
        "@tauri-apps/plugin-opener",
      ],
      output: {
        assetFileNames: (assetInfo) => {
          if (assetInfo.name?.endsWith(".css")) return "style.css";
          return "assets/[name]-[hash][extname]";
        },
      },
    },
  },
});
