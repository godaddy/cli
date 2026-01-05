/**
 * Presigned URL service for extension artifact uploads
 * Phase 3: GraphQL integration for generateReleaseUploadUrl mutation
 */

import { getRequestHeaders, initApiBaseUrl } from "@/services/http-helpers";
import { getLogger } from "@/services/logger";
import { graphql } from "gql.tada";
import { request } from "graphql-request";

const logger = getLogger();

/**
 * Upload target information from GraphQL
 */
export interface UploadTarget {
	uploadId: string;
	url: string;
	key: string;
	expiresAt: string;
	maxSizeBytes: number;
	requiredHeaders: Record<string, string>;
}

/**
 * Parameters for requesting an upload URL
 */
export interface GetUploadTargetParams {
	applicationId: string;
	releaseId: string;
	contentType?: "JS" | "ZIP" | "TAR";
	/** Target location for the extension (e.g., "body.end") - becomes filename {target}.js */
	target?: string;
}

const GenerateReleaseUploadUrlMutation = graphql(`
	mutation GenerateReleaseUploadUrl($input: MutationGenerateReleaseUploadUrlInput!) {
		generateReleaseUploadUrl(input: $input) {
			uploadId
			url
			key
			expiresAt
			maxSizeBytes
			requiredHeaders
		}
	}
`);

/**
 * Get a presigned upload URL for an extension artifact
 */
export async function getUploadTarget(
	params: GetUploadTargetParams,
	accessToken: string,
): Promise<UploadTarget> {
	logger.debug(
		{
			applicationId: params.applicationId,
			releaseId: params.releaseId,
			contentType: params.contentType ?? "JS",
		},
		"Requesting presigned upload URL",
	);

	const baseUrl = await initApiBaseUrl();
	const response = await request(
		baseUrl,
		GenerateReleaseUploadUrlMutation,
		{
			input: {
				applicationId: params.applicationId,
				releaseId: params.releaseId,
				contentType: params.contentType ?? "JS",
				target: params.target,
			},
		},
		getRequestHeaders(accessToken),
	);

	if (!response.generateReleaseUploadUrl) {
		throw new Error("Failed to generate upload URL: empty response");
	}

	const data = response.generateReleaseUploadUrl;

	// Parse requiredHeaders from array of "key:value" strings to Record
	const headersMap: Record<string, string> = {};
	for (const header of data.requiredHeaders) {
		const [key, ...valueParts] = header.split(":");
		if (key && valueParts.length > 0) {
			headersMap[key.trim()] = valueParts.join(":").trim();
		}
	}

	logger.debug(
		{
			uploadId: data.uploadId,
			key: data.key,
			expiresAt: data.expiresAt,
			maxSizeBytes: data.maxSizeBytes,
		},
		"Received presigned upload URL",
	);

	return {
		uploadId: data.uploadId,
		url: data.url,
		key: data.key,
		expiresAt: data.expiresAt,
		maxSizeBytes: data.maxSizeBytes,
		requiredHeaders: headersMap,
	};
}
