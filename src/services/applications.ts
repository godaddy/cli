import { type } from "arktype";
import { graphql } from "gql.tada";
import { ClientError, request } from "graphql-request";
import { getRequestHeaders, initApiBaseUrl } from "./http-helpers";

const ApplicationQuery = graphql(`
  query Application($name: String!) {
    application(name: $name) {
      id
      label
      name
      description
      status
      url
      proxyUrl
    }
  }
`);

const ApplicationWithLatestReleaseQuery = graphql(`
  query ApplicationWithLatestRelease($name: String!) {
    application(name: $name) {
      id
      label
      name
      description
      status
      url
      proxyUrl
      authorizationScopes
      releases(first: 1, orderBy: { createdAt: desc }) {
        edges {
          node {
            id
            version
            description
            createdAt
          }
        }
      }
    }
  }
`);

const ApplicationsListQuery = graphql(`
  query ApplicationsList {
    applications {
      id
      label
      name
      description
      status
      url
      proxyUrl
    }
  }
`);

export const CreateApplicationMutation = graphql(`
  mutation CreateApplication($input: MutationCreateApplicationInput!) {
    createApplication(input: $input) {
      id
      clientId
      clientSecret
      label
      name
      description
      status
      url
      proxyUrl
      authorizationScopes
      secret
      publicKey
    }
  }
`);

export const UpdateApplicationMutation = graphql(`
  mutation UpdateApplication(
    $id: String!
    $input: MutationUpdateApplicationInput!
  ) {
    updateApplication(id: $id, input: $input) {
      id
      clientId
      label
      name
      description
      status
      url
      proxyUrl
      authorizationScopes
    }
  }
`);

export const CreateReleaseMutation = graphql(`
  mutation CreateRelease($input: MutationCreateReleaseInput!) {
    createRelease(input: $input) {
      id
      version
      description
      createdAt
    }
  }
`);

export const EnableApplicationMutation = graphql(`
  mutation EnableApplication($input: MutationEnableStoreApplicationInput!) {
    enableStoreApplication(input: $input) {
      id
    }
  }
`);

export const DisableApplicationMutation = graphql(`
  mutation DisableApplication($input: MutationDisableStoreApplicationInput!) {
    disableStoreApplication(input: $input) {
      id
    }
  }
`);

export const ArchiveApplicationMutation = graphql(`
  mutation ArchiveApplication($id: String!) {
    archiveApplication(id: $id) {
      id
      label
      name
      status
      createdAt
      archivedAt
    }
  }
`);

export const applicationInput = type({
	label: "string",
	name: "string",
	description: "string",
	url: type.keywords.string.url.root,
	proxyUrl: type.keywords.string.url.root,
	authorizationScopes: type.string.array().moreThanLength(0),
});

export const updateApplicationInput = type({
	label: "string?",
	description: "string?",
	status: '"ACTIVE" | "INACTIVE"?',
});

export async function createApplication(
	input: typeof applicationInput.infer,
	{ accessToken }: { accessToken: string | null },
) {
	if (!accessToken) {
		throw new Error("Access token is required");
	}

	const inputParseResult = applicationInput(input);
	if (inputParseResult instanceof type.errors) {
		throw new Error(inputParseResult.summary);
	}

	try {
		const baseUrl = await initApiBaseUrl();

		const result = await request(
			baseUrl,
			CreateApplicationMutation,
			{ input: inputParseResult },
			getRequestHeaders(accessToken),
		);

		return result;
	} catch (err) {
		if (err instanceof ClientError) {
			const graphqlErrors = err.response.errors;
			if (graphqlErrors?.length) {
				const error = graphqlErrors[0];
				const errorCode = error.extensions?.code;
				const errorMessage = errorCode
					? `${error.message} (${errorCode})`
					: error.message;
				throw new Error(errorMessage);
			}
			console.log(err);
			throw new Error("An unexpected error occurred");
		}

		throw new Error("An unexpected error occurred");
	}
}

export async function updateApplication(
	id: string,
	input: typeof updateApplicationInput.infer,
	{ accessToken }: { accessToken: string | null },
) {
	if (!accessToken) {
		throw new Error("Access token is required");
	}

	try {
		const baseUrl = await initApiBaseUrl();
		const result = await request(
			baseUrl,
			UpdateApplicationMutation,
			{ id, input },
			getRequestHeaders(accessToken),
		);
		return result;
	} catch (err) {
		if (err instanceof ClientError) {
			const graphqlErrors = err.response.errors;
			if (graphqlErrors?.length) {
				const error = graphqlErrors[0];
				const errorCode = error.extensions?.code;
				const errorMessage = errorCode
					? `${error.message} (${errorCode})`
					: error.message;
				throw new Error(errorMessage);
			}
			console.log(err);
			throw new Error("An unexpected error occurred");
		}

		throw new Error("An unexpected error occurred");
	}
}

export async function getApplication(
	name: string,
	{ accessToken }: { accessToken: string | null },
) {
	if (!accessToken) {
		throw new Error("Access token is required");
	}

	const baseUrl = await initApiBaseUrl();
	const result = await request(
		baseUrl,
		ApplicationQuery,
		{ name },
		getRequestHeaders(accessToken),
	);
	return result;
}

export async function getApplicationAndLatestRelease(
	name: string,
	{ accessToken }: { accessToken: string | null },
) {
	if (!accessToken) {
		throw new Error("Access token is required");
	}

	const baseUrl = await initApiBaseUrl();
	const result = await request(
		baseUrl,
		ApplicationWithLatestReleaseQuery,
		{ name },
		getRequestHeaders(accessToken),
	);
	return result;
}

const actionInput = type({
	name: "string",
	url: "string",
});

export const subscriptionInput = type({
	name: "string",
	events: "string[]",
	url: "string",
});

export const releaseInput = type({
	applicationId: "string",
	version: "string",
	description: "string?",
	actions: actionInput.array().optional(),
	subscriptions: subscriptionInput.array().optional(),
});

export async function createRelease(
	input: typeof releaseInput.infer,
	{ accessToken }: { accessToken: string | null },
) {
	if (!accessToken) {
		throw new Error("Access token is required");
	}

	const inputParseResult = releaseInput(input);
	if (inputParseResult instanceof type.errors) {
		throw new Error(inputParseResult.summary);
	}

	// Default actions to empty array if undefined
	const releaseData = {
		...inputParseResult,
		actions: inputParseResult.actions ?? [],
	};

	try {
		const baseUrl = await initApiBaseUrl();
		const result = await request(
			baseUrl,
			CreateReleaseMutation,
			{ input: releaseData },
			getRequestHeaders(accessToken),
		);
		return result;
	} catch (err) {
		if (err instanceof ClientError) {
			const graphqlErrors = err.response.errors;
			if (graphqlErrors?.length) {
				const error = graphqlErrors[0];
				const errorCode = error.extensions?.code;
				const errorMessage = errorCode
					? `${error.message} (${errorCode})`
					: error.message;
				throw new Error(errorMessage);
			}
			console.log(err);
			throw new Error("An unexpected error occurred");
		}

		throw new Error("An unexpected error occurred");
	}
}

export async function enableApplication(
	input: { applicationName: string; storeId: string },
	{ accessToken }: { accessToken: string | null },
) {
	if (!accessToken) {
		throw new Error("Access token is required");
	}

	try {
		const baseUrl = await initApiBaseUrl();
		const result = await request(
			baseUrl,
			EnableApplicationMutation,
			{ input },
			getRequestHeaders(accessToken),
		);
		return result;
	} catch (err) {
		if (err instanceof ClientError) {
			const graphqlErrors = err.response.errors;
			if (graphqlErrors?.length) {
				const error = graphqlErrors[0];
				const errorCode = error.extensions?.code;
				const errorMessage = errorCode
					? `${error.message} (${errorCode})`
					: error.message;
				throw new Error(errorMessage);
			}
			console.log(err);
			throw new Error("An unexpected error occurred");
		}

		throw new Error("An unexpected error occurred");
	}
}

export async function disableApplication(
	input: { applicationName: string; storeId: string },
	{ accessToken }: { accessToken: string | null },
) {
	if (!accessToken) {
		throw new Error("Access token is required");
	}

	try {
		const baseUrl = await initApiBaseUrl();
		const result = await request(
			baseUrl,
			DisableApplicationMutation,
			{ input },
			getRequestHeaders(accessToken),
		);
		return result;
	} catch (err) {
		if (err instanceof ClientError) {
			const graphqlErrors = err.response.errors;
			if (graphqlErrors?.length) {
				const error = graphqlErrors[0];
				const errorCode = error.extensions?.code;
				const errorMessage = errorCode
					? `${error.message} (${errorCode})`
					: error.message;
				throw new Error(errorMessage);
			}
			console.log(err);
			throw new Error("An unexpected error occurred");
		}

		throw new Error("An unexpected error occurred");
	}
}

export async function listApplications({
	accessToken,
}: { accessToken: string | null }) {
	if (!accessToken) {
		throw new Error("Access token is required");
	}

	const baseUrl = await initApiBaseUrl();
	const result = await request(
		baseUrl,
		ApplicationsListQuery,
		{},
		getRequestHeaders(accessToken),
	);
	return result;
}

export async function archiveApplication(
	id: string,
	{ accessToken }: { accessToken: string | null },
) {
	if (!accessToken) {
		throw new Error("Access token is required");
	}

	try {
		const baseUrl = await initApiBaseUrl();
		const result = await request(
			baseUrl,
			ArchiveApplicationMutation,
			{ id },
			getRequestHeaders(accessToken),
		);
		return result;
	} catch (err) {
		if (err instanceof ClientError) {
			const graphqlErrors = err.response.errors;
			if (graphqlErrors?.length) {
				const error = graphqlErrors[0];
				const errorCode = error.extensions?.code;
				const errorMessage = errorCode
					? `${error.message} (${errorCode})`
					: error.message;
				throw new Error(errorMessage);
			}
			console.log(err);
			throw new Error("An unexpected error occurred");
		}

		throw new Error("An unexpected error occurred");
	}
}
