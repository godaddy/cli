import { vi } from "vitest";

// Mock auth services to prevent real network calls and server startup for TUI tests
vi.mock("../../src/services/auth", () => ({
	authenticate: vi.fn().mockResolvedValue("test-token"),
	getFromKeychain: vi.fn().mockResolvedValue("test-token"),
	logout: vi.fn().mockResolvedValue(undefined),
}));

// Mock the problematic hooks that cause stdin.ref issues
vi.mock("ink", async (importOriginal) => {
	const actual = await importOriginal<typeof import("ink")>();
	return {
		...actual,
		useInput: vi.fn(),
		useApp: vi.fn(() => ({ exit: vi.fn() })),
	};
});

// Mock SelectInput to avoid stdin issues - return a proper React component
vi.mock("ink-select-input", () => ({
	default: ({ items }: { items: Array<{ label: string; value: string }> }) => {
		const React = require("react");
		const { Text, Box } = require("ink");
		return React.createElement(
			Box,
			{ flexDirection: "column" },
			items.map((item, index) =>
				React.createElement(Text, { key: index }, item.label),
			),
		);
	},
}));
