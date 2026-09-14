# Native extension DevX Core integration plan

## Goal

Update `gddy platform app add native-extension` so it registers or updates the
native-app draft through DevX Core while keeping the local `[native_extension]`
manifest section synchronized.

## Plan

1. Add a DevX Core native-app client.
   - Implement `GET`, `POST`, and `PATCH /api/v1/native-apps/:appId`.
   - Reuse `environments::devx_core_url()` and the CLI's standard User-Agent.
   - Send the current credential as `Authorization: Bearer <token>`.
   - Parse DevX Core success and error envelopes into stable CLI errors.

2. Enable authentication for `add native-extension`.
   - Remove `.no_auth(true)` from the command definition.
   - Declare `APP_REGISTRY_READ` and `APP_REGISTRY_WRITE` scopes.
   - Resolve the credential lazily through `ctx.credential()`.

3. Resolve the required identifiers.
   - Look up the App Registry application using `config.name`.
   - Use the returned application `id`; `config.client_id` is an OAuth client ID
     and must not be used as the application ID.
   - Obtain `organizationId` through `OnboardingClient::status()` while the
     existing DevX Core request schema requires it.
   - Follow up with DevX Core to remove `organizationId` from the client request,
     because the handler already derives and overrides it from the verified app.

4. Validate and map the native-extension fields.
   - Validate `support_contact` as an email address, matching DevX Core's schema.
   - Map the request body as follows:
     - `name`: `[native_extension].name`, falling back to the application name.
     - `description`: the application description, falling back to an empty
       string.
     - `supportEmail`: `support_contact`.
     - `androidPackageName`: `android_package_name`.
     - `appCategory` and `merchantCategory`: empty strings because those fields
       remain portal-owned.
     - `status`: `draft`.

5. Make registration idempotent.
   - Call `GET /api/v1/native-apps/:appId` first.
   - Call `POST` when no native app exists.
   - Call `PATCH` when a native app already exists.
   - After `POST`, issue the follow-up `PATCH` needed to persist `supportEmail`,
     matching the current DevX Portal workaround.
   - Preserve DevX Core errors such as an immutable package name after release.

6. Coordinate local and remote state.
   - Build and validate the updated TOML before making the remote request.
   - Perform the remote upsert.
   - Write `[native_extension]` locally after the remote operation succeeds.
   - If the remote operation succeeds but the local write fails, return an error
     that explains the remote draft exists and that rerunning the idempotent
     command is safe.

7. Update command documentation and output.
   - Remove wording that describes the command as local-only.
   - Explain that the command immediately registers the DevX Core native draft.
   - Document authentication, application lookup, and rerun behavior.
   - Return the application ID and whether the remote operation created or
     updated the draft.

8. Add automated coverage.
   - Verify request paths, Bearer headers, payloads, response decoding, and error
     mapping in client tests.
   - Test application-name-to-ID resolution.
   - Test `POST` when absent and `PATCH` when present.
   - Test the post-create support-email patch.
   - Test invalid email rejection before network access.
   - Test that a remote failure leaves the local manifest unchanged.
   - Retain the existing argument parsing and TOML round-trip coverage.

9. Run the required verification from `rust/`.
   - `cargo check`
   - `cargo clippy -- -D warnings`
   - `cargo test`
   - `cargo fmt --check`
   - `./scripts/check-module-size.sh`

## Relevant existing code

- CLI command: `rust/src/application/commands/add.rs`
- CLI App Registry client: `rust/src/application/client.rs`
- CLI DevX Core URL resolution: `rust/src/environments/devx_core.rs`
- CLI onboarding client: `rust/src/onboarding/client.rs`
- Portal reference client: `devx-portal/lib/native-apps.ts`
- DevX Core contract: `developer-ecosystem-core/apis/rest/src/contract.ts`
- DevX Core native-app handler:
  `developer-ecosystem-core/apis/rest/src/handlers/native-apps-handler.ts`

## Out of scope

- Changing native APK upload or release lifecycle behavior.
- Calling application-service directly from the CLI.
- Moving portal-owned native-app category management into the CLI.
