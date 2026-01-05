import path from "node:path";
import { defineConfig } from "vitest/config";

export default defineConfig({
	resolve: {
		alias: {
			"@": path.resolve(__dirname, "./src"),
			"@core": path.resolve(__dirname, "./src/core"),
			"@cli": path.resolve(__dirname, "./src/cli"),
			"@tui": path.resolve(__dirname, "./src/tui"),
			"@shared": path.resolve(__dirname, "./src/shared"),
		},
	},
	test: {
		environment: "node",
		setupFiles: ["./tests/setup/global-setup.ts"],
		globals: true,
		testTimeout: 10000,
		coverage: {
			provider: "v8",
			reporter: ["text", "json", "html"],
			exclude: ["node_modules/", "tests/", "dist/", "**/*.d.ts"],
		},
	},
});
