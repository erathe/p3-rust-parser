import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig, loadEnv } from 'vite';

export default defineConfig(({ mode }) => {
	const env = loadEnv(mode, '.', '');
	const apiProxyTarget = env.VITE_API_PROXY_TARGET || 'http://localhost:3001';
	const wsProxyTarget = env.VITE_WS_PROXY_TARGET || 'ws://localhost:3001';

	return {
		plugins: [tailwindcss(), sveltekit()],
		server: {
			proxy: {
				'/api': apiProxyTarget,
				'/ws': {
					target: wsProxyTarget,
					ws: true
				}
			}
		}
	};
});
