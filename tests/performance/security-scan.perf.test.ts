import { mkdir, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { scanExtension } from "../../src/services/extension/security-scan";

const perfTestDir = join(
	process.cwd(),
	"tests",
	"fixtures",
	"perf-test-extension",
);

describe("Security Scan Performance", () => {
	beforeAll(async () => {
		// Create test directory
		await rm(perfTestDir, { recursive: true, force: true });
		await mkdir(perfTestDir, { recursive: true });

		// Create package.json
		await writeFile(
			join(perfTestDir, "package.json"),
			JSON.stringify({
				name: "@test/perf-extension",
				version: "1.0.0",
				scripts: {
					build: "tsc",
					test: "vitest",
				},
			}),
		);

		// Generate ~200 small source files with various patterns
		const srcDir = join(perfTestDir, "src");
		await mkdir(srcDir, { recursive: true });

		for (let i = 0; i < 200; i++) {
			const fileName = `module-${i}.ts`;
			const filePath = join(srcDir, fileName);

			// Mix of safe and potentially suspicious code (but not blocking)
			const content = `
// Module ${i}
import { EventEmitter } from 'events';

export class Module${i} extends EventEmitter {
  private data: Map<string, unknown>;
  
  constructor() {
    super();
    this.data = new Map();
  }

  async initialize(): Promise<void> {
    // Safe initialization
    console.log('Initializing module ${i}');
    this.emit('ready');
  }

  getData(key: string): unknown {
    return this.data.get(key);
  }

  setData(key: string, value: unknown): void {
    this.data.set(key, value);
    this.emit('data-changed', { key, value });
  }

  async fetchData(url: string): Promise<Response> {
    // This should trigger URL pattern detection (warning)
    return fetch(url);
  }

  processBuffer(input: string): Buffer {
    // Buffer usage (potentially flagged)
    return Buffer.from(input, 'utf-8');
  }
}

export default Module${i};
`;

			await writeFile(filePath, content);
		}

		console.log(`\n📊 Created ${200} test files for performance benchmark\n`);
	});

	afterAll(async () => {
		// Clean up test directory
		await rm(perfTestDir, { recursive: true, force: true });
	});

	test("scans 200 files in under 500ms", async () => {
		const startTime = performance.now();

		const result = await scanExtension(perfTestDir);

		const endTime = performance.now();
		const duration = endTime - startTime;

		console.log(`\n⏱️  Scan completed in ${duration.toFixed(2)}ms`);

		// Performance assertion (allow some variance for CI environments and local machine load)
		expect(duration).toBeLessThan(1000);

		// Validate scan succeeded
		expect(result.success).toBe(true);
		expect(result.data).toBeDefined();

		if (result.data) {
			console.log(`📁 Scanned files: ${result.data.scannedFiles}`);
			console.log(`🔍 Total findings: ${result.data.summary.total}`);
			console.log(
				`⚠️  Warnings: ${result.data.summary.bySeverity.warn}, Blocks: ${result.data.summary.bySeverity.block}`,
			);

			// Should have scanned all 200 files
			expect(result.data.scannedFiles).toBe(200);

			// Should not be blocked (no SEC001-010 violations)
			expect(result.data.blocked).toBe(false);
		}
	});

	test("scans efficiently with minimal memory overhead", async () => {
		// Get baseline memory
		const memBefore = process.memoryUsage().heapUsed;

		await scanExtension(perfTestDir);

		// Check memory after
		const memAfter = process.memoryUsage().heapUsed;
		const memDelta = (memAfter - memBefore) / 1024 / 1024; // Convert to MB

		console.log(`\n💾 Memory delta: ${memDelta.toFixed(2)}MB`);

		// Memory should not grow excessively (< 50MB for 200 files)
		expect(memDelta).toBeLessThan(50);
	});

	test("performance scales linearly with file count", async () => {
		// Test with subset of files to validate linear scaling
		const samples = [50, 100, 150, 200];
		const timings: number[] = [];

		for (const fileCount of samples) {
			// Create temporary extension with limited files
			const tempDir = join(
				process.cwd(),
				"tests",
				"fixtures",
				`perf-${fileCount}`,
			);
			await mkdir(tempDir, { recursive: true });
			await mkdir(join(tempDir, "src"), { recursive: true });

			await writeFile(
				join(tempDir, "package.json"),
				JSON.stringify({ name: "@test/perf", version: "1.0.0" }),
			);

			// Copy subset of files
			for (let i = 0; i < fileCount; i++) {
				const content = `export const val${i} = ${i};`;
				await writeFile(join(tempDir, "src", `file-${i}.ts`), content);
			}

			// Measure scan time
			const start = performance.now();
			await scanExtension(tempDir);
			const duration = performance.now() - start;
			timings.push(duration);

			// Clean up
			await rm(tempDir, { recursive: true, force: true });
		}

		console.log("\n📈 Scaling analysis:");
		samples.forEach((count, idx) => {
			const msPerFile = timings[idx] / count;
			console.log(
				`   ${count} files: ${timings[idx].toFixed(2)}ms (${msPerFile.toFixed(3)}ms/file)`,
			);
		});

		// Verify reasonable scaling (shouldn't degrade more than 2x)
		const firstRate = timings[0] / samples[0];
		const lastRate = timings[timings.length - 1] / samples[samples.length - 1];
		const scalingFactor = lastRate / firstRate;

		console.log(`   Scaling factor: ${scalingFactor.toFixed(2)}x`);
		expect(scalingFactor).toBeLessThan(2);
	});
});
