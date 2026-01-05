import * as fs from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

type Credentials = {
	CLIENT_ID: string;
	CLIENT_SECRET: string;
};

const PATH = join(homedir(), ".godaddy", "credentials");

export async function getCredentials(): Promise<Credentials> {
	let text = "";
	try {
		if (fs.existsSync(PATH)) {
			text = fs.readFileSync(PATH, "utf-8");
		} else {
			return {
				CLIENT_ID: "",
				CLIENT_SECRET: "",
			};
		}
	} catch (error: unknown) {
		// Handle potential read errors
		const errorMessage =
			error instanceof Error ? error.message : "An unknown error occurred";
		console.error(`Error reading credentials file at ${PATH}: ${errorMessage}`);
		return {
			CLIENT_ID: "",
			CLIENT_SECRET: "",
		};
	}

	let CLIENT_ID = "";
	let CLIENT_SECRET = "";

	const lines = text.split("\n").filter(Boolean);
	for (const line of lines) {
		const [key, value] = line.split("=");

		if (key === "client_id") {
			CLIENT_ID = value;
		} else if (key === "client_secret") {
			CLIENT_SECRET = value;
		}
	}

	return {
		CLIENT_ID,
		CLIENT_SECRET,
	};
}
