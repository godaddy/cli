import { http, HttpResponse } from "msw";
import { checkAuthentication } from "../auth-utils";
import {
	applicationFixtures,
	createApplicationResponse,
	createReleaseResponse,
	findApplicationByName,
	findApplicationWithReleases,
} from "../fixtures/fixture-utils";

// Helper function to extract operation name from GraphQL query
function extractOperationNameFromQuery(query: string): string {
	const match = query.match(/(query|mutation)\s+(\w+)/);
	return match ? match[2] : "Unknown";
}

export const applicationHandlers = [
	// GraphQL endpoint handler using HTTP POST
	http.post("*/v1/apps/app-registry-subgraph", async ({ request }) => {
		// Check authentication for all operations
		const authError = checkAuthentication(request);
		if (authError) return authError;

		// Parse GraphQL request body
		const body = (await request.json()) as {
			operationName?: string;
			query: string;
			variables: Record<string, unknown>;
		};
		const { operationName, query, variables } = body;

		// Extract operation name from query if not provided
		const extractedOperationName =
			operationName || extractOperationNameFromQuery(query);

		switch (extractedOperationName) {
			case "Application":
				return handleApplicationQuery(variables);

			case "ApplicationWithLatestRelease":
				return handleApplicationWithReleaseQuery(variables);

			case "CreateApplication":
				return handleCreateApplicationMutation(variables);

			case "UpdateApplication":
				return handleUpdateApplicationMutation(variables);

			case "CreateRelease":
				return handleCreateReleaseMutation(variables);

			case "EnableApplication":
				return handleEnableApplicationMutation(variables);

			case "DisableApplication":
				return handleDisableApplicationMutation(variables);

			case "ArchiveApplication":
				return handleArchiveApplicationMutation(variables);

			default:
				return HttpResponse.json(
					{
						errors: [
							{
								message: `Unknown operation: ${extractedOperationName}`,
								extensions: { code: "OPERATION_NOT_SUPPORTED" },
							},
						],
					},
					{ status: 400 },
				);
		}
	}),
];

// Query handlers
function handleApplicationQuery(variables: Record<string, unknown>) {
	const { name } = variables;

	if (!name) {
		return HttpResponse.json(
			{
				errors: [
					{
						message: 'Variable "name" is required',
						extensions: { code: "BAD_USER_INPUT" },
					},
				],
			},
			{ status: 400 },
		);
	}

	const app = findApplicationByName(name as string);

	return HttpResponse.json({
		data: { application: app || null },
	});
}

function handleApplicationWithReleaseQuery(variables: Record<string, unknown>) {
	const { name } = variables;

	if (!name) {
		return HttpResponse.json(
			{
				errors: [
					{
						message: 'Variable "name" is required',
						extensions: { code: "BAD_USER_INPUT" },
					},
				],
			},
			{ status: 400 },
		);
	}

	const app = findApplicationWithReleases(name as string);

	return HttpResponse.json({
		data: { application: app || null },
	});
}

// Mutation handlers
function handleCreateApplicationMutation(variables: Record<string, unknown>) {
	const { input } = variables;

	if (!input || typeof input !== "object") {
		return HttpResponse.json(
			{
				errors: [
					{
						message: 'Variable "input" is required',
						extensions: { code: "BAD_USER_INPUT" },
					},
				],
			},
			{ status: 400 },
		);
	}

	const inputData = input as Record<string, unknown>;

	// Validate required fields
	const requiredFields = ["name", "label", "url"];
	const missingFields = requiredFields.filter((field) => !inputData[field]);

	if (missingFields.length > 0) {
		return HttpResponse.json(
			{
				errors: [
					{
						message: "Validation failed",
						extensions: {
							code: "VALIDATION_ERROR",
							fieldErrors: Object.fromEntries(
								missingFields.map((field) => [field, [`${field} is required`]]),
							),
						},
					},
				],
			},
			{ status: 400 },
		);
	}

	// Check if application with same name already exists
	const existingApp = findApplicationByName(inputData.name as string);
	if (existingApp) {
		return HttpResponse.json(
			{
				data: null,
				errors: [
					{
						message: `Application with name "${inputData.name}" already exists`,
						extensions: { code: "DUPLICATE_NAME" },
					},
				],
			},
			{ status: 200 },
		);
	}

	const newApp = createApplicationResponse(inputData);

	return HttpResponse.json({
		data: { createApplication: newApp },
	});
}

function handleUpdateApplicationMutation(variables: Record<string, unknown>) {
	const { id, input } = variables;

	if (!id || !input) {
		return HttpResponse.json(
			{
				errors: [
					{
						message: 'Variables "id" and "input" are required',
						extensions: { code: "BAD_USER_INPUT" },
					},
				],
			},
			{ status: 400 },
		);
	}

	// Find existing application
	const existingApp = applicationFixtures.applications.find(
		(app) => app.id === id,
	);

	if (!existingApp) {
		return HttpResponse.json(
			{
				errors: [
					{
						message: `Application with id "${id}" not found`,
						extensions: { code: "NOT_FOUND" },
					},
				],
			},
			{ status: 404 },
		);
	}

	// Merge updates
	const updatedApp = {
		...existingApp,
		...(input as Record<string, unknown>),
	};

	return HttpResponse.json({
		data: { updateApplication: updatedApp },
	});
}

function handleCreateReleaseMutation(variables: Record<string, unknown>) {
	const { input } = variables;

	if (!input || typeof input !== "object") {
		return HttpResponse.json(
			{
				errors: [
					{
						message: 'Variable "input" is required',
						extensions: { code: "BAD_USER_INPUT" },
					},
				],
			},
			{ status: 400 },
		);
	}

	const inputData = input as Record<string, unknown>;

	// Validate required fields for release
	if (!inputData.applicationId || !inputData.version) {
		return HttpResponse.json(
			{
				errors: [
					{
						message: "Application ID and version are required",
						extensions: { code: "VALIDATION_ERROR" },
					},
				],
			},
			{ status: 400 },
		);
	}

	// Check if application exists
	const app = applicationFixtures.applications.find(
		(app) => app.id === inputData.applicationId,
	);
	if (!app) {
		return HttpResponse.json(
			{
				errors: [
					{
						message: `Application with id "${inputData.applicationId}" not found`,
						extensions: { code: "NOT_FOUND" },
					},
				],
			},
			{ status: 404 },
		);
	}

	const newRelease = createReleaseResponse({
		version: inputData.version as string,
		description: inputData.description as string,
	});

	return HttpResponse.json({
		data: { createRelease: newRelease },
	});
}

function handleEnableApplicationMutation(variables: Record<string, unknown>) {
	const { input } = variables;

	if (!input || typeof input !== "object") {
		return HttpResponse.json(
			{
				errors: [
					{
						message: "applicationName and storeId are required",
						extensions: { code: "BAD_USER_INPUT" },
					},
				],
			},
			{ status: 400 },
		);
	}

	const inputData = input as Record<string, unknown>;

	if (!inputData.applicationName || !inputData.storeId) {
		return HttpResponse.json(
			{
				errors: [
					{
						message: "applicationName and storeId are required",
						extensions: { code: "BAD_USER_INPUT" },
					},
				],
			},
			{ status: 400 },
		);
	}

	// Check if application exists by name
	const app = findApplicationByName(inputData.applicationName as string);
	if (!app) {
		return HttpResponse.json(
			{
				errors: [
					{
						message: `Application with name "${inputData.applicationName}" not found`,
						extensions: { code: "NOT_FOUND" },
					},
				],
			},
			{ status: 404 },
		);
	}

	return HttpResponse.json({
		data: {
			enableStoreApplication: {
				id: app.id,
				status: "ENABLED",
			},
		},
	});
}

function handleDisableApplicationMutation(variables: Record<string, unknown>) {
	const { input } = variables;

	if (!input || typeof input !== "object") {
		return HttpResponse.json(
			{
				errors: [
					{
						message: "applicationName and storeId are required",
						extensions: { code: "BAD_USER_INPUT" },
					},
				],
			},
			{ status: 400 },
		);
	}

	const inputData = input as Record<string, unknown>;

	if (!inputData.applicationName || !inputData.storeId) {
		return HttpResponse.json(
			{
				errors: [
					{
						message: "applicationName and storeId are required",
						extensions: { code: "BAD_USER_INPUT" },
					},
				],
			},
			{ status: 400 },
		);
	}

	// Check if application exists by name
	const app = findApplicationByName(inputData.applicationName as string);
	if (!app) {
		return HttpResponse.json(
			{
				errors: [
					{
						message: `Application with name "${inputData.applicationName}" not found`,
						extensions: { code: "NOT_FOUND" },
					},
				],
			},
			{ status: 404 },
		);
	}

	return HttpResponse.json({
		data: {
			disableStoreApplication: {
				id: app.id,
				status: "DISABLED",
			},
		},
	});
}

function handleArchiveApplicationMutation(variables: Record<string, unknown>) {
	const { id } = variables;

	if (!id) {
		return HttpResponse.json(
			{
				errors: [
					{
						message: 'Variable "id" is required',
						extensions: { code: "BAD_USER_INPUT" },
					},
				],
			},
			{ status: 400 },
		);
	}

	// Find existing application
	const existingApp = applicationFixtures.applications.find(
		(app) => app.id === id,
	);

	if (!existingApp) {
		return HttpResponse.json(
			{
				errors: [
					{
						message: `Application with id "${id}" not found`,
						extensions: { code: "NOT_FOUND" },
					},
				],
			},
			{ status: 404 },
		);
	}

	const archivedApp = {
		...existingApp,
		status: "ARCHIVED",
		archivedAt: new Date().toISOString(),
	};

	return HttpResponse.json({
		data: { archiveApplication: archivedApp },
	});
}
