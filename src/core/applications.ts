import { rm } from "node:fs/promises";
import { resolve } from "node:path";
import { type } from "arktype";
import {
	archiveApplication as archiveAppService,
	createApplication,
	createRelease as createReleaseService,
	disableApplication as disableAppService,
	enableApplication as enableAppService,
	getApplication,
	getApplicationAndLatestRelease,
	updateApplication as updateAppService,
} from "../services/applications";
import {
	type ActionConfig,
	type ConfigExtensionInfo,
	type SubscriptionConfig,
	createConfigFile,
	createEnvFile,
	getConfigFile,
	getExtensionsFromConfig,
} from "../services/config";
import { bundleExtension } from "../services/extension/bundler";
import { getUploadTarget } from "../services/extension/presigned-url";
import { scanBundle, scanExtension } from "../services/extension/security-scan";
import { uploadArtifact } from "../services/extension/upload";
import {
	AuthenticationError,
	type CmdResult,
	ConfigurationError,
	NetworkError,
	ValidationError,
} from "../shared/types";
import { getFromKeychain } from "./auth";
import type { Environment } from "./environment";

// Type definitions for core application functions
export interface ApplicationInfo {
	id: string;
	label: string;
	name: string;
	description: string;
	status: string;
	url: string;
	proxyUrl: string;
	authorizationScopes?: string[];
	releases?: Array<{
		id: string;
		version: string;
		description?: string;
		createdAt: string;
	}>;
}

export interface Application {
	id: string;
	label: string;
	name: string;
	description: string;
	status: string;
	url: string;
	proxyUrl: string;
}

export interface ValidationResult {
	valid: boolean;
	errors: string[];
	warnings: string[];
}

export interface UpdateApplicationInput {
	label?: string;
	description?: string;
	status?: "ACTIVE" | "INACTIVE";
}

export interface CreateApplicationInput {
	name: string;
	description: string;
	url: string;
	proxyUrl: string;
	authorizationScopes: string[];
}

export interface CreatedApplicationInfo {
	id: string;
	clientId: string;
	clientSecret: string;
	name: string;
	description: string;
	status: string;
	url: string;
	proxyUrl: string;
	authorizationScopes: string[];
	secret: string;
	publicKey: string;
}

// Input validation schemas
const updateApplicationInputValidator = type({
	label: "string?",
	description: "string?",
	status: '"ACTIVE" | "INACTIVE"?',
});

const createApplicationInputValidator = type({
	name: "string",
	description: "string",
	url: type.keywords.string.url.root,
	proxyUrl: type.keywords.string.url.root,
	authorizationScopes: type.string.array().moreThanLength(0),
});

/**
 * Initialize/create a new application
 */
export async function applicationInit(
	input: CreateApplicationInput,
	environment?: Environment,
): Promise<CmdResult<CreatedApplicationInfo>> {
	try {
		// Validate input
		const validationResult = createApplicationInputValidator(input);
		if (validationResult instanceof type.errors) {
			return {
				success: false,
				error: new ValidationError(
					validationResult.summary,
					"Invalid application configuration",
				),
			};
		}

		const accessToken = await getFromKeychain("token");
		if (!accessToken) {
			return {
				success: false,
				error: new AuthenticationError(
					"Not authenticated",
					"Please run 'godaddy auth login' first",
				),
			};
		}

		// Call service function with proper format
		const createInput = {
			label: input.name,
			name: input.name,
			description: input.description,
			url: input.url,
			proxyUrl: input.proxyUrl,
			authorizationScopes: input.authorizationScopes,
		};

		const result = await createApplication(createInput, { accessToken });

		if (!result.createApplication) {
			return {
				success: false,
				error: new NetworkError(
					"Failed to create application",
					"Application creation failed - no data returned",
				),
			};
		}

		const app = result.createApplication;
		const createdApp: CreatedApplicationInfo = {
			id: app.id,
			clientId: String(app.clientId || ""),
			clientSecret: String(app.clientSecret || ""),
			name: app.name,
			description: app.description || "",
			status: app.status,
			url: app.url,
			proxyUrl: app.proxyUrl,
			authorizationScopes: app.authorizationScopes || [],
			secret: String(app.secret || ""),
			publicKey: String(app.publicKey || ""),
		};

		// Create config and env files
		try {
			await createConfigFile(
				{
					client_id: createdApp.clientId,
					name: createdApp.name,
					description: createdApp.description,
					url: createdApp.url,
					proxy_url: createdApp.proxyUrl,
					authorization_scopes: createdApp.authorizationScopes,
					version: "0.0.0",
					actions: [],
					subscriptions: { webhook: [] },
				},
				environment,
			);

			await createEnvFile(
				{
					secret: createdApp.secret,
					publicKey: createdApp.publicKey,
					clientId: createdApp.clientId,
					clientSecret: createdApp.clientSecret,
				},
				environment,
			);
		} catch (fileError) {
			// Note the file error but don't fail the entire operation
			console.warn(`Warning: Failed to create config files: ${fileError}`);
		}

		return {
			success: true,
			data: createdApp,
		};
	} catch (error) {
		return {
			success: false,
			error: new NetworkError(
				`Failed to create application: ${error}`,
				error as Error,
			),
		};
	}
}

/**
 * Get application information by name
 */
export async function applicationInfo(
	name?: string,
): Promise<CmdResult<ApplicationInfo>> {
	try {
		if (!name) {
			return {
				success: false,
				error: new ValidationError(
					"Application name is required",
					"Please specify an application name",
				),
			};
		}

		const accessToken = await getFromKeychain("token");
		if (!accessToken) {
			return {
				success: false,
				error: new AuthenticationError(
					"Not authenticated",
					"Please run 'godaddy auth login' first",
				),
			};
		}

		const result = await getApplicationAndLatestRelease(name, { accessToken });

		if (!result.application) {
			return {
				success: false,
				error: new ValidationError(
					`Application '${name}' not found`,
					`Application '${name}' does not exist`,
				),
			};
		}

		const app = result.application;
		const applicationInfo: ApplicationInfo = {
			id: app.id,
			label: app.label,
			name: app.name,
			description: app.description,
			status: app.status,
			url: app.url,
			proxyUrl: app.proxyUrl,
			authorizationScopes: app.authorizationScopes,
			releases: app.releases || [],
		};

		return {
			success: true,
			data: applicationInfo,
		};
	} catch (error) {
		return {
			success: false,
			error: new NetworkError(
				`Failed to get application info: ${error}`,
				error as Error,
			),
		};
	}
}

/**
 * List all applications (placeholder - needs query implementation)
 */
export async function applicationList(): Promise<CmdResult<Application[]>> {
	return {
		success: false,
		error: new ConfigurationError(
			"Application listing not available",
			"The GraphQL API does not support listing all applications. Use 'application info <name>' for specific applications.",
		),
	};
}

/**
 * Validate application configuration
 */
export async function applicationValidate(
	name?: string,
): Promise<CmdResult<ValidationResult>> {
	try {
		if (!name) {
			return {
				success: false,
				error: new ValidationError(
					"Application name is required",
					"Please specify an application name",
				),
			};
		}

		const accessToken = await getFromKeychain("token");
		if (!accessToken) {
			return {
				success: false,
				error: new AuthenticationError(
					"Not authenticated",
					"Please run 'godaddy auth login' first",
				),
			};
		}

		// Get application to validate it exists and check basic properties
		const result = await getApplication(name, { accessToken });

		if (!result.application) {
			return {
				success: false,
				error: new ValidationError(
					`Application '${name}' not found`,
					`Application '${name}' does not exist`,
				),
			};
		}

		const app = result.application;
		const errors: string[] = [];
		const warnings: string[] = [];

		// Basic validation checks
		if (!app.url) {
			errors.push("Application URL is required");
		}
		if (!app.proxyUrl) {
			warnings.push("Proxy URL is not set");
		}
		if (app.status === "INACTIVE") {
			warnings.push("Application is currently inactive");
		}

		const validationResult: ValidationResult = {
			valid: errors.length === 0,
			errors,
			warnings,
		};

		return {
			success: true,
			data: validationResult,
		};
	} catch (error) {
		return {
			success: false,
			error: new NetworkError(
				`Failed to validate application: ${error}`,
				error as Error,
			),
		};
	}
}

/**
 * Update application configuration
 */
export async function applicationUpdate(
	name: string,
	config: UpdateApplicationInput,
): Promise<CmdResult<void>> {
	try {
		if (!name) {
			return {
				success: false,
				error: new ValidationError(
					"Application name is required",
					"Please specify an application name",
				),
			};
		}

		// Validate input
		const validationResult = updateApplicationInputValidator(config);
		if (validationResult instanceof type.errors) {
			return {
				success: false,
				error: new ValidationError(
					validationResult.summary,
					"Invalid update configuration",
				),
			};
		}

		const accessToken = await getFromKeychain("token");
		if (!accessToken) {
			return {
				success: false,
				error: new AuthenticationError(
					"Not authenticated",
					"Please run 'godaddy auth login' first",
				),
			};
		}

		// Get application ID first
		const appResult = await getApplication(name, { accessToken });
		if (!appResult.application) {
			return {
				success: false,
				error: new ValidationError(
					`Application '${name}' not found`,
					`Application '${name}' does not exist`,
				),
			};
		}

		await updateAppService(appResult.application.id, config, { accessToken });

		return {
			success: true,
		};
	} catch (error) {
		return {
			success: false,
			error: new NetworkError(
				`Failed to update application: ${error}`,
				error as Error,
			),
		};
	}
}

/**
 * Enable application on a store
 */
export async function applicationEnable(
	name: string,
	storeId?: string,
): Promise<CmdResult<void>> {
	try {
		if (!name) {
			return {
				success: false,
				error: new ValidationError(
					"Application name is required",
					"Please specify an application name",
				),
			};
		}

		if (!storeId) {
			return {
				success: false,
				error: new ValidationError(
					"Store ID is required",
					"Please specify a store ID",
				),
			};
		}

		const accessToken = await getFromKeychain("token");
		if (!accessToken) {
			return {
				success: false,
				error: new AuthenticationError(
					"Not authenticated",
					"Please run 'godaddy auth login' first",
				),
			};
		}

		await enableAppService({ applicationName: name, storeId }, { accessToken });

		return {
			success: true,
		};
	} catch (error) {
		return {
			success: false,
			error: new NetworkError(
				`Failed to enable application: ${error}`,
				error as Error,
			),
		};
	}
}

/**
 * Disable application on a store
 */
export async function applicationDisable(
	name: string,
	storeId?: string,
): Promise<CmdResult<void>> {
	try {
		if (!name) {
			return {
				success: false,
				error: new ValidationError(
					"Application name is required",
					"Please specify an application name",
				),
			};
		}

		if (!storeId) {
			return {
				success: false,
				error: new ValidationError(
					"Store ID is required",
					"Please specify a store ID",
				),
			};
		}

		const accessToken = await getFromKeychain("token");
		if (!accessToken) {
			return {
				success: false,
				error: new AuthenticationError(
					"Not authenticated",
					"Please run 'godaddy auth login' first",
				),
			};
		}

		await disableAppService(
			{ applicationName: name, storeId },
			{ accessToken },
		);

		return {
			success: true,
		};
	} catch (error) {
		return {
			success: false,
			error: new NetworkError(
				`Failed to disable application: ${error}`,
				error as Error,
			),
		};
	}
}

/**
 * Archive application
 */
export async function applicationArchive(
	name: string,
): Promise<CmdResult<void>> {
	try {
		if (!name) {
			return {
				success: false,
				error: new ValidationError(
					"Application name is required",
					"Please specify an application name",
				),
			};
		}

		const accessToken = await getFromKeychain("token");
		if (!accessToken) {
			return {
				success: false,
				error: new AuthenticationError(
					"Not authenticated",
					"Please run 'godaddy auth login' first",
				),
			};
		}

		// Get application ID first
		const appResult = await getApplication(name, { accessToken });
		if (!appResult.application) {
			return {
				success: false,
				error: new ValidationError(
					`Application '${name}' not found`,
					`Application '${name}' does not exist`,
				),
			};
		}

		await archiveAppService(appResult.application.id, { accessToken });

		return {
			success: true,
		};
	} catch (error) {
		return {
			success: false,
			error: new NetworkError(
				`Failed to archive application: ${error}`,
				error as Error,
			),
		};
	}
}

export interface CreateReleaseInput {
	applicationName: string;
	version: string;
	description?: string;
	configPath?: string;
	env?: string;
}

export interface ReleaseInfo {
	id: string;
	version: string;
	description?: string;
	createdAt: string;
}

/**
 * Create a new release for an application
 */
export async function applicationRelease(
	input: CreateReleaseInput,
): Promise<CmdResult<ReleaseInfo>> {
	try {
		const accessToken = await getFromKeychain("token");

		if (!accessToken) {
			return {
				success: false,
				error: new AuthenticationError(
					"Not authenticated",
					"Please run 'godaddy auth login' first",
				),
			};
		}

		// Get application information first
		const appResult = await getApplication(input.applicationName, {
			accessToken,
		});
		if (!appResult.application) {
			return {
				success: false,
				error: new ValidationError(
					`Application '${input.applicationName}' not found`,
					`Application '${input.applicationName}' does not exist`,
				),
			};
		}

		// Load configuration to get actions and subscriptions
		let actions: ActionConfig[] = [];
		let subscriptions: SubscriptionConfig[] = [];

		try {
			const config = getConfigFile({
				configPath: input.configPath,
				env: input.env as Environment,
			});

			if (typeof config === "object" && !("problems" in config)) {
				actions = config.actions || [];
				subscriptions = config.subscriptions?.webhook || [];
			}
		} catch (configError) {
			// Config file might not exist, that's okay, just continue without actions/subscriptions
		}

		const releaseData = {
			applicationId: appResult.application.id,
			version: input.version,
			description: input.description,
			actions,
			subscriptions,
		};

		const result = await createReleaseService(releaseData, { accessToken });

		return {
			success: true,
			data: {
				id: result.createRelease.id,
				version: result.createRelease.version,
				description: result.createRelease.description,
				createdAt: result.createRelease.createdAt,
			},
		};
	} catch (error) {
		return {
			success: false,
			error: new NetworkError(
				`Failed to create release: ${error}`,
				error as Error,
			),
		};
	}
}

export interface ExtensionSecurityReport {
	extensionName: string;
	extensionDir: string;
	scannedFiles: number;
	totalFindings: number;
	blockedFindings: number;
	warnings: number;
	blocked: boolean;
}

export interface ExtensionBundleReport {
	extensionName: string;
	artifactName: string;
	artifactPath: string;
	size: number;
	sha256: string;
	/** Upload IDs - one per target (or single ID if no targets) */
	uploadIds?: string[];
	/** Targets that were uploaded */
	targets?: string[];
	uploaded?: boolean;
}

export interface DeployResult {
	securityReports: ExtensionSecurityReport[];
	bundleReports: ExtensionBundleReport[];
	totalExtensions: number;
	blockedExtensions: number;
}

/**
 * Deploy an application (change status to ACTIVE)
 * Performs security scan, bundling, and upload before deployment
 *
 * Prerequisites:
 * - Application must have at least one release created via `application release` command
 *
 * Flow:
 * 1. Get application and verify it exists
 * 2. Validate that application has a release
 * 3. Discover extensions in workspace
 * 4. Security scan each extension (Phase 1.5)
 * 5. Bundle each extension (Phase 2)
 * 6. Post-bundle security scan (Phase 2.5)
 * 7. Get presigned upload URLs (Phase 3)
 * 8. Upload artifacts to S3 (Phase 4)
 * 9. Update application status to ACTIVE
 */
export async function applicationDeploy(
	applicationName: string,
	options?: { configPath?: string; env?: Environment },
): Promise<CmdResult<DeployResult>> {
	try {
		const accessToken = await getFromKeychain("token");

		if (!accessToken) {
			return {
				success: false,
				error: new AuthenticationError(
					"Not authenticated",
					"Please run 'godaddy auth login' first",
				),
			};
		}

		// Get application and latest release
		const appResult = await getApplicationAndLatestRelease(applicationName, {
			accessToken,
		});

		if (!appResult.application) {
			return {
				success: false,
				error: new ValidationError(
					`Application '${applicationName}' not found`,
					`Application '${applicationName}' does not exist`,
				),
			};
		}

		const applicationId = appResult.application.id;

		// Validate that a release exists
		const releases = appResult.application.releases?.edges;
		if (!releases || releases.length === 0) {
			return {
				success: false,
				error: new ValidationError(
					"No release found for application",
					`Application '${applicationName}' has no releases. Create a release first with: godaddy application release ${applicationName} --release-version <version>`,
				),
			};
		}

		const latestRelease = releases[0].node;
		if (!latestRelease) {
			return {
				success: false,
				error: new ValidationError(
					"Invalid release data",
					"Unable to retrieve release information",
				),
			};
		}

		const releaseId = latestRelease.id;

		// Get extensions from config file (source of truth)
		const repoRoot = process.cwd();
		const extensions = getExtensionsFromConfig({
			configPath: options?.configPath,
			env: options?.env,
		});

		const securityReports: ExtensionSecurityReport[] = [];
		let blockedExtensions = 0;

		// If no extensions found, skip security scan and bundling (no-op)
		if (extensions.length === 0) {
			// No extensions to scan/bundle, proceed with deployment
			await updateAppService(
				appResult.application.id,
				{ status: "ACTIVE" },
				{ accessToken },
			);

			return {
				success: true,
				data: {
					securityReports: [],
					bundleReports: [],
					totalExtensions: 0,
					blockedExtensions: 0,
				},
			};
		}

		// Scan each extension (scan the directory containing the source file)
		// Extensions live at extensions/{handle}/ and source is relative to that
		for (const extension of extensions) {
			const extensionDir = resolve(repoRoot, "extensions", extension.handle);
			const sourcePath = resolve(extensionDir, extension.source);

			const scanResult = await scanExtension(extensionDir);

			if (!scanResult.success || !scanResult.data) {
				return {
					success: false,
					error: new ValidationError(
						`Security scan failed for extension '${extension.name}'`,
						scanResult.error?.message || "Unable to perform security scan",
					),
				};
			}

			const report = scanResult.data;

			if (report.blocked) {
				blockedExtensions++;
			}

			securityReports.push({
				extensionName: extension.name,
				extensionDir,
				scannedFiles: report.scannedFiles,
				totalFindings: report.summary.total,
				blockedFindings: report.summary.bySeverity.block,
				warnings: report.summary.bySeverity.warn,
				blocked: report.blocked,
			});
		}

		// If any extension has blocking issues, fail deployment
		if (blockedExtensions > 0) {
			return {
				success: false,
				error: new ValidationError(
					"Security violations detected",
					`${blockedExtensions} extension(s) blocked due to security violations. Deployment blocked.`,
				),
			};
		}

		// Bundle each extension
		const bundleReports: ExtensionBundleReport[] = [];
		const timestamp = new Date().toISOString().replace(/[:.]/g, "-");

		for (const extension of extensions) {
			const extensionDir = resolve(repoRoot, "extensions", extension.handle);
			const sourcePath = resolve(extensionDir, extension.source);

			const bundleResult = await bundleExtension(
				{ name: extension.handle, version: undefined },
				sourcePath,
				{ repoRoot, timestamp, extensionDir, extensionType: extension.type },
			);

			if (!bundleResult.success || !bundleResult.data) {
				return {
					success: false,
					error: new ValidationError(
						`Bundle failed for extension '${extension.name}'`,
						bundleResult.error?.message || "Unable to bundle extension",
					),
				};
			}

			const bundle = bundleResult.data;

			// Post-bundle security scan
			const postScanResult = await scanBundle(bundle.artifactPath);

			// Cleanup on scan failure
			if (!postScanResult.success) {
				await rm(bundle.artifactPath, { force: true });
				if (bundle.sourcemapPath) {
					await rm(bundle.sourcemapPath, { force: true });
				}
				return {
					success: false,
					error: new ValidationError(
						`Post-bundle security scan failed for extension '${extension.name}'`,
						postScanResult.error?.message || "Unable to scan bundled artifact",
					),
				};
			}

			// Cleanup and block deployment if security violations found
			if (postScanResult.data?.blocked) {
				await rm(bundle.artifactPath, { force: true });
				if (bundle.sourcemapPath) {
					await rm(bundle.sourcemapPath, { force: true });
				}
				return {
					success: false,
					error: new ValidationError(
						`Security violations detected in bundled code for extension '${extension.name}'`,
						`${postScanResult.data.findings.length} security violation(s) found. Deployment blocked.`,
					),
				};
			}

			// Get presigned upload URL(s) and upload (Phase 3 & 4)
			// For blocks extensions, use "blocks" as target
			// For extensions with targets, upload once per target
			const targets =
				extension.type === "blocks"
					? ["blocks"]
					: extension.targets?.length
						? extension.targets.map((t) => t.target)
						: [undefined]; // No targets = single upload without target info

			const uploadIds: string[] = [];
			let uploaded = false;

			try {
				for (const target of targets) {
					const uploadTarget = await getUploadTarget(
						{
							applicationId,
							releaseId,
							contentType: "JS",
							target,
						},
						accessToken,
					);

					uploadIds.push(uploadTarget.uploadId);

					// Upload to S3 (Phase 4)
					await uploadArtifact(uploadTarget, bundle.artifactPath, {
						contentType: "application/javascript",
					});
				}

				uploaded = true;

				// Clean up artifacts after successful upload
				await rm(bundle.artifactPath, { force: true });
				if (bundle.sourcemapPath) {
					await rm(bundle.sourcemapPath, { force: true });
				}
			} catch (uploadError) {
				// Cleanup on upload failure
				await rm(bundle.artifactPath, { force: true });
				if (bundle.sourcemapPath) {
					await rm(bundle.sourcemapPath, { force: true });
				}
				return {
					success: false,
					error: new NetworkError(
						`Upload failed for extension '${extension.name}'`,
						uploadError as Error,
					),
				};
			}

			bundleReports.push({
				extensionName: extension.name,
				artifactName: bundle.artifactName,
				artifactPath: bundle.artifactPath,
				size: bundle.size,
				sha256: bundle.sha256,
				uploadIds,
				targets:
					extension.type === "blocks"
						? ["blocks"]
						: extension.targets?.map((t) => t.target),
				uploaded,
			});
		}

		// Update application status to ACTIVE
		await updateAppService(
			appResult.application.id,
			{ status: "ACTIVE" },
			{ accessToken },
		);

		return {
			success: true,
			data: {
				securityReports,
				bundleReports,
				totalExtensions: extensions.length,
				blockedExtensions,
			},
		};
	} catch (error) {
		return {
			success: false,
			error: new NetworkError(
				`Failed to deploy application: ${error}`,
				error as Error,
			),
		};
	}
}
