/**
 * Core actions functionality
 */

import type { CmdResult } from "../shared/types";

// Available actions list
const AVAILABLE_ACTIONS = [
	"location.address.verify",
	"commerce.taxes.calculate",
	"commerce.shipping-rates.calculate",
	"commerce.price-adjustment.apply",
	"commerce.price-adjustment.list",
	"notifications.email.send",
	"commerce.payment.get",
	"commerce.payment.cancel",
	"commerce.payment.refund",
	"commerce.payment.process",
	"commerce.payment.auth",
];

// Action interface definitions
interface ActionInterface {
	name: string;
	description: string;
	requestSchema: object;
	responseSchema: object;
	examples?: {
		request: object;
		response: object;
	};
}

// Import the action interfaces from the CLI command
import { ACTION_INTERFACES } from "../cli/commands/actions";

/**
 * Get list of all available actions
 */
export async function actionsList(): Promise<CmdResult<string[]>> {
	try {
		return {
			success: true,
			data: AVAILABLE_ACTIONS,
		};
	} catch (error) {
		return {
			success: false,
			error:
				error instanceof Error
					? error
					: new Error("Failed to get actions list"),
		};
	}
}

/**
 * Get detailed interface information for a specific action
 */
export async function actionsDescribe(
	actionName: string,
): Promise<CmdResult<ActionInterface>> {
	try {
		if (!AVAILABLE_ACTIONS.includes(actionName)) {
			return {
				success: false,
				error: new Error(
					`Action '${actionName}' not found. Available actions: ${AVAILABLE_ACTIONS.join(", ")}`,
				),
			};
		}

		const actionInterface = ACTION_INTERFACES[actionName];

		if (!actionInterface) {
			return {
				success: false,
				error: new Error(
					`Interface definition not available for action '${actionName}'`,
				),
			};
		}

		return {
			success: true,
			data: actionInterface,
		};
	} catch (error) {
		return {
			success: false,
			error:
				error instanceof Error ? error : new Error("Failed to describe action"),
		};
	}
}
