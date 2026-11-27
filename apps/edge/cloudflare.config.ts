// vox.fail's Worker (src/worker.ts): in front of every request, passing it on to the server
// through the tunnel, and showing the offline page when the server can't be reached.
// Deploy: bun run build, then cf deploy (cf runs on Node, not Bun). See README.md.
import { bindings, defineConfig, triggers } from "cf/config";

export default defineConfig({
	worker: {
		name: "vox-edge",
		compatibilityDate: "2026-11-01",
		entrypoint: "src/worker.ts",
		workersDev: false,
		previewUrls: false,
		observability: {
			enabled: true,
		},
		assets: {
			runWorkerFirst: true,
		},
		triggers: [
			triggers.fetch({
				pattern: "vox.fail/*",
				zone: "vox.fail",
			}),
		],
		env: {
			ASSETS: bindings.assets(),
		},
	},
});
