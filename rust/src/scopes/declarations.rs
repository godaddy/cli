//! The OAuth scopes the CLI requests. To support a new scope:
//!
//! 1. Add the scope to [`declare_scopes!`]: a one-line `///` description (it is
//!    what `gddy auth scope` shows) above `NAME => "scope",`, placed in the
//!    `permissions` block's `default` section (requested at login) or its
//!    `step_up` section (requested only when a command that declares it runs).
//!    Directive scopes that aren't `resource:action` permissions go in the
//!    `directives` block instead. Use `step_up` only when the action spends
//!    money, can take a site offline, deletes a whole billable resource, or is
//!    raw data access. The login request ([`DEFAULT_OAUTH_SCOPES`]) is derived
//!    from these sections, so there is nothing else to keep in sync.
//! 2. Reference the new constant from the command via `.with_scopes(&[scopes::…])`.
//! 3. **Register the same scope on the CLI's OAuth client**, or it will be
//!    ungrantable at runtime.
//!
//! This registry covers the scopes the CLI requests for *itself* (login defaults
//! plus per-command step-up). It intentionally does NOT cover scopes that are
//! user data rather than the CLI's own grants — e.g. the `authorizationScopes`
//! assigned to a third-party app created via `gddy platform app init`, or the
//! ad-hoc scopes `gddy api call --scope …` derives from the API catalog at
//! runtime.
declare_scopes! {
    permissions {
        default {
            /// Read the caller's registered applications
            APP_REGISTRY_READ => "apps.app-registry:read",
            /// Create, update, or archive the caller's registered applications
            APP_REGISTRY_WRITE => "apps.app-registry:write",

            /// Read domains, availability, suggestions, quotes, and DNS records
            DOMAINS_READ => "domains.domain:read",
            /// Create, replace, or delete DNS records
            DOMAINS_DNS_UPDATE => "domains.dns:update",

            /// Read hosting applications
            HOSTING_APPLICATION_READ => "hosting.application:read",
            /// Create a hosting application
            HOSTING_APPLICATION_CREATE => "hosting.application:create",
            /// Update a hosting application
            HOSTING_APPLICATION_UPDATE => "hosting.application:update",
            /// Trigger deployments and restarts
            HOSTING_DEPLOYMENT_EXECUTE => "hosting.deployment:execute",
            /// Read source imports for hosting applications
            HOSTING_SOURCE_READ => "hosting.source:read",
            /// Create source imports for hosting applications
            HOSTING_SOURCE_WRITE => "hosting.source:write",
            /// Read application secret names
            HOSTING_SECRET_READ => "hosting.secret:read",
            /// Create, update, or delete application secrets
            HOSTING_SECRET_WRITE => "hosting.secret:write",
            /// Read application logs
            HOSTING_LOG_READ => "hosting.log:read",
            /// Read domains attached to hosting applications
            HOSTING_DOMAIN_READ => "hosting.domain:read",
            /// Attach or detach domains from hosting applications
            HOSTING_DOMAIN_WRITE => "hosting.domain:write",
            /// Read hosting subscriptions
            HOSTING_SUBSCRIPTION_READ => "hosting.subscription:read",
            /// Attach a hosting subscription to an application
            HOSTING_SUBSCRIPTION_WRITE => "hosting.subscription:write",

            /// Read mailboxes and check mailbox-creation eligibility
            EMAIL_READ => "email.mailbox:read",
            /// Create a mailbox
            EMAIL_CREATE => "email.mailbox:create",

            /// Trigger an on-demand malware scan
            SECURITY_MALWARE_SCAN => "security.website-security-scan:execute",

            /// Browse and resolve Shopping catalog products
            SHOPPING_CATALOG_READ => "shopping.catalog:read",
        }
        step_up {
            // Spends money.
            /// Register a domain
            DOMAINS_CREATE => "domains.domain:create",
            // Can take a live site offline.
            /// Replace a domain's nameservers
            DOMAINS_NAMESERVER_UPDATE => "domains.nameserver:update",

            // Deletes a whole billable resource.
            /// Delete a hosting application
            HOSTING_APPLICATION_DELETE => "hosting.application:delete",
            // Raw data access. A dedicated grant, separate from
            // HOSTING_DEPLOYMENT_EXECUTE: authority to publish a deployment does
            // not by itself grant raw database read/write.
            /// Open a MySQL tunnel to a Node.js or Managed WordPress app's database
            HOSTING_DATABASE_TUNNEL_EXECUTE => "hosting.database.tunnel:execute",

            // Spends money.
            /// Create, update, read, and complete Shopping checkout sessions
            SHOPPING_CHECKOUT_EXECUTE => "shopping.checkout:execute",
            // Granted alongside SHOPPING_CHECKOUT_EXECUTE so the order read after
            // a purchase is covered.
            /// Read completed Shopping orders
            SHOPPING_ORDER_READ => "shopping.order:read",
        }
    }
    // OIDC directives, not `resource:action` grants. They still must be
    // registered on the OAuth client server-side, or the authorization server
    // will refuse or silently drop them like any other unregistered scope.
    directives {
        // Requests a `refresh_token` alongside the access token.
        /// Request a refresh token
        OFFLINE_ACCESS => "offline_access",
        // Marks the request as an identity (not just authorization) request —
        // some IDPs only populate profile claims when this is present alongside
        // PROFILE.
        /// Mark the request as an OIDC identity request
        OPENID => "openid",
        // Standard profile claims (`name`, `given_name`, `preferred_username`, ...).
        /// Request standard OIDC profile claims on the token
        PROFILE => "profile",
    }
}
