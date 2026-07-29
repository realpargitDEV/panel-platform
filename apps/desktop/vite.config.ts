import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  plugins: [react(), tailwindcss()],
  // Tauri serves the dev build from a fixed port and fails loudly rather than
  // silently picking another one, which would leave the window blank.
  server: { port: 5173, strictPort: true },
  build: { outDir: 'dist', emptyOutDir: true, target: 'chrome105' },
  clearScreen: false,
});
