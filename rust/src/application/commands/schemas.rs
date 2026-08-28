//! Output schemas shared across `gddy platform app` commands.

use crate::output_schema::output_schema;

output_schema!(ApplicationSummary {
    "id": "string";
    "name": "string";
    "label": "string", optional;
    "description": "string", optional;
    "status": "string";
    "url": "string", optional;
    "proxyUrl": "string", optional;
});

output_schema!(ApplicationInit {
    "id": "string";
    "name": "string";
    "status": "string";
    "clientId": "string";
    "orgId": "string";
    "url": "string";
    "proxyUrl": "string";
    "authorizationScopes": "[]string";
    "oauthGrantTypes": "[]string";
    "filesWritten": "object";
});

output_schema!(ApplicationUpdate {
    "id": "string";
    "clientId": "string";
    "name": "string";
    "label": "string", optional;
    "description": "string", optional;
    "status": "string";
    "url": "string", optional;
    "proxyUrl": "string", optional;
    "authorizationScopes": "[]string";
});

output_schema!(ApplicationRef {
    "id": "string";
});

output_schema!(ApplicationArchive {
    "id": "string";
    "name": "string";
    "label": "string", optional;
    "status": "string";
    "createdAt": "string";
    "archivedAt": "string";
});

output_schema!(ApplicationRelease {
    "id": "string";
    "version": "string";
    "description": "string", optional;
    "createdAt": "string";
    "nativeExtensions": "[]object", optional;
});

output_schema!(ValidationResult {
    "valid": "bool";
    "errors": "[]string";
    "warnings": "[]string";
});

output_schema!(ConfigAction {
    "name": "string";
    "url": "string";
});

output_schema!(ConfigSubscription {
    "name": "string";
    "url": "string";
    "events": "[]string";
});

output_schema!(ExtensionHandle {
    "name": "string";
    "handle": "string";
    "type": "string";
});

output_schema!(ExtensionBlocks {
    "source": "string";
    "type": "string";
});

output_schema!(ConfigSetting {
    "group": "string";
    "slug": "string";
    "entryPath": "string";
});

output_schema!(ConfigNativeExtension {
    "name": "string", optional;
    "supportContact": "string";
    "androidPackageName": "string";
});
