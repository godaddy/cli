import { applicationFixtures } from "./application-fixtures";
import { authFixtures } from "./auth-fixtures";
import { errorFixtures } from "./error-fixtures";
import { webhookFixtures } from "./webhook-fixtures";

// Helper to find application by name
export const findApplicationByName = (name: string) => {
	return applicationFixtures.applications.find((app) => app.name === name);
};

// Helper to find application with releases by name
export const findApplicationWithReleases = (name: string) => {
	return applicationFixtures.applicationsWithReleases.find(
		(app) => app.name === name,
	);
};

// Helper to create dynamic responses
export const createApplicationResponse = (input: Record<string, unknown>) => {
	return applicationFixtures.createApplicationResponse(input);
};

export const createReleaseResponse = (input: {
	version: string;
	description: string;
}) => {
	return applicationFixtures.createReleaseResponse(input);
};

// Helper to get error responses by type
export const getGraphQLError = (type: keyof typeof errorFixtures.graphql) => {
	return errorFixtures.graphql[type];
};

export const getRESTError = (type: keyof typeof errorFixtures.rest) => {
	return errorFixtures.rest[type];
};

// Export all fixtures for easy importing
export { applicationFixtures, webhookFixtures, authFixtures, errorFixtures };
