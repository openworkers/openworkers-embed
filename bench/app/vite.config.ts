import openworkers from '@openworkers/adapter-sveltekit';
import node from '@sveltejs/adapter-node';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

// The same app for every server: `build` for Embed, `build-node` for Node and Bun
const adapter =
  process.env.ADAPTER === 'node' ? node({ out: 'build-node' }) : openworkers({ outDir: 'build' });

export default defineConfig({
  plugins: [sveltekit({ adapter })]
});
