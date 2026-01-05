export const applicationFixtures = {
	applications: [
		{
			id: "app-1",
			label: "Test Application 1",
			name: "test-app-1",
			description: "A test application for development",
			status: "ACTIVE",
			url: "https://test-app-1.example.com",
			proxyUrl: "https://proxy.test-app-1.example.com",
			authorizationScopes: ["read", "write"],
		},
		{
			id: "app-2",
			label: "Test Application 2",
			name: "test-app-2",
			description: "Another test application",
			status: "INACTIVE",
			url: "https://test-app-2.example.com",
			proxyUrl: "https://proxy.test-app-2.example.com",
			authorizationScopes: ["read"],
		},
		{
			id: "app-3",
			label: "Archived Application",
			name: "archived-app",
			description: "An archived test application",
			status: "ARCHIVED",
			url: "https://archived-app.example.com",
			proxyUrl: "https://proxy.archived-app.example.com",
			authorizationScopes: ["read"],
			archivedAt: "2024-11-01T10:00:00Z",
		},
	],

	applicationsWithReleases: [
		{
			id: "app-1",
			label: "Test Application 1",
			name: "test-app-1",
			description: "A test application for development",
			status: "ACTIVE",
			url: "https://test-app-1.example.com",
			proxyUrl: "https://proxy.test-app-1.example.com",
			authorizationScopes: ["read", "write"],
			releases: [
				{
					id: "release-1",
					version: "1.2.3",
					description: "Latest release with bug fixes",
					createdAt: "2024-12-01T10:00:00Z",
				},
				{
					id: "release-2",
					version: "1.2.2",
					description: "Previous stable release",
					createdAt: "2024-11-15T10:00:00Z",
				},
			],
		},
	],

	// Response templates for mutations
	createApplicationResponse: (input: Record<string, unknown>) => ({
		id: `app-${Date.now()}`,
		clientId: `client-${Date.now()}`,
		clientSecret: "generated-secret",
		...input,
		status: "ACTIVE",
		secret: "app-secret",
		publicKey: "public-key-data",
	}),

	createReleaseResponse: (input: { version: string; description: string }) => ({
		id: `release-${Date.now()}`,
		version: input.version,
		description: input.description,
		createdAt: new Date().toISOString(),
	}),
};
