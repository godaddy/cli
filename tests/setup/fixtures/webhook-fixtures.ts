export const webhookFixtures = {
	eventTypes: [
		{
			eventType: "application.created",
			description: "Triggered when a new application is created",
		},
		{
			eventType: "application.updated",
			description: "Triggered when an application is updated",
		},
		{
			eventType: "application.deleted",
			description: "Triggered when an application is deleted",
		},
		{
			eventType: "application.enabled",
			description: "Triggered when an application is enabled in store",
		},
		{
			eventType: "application.disabled",
			description: "Triggered when an application is disabled in store",
		},
		{
			eventType: "application.archived",
			description: "Triggered when an application is archived",
		},
		{
			eventType: "release.created",
			description: "Triggered when a new release is created",
		},
		{
			eventType: "release.deployed",
			description: "Triggered when a release is deployed",
		},
	],

	subscriptions: [
		{
			id: "sub-1",
			eventType: "application.created",
			url: "https://example.com/webhook/app-created",
			active: true,
			createdAt: "2024-12-01T10:00:00Z",
		},
		{
			id: "sub-2",
			eventType: "release.created",
			url: "https://example.com/webhook/release-created",
			active: true,
			createdAt: "2024-12-01T11:00:00Z",
		},
	],
};

// Webhook event types fixture for API response
export const webhookEventTypesFixture = {
	events: webhookFixtures.eventTypes,
};
