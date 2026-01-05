import { setupServer } from "msw/node";
import { applicationHandlers } from "./handlers/application-handlers";
import { authHandlers } from "./handlers/auth-handlers";
import { webhookHandlers } from "./handlers/webhook-handlers";

export const server = setupServer(
	...authHandlers,
	...applicationHandlers,
	...webhookHandlers,
);
