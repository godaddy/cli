import pino from "pino";

// Global debug state
let isDebugEnabled = false;

// Configure logger based on environment and debug flag
const createLogger = () => {
	const isDev = process.env.NODE_ENV === "development";
	const level = isDebugEnabled ? "debug" : "info";

	const redact = {
		paths: [
			"headers.Authorization",
			"headers.authorization",
			"body.headers.Authorization",
			"body.headers.authorization",
		],
		censor: "[REDACTED]",
	};

	if (isDev || isDebugEnabled) {
		return pino({
			level,
			redact,
			transport: {
				target: "pino-pretty",
				options: {
					colorize: true,
					translateTime: "HH:MM:ss",
					ignore: "pid,hostname",
				},
			},
		});
	}

	return pino({
		level,
		redact,
	});
};

let logger = createLogger();

export const setDebugMode = (enabled: boolean) => {
	isDebugEnabled = enabled;
	logger = createLogger();
};

export const getLogger = () => logger;

// HTTP request logging utilities
export const logHttpRequest = (options: {
	method: string;
	url: string;
	headers?: Record<string, string>;
	body?: unknown;
}) => {
	if (isDebugEnabled) {
		logger.debug(
			{
				type: "http_request",
				method: options.method,
				url: options.url,
				headers: options.headers,
				body: options.body,
			},
			`→ ${options.method} ${options.url}`,
		);
	}
};

export const logHttpResponse = (options: {
	method: string;
	url: string;
	status: number;
	statusText?: string;
	headers?: Record<string, string>;
	body?: unknown;
	duration?: number;
}) => {
	if (isDebugEnabled) {
		logger.debug(
			{
				type: "http_response",
				method: options.method,
				url: options.url,
				status: options.status,
				statusText: options.statusText,
				headers: options.headers,
				body: options.body,
				duration: options.duration,
			},
			`← ${options.status} ${options.method} ${options.url} ${
				options.duration ? `(${options.duration}ms)` : ""
			}`,
		);
	}
};
