import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { villagePorts } from '../tools/config.js';

const { webPort, apiOrigin } = villagePorts(process.env);

export default defineConfig({
  root: import.meta.dirname,
  plugins: [react()],
  server: {
    host: '127.0.0.1',
    port: webPort,
    strictPort: true,
    proxy: {
      '/api/village': process.env.VILLAGE_API_ORIGIN ?? apiOrigin,
    },
  },
  build: { outDir: 'dist', emptyOutDir: true },
});
