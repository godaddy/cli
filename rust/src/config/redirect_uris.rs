use std::collections::BTreeSet;

const MAX_REDIRECT_URI_LENGTH: usize = 2048;

pub(super) fn validate(errors: &mut Vec<String>, redirect_uris: Option<&[String]>, app_url: &str) {
    let Some(redirect_uris) = redirect_uris else {
        return;
    };

    if redirect_uris.len() > 5 {
        errors.push(format!(
            "redirect_uris must contain at most 5 URLs (got {})",
            redirect_uris.len()
        ));
    }

    let app_default = url::Url::parse(app_url).ok();
    let callback_default = app_default
        .as_ref()
        .and_then(|url| url.join("/api/godaddy/callback").ok());
    let mut seen = BTreeSet::new();

    for (index, value) in redirect_uris.iter().enumerate() {
        let path = format!("redirect_uris[{index}]");
        if value.encode_utf16().count() > MAX_REDIRECT_URI_LENGTH {
            errors.push(format!(
                "{path} must be at most {MAX_REDIRECT_URI_LENGTH} characters"
            ));
        }
        let Ok(parsed) = url::Url::parse(value) else {
            errors.push(format!(
                "{path} must be an absolute HTTPS URL (got {value:?})"
            ));
            continue;
        };

        if parsed.scheme() != "https" || parsed.host_str().is_none() {
            errors.push(format!(
                "{path} must be an absolute HTTPS URL (got {value:?})"
            ));
        }
        let authority_has_credentials = value.split_once("://").is_some_and(|(_, rest)| {
            rest.split(['/', '?', '#'])
                .next()
                .is_some_and(|authority| authority.contains('@'))
        });
        if !parsed.username().is_empty() || parsed.password().is_some() || authority_has_credentials
        {
            errors.push(format!("{path} must not contain credentials"));
        }
        if value.contains('#') {
            errors.push(format!("{path} must not contain a fragment"));
        }

        if !seen.insert(value.as_str()) {
            errors.push(format!("{path} duplicates another redirect URI"));
        }
        if app_default.as_ref() == Some(&parsed) || callback_default.as_ref() == Some(&parsed) {
            errors.push(format!(
                "{path} duplicates an automatically registered redirect URI derived from url"
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    fn valid_config() -> Config {
        Config {
            name: "my-app".to_owned(),
            client_id: "550e8400-e29b-41d4-a716-446655440000".to_owned(),
            description: Some("test".to_owned()),
            version: "1.2.3".to_owned(),
            url: "https://example.com".to_owned(),
            proxy_url: "https://proxy.example.com".to_owned(),
            authorization_scopes: vec!["openid".to_owned()],
            redirect_uris: None,
            actions: vec![],
            subscriptions: None,
            dependencies: vec![],
            extensions: None,
            settings: vec![],
        }
    }

    #[test]
    fn round_trip_preserves_omitted_vs_empty() {
        let mut config = valid_config();
        let without_redirects = toml::to_string_pretty(&config).expect("serialize omitted field");
        assert!(!without_redirects.contains("redirect_uris"));

        config.redirect_uris = Some(vec![]);
        let with_empty_redirects =
            toml::to_string_pretty(&config).expect("serialize explicit empty field");
        assert!(with_empty_redirects.contains("redirect_uris = []"));

        config.redirect_uris = Some(vec![
            "https://auth.example.net/oauth/callback".to_owned(),
            "https://staging.example.net/oauth/callback".to_owned(),
        ]);
        let encoded = toml::to_string_pretty(&config).expect("serialize redirect URIs");
        let decoded: Config = toml::from_str(&encoded).expect("parse redirect URIs");
        assert_eq!(decoded.redirect_uris, config.redirect_uris);
    }

    #[test]
    fn accepts_valid_redirect_uris_and_an_explicit_clear() {
        let mut config = valid_config();
        config.redirect_uris = Some(vec![
            "https://auth.example.net/oauth/callback".to_owned(),
            "https://staging.example.net/oauth/callback?tenant=demo".to_owned(),
        ]);
        config.validate().expect("valid redirect URIs should pass");

        config.redirect_uris = Some(vec![]);
        config
            .validate()
            .expect("empty list explicitly clears extras");
    }

    #[test]
    fn rejects_too_many_redirect_uris() {
        let mut config = valid_config();
        config.redirect_uris = Some(
            (0..6)
                .map(|index| format!("https://auth{index}.example.net/callback"))
                .collect(),
        );
        let err = config.validate().expect_err("more than five redirect URIs");
        assert!(err.to_string().contains("at most 5"), "{err}");
    }

    #[test]
    fn rejects_invalid_redirect_uri_shapes() {
        let mut config = valid_config();
        config.redirect_uris = Some(vec![
            "http://auth.example.net/callback".to_owned(),
            "https://user:secret@auth.example.net/callback".to_owned(),
            "https://auth.example.net/callback#complete".to_owned(),
            "/oauth/callback".to_owned(),
        ]);
        let message = config
            .validate()
            .expect_err("invalid redirect URI shapes")
            .to_string();
        assert!(message.contains("redirect_uris[0] must be an absolute HTTPS URL"));
        assert!(message.contains("redirect_uris[1] must not contain credentials"));
        assert!(message.contains("redirect_uris[2] must not contain a fragment"));
        assert!(message.contains("redirect_uris[3] must be an absolute HTTPS URL"));
    }

    #[test]
    fn rejects_duplicate_and_default_redirect_uris() {
        let mut config = valid_config();
        config.redirect_uris = Some(vec![
            "https://auth.example.net/callback".to_owned(),
            "https://auth.example.net/callback".to_owned(),
            "https://example.com".to_owned(),
            "https://example.com/api/godaddy/callback".to_owned(),
        ]);
        let message = config
            .validate()
            .expect_err("duplicates and defaults")
            .to_string();
        assert!(message.contains("redirect_uris[1] duplicates another redirect URI"));
        assert!(message.contains("redirect_uris[2] duplicates an automatically registered"));
        assert!(message.contains("redirect_uris[3] duplicates an automatically registered"));
    }

    #[test]
    fn rejects_root_callback_default_for_path_bearing_app_url() {
        let mut config = valid_config();
        config.url = "https://example.com/app".to_owned();
        config.redirect_uris = Some(vec!["https://example.com/api/godaddy/callback".to_owned()]);

        let message = config
            .validate()
            .expect_err("App Registry resolves the automatic callback from the origin root")
            .to_string();
        assert!(message.contains("duplicates an automatically registered redirect URI"));
    }

    #[test]
    fn accepts_distinct_exact_strings_that_normalize_to_the_same_url() {
        let mut config = valid_config();
        config.redirect_uris = Some(vec![
            "https://auth.example.net".to_owned(),
            "https://auth.example.net/".to_owned(),
        ]);

        config
            .validate()
            .expect("App Registry uniqueness uses the original strings");
    }

    #[test]
    fn rejects_registry_length_limit_and_raw_fragment_marker() {
        let mut config = valid_config();
        config.redirect_uris = Some(vec![
            format!("https://example.net/{}", "a".repeat(2040)),
            "https://auth.example.net/callback#".to_owned(),
        ]);

        let message = config
            .validate()
            .expect_err("registry validation limits")
            .to_string();
        assert!(message.contains("at most 2048 characters"));
        assert!(message.contains("redirect_uris[1] must not contain a fragment"));
    }
}
