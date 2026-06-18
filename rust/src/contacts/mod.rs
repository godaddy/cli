//! User-level default domain-purchase contacts (`~/.config/gddy/contacts.toml`).
//!
//! A domain purchase accepts registrant/admin/billing/tech contacts; when a
//! contact is omitted the Domains API falls back to the shopper's account-default
//! contact. Buyers who register many domains (e.g. domain investors) can instead
//! record their contacts once in a gitignored `contacts.toml` under the OS config
//! directory — the same directory as `environments.toml`
//! (`dirs::config_dir()`: `~/.config` on Linux/XDG, `%APPDATA%` on Windows,
//! `~/Library/Application Support` on macOS). `domain purchase` reads the file and
//! sends each role that is present; absent roles fall back to the account default.
//!
//! `domain purchase` only ever reads this file; the sole writer is
//! `domain contacts init`, which scaffolds a commented starter template the user
//! then edits by hand. See `gddy guide domain-purchase` for the format.

use domains_client::types as api;
use serde::Deserialize;

/// Error reading or parsing `contacts.toml`.
#[derive(Debug, thiserror::Error)]
pub enum ContactsError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {source}")]
    Parse {
        path: String,
        source: toml::de::Error,
    },
}

/// Schema of `contacts.toml`. Every role is optional; an absent role means the
/// purchase omits that contact and the API uses the account default.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ContactsFile {
    #[serde(default)]
    pub registrant: Option<Contact>,
    #[serde(default)]
    pub admin: Option<Contact>,
    #[serde(default)]
    pub billing: Option<Contact>,
    #[serde(default)]
    pub tech: Option<Contact>,
}

/// One contact's details. Field names mirror the Domains API `Contact`/`Address`
/// schema (in snake_case), so a complete entry maps directly to a request
/// contact. The fields the API requires (`name_first`/`name_last`/`email`/`phone`
/// and the mailing-address `address1`/`city`/`state`/`postal_code`/`country`) are
/// mandatory here too — a partial entry fails to parse rather than producing an
/// invalid request.
#[derive(Debug, Clone, Deserialize)]
pub struct Contact {
    pub name_first: String,
    pub name_last: String,
    pub email: String,
    pub phone: String,
    #[serde(default)]
    pub name_middle: Option<String>,
    #[serde(default)]
    pub organization: Option<String>,
    #[serde(default)]
    pub job_title: Option<String>,
    #[serde(default)]
    pub fax: Option<String>,
    pub address1: String,
    #[serde(default)]
    pub address2: Option<String>,
    pub city: String,
    pub state: String,
    pub postal_code: String,
    pub country: String,
}

/// The four purchase contact roles, in the order `domain purchase` reports them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Registrant,
    Admin,
    Billing,
    Tech,
}

impl Role {
    /// Lower-case label used in user-facing messages.
    pub fn label(self) -> &'static str {
        match self {
            Role::Registrant => "registrant",
            Role::Admin => "admin",
            Role::Billing => "billing",
            Role::Tech => "tech",
        }
    }
}

impl Contact {
    /// Convert to the v2 API contact (`ContactDomainCreate`), validating the
    /// country code. Returns a human-readable error (surfaced by
    /// `domain purchase`) when the country is not a recognized two-letter ISO
    /// code. `encoding` is `ASCII` (the API default) — contacts.toml is plain
    /// ASCII in practice.
    pub fn to_api(&self, role: Role) -> Result<api::ContactDomainCreate, String> {
        let country =
            api::AddressCountry::try_from(self.country.to_uppercase().as_str()).map_err(|_| {
                format!(
                    "{} contact in contacts.toml has invalid country {:?} \
                     (expected a two-letter ISO code, e.g. US)",
                    role.label(),
                    self.country,
                )
            })?;
        Ok(api::ContactDomainCreate {
            address_mailing: api::Address {
                address1: self.address1.clone(),
                address2: self.address2.clone(),
                city: self.city.clone(),
                country,
                postal_code: self.postal_code.clone(),
                state: self.state.clone(),
            },
            email: self.email.clone(),
            encoding: api::ContactDomainCreateEncoding::Ascii,
            fax: self.fax.clone(),
            job_title: self.job_title.clone(),
            metadata: Default::default(),
            name_first: self.name_first.clone(),
            name_last: self.name_last.clone(),
            name_middle: self.name_middle.clone(),
            organization: self.organization.clone(),
            phone: self.phone.clone(),
        })
    }
}

impl ContactsFile {
    /// The configured contact for a role, if any.
    pub fn get(&self, role: Role) -> Option<&Contact> {
        match role {
            Role::Registrant => self.registrant.as_ref(),
            Role::Admin => self.admin.as_ref(),
            Role::Billing => self.billing.as_ref(),
            Role::Tech => self.tech.as_ref(),
        }
    }

    /// The v2 API contact for a role: `None` when the role is absent (→ omit
    /// from the request → account default), or an error when the configured
    /// country is invalid.
    pub fn to_api(&self, role: Role) -> Result<Option<api::ContactDomainCreate>, String> {
        self.get(role).map(|c| c.to_api(role)).transpose()
    }
}

/// Path to the local contacts file, if a config dir can be resolved. Mirrors
/// [`crate::environments::environments_path`] (same `gddy/` config directory).
pub fn contacts_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("gddy").join("contacts.toml"))
}

/// Load the local contacts file. A missing file is not an error (it just means
/// no defaults are configured); a present-but-malformed file is.
pub fn load() -> Result<ContactsFile, ContactsError> {
    let Some(path) = contacts_path() else {
        return Ok(ContactsFile::default());
    };
    match std::fs::read_to_string(&path) {
        Ok(contents) => toml::from_str(&contents).map_err(|source| ContactsError::Parse {
            path: path.display().to_string(),
            source,
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ContactsFile::default()),
        Err(source) => Err(ContactsError::Io {
            path: path.display().to_string(),
            source,
        }),
    }
}

/// A starter `contacts.toml` for `domain contacts init`.
///
/// Every role is commented out, so the file parses to "no defaults" (all roles
/// fall back to the account) until the user deliberately uncomments and fills a
/// role — they can't accidentally register a domain with the placeholder values.
pub fn sample_toml() -> &'static str {
    r#"# gddy domain-purchase contacts
#
# Default contacts for `gddy domain purchase`. Any role you define here is sent
# with the purchase; any role you leave out falls back to your GoDaddy account's
# default contact for that role.
#
# To use a role, uncomment its block below and replace the placeholder values.
# Required fields per role: name_first, name_last, email, phone, address1, city,
# state, postal_code, country (a two-letter ISO code). Optional fields:
# name_middle, organization, job_title, fax, address2. A role you uncomment must
# have all required fields or the file will fail to load.

# [registrant]
# name_first   = "Jane"
# name_last    = "Doe"
# email        = "jane@example.com"
# phone        = "+1.4805551212"
# organization = "Example LLC"      # optional
# address1     = "123 Main St"
# address2     = "Suite 100"        # optional
# city         = "Phoenix"
# state        = "AZ"
# postal_code  = "85001"
# country      = "US"

# The admin, billing, and tech roles take the same fields as [registrant].
# Uncomment and fill any you want to override (otherwise the account default is
# used).

# [admin]

# [billing]

# [tech]
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"
[registrant]
name_first = "Ada"
name_last = "Lovelace"
email = "ada@example.com"
phone = "+1.4805551212"
organization = "Analytical Engines"
address1 = "1 Bletchley Park"
city = "Tempe"
state = "AZ"
postal_code = "85281"
country = "us"

[tech]
name_first = "Grace"
name_last = "Hopper"
email = "grace@example.com"
phone = "+1.4805551213"
address1 = "2 Navy Yard"
city = "Tempe"
state = "AZ"
postal_code = "85281"
country = "US"
"#;

    #[test]
    fn parses_present_roles_and_leaves_others_none() {
        let file: ContactsFile = toml::from_str(FULL).expect("parses");
        assert!(file.get(Role::Registrant).is_some());
        assert!(file.get(Role::Tech).is_some());
        assert!(file.get(Role::Admin).is_none());
        assert!(file.get(Role::Billing).is_none());
    }

    #[test]
    fn to_api_maps_fields_and_uppercases_country() {
        let file: ContactsFile = toml::from_str(FULL).expect("parses");
        let registrant = file
            .to_api(Role::Registrant)
            .expect("valid country")
            .expect("present");
        assert_eq!(registrant.name_first, "Ada");
        assert_eq!(registrant.name_last, "Lovelace");
        assert_eq!(registrant.address_mailing.city, "Tempe");
        // "us" in the file resolves to the AZ-uppercased ISO enum variant.
        assert_eq!(registrant.address_mailing.country, api::AddressCountry::Us);
        // An absent role resolves to None (→ account default).
        assert!(file.to_api(Role::Admin).expect("ok").is_none());
    }

    #[test]
    fn to_api_rejects_unknown_country() {
        let toml = r#"
[registrant]
name_first = "Ada"
name_last = "Lovelace"
email = "ada@example.com"
phone = "+1.4805551212"
address1 = "1 Bletchley Park"
city = "Tempe"
state = "AZ"
postal_code = "85281"
country = "ZZ"
"#;
        let file: ContactsFile = toml::from_str(toml).expect("parses");
        let err = file
            .to_api(Role::Registrant)
            .expect_err("ZZ is not a valid ISO country");
        assert!(err.contains("invalid country"));
        assert!(err.contains("registrant"));
    }

    #[test]
    fn partial_contact_fails_to_parse() {
        // Missing required `country` (and address fields) — a partial entry must
        // not silently produce an invalid request.
        let toml = r#"
[registrant]
name_first = "Ada"
name_last = "Lovelace"
email = "ada@example.com"
phone = "+1.4805551212"
"#;
        assert!(toml::from_str::<ContactsFile>(toml).is_err());
    }

    #[test]
    fn empty_file_parses_to_all_none() {
        let file: ContactsFile = toml::from_str("").expect("empty parses");
        for role in [Role::Registrant, Role::Admin, Role::Billing, Role::Tech] {
            assert!(file.get(role).is_none());
        }
    }

    #[test]
    fn sample_template_is_valid_toml_and_inert_until_edited() {
        // The generated starter file must parse, and (everything commented) must
        // resolve to no roles — so a freshly generated file is a no-op, not an
        // accidental purchase with placeholder contacts.
        let file: ContactsFile =
            toml::from_str(sample_toml()).expect("sample template is valid TOML");
        for role in [Role::Registrant, Role::Admin, Role::Billing, Role::Tech] {
            assert!(
                file.get(role).is_none(),
                "{} should start commented out",
                role.label()
            );
        }
        // The documented required field names appear in the template so users
        // know what to fill in.
        for field in [
            "name_first",
            "email",
            "phone",
            "address1",
            "postal_code",
            "country",
        ] {
            assert!(
                sample_toml().contains(field),
                "template should mention {field}"
            );
        }
    }
}
