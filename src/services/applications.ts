import { AuthenticationError, ValidationError } from "@/effect/errors";
import { mapGraphQLError } from "@/services/graphql-error";
import { type } from "arktype";
import * as Effect from "effect/Effect";
import { graphql } from "gql.tada";
import { getRequestHeaders, makeGraphQLClientEffect } from "./http-helpers";
import { publicHttpUrl } from "./public-url";

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
  url: publicHttpUrl,
  proxyUrl: publicHttpUrl,
  authorizationScopes: type.string.array().moreThanLength(0),
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
        new ValidationError({
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
      catch: mapGraphQLError,
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
      catch: mapGraphQLError,
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
      catch: mapGraphQLError,
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
      catch: mapGraphQLError,
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
        new ValidationError({
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
      catch: mapGraphQLError,
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
      catch: mapGraphQLError,
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
      catch: mapGraphQLError,
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
      catch: mapGraphQLError,
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
      catch: mapGraphQLError,
    });
  });
}
