import ms from "ms";

export const name = "extension-with-deps";

export function getTimeout() {
	return ms("1 year");
}
