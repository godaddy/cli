import { type } from "arktype";
import * as Effect from "effect/Effect";
import { graphql } from "gql.tada";
import { ClientError } from "graphql-request";
import { AuthenticationError, NetworkError } from "../effect/errors";
import { getRequestHeaders, makeGraphQLClientEffect } from "./http-helpers";

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
      releases(first: 1, orderBy: { createdAt: DESC }) {
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
      edges {
        node {
          id
          label
          name
          description
          status
          url
          proxyUrl
        }
      }
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
  authorizationScopes: "string[]?",
});

export const updateApplicationInput = type({
  label: "string?",
  description: "string?",
  status: '"ACTIVE" | "INACTIVE"?',
});

export const actionInput = type({
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

/**
 * Extract the internal error detail from a GraphQL ClientError.
 * This may include server-side messages and extension codes.
 */
function extractGraphQLError(err: unknown): string {
  if (err instanceof ClientError) {
    const graphqlErrors = err.response.errors;
    if (graphqlErrors?.length) {
      const error = graphqlErrors[0];
      const errorCode = error.extensions?.code;
      return errorCode ? `${error.message} (${errorCode})` : error.message;
    }
  }
  return "An unexpected error occurred";
}

/**
 * Return a safe, generic user-facing message for a GraphQL error.
 * Avoids leaking internal server details in the CLI JSON envelope.
 */
function safeGraphQLUserMessage(err: unknown): string {
  if (err instanceof ClientError) {
    const status = err.response.status;
    if (status === 401)
      return "Authentication failed. Run 'godaddy auth login'.";
    if (status === 403)
      return "Access denied. You may not have permission for this operation in the current environment.";
    if (status === 404) return "The requested resource was not found.";
    if (status && status >= 500)
      return "The server encountered an error. Please try again later.";
    // For 4xx with GraphQL-level error messages, allow the first message
    // through since these are validation-style errors the user can act on.
    const graphqlErrors = err.response.errors;
    if (graphqlErrors?.length && graphqlErrors[0].message) {
      return graphqlErrors[0].message;
    }
  }
  return "An unexpected error occurred";
}

export function createApplicationEffect(
  input: typeof applicationInput.infer,
  { accessToken }: { accessToken: string | null },
) {
  return Effect.gen(function* () {
    if (!accessToken) {
      return yield* Effect.fail(
        new AuthenticationError({
          message: "Access token is required",
          userMessage: "Authentication required",
        }),
      );
    }

    const inputParseResult = applicationInput(input);
    if (inputParseResult instanceof type.errors) {
      return yield* Effect.fail(
        new NetworkError({
          message: inputParseResult.summary,
          userMessage: inputParseResult.summary,
        }),
      );
    }

    const client = yield* makeGraphQLClientEffect();

    return yield* Effect.tryPromise({
      try: () =>
        client.request(
          CreateApplicationMutation,
          { input: inputParseResult },
          getRequestHeaders(accessToken),
        ),
      catch: (err) =>
        new NetworkError({
          message: extractGraphQLError(err),
          userMessage: safeGraphQLUserMessage(err),
        }),
    });
  });
}

export function updateApplicationEffect(
  id: string,
  input: typeof updateApplicationInput.infer,
  { accessToken }: { accessToken: string | null },
) {
  return Effect.gen(function* () {
    if (!accessToken) {
      return yield* Effect.fail(
        new AuthenticationError({
          message: "Access token is required",
          userMessage: "Authentication required",
        }),
      );
    }

    const client = yield* makeGraphQLClientEffect();

    return yield* Effect.tryPromise({
      try: () =>
        client.request(
          UpdateApplicationMutation,
          { id, input },
          getRequestHeaders(accessToken),
        ),
      catch: (err) =>
        new NetworkError({
          message: extractGraphQLError(err),
          userMessage: safeGraphQLUserMessage(err),
        }),
    });
  });
}

export function getApplicationEffect(
  name: string,
  { accessToken }: { accessToken: string | null },
) {
  return Effect.gen(function* () {
    if (!accessToken) {
      return yield* Effect.fail(
        new AuthenticationError({
          message: "Access token is required",
          userMessage: "Authentication required",
        }),
      );
    }

    const client = yield* makeGraphQLClientEffect();

    return yield* Effect.tryPromise({
      try: () =>
        client.request(
          ApplicationQuery,
          { name },
          getRequestHeaders(accessToken),
        ),
      catch: (err) =>
        new NetworkError({
          message: extractGraphQLError(err),
          userMessage: safeGraphQLUserMessage(err),
        }),
    });
  });
}

export function getApplicationAndLatestReleaseEffect(
  name: string,
  { accessToken }: { accessToken: string | null },
) {
  return Effect.gen(function* () {
    if (!accessToken) {
      return yield* Effect.fail(
        new AuthenticationError({
          message: "Access token is required",
          userMessage: "Authentication required",
        }),
      );
    }

    const client = yield* makeGraphQLClientEffect();

    return yield* Effect.tryPromise({
      try: () =>
        client.request(
          ApplicationWithLatestReleaseQuery,
          { name },
          getRequestHeaders(accessToken),
        ),
      catch: (err) =>
        new NetworkError({
          message: extractGraphQLError(err),
          userMessage: safeGraphQLUserMessage(err),
        }),
    });
  });
}

export function createReleaseEffect(
  input: typeof releaseInput.infer,
  { accessToken }: { accessToken: string | null },
) {
  return Effect.gen(function* () {
    if (!accessToken) {
      return yield* Effect.fail(
        new AuthenticationError({
          message: "Access token is required",
          userMessage: "Authentication required",
        }),
      );
    }

    const inputParseResult = releaseInput(input);
    if (inputParseResult instanceof type.errors) {
      return yield* Effect.fail(
        new NetworkError({
          message: inputParseResult.summary,
          userMessage: inputParseResult.summary,
        }),
      );
    }

    // Default actions to empty array if undefined
    const releaseData = {
      ...inputParseResult,
      actions: inputParseResult.actions ?? [],
    };

    const client = yield* makeGraphQLClientEffect();

    return yield* Effect.tryPromise({
      try: () =>
        client.request(
          CreateReleaseMutation,
          { input: releaseData },
          getRequestHeaders(accessToken),
        ),
      catch: (err) =>
        new NetworkError({
          message: extractGraphQLError(err),
          userMessage: safeGraphQLUserMessage(err),
        }),
    });
  });
}

export function enableApplicationEffect(
  input: { applicationName: string; storeId: string },
  { accessToken }: { accessToken: string | null },
) {
  return Effect.gen(function* () {
    if (!accessToken) {
      return yield* Effect.fail(
        new AuthenticationError({
          message: "Access token is required",
          userMessage: "Authentication required",
        }),
      );
    }

    const client = yield* makeGraphQLClientEffect();

    return yield* Effect.tryPromise({
      try: () =>
        client.request(
          EnableApplicationMutation,
          { input },
          getRequestHeaders(accessToken),
        ),
      catch: (err) =>
        new NetworkError({
          message: extractGraphQLError(err),
          userMessage: safeGraphQLUserMessage(err),
        }),
    });
  });
}

export function disableApplicationEffect(
  input: { applicationName: string; storeId: string },
  { accessToken }: { accessToken: string | null },
) {
  return Effect.gen(function* () {
    if (!accessToken) {
      return yield* Effect.fail(
        new AuthenticationError({
          message: "Access token is required",
          userMessage: "Authentication required",
        }),
      );
    }

    const client = yield* makeGraphQLClientEffect();

    return yield* Effect.tryPromise({
      try: () =>
        client.request(
          DisableApplicationMutation,
          { input },
          getRequestHeaders(accessToken),
        ),
      catch: (err) =>
        new NetworkError({
          message: extractGraphQLError(err),
          userMessage: safeGraphQLUserMessage(err),
        }),
    });
  });
}

export function listApplicationsEffect({
  accessToken,
}: { accessToken: string | null }) {
  return Effect.gen(function* () {
    if (!accessToken) {
      return yield* Effect.fail(
        new AuthenticationError({
          message: "Access token is required",
          userMessage: "Authentication required",
        }),
      );
    }

    const client = yield* makeGraphQLClientEffect();

    return yield* Effect.tryPromise({
      try: () =>
        client.request(
          ApplicationsListQuery,
          {},
          getRequestHeaders(accessToken),
        ),
      catch: (err) =>
        new NetworkError({
          message: extractGraphQLError(err),
          userMessage: safeGraphQLUserMessage(err),
        }),
    });
  });
}

export function archiveApplicationEffect(
  id: string,
  { accessToken }: { accessToken: string | null },
) {
  return Effect.gen(function* () {
    if (!accessToken) {
      return yield* Effect.fail(
        new AuthenticationError({
          message: "Access token is required",
          userMessage: "Authentication required",
        }),
      );
    }

    const client = yield* makeGraphQLClientEffect();

    return yield* Effect.tryPromise({
      try: () =>
        client.request(
          ArchiveApplicationMutation,
          { id },
          getRequestHeaders(accessToken),
        ),
      catch: (err) =>
        new NetworkError({
          message: extractGraphQLError(err),
          userMessage: safeGraphQLUserMessage(err),
        }),
    });
  });
}
