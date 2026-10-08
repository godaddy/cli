//! Request-body assembly for `api call`: the raw body from `--file`/`--body`,
//! with `--field` and body-bound `--param` values merged on top.

use cli_engine::CliCoreError;
use serde_json::{Map, Value, json};

use super::http::split_kv;

/// Builds the request body. `--file` takes precedence over `--body`; `--field`
/// values are then merged on top, followed by `param_body` (the `--param`
/// values routed to the body) so a repeated key resolves in `--param`'s favor.
///
/// Merging only makes sense into a JSON object, so supplying any `--field` or
/// body-bound `--param` alongside a non-object fails with a validation error.
pub(super) fn build_request_body(
    file: Option<&str>,
    body: Option<&str>,
    fields: &[String],
    param_body: &[(String, String)],
) -> Result<Option<Value>, CliCoreError> {
    let (mut request_body, source) = if let Some(file_path) = file {
        let content = std::fs::read_to_string(file_path).map_err(|e| {
            crate::error::GddyError::validation(format!("failed to read file '{file_path}': {e}"))
                .into_cli_error()
        })?;
        let parsed = serde_json::from_str(&content).map_err(|e| {
            crate::error::GddyError::validation(format!("invalid JSON in '{file_path}': {e}"))
                .into_cli_error()
        })?;
        (Some(parsed), "--file")
    } else if let Some(body_str) = body {
        let parsed = serde_json::from_str(body_str).map_err(|e| {
            crate::error::GddyError::validation(format!("invalid JSON body: {e}")).into_cli_error()
        })?;
        (Some(parsed), "--body")
    } else {
        (None, "")
    };

    if fields.is_empty() && param_body.is_empty() {
        return Ok(request_body);
    }

    let base = request_body.get_or_insert_with(|| json!({}));
    let Some(obj) = base.as_object_mut() else {
        return Err(crate::error::GddyError::validation(format!(
            "--field/--param values can only be added to a JSON object body, but the {source} \
             body is {}",
            json_kind(base)
        ))
        .with_fix(
            "Make the body a JSON object, or drop --field/--param and put those values in the body",
        )
        .into_cli_error());
    };
    merge_fields(obj, fields, param_body)?;
    Ok(request_body)
}

fn merge_fields(
    obj: &mut Map<String, Value>,
    fields: &[String],
    param_body: &[(String, String)],
) -> Result<(), CliCoreError> {
    for s in fields {
        let (key, val) = split_kv(s).ok_or_else(|| {
            crate::error::GddyError::validation(format!(
                "invalid field format '{s}': expected key=value"
            ))
            .into_cli_error()
        })?;
        obj.insert(key.to_owned(), json!(val));
    }
    for (key, val) in param_body {
        obj.insert(key.clone(), json!(val));
    }
    Ok(())
}

fn json_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::build_request_body;

    fn fields(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn merges_fields_then_params_onto_an_object_body() {
        let body = build_request_body(
            None,
            Some(r#"{"a":"1","b":"2"}"#),
            &fields(&["b=3", "c=4"]),
            &[("c".to_owned(), "5".to_owned())],
        )
        .expect("object body merges");
        assert_eq!(body, Some(json!({"a": "1", "b": "3", "c": "5"})));
    }

    #[test]
    fn fields_alone_build_an_object_body() {
        let body = build_request_body(None, None, &fields(&["a=1"]), &[]).expect("builds");
        assert_eq!(body, Some(json!({"a": "1"})));
    }

    #[test]
    fn non_object_body_without_fields_passes_through() {
        let body = build_request_body(None, Some("[1,2]"), &[], &[]).expect("passes through");
        assert_eq!(body, Some(json!([1, 2])));
    }

    #[test]
    fn rejects_fields_on_a_non_object_body() {
        for raw in ["[1,2]", "\"text\"", "7", "true", "null"] {
            let err = build_request_body(None, Some(raw), &fields(&["a=1"]), &[])
                .expect_err("non-object body must reject --field");
            assert!(err.to_string().contains("JSON object body"), "{raw}: {err}");
        }
    }

    #[test]
    fn rejects_body_bound_params_on_a_non_object_body() {
        let err = build_request_body(None, Some("[]"), &[], &[("a".to_owned(), "1".to_owned())])
            .expect_err("non-object body must reject body-bound --param");
        assert!(err.to_string().contains("an array"), "{err}");
    }

    #[test]
    fn rejects_fields_on_a_non_object_file_body() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("body.json");
        std::fs::write(&path, "[1]").expect("write body file");
        let err = build_request_body(path.to_str(), Some("{}"), &fields(&["a=1"]), &[])
            .expect_err("array file body must reject --field");
        assert!(err.to_string().contains("--file"), "{err}");
    }

    #[test]
    fn rejects_a_malformed_field() {
        let err =
            build_request_body(None, None, &fields(&["nokey"]), &[]).expect_err("malformed field");
        assert!(err.to_string().contains("expected key=value"), "{err}");
    }
}
