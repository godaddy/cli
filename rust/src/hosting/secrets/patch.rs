use serde_json::{Value, json};

use crate::error::GddyError;

pub(super) fn secret_pointer(name: &str) -> String {
    format!("/{}", name.replace('~', "~0").replace('/', "~1"))
}

pub(super) fn build_secret_patch(
    additions: &[(String, String)],
    updates: &[(String, String)],
    deletions: &[String],
) -> Result<Value, GddyError> {
    let mut ops: Vec<Value> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let mut push = |op: Value| -> Result<(), GddyError> {
        let path = op["path"]
            .as_str()
            .expect("patch op always has a string path")
            .to_owned();
        if !seen.insert(path.clone()) {
            return Err(GddyError::validation(format!(
                "secret path {path} appears more than once in this request"
            )));
        }
        ops.push(op);
        Ok(())
    };

    for (name, value) in additions {
        push(json!({
            "op": "add",
            "path": secret_pointer(name),
            "value": value,
        }))?;
    }
    for (name, value) in updates {
        push(json!({
            "op": "replace",
            "path": secret_pointer(name),
            "value": value,
        }))?;
    }
    for name in deletions {
        push(json!({
            "op": "remove",
            "path": secret_pointer(name),
        }))?;
    }

    Ok(Value::Array(ops))
}

pub(super) fn parse_name_value_pairs(
    value: Option<Value>,
    flag: &str,
) -> Result<Vec<(String, String)>, GddyError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let items = value.as_array().ok_or_else(|| {
        GddyError::validation(format!("--{flag} must be a JSON array of objects"))
    })?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let name = required_string(item, "name", flag)?;
        let secret_value = required_string(item, "value", flag)?;
        out.push((name, secret_value));
    }
    Ok(out)
}

pub(super) fn parse_names(value: Option<Value>, flag: &str) -> Result<Vec<String>, GddyError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let items = value.as_array().ok_or_else(|| {
        GddyError::validation(format!("--{flag} must be a JSON array of objects"))
    })?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(required_string(item, "name", flag)?);
    }
    Ok(out)
}

fn required_string(item: &Value, field: &str, flag: &str) -> Result<String, GddyError> {
    item.get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            GddyError::validation(format!("--{flag} items must include a string \"{field}\""))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_add_replace_remove() {
        let patch = build_secret_patch(
            &[("NEW".into(), "n".into())],
            &[("OLD".into(), "o".into())],
            &["GONE".into()],
        )
        .expect("patch");
        assert_eq!(
            patch,
            json!([
                { "op": "add", "path": "/NEW", "value": "n" },
                { "op": "replace", "path": "/OLD", "value": "o" },
                { "op": "remove", "path": "/GONE" },
            ])
        );
    }

    #[test]
    fn escapes_json_pointer_specials() {
        assert_eq!(secret_pointer("FOO/BAR"), "/FOO~1BAR");
        assert_eq!(secret_pointer("A~B"), "/A~0B");
        assert_eq!(secret_pointer("~slash/"), "/~0slash~1");
    }

    #[test]
    fn rejects_duplicate_paths() {
        let err = build_secret_patch(
            &[("KEY".into(), "a".into())],
            &[("KEY".into(), "b".into())],
            &[],
        )
        .expect_err("duplicate");
        assert!(err.to_string().contains("/KEY"));
    }

    #[test]
    fn parse_additions_and_deletions() {
        let additions =
            parse_name_value_pairs(Some(json!([{ "name": "K", "value": "V" }])), "additions")
                .expect("additions");
        assert_eq!(additions, vec![("K".to_owned(), "V".to_owned())]);

        let deletions =
            parse_names(Some(json!([{ "name": "K" }])), "deletions").expect("deletions");
        assert_eq!(deletions, vec!["K".to_owned()]);
    }
}
