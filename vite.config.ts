import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  build: { target: 'es2022', assetsInlineLimit: 0 },
  server: { strictPort: true, watch: { ignored: ['**/src-tauri/**'] } },
});
