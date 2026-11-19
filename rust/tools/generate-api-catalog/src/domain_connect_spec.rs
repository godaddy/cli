//! Adds the Domain Connect Templates operations to the `domains-client`
//! codegen spec. They are served from the same host as the Domains API, under
//! `/v2/domains`, so one generated client covers both.

use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};

use crate::domains_spec::{load_dereferenced, only_2xx_paths, relax};

/// Baked into the merged paths the same way `/v3/domains` is, so one host
/// `base_url` serves every operation in the generated client.
const BASE_PATH: &str = "/v2/domains";

/// Request bodies the CLI constructs; each carries the template.
const TEMPLATE_INPUTS: &[&str] = &["TemplateValidationCriteria", "TemplateSubmissionInput"];

/// Kept exactly as upstream declares them, like the inputs above: a result or
/// issue missing a required field must fail to deserialize rather than read as
/// an empty success.
const STRICT: &[&str] = &["TemplateActionResult", "TemplateIssue"];

/// The component sections a success-only operation can still reference.
const SECTIONS: &[&str] = &["schemas", "parameters", "headers", "securitySchemes"];

pub(crate) fn merge_into(
    doc: &mut Value,
    spec_path: &Path,
    common_types: Option<&Path>,
) -> Result<()> {
    let spec = load_dereferenced(spec_path, common_types)?;
    merge(doc, spec)
}

fn merge(doc: &mut Value, mut spec: Value) -> Result<()> {
    crate::spec_normalize::strip_external_formats(&mut spec, true);
    send_template_as_written(&mut spec)?;

    let spec_obj = spec
        .as_object_mut()
        .context("Domain Connect spec is not a JSON object")?;
    let paths_in = spec_obj
        .remove("paths")
        .and_then(|paths| paths.as_object().cloned())
        .context("Domain Connect spec missing paths")?;
    let mut paths = Map::new();
    for (key, item) in paths_in {
        if key.starts_with('/') {
            paths.insert(format!("{BASE_PATH}{key}"), item);
        }
    }
    let mut paths = Value::Object(paths);
    only_2xx_paths(&mut paths);

    let components = spec_obj
        .remove("components")
        .and_then(|components| components.as_object().cloned())
        .unwrap_or_default();
    let doc_components = doc
        .as_object_mut()
        .context("domains spec is not a JSON object")?
        .entry("components")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .context("domains components is not a JSON object")?;
    for (section, members) in components {
        let Value::Object(members) = members else {
            continue;
        };
        if !SECTIONS.contains(&section.as_str()) {
            continue;
        }
        let is_schemas = section == "schemas";
        let existing = doc_components
            .entry(section)
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .context("domains component section is not a JSON object")?;
        for (name, mut body) in members {
            let strict =
                STRICT.contains(&name.as_str()) || TEMPLATE_INPUTS.contains(&name.as_str());
            if is_schemas && !strict {
                relax(&mut body, "nullable");
            }
            match existing.get(&name) {
                None => {
                    existing.insert(name, body);
                }
                // Shared pieces (the request-id header, common types) are
                // already present from the Domains spec; keep that copy.
                Some(current) if !is_schemas || *current == body => {}
                Some(_) => bail!(
                    "Domain Connect schema '{name}' differs from the Domains schema of the same name"
                ),
            }
        }
    }

    let doc_paths = doc
        .pointer_mut("/paths")
        .and_then(Value::as_object_mut)
        .context("domains spec missing paths")?;
    if let Value::Object(paths) = paths {
        for (path, item) in paths {
            if doc_paths.insert(path.clone(), item).is_some() {
                bail!("Domain Connect path '{path}' is already defined by the Domains spec");
            }
        }
    }
    Ok(())
}

/// Deliberate deviation: a request's `template` becomes a free-form object, so
/// the generated client sends the user's file exactly as written. The upstream
/// template schema is open by design, and a typed struct would drop fields it
/// does not model.
fn send_template_as_written(spec: &mut Value) -> Result<()> {
    for name in TEMPLATE_INPUTS {
        let template = spec
            .pointer_mut(&format!("/components/schemas/{name}/properties/template"))
            .with_context(|| format!("Domain Connect spec has no {name}.template"))?;
        *template = serde_json::json!({ "type": "object", "additionalProperties": true });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn domains_doc() -> Value {
        json!({
            "paths": { "/v3/domains/suggestions": { "get": {} } },
            "components": {
                "parameters": { "xRequestId": { "name": "X-Request-Id", "in": "header" } },
                "schemas": { "uuid": { "type": "string" } }
            }
        })
    }

    fn domain_connect_spec() -> Value {
        json!({
            "paths": {
                "x-visibility": { "extent": "public" },
                "/submit-domain-connect-template": {
                    "post": {
                        "operationId": "submitDomainConnectTemplate",
                        "responses": { "200": {}, "409": {}, "422": {} }
                    }
                }
            },
            "components": {
                "parameters": { "xRequestId": { "name": "X-Request-Id", "in": "header", "description": "differs" } },
                "responses": { "429": { "description": "Rate limit exceeded." } },
                "schemas": {
                    "uuid": { "type": "string", "format": "uuid" },
                    "TemplateValidationCriteria": {
                        "type": "object",
                        "required": ["template"],
                        "properties": { "template": { "$ref": "#/components/schemas/Template" } }
                    },
                    "TemplateSubmissionInput": {
                        "type": "object",
                        "required": ["template"],
                        "properties": { "template": { "$ref": "#/components/schemas/Template" } }
                    },
                    "TemplatePullRequest": {
                        "type": "object",
                        "required": ["url"],
                        "properties": { "url": { "type": "string" } }
                    }
                }
            }
        })
    }

    #[test]
    fn merge_prefixes_paths_and_keeps_only_success_responses() {
        let mut doc = domains_doc();
        merge(&mut doc, domain_connect_spec()).expect("merge");

        let op = doc
            .pointer("/paths/~1v2~1domains~1submit-domain-connect-template/post")
            .expect("prefixed path");
        assert_eq!(
            op["responses"]
                .as_object()
                .expect("responses")
                .keys()
                .collect::<Vec<_>>(),
            ["200"]
        );
        assert!(doc.pointer("/paths/~1v3~1domains~1suggestions").is_some());
        assert!(doc.pointer("/paths/x-visibility").is_none());
    }

    #[test]
    fn merge_sends_the_template_as_a_free_form_object() {
        let mut doc = domains_doc();
        merge(&mut doc, domain_connect_spec()).expect("merge");

        let input = &doc["components"]["schemas"]["TemplateSubmissionInput"];
        assert_eq!(
            input["properties"]["template"],
            json!({ "type": "object", "additionalProperties": true })
        );
        assert_eq!(input["required"], json!(["template"]));
        assert!(
            doc["components"]["schemas"]["TemplatePullRequest"]
                .get("required")
                .is_none()
        );
    }

    #[test]
    fn merge_keeps_shared_components_from_the_domains_spec() {
        let mut doc = domains_doc();
        merge(&mut doc, domain_connect_spec()).expect("merge");

        assert!(
            doc["components"]["parameters"]["xRequestId"]
                .get("description")
                .is_none()
        );
        assert_eq!(
            doc["components"]["schemas"]["uuid"],
            json!({ "type": "string" })
        );
        assert!(doc["components"].get("responses").is_none());
    }

    #[test]
    fn merge_fails_when_an_input_has_no_template() {
        let mut doc = domains_doc();
        let mut spec = domain_connect_spec();
        spec["components"]["schemas"]
            .as_object_mut()
            .expect("schemas")
            .remove("TemplateValidationCriteria");

        let err = merge(&mut doc, spec).expect_err("missing input");
        assert!(
            err.to_string().contains("TemplateValidationCriteria"),
            "{err}"
        );
    }

    #[test]
    fn merge_rejects_a_schema_that_conflicts_with_the_domains_spec() {
        let mut doc = domains_doc();
        let mut spec = domain_connect_spec();
        spec["components"]["schemas"]["uuid"] = json!({ "type": "integer" });

        let err = merge(&mut doc, spec).expect_err("conflict");
        assert!(err.to_string().contains("uuid"), "{err}");
    }
}
