import { Command } from "commander";
import {
	type HttpMethod,
	apiRequest,
	parseFields,
	parseHeaders,
	readBodyFromFile,
} from "../../core/api";

const VALID_METHODS: HttpMethod[] = ["GET", "POST", "PUT", "PATCH", "DELETE"];

/**
 * Extract a value from an object using a simple JSON path
 * Supports: .key, .key.nested, .key[0], .key[0].nested
 */
function extractPath(obj: unknown, path: string): unknown {
	if (!path || path === ".") {
		return obj;
	}

	// Remove leading dot if present
	const normalizedPath = path.startsWith(".") ? path.slice(1) : path;
	if (!normalizedPath) {
		return obj;
	}

	// Parse path into segments
	const segments: (string | number)[] = [];
	const regex = /([\w-]+)|\[(\d+)\]/g;
	let match: RegExpExecArray | null;

	while ((match = regex.exec(normalizedPath)) !== null) {
		if (match[1] !== undefined) {
			segments.push(match[1]);
		} else if (match[2] !== undefined) {
			segments.push(Number.parseInt(match[2], 10));
		}
	}

	// Traverse the object
	let current: unknown = obj;
	for (const segment of segments) {
		if (current === null || current === undefined) {
			return undefined;
		}
		if (typeof segment === "number") {
			if (!Array.isArray(current)) {
				throw new Error(`Cannot index non-array with [${segment}]`);
			}
			current = current[segment];
		} else {
			if (typeof current !== "object") {
				throw new Error(`Cannot access property "${segment}" on non-object`);
			}
			current = (current as Record<string, unknown>)[segment];
		}
	}

	return current;
}

export function createApiCommand(): Command {
	const api = new Command("api")
		.description("Make authenticated requests to the GoDaddy API")
		.argument("<endpoint>", "API endpoint (e.g., /v1/domains)")
		.option(
			"-X, --method <method>",
			"HTTP method (GET, POST, PUT, PATCH, DELETE)",
			"GET",
		)
		.option(
			"-f, --field <key=value...>",
			"Add field to request body (can be repeated)",
		)
		.option("-F, --file <path>", "Read request body from JSON file")
		.option("-H, --header <header...>", "Add custom header (can be repeated)")
		.option(
			"-q, --query <path>",
			"Extract value at JSON path (e.g., .status, .data[0].name)",
		)
		.option("-i, --include", "Include response headers in output")
		.action(async (endpoint: string, options) => {
			// Validate HTTP method
			const method = options.method.toUpperCase() as HttpMethod;
			if (!VALID_METHODS.includes(method)) {
				console.error(
					`Invalid HTTP method: ${options.method}. Must be one of: ${VALID_METHODS.join(", ")}`,
				);
				process.exit(1);
			}

			// Parse fields
			let fields: Record<string, string> | undefined;
			if (options.field) {
				const fieldArray = Array.isArray(options.field)
					? options.field
					: [options.field];
				const fieldsResult = parseFields(fieldArray);
				if (!fieldsResult.success) {
					console.error(
						fieldsResult.error?.userMessage || "Invalid field format",
					);
					process.exit(1);
				}
				fields = fieldsResult.data;
			}

			// Read body from file
			let body: string | undefined;
			if (options.file) {
				const bodyResult = readBodyFromFile(options.file);
				if (!bodyResult.success) {
					console.error(bodyResult.error?.userMessage || "Failed to read file");
					process.exit(1);
				}
				body = bodyResult.data;
			}

			// Parse headers
			let headers: Record<string, string> | undefined;
			if (options.header) {
				const headerArray = Array.isArray(options.header)
					? options.header
					: [options.header];
				const headersResult = parseHeaders(headerArray);
				if (!headersResult.success) {
					console.error(
						headersResult.error?.userMessage || "Invalid header format",
					);
					process.exit(1);
				}
				headers = headersResult.data;
			}

			// Get debug flag from parent command
			const parentOptions = api.parent?.opts() || {};
			const debug = parentOptions.debug || false;

			// Make the request
			const result = await apiRequest({
				endpoint,
				method,
				fields,
				body,
				headers,
				debug,
			});

			if (!result.success) {
				console.error(result.error?.userMessage || "API request failed");
				process.exit(1);
			}

			const response = result.data;
			if (!response) {
				console.error("No response data");
				process.exit(1);
			}

			// Include headers if requested
			if (options.include) {
				console.log(`HTTP/1.1 ${response.status} ${response.statusText}`);
				for (const [key, value] of Object.entries(response.headers)) {
					console.log(`${key}: ${value}`);
				}
				console.log("");
			}

			// Apply query filter if specified
			let output = response.data;
			if (options.query && output !== undefined) {
				try {
					output = extractPath(output, options.query);
				} catch (err) {
					const message = err instanceof Error ? err.message : String(err);
					console.error(`Query error: ${message}`);
					process.exit(1);
				}
			}

			if (output !== undefined) {
				// Output JSON (pretty print objects, raw strings)
				if (typeof output === "string") {
					console.log(output);
				} else {
					console.log(JSON.stringify(output, null, 2));
				}
			}

			process.exit(0);
		});

	return api;
}
