import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';

const geometryModules = ['BufferAttribute', 'BufferGeometry', 'EventDispatcher', 'Layers', 'Object3D'];

export default defineConfig({
  root: 'frontend',
  resolve: {
    alias: [{ find: /^three$/, replacement: fileURLToPath(new URL('./node_modules/three/src/Three.js', import.meta.url)) }],
  },
  plugins: [svelte()],
  build: {
    outDir: '../dist',
    emptyOutDir: true,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes('three/src/math/') || geometryModules.some((name) => id.endsWith(`three/src/core/${name}.js`))
            || id.endsWith('three/src/constants.js') || id.endsWith('three/src/utils.js')) return 'three-geometry';
          if (id.includes('three/')) return 'three-webgl';
        },
      },
    },
  },
  clearScreen: false,
});
