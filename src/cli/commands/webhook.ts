import { Command } from "commander";
import { type WebhookEvent, webhookEvents } from "../../core/webhooks";

export function createWebhookCommand(): Command {
	const webhook = new Command("webhook").description(
		"Manage webhook integrations",
	);

	// webhook events
	webhook
		.command("events")
		.description("List available webhook event types")
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (options) => {
			const result = await webhookEvents();

			if (!result.success) {
				console.error(
					result.error?.userMessage || "Failed to get webhook events",
				);
				process.exit(1);
			}

			const events = result.data as WebhookEvent[];

			if (options.output === "json") {
				console.log(JSON.stringify(events, null, 2));
			} else {
				if (events.length === 0) {
					console.log("No webhook events available");
					return;
				}

				console.log(`Available Webhook Events (${events.length}):`);
				console.log("");
				for (const event of events) {
					console.log(`• ${event.eventType}`);
					if (event.description) {
						console.log(`  ${event.description}`);
					}
					console.log("");
				}
			}
			process.exit(0);
		});

	return webhook;
}
