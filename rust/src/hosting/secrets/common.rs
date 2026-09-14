use std::collections::HashSet;

use cli_engine::CliCoreError;
use serde_json::{Value, json};

use crate::error::GddyError;

pub(super) fn add_secret_op(name: &str, value: &str) -> Value {
    json!({ "op": "add", "path": secret_path(name), "value": value })
}

pub(super) fn replace_secret_op(name: &str, value: &str) -> Value {
    json!({ "op": "replace", "path": secret_path(name), "value": value })
}

pub(super) fn remove_secret_op(name: &str) -> Value {
    json!({ "op": "remove", "path": secret_path(name) })
}

pub(super) fn operations_from_sync_flags(
    additions: Option<Value>,
    updates: Option<Value>,
    deletions: Option<Value>,
) -> Result<Vec<Value>, CliCoreError> {
    let mut ops = Vec::new();
    if let Some(raw) = additions {
        ops.extend(ops_from_array(&raw, "add", true, "additions")?);
    }
    if let Some(raw) = updates {
        ops.extend(ops_from_array(&raw, "replace", true, "updates")?);
    }
    if let Some(raw) = deletions {
        ops.extend(ops_from_array(&raw, "remove", false, "deletions")?);
    }
    if ops.is_empty() {
        return Err(GddyError::validation(
            "at least one of --additions, --updates, or --deletions is required",
        )
        .into_cli_error());
    }

    let mut seen = HashSet::new();
    for op in &ops {
        let path = op
            .get("path")
            .and_then(|v| v.as_str())
            .expect("secret patch op has a path");
        if !seen.insert(path.to_owned()) {
            return Err(GddyError::validation(format!(
                "each secret path may appear at most once (duplicate {path})"
            ))
            .into_cli_error());
        }
    }
    Ok(ops)
}

fn secret_path(name: &str) -> String {
    format!("/{}", escape_json_pointer(name))
}

fn escape_json_pointer(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

fn ops_from_array(
    raw: &Value,
    op: &str,
    require_value: bool,
    flag: &str,
) -> Result<Vec<Value>, CliCoreError> {
    let arr = raw.as_array().ok_or_else(|| {
        GddyError::validation(format!("--{flag} must be a JSON array")).into_cli_error()
    })?;
    let mut out = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        let obj = item.as_object().ok_or_else(|| {
            GddyError::validation(format!("--{flag}[{i}] must be an object")).into_cli_error()
        })?;
        let name = obj.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
            GddyError::validation(format!("--{flag}[{i}] is missing a string \"name\""))
                .into_cli_error()
        })?;
        if require_value {
            let value = obj.get("value").ok_or_else(|| {
                GddyError::validation(format!("--{flag}[{i}] is missing \"value\""))
                    .into_cli_error()
            })?;
            out.push(json!({ "op": op, "path": secret_path(name), "value": value }));
        } else {
            out.push(json!({ "op": op, "path": secret_path(name) }));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_escapes_tilde_before_slash() {
        assert_eq!(secret_path("a~b/c"), "/a~0b~1c");
    }

    #[test]
    fn sync_flags_map_to_rfc6902_ops() {
        let ops = operations_from_sync_flags(
            Some(json!([{ "name": "A", "value": "1" }])),
            Some(json!([{ "name": "B", "value": "2" }])),
            Some(json!([{ "name": "C" }])),
        )
        .expect("ops");
        assert_eq!(
            ops,
            vec![
                json!({ "op": "add", "path": "/A", "value": "1" }),
                json!({ "op": "replace", "path": "/B", "value": "2" }),
                json!({ "op": "remove", "path": "/C" }),
            ]
        );
    }

    #[test]
    fn duplicate_paths_are_rejected() {
        let err = operations_from_sync_flags(
            Some(json!([{ "name": "A", "value": "1" }])),
            Some(json!([{ "name": "A", "value": "2" }])),
            None,
        )
        .expect_err("duplicate");
        assert!(err.to_string().contains("at most once"));
    }

    #[test]
    fn empty_arrays_with_no_ops_are_rejected() {
        let err = operations_from_sync_flags(Some(json!([])), None, None).expect_err("empty");
        assert!(err.to_string().contains("at least one"));
    }
}
