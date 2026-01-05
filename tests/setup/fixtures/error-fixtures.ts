export const errorFixtures = {
	graphql: {
		unauthorized: {
			errors: [
				{
					message: "Unauthorized",
					extensions: { code: "UNAUTHENTICATED" },
				},
			],
		},

		notFound: {
			errors: [
				{
					message: "Application not found",
					extensions: { code: "NOT_FOUND" },
				},
			],
		},

		validationError: {
			errors: [
				{
					message: "Validation failed",
					extensions: {
						code: "VALIDATION_ERROR",
						fieldErrors: {
							name: ["Name is required"],
							url: ["Invalid URL format"],
						},
					},
				},
			],
		},

		serverError: {
			errors: [
				{
					message: "Internal server error",
				},
			],
		},
	},

	rest: {
		unauthorized: {
			error: "Unauthorized",
			message: "Invalid or missing authorization token",
		},

		notFound: {
			error: "Not Found",
			message: "Resource not found",
		},

		serverError: {
			error: "Internal Server Error",
			message: "An unexpected error occurred",
		},
	},
};
