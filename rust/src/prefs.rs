//! User preferences stored in `config.toml` and managed with `gddy config`.
//!
//! Each field is a key users can `gddy config set`; doc comments are shown by
//! `gddy config list`. A preference is only a default: the matching flag and
//! `GDDY_*` environment variable override it per invocation.

use cli_engine::CommandContext;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::error::GddyError;

#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub(crate) struct Prefs {
    /// Show domain names in their ASCII (punycode) form, e.g. xn--mnchen-3ya.de, instead of
    /// Unicode. Overridden by `--punycode` and the GDDY_PUNYCODE environment variable.
    pub(crate) punycode: bool,

    /// Currency to show shopping prices in, as an ISO 4217 code such as USD or GBP. Overridden by
    /// `--currency` and the GDDY_CURRENCY environment variable.
    #[serde(deserialize_with = "currency_code")]
    pub(crate) currency: Option<String>,
}

/// A saved currency must be a real ISO 4217 code, so `gddy config set currency banana` fails
/// instead of storing a value that every shopping command would then reject.
fn currency_code<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)?
        .map(|saved| {
            crate::shopping::currency_code(&saved).map_err(|_| {
                serde::de::Error::custom(format!(
                    "invalid currency {saved:?}: expected an ISO 4217 code such as USD or GBP"
                ))
            })
        })
        .transpose()
}

impl Prefs {
    /// The saved preferences. A config file that is malformed, or holds a value
    /// of the wrong type, is an error rather than silently reading as defaults.
    pub(crate) fn load(ctx: &CommandContext) -> Result<Self, GddyError> {
        ctx.config().deserialize().map_err(|e| {
            GddyError::validation(e.to_string()).with_fix(
                "Run `gddy config list` to inspect your settings, or `gddy config unset <key>` \
                 to reset one.",
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use cli_engine::{ConfigFile, ConfigSchema};

    use super::*;

    fn set(key: &str, raw: &str) -> Result<ConfigFile, String> {
        let mut file = ConfigFile::default();
        ConfigSchema::of::<Prefs>()
            .set(&mut file, key, raw)
            .map(|_| file)
            .map_err(|e| e.to_string())
    }

    #[test]
    fn currency_accepts_iso_codes_in_any_case() {
        for raw in ["USD", "gbp", "Eur"] {
            let file = set("currency", raw).expect("valid currency");
            let prefs: Prefs = file.deserialize().expect("prefs");
            assert_eq!(prefs.currency.as_deref(), Some(&*raw.to_ascii_uppercase()));
        }
    }

    #[test]
    fn currency_rejects_non_iso_values() {
        let err = set("currency", "banana").expect_err("not a currency");
        assert!(err.contains("ISO 4217"), "{err}");
    }

    #[test]
    fn currency_defaults_to_unset() {
        let prefs: Prefs = ConfigFile::default().deserialize().expect("prefs");
        assert_eq!(prefs.currency, None);
    }
}
