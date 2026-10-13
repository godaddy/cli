//! Refreshes the vendored Shopping OpenAPI document used by `shopping-client`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Map, Value};

const HOST: &str = "https://api.godaddy.com";

pub(crate) fn shopping_client_oas3_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../shopping-client/openapi/shopping.oas3.json")
}

/// Fully dereferences the approved Shopping contract and applies the small
/// compatibility normalization needed by Progenitor 0.14, which consumes OAS
/// 3.0 documents. The source contract is OAS 3.1 but does not use 3.1-only
/// schema constructs in the generated API surface.
pub(crate) fn refresh(
    spec_path: &Path,
    common_types: Option<&Path>,
    out_path: &Path,
) -> Result<()> {
    let (mut spec, defs) = crate::dereference::load_and_dereference(spec_path, common_types)
        .with_context(|| {
            format!(
                "failed to dereference Shopping spec {}",
                spec_path.display()
            )
        })?;
    let object = spec
        .as_object_mut()
        .context("Shopping spec is not a JSON object")?;
    let schemas = object
        .entry("components")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .context("Shopping spec components is not an object")?
        .entry("schemas")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .context("Shopping spec components.schemas is not an object")?;
    for (name, definition) in defs {
        schemas.insert(name, definition);
    }
    crate::spec_normalize::rewrite_defs_refs(&mut spec);
    crate::spec_normalize::remove_remaining_relative_refs(&mut spec);
    crate::spec_normalize::remove_self_referencing_schemas(&mut spec);
    dealias_name_colliding_schemas(
        &mut spec,
        &[(
            "get_product_response",
            "catalog_lookup_get_product_response",
        )],
    );

    let object = spec
        .as_object_mut()
        .context("Shopping spec is not a JSON object")?;
    object.insert("openapi".to_owned(), Value::String("3.0.3".to_owned()));
    object.insert(
        "servers".to_owned(),
        serde_json::json!([{ "url": HOST, "description": "Shopping API host" }]),
    );
    crate::spec_normalize::prefix_paths(&mut spec, "/v1/shopping")?;
    remove_protocol_header_parameters(&mut spec);
    crate::spec_normalize::retain_2xx_responses(&mut spec);
    relax_response_schemas(&mut spec);
    preserve_dynamic_ucp_response_metadata(&mut spec)?;
    preserve_payment_instrument_fields(&mut spec)?;
    add_completion_consent(&mut spec)?;
    add_idempotency_headers(&mut spec)?;
    prune_documentation_fields(&mut spec);
    crate::spec_normalize::add_required_response_descriptions(&mut spec);

    let json = serde_json::to_string_pretty(&spec).context("serialize Shopping codegen spec")?;
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    std::fs::write(out_path, json).with_context(|| format!("write {}", out_path.display()))?;
    eprintln!("   wrote {}", out_path.display());
    Ok(())
}

/// Progenitor names an operation's `oneOf` response enum from its
/// `operationId` (`get_product` -> `GetProductResponse`), independently of
/// how it names the schema-alias newtype it generates for a bare `$ref`
/// schema (`get_product_response` -> `GetProductResponse`). When those two
/// derived names collide, progenitor silently reuses the schema newtype as
/// the operation's response type instead of generating the `oneOf` enum,
/// so the `error_response` branch has no Rust variant to match against at
/// all — unlike every other operation in this contract, whose schema and
/// operationId names differ enough not to collide. Point every reference
/// to `alias` directly at what it aliases and drop the now-unreferenced
/// alias schema, freeing its derived name for the operation's own enum.
fn dealias_name_colliding_schemas(spec: &mut Value, aliases: &[(&str, &str)]) {
    for (alias, target) in aliases {
        let from = format!("#/components/schemas/{alias}");
        let to = format!("#/components/schemas/{target}");
        replace_ref(spec, &from, &to);
        if let Some(schemas) = spec
            .pointer_mut("/components/schemas")
            .and_then(Value::as_object_mut)
        {
            schemas.remove(*alias);
        }
    }
}

fn replace_ref(value: &mut Value, from: &str, to: &str) {
    match value {
        Value::Object(map) => {
            if map.get("$ref").and_then(Value::as_str) == Some(from) {
                map.insert("$ref".to_owned(), Value::String(to.to_owned()));
            }
            for child in map.values_mut() {
                replace_ref(child, from, to);
            }
        }
        Value::Array(values) => {
            for value in values {
                replace_ref(value, from, to);
            }
        }
        _ => {}
    }
}

fn remove_protocol_header_parameters(spec: &mut Value) {
    let Some(paths) = spec.pointer_mut("/paths").and_then(Value::as_object_mut) else {
        return;
    };
    for item in paths.values_mut().filter_map(Value::as_object_mut) {
        for method in ["get", "post", "put", "delete", "patch"] {
            let Some(parameters) = item
                .get_mut(method)
                .and_then(|operation| operation.get_mut("parameters"))
                .and_then(Value::as_array_mut)
            else {
                continue;
            };
            parameters.retain(|parameter| {
                !matches!(
                    parameter.get("name").and_then(Value::as_str),
                    Some("UCP-Agent") | Some("Request-Id") | Some("Idempotency-Key")
                )
            });
        }
    }
}

fn preserve_dynamic_ucp_response_metadata(spec: &mut Value) -> Result<()> {
    let schemas = spec
        .get_mut("components")
        .and_then(|components| components.get_mut("schemas"))
        .and_then(Value::as_object_mut)
        .context("Shopping component schemas are missing")?;
    for name in [
        "ucp_response_catalog_schema",
        "ucp_response_checkout_schema",
        "ucp_response_order_schema",
        "ucp_success",
    ] {
        let properties = schemas
            .get_mut(name)
            .and_then(|schema| schema.get_mut("allOf"))
            .and_then(Value::as_array_mut)
            .and_then(|schemas| schemas.get_mut(0))
            .and_then(|schema| schema.get_mut("properties"))
            .and_then(Value::as_object_mut)
            .with_context(|| format!("Shopping {name} properties are missing"))?;
        // Deployed UCP responses use capabilities as an array while the
        // published schema describes a keyed object. Keep metadata dynamic so
        // the typed client preserves the response for CLI projection.
        properties.insert("capabilities".to_owned(), serde_json::json!({}));
    }
    Ok(())
}

fn preserve_payment_instrument_fields(spec: &mut Value) -> Result<()> {
    let properties = spec
        .get_mut("components")
        .and_then(|components| components.get_mut("schemas"))
        .and_then(|schemas| schemas.get_mut("payment_instrument_selected_payment_instrument"))
        .and_then(|schema| schema.get_mut("allOf"))
        .and_then(Value::as_array_mut)
        .and_then(|schemas| schemas.get_mut(1))
        .and_then(|schema| schema.get_mut("properties"))
        .and_then(Value::as_object_mut)
        .context("Shopping selected payment instrument properties are missing")?;
    // The contract's dynamic instrument reference cannot be represented by
    // Progenitor. Retain the fields required by deployed APIs, including the
    // standard postal address used by the completion billing-address input,
    // and the handler routing metadata (`handler_id`, `type`) that a
    // checkout update must echo back unchanged to keep the selected
    // instrument routable.
    properties.insert("id".to_owned(), serde_json::json!({"type": "string"}));
    properties.insert(
        "billing_address".to_owned(),
        serde_json::json!({"type": "object", "additionalProperties": true}),
    );
    properties.insert(
        "handler_id".to_owned(),
        serde_json::json!({"type": "string"}),
    );
    properties.insert("type".to_owned(), serde_json::json!({"type": "string"}));
    Ok(())
}

fn add_completion_consent(spec: &mut Value) -> Result<()> {
    let schemas = spec
        .get_mut("components")
        .and_then(|components| components.get_mut("schemas"))
        .and_then(Value::as_object_mut)
        .context("Shopping component schemas are missing")?;

    schemas.insert(
        "shopping_consent_acceptance".to_owned(),
        serde_json::json!({
            "type": "object",
            "properties": {
                "agreement_types": {
                    "type": "array",
                    "items": {"type": "string"}
                },
                "agreed_at": {"type": "string"}
            }
        }),
    );
    schemas.insert(
        "shopping_required_agreement".to_owned(),
        serde_json::json!({
            "type": "object",
            "properties": {
                "key": {"type": "string"},
                "title": {"type": "string"},
                "url": {"type": "string"},
                "content": {"type": "string"},
                "required": {"type": "boolean"}
            }
        }),
    );

    let completion_properties = schemas
        .get_mut("checkout-complete-request_schema")
        .and_then(|schema| schema.get_mut("properties"))
        .and_then(Value::as_object_mut)
        .context("Shopping completion request properties are missing")?;
    completion_properties.insert(
        "consent".to_owned(),
        serde_json::json!({"$ref": "#/components/schemas/shopping_consent_acceptance"}),
    );

    let checkout_properties = schemas
        .get_mut("checkout")
        .and_then(|schema| schema.get_mut("properties"))
        .and_then(Value::as_object_mut)
        .context("Shopping checkout response properties are missing")?;
    checkout_properties.insert(
        "required_agreements".to_owned(),
        serde_json::json!({
            "type": "array",
            "items": {"$ref": "#/components/schemas/shopping_required_agreement"}
        }),
    );
    Ok(())
}

fn add_idempotency_headers(spec: &mut Value) -> Result<()> {
    let paths = spec
        .get_mut("paths")
        .and_then(Value::as_object_mut)
        .context("Shopping spec has no paths")?;
    for (path, method) in [
        ("/v1/shopping/checkout-sessions", "post"),
        ("/v1/shopping/checkout-sessions/{id}", "put"),
        ("/v1/shopping/checkout-sessions/{id}/complete", "post"),
    ] {
        let operation = paths
            .get_mut(path)
            .and_then(|path| path.get_mut(method))
            .and_then(Value::as_object_mut)
            .with_context(|| format!("Shopping {method} {path} operation is missing"))?;
        let parameters = operation
            .entry("parameters")
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .with_context(|| format!("Shopping {method} {path} parameters are not an array"))?;
        if parameters.iter().any(|parameter| {
            parameter.get("name").and_then(Value::as_str) == Some("Idempotency-Key")
        }) {
            continue;
        }
        parameters.push(serde_json::json!({
            "name": "Idempotency-Key",
            "in": "header",
            "required": true,
            "schema": {"type": "string"}
        }));
    }
    Ok(())
}

/// Removes source-contract prose and verbose object examples that are irrelevant
/// to typed Rust client generation. Restore generic required response descriptions
/// afterward, and retain protocol metadata plus string-array examples.
///
/// A JSON Schema `properties` map, and `components.schemas`, are keyed by
/// field/schema *names* rather than documentation metadata, even though a
/// name can collide with a metadata keyword (e.g. the Shopping contract has
/// a `product.properties.description` field and a schema literally named
/// `description`). Pruning must never remove those keys — only recurse into
/// their values.
fn prune_documentation_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("description");
            map.remove("summary");
            map.remove("example");
            map.remove("externalDocs");
            if !map
                .get("examples")
                .is_some_and(|examples| matches!(examples, Value::Array(values) if values.iter().all(Value::is_string)))
            {
                map.remove("examples");
            }
            for (key, child) in map.iter_mut() {
                if key == "properties" || key == "schemas" {
                    prune_named_schema_map(child);
                } else {
                    prune_documentation_fields(child);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                prune_documentation_fields(value);
            }
        }
        _ => {}
    }
}

/// Recurses into the *values* of a name-keyed schema map (a Schema Object's
/// `properties`, or `components.schemas`) without pruning the map's own
/// keys, which are field/schema names rather than documentation metadata.
fn prune_named_schema_map(value: &mut Value) {
    if let Value::Object(map) = value {
        for child in map.values_mut() {
            prune_documentation_fields(child);
        }
    }
}

fn relax_response_schemas(spec: &mut Value) {
    let Some(schemas) = spec
        .pointer_mut("/components/schemas")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    for schema in schemas.values_mut() {
        relax(schema);
    }
    if matches!(schemas.get("schema"), Some(Value::Bool(_))) {
        schemas.insert("schema".to_owned(), serde_json::json!({}));
    }
}

fn relax(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("required");
            map.remove("format");
            if map.get("$ref").and_then(Value::as_str) == Some("#") {
                map.clear();
            }
            map.remove("pattern");
            if let Some(Value::Array(types)) = map.get("type") {
                if let Some(Value::String(value_type)) =
                    types.iter().find(|value| value.is_string())
                {
                    map.insert("type".to_owned(), Value::String(value_type.clone()));
                } else {
                    map.remove("type");
                }
            }
            for child in map.values_mut() {
                relax(child);
            }
        }
        Value::Array(values) => {
            for value in values {
                relax(value);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        dealias_name_colliding_schemas, preserve_payment_instrument_fields,
        prune_documentation_fields,
    };

    #[test]
    fn pruning_removes_documentation_and_examples_recursively() {
        let mut spec = json!({
            "summary": "operation summary",
            "description": "operation description",
            "externalDocs": {"url": "https://example.test"},
            "paths": {
                "/things": {
                    "get": {
                        "description": "get things",
                        "responses": {
                            "200": {
                                "description": "response description",
                                "content": {
                                    "application/json": {
                                        "examples": {"one": {"value": {"id": "1"}}},
                                        "schema": {
                                            "type": "object",
                                            "properties": {
                                                "id": {"type": "string", "examples": ["1", "2"]}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "title": "Shopping API",
            "servers": [{"url": "https://api.godaddy.com"}]
        });

        prune_documentation_fields(&mut spec);

        assert_eq!(spec["title"], "Shopping API");
        assert_eq!(spec["servers"][0]["url"], "https://api.godaddy.com");
        assert!(
            spec.pointer("/paths/~1things/get/responses/200/description")
                .is_none()
        );
        assert!(spec.pointer("/paths/~1things/get/description").is_none());
        assert!(!spec.to_string().contains("summary"));
        assert_eq!(
            spec.pointer("/paths/~1things/get/responses/200/content/application~1json/schema/properties/id/examples"),
            Some(&json!(["1", "2"]))
        );
        assert!(!spec.to_string().contains("example\":"));
        assert!(!spec.to_string().contains("externalDocs"));
    }

    #[test]
    fn pruning_preserves_property_and_schema_names_that_collide_with_metadata_keywords() {
        let mut spec = json!({
            "components": {
                "schemas": {
                    "description": {"type": "string"},
                    "product": {
                        "type": "object",
                        "properties": {
                            "description": {
                                "description": "Human-readable product description.",
                                "$ref": "#/components/schemas/description"
                            },
                            "summary": {"description": "Short blurb.", "type": "string"}
                        }
                    }
                }
            }
        });

        prune_documentation_fields(&mut spec);

        assert!(
            spec.pointer("/components/schemas/description").is_some(),
            "a schema literally named `description` must survive pruning"
        );
        assert_eq!(
            spec.pointer("/components/schemas/product/properties/description/$ref"),
            Some(&json!("#/components/schemas/description")),
            "a `description` field on `product` must survive pruning"
        );
        assert!(
            spec.pointer("/components/schemas/product/properties/summary")
                .is_some(),
            "a `summary` field on `product` must survive pruning"
        );
        assert!(
            spec.pointer("/components/schemas/product/properties/description/description")
                .is_none(),
            "the nested schema's own doc string must still be pruned"
        );
    }

    #[test]
    fn preserving_payment_instrument_fields_adds_handler_routing_metadata() {
        let mut spec = json!({
            "components": {
                "schemas": {
                    "payment_instrument_selected_payment_instrument": {
                        "allOf": [
                            {},
                            {
                                "type": "object",
                                "properties": {
                                    "selected": {"type": "boolean"}
                                }
                            }
                        ]
                    }
                }
            }
        });

        preserve_payment_instrument_fields(&mut spec).expect("schema shape is present");

        let properties = spec
            .pointer("/components/schemas/payment_instrument_selected_payment_instrument/allOf/1/properties")
            .expect("properties");
        assert_eq!(properties["id"]["type"], "string");
        assert_eq!(properties["billing_address"]["type"], "object");
        assert_eq!(properties["billing_address"]["additionalProperties"], true);
        assert_eq!(
            properties["handler_id"]["type"], "string",
            "handler_id must survive so a checkout update can echo the selected instrument's routing metadata back unchanged"
        );
        assert_eq!(
            properties["type"]["type"], "string",
            "type must survive so a checkout update can echo the selected instrument's routing metadata back unchanged"
        );
        assert_eq!(properties["selected"]["type"], "boolean");
    }

    #[test]
    fn dealiasing_points_references_at_the_target_and_drops_the_alias_schema() {
        let mut spec = json!({
            "components": {
                "schemas": {
                    "get_product_response": {"$ref": "#/components/schemas/catalog_lookup_get_product_response"},
                    "catalog_lookup_get_product_response": {"type": "object"}
                }
            },
            "paths": {
                "/product": {
                    "post": {
                        "responses": {
                            "200": {
                                "content": {
                                    "application/json": {
                                        "schema": {
                                            "oneOf": [
                                                {"$ref": "#/components/schemas/get_product_response"},
                                                {"$ref": "#/components/schemas/error_response"}
                                            ]
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        dealias_name_colliding_schemas(
            &mut spec,
            &[(
                "get_product_response",
                "catalog_lookup_get_product_response",
            )],
        );

        assert!(
            spec.pointer("/components/schemas/get_product_response")
                .is_none(),
            "the alias schema must be removed so its derived name is free for the operation's own response enum"
        );
        assert_eq!(
            spec.pointer(
                "/paths/~1product/post/responses/200/content/application~1json/schema/oneOf/0/$ref"
            ),
            Some(&json!(
                "#/components/schemas/catalog_lookup_get_product_response"
            )),
            "the oneOf branch must point directly at the real schema"
        );
        assert_eq!(
            spec.pointer(
                "/paths/~1product/post/responses/200/content/application~1json/schema/oneOf/1/$ref"
            ),
            Some(&json!("#/components/schemas/error_response")),
            "the untouched error_response branch must be left alone"
        );
    }
}
