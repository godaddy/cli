//! Validation for `[native_extension]` manifest configuration.

pub(super) fn validate(
    errors: &mut Vec<String>,
    path: &str,
    support_contact: &str,
    android_package_name: &str,
) {
    if !is_valid_email(support_contact) {
        errors.push(format!(
            "{path}.support_contact must be a valid email address (got {support_contact:?})"
        ));
    }
    if android_package_name.is_empty() {
        errors.push(format!("{path}.android_package_name must be non-empty"));
    }
}

/// Match the email shape enforced by DevX Core's Zod v3 `email()` validator.
fn is_valid_email(value: &str) -> bool {
    if value.starts_with('.') || value.contains("..") {
        return false;
    }
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    if domain.contains('@') || local.is_empty() {
        return false;
    }
    let Some(last_local) = local.bytes().next_back() else {
        return false;
    };
    if !local.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'\'' | b'+' | b'-' | b'.')
    }) || !(last_local.is_ascii_alphanumeric() || matches!(last_local, b'_' | b'+' | b'-'))
    {
        return false;
    }

    let mut labels = domain.split('.').peekable();
    let Some(first_label) = labels.next() else {
        return false;
    };
    if labels.peek().is_none() || !is_valid_domain_label(first_label) {
        return false;
    }
    let remaining: Vec<&str> = labels.collect();
    let Some(top_level_domain) = remaining.last() else {
        return false;
    };
    remaining.iter().all(|label| is_valid_domain_label(label))
        && top_level_domain.len() >= 2
        && top_level_domain
            .bytes()
            .all(|byte| byte.is_ascii_alphabetic())
}

fn is_valid_domain_label(label: &str) -> bool {
    label
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::is_valid_email;

    #[test]
    fn rejects_malformed_addresses() {
        for value in [
            "",
            "not-an-email",
            ".support@example.com",
            "support..team@example.com",
            "support@example",
            "support@example.c",
        ] {
            assert!(!is_valid_email(value), "unexpected valid email: {value:?}");
        }
    }

    #[test]
    fn accepts_devx_core_email_shape() {
        for value in [
            "support@example.com",
            "native.app+alerts@sub.example.co.uk",
            "team_member@example.io",
        ] {
            assert!(is_valid_email(value), "unexpected invalid email: {value:?}");
        }
    }
}
