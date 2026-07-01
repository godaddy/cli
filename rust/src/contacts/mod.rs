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

/// One contact's details. Field names mirror the contact/address fields, so a
/// complete entry maps directly to a v3 request contact. The fields v3 requires
/// (`name_first`/`name_last`/`email`/`phone` and the address
/// `address1`/`city`/`state`/`postal_code`/`country`) are mandatory here too — a
/// partial entry fails to parse rather than producing an invalid request.
///
/// v3's `Contact` has no middle name, job title, or fax; a `name_middle`/
/// `job_title`/`fax` key left over from a v1/v2-era file is accepted (serde
/// ignores unknown keys) but not sent.
#[derive(Debug, Clone, Deserialize)]
pub struct Contact {
    pub name_first: String,
    pub name_last: String,
    pub email: String,
    pub phone: String,
    #[serde(default)]
    pub organization: Option<String>,
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
    /// Convert to the v3 API contact (`Contact`), validating the country code.
    /// Returns a human-readable error (surfaced by `domain purchase`/`quote`) when
    /// the country is not a recognized two-letter ISO code or the phone can't be
    /// parsed.
    ///
    /// v3's `Contact` is leaner than v2's: it has no middle name, job title, fax,
    /// or character-encoding field, and the phone is a structured object
    /// (`countryCode`/`nationalNumber`) rather than a dotted `+1.4805551212`
    /// string. The `name_middle`/`job_title`/`fax` columns in `contacts.toml` are
    /// kept in the file schema for backward compatibility but are not sent to v3.
    pub fn to_api(&self, role: Role) -> Result<api::Contact, String> {
        let country_upper = self.country.to_ascii_uppercase();
        validate_country(&country_upper, role)?;
        let phone = to_api_phone(&self.phone, &country_upper, role)?;
        Ok(api::Contact {
            address: api::SimpleAddress {
                line1: self.address1.clone(),
                line2: empty_to_none(&self.address2),
                city: self.city.clone(),
                state: Some(self.state.clone()),
                postal_code: Some(self.postal_code.clone()),
                country_code: api::CountryCode(country_upper),
            },
            email: api::EmailAddress(self.email.clone()),
            first_name: self.name_first.clone(),
            last_name: self.name_last.clone(),
            organization: empty_to_none(&self.organization),
            phone,
        })
    }
}

/// Validate a (already upper-cased) country code is a two-letter ISO code (or the
/// special `C2`), preserving the clear early error the v2 path gave before the
/// strict `AddressCountry` enum was dropped from the generated client.
fn validate_country(country_upper: &str, role: Role) -> Result<(), String> {
    let ok = country_upper == "C2"
        || (country_upper.len() == 2 && country_upper.bytes().all(|b| b.is_ascii_uppercase()));
    if ok {
        Ok(())
    } else {
        Err(format!(
            "{} contact in contacts.toml has invalid country {:?} \
             (expected a two-letter ISO code, e.g. US)",
            role.label(),
            country_upper,
        ))
    }
}

/// Trim an optional string and treat an empty value as absent. A blank optional
/// field in `contacts.toml` (e.g. `address2 = ""`) would otherwise deserialize to
/// `Some("")` and be sent as `"address2": ""`; the Domains API enforces a minimum
/// length of 1 on these optional fields when present, so an empty string is
/// rejected with a `LENGTH_UNDER` 422. Returning `None` lets serde omit the field
/// entirely (the generated types `skip_serializing_if = Option::is_none`).
fn empty_to_none(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Parse a user-entered phone number into v3's structured `Phone`
/// (`countryCode` = the calling code digits, e.g. `44`; `nationalNumber` = the
/// national digits, e.g. `7793601890`).
///
/// `region` is the contact's two-letter ISO country, used as the default region so
/// a number typed without a `+` country prefix (e.g. a local `07793 601890`) still
/// parses. Returns a human-readable error (surfaced by `domain purchase`, like the
/// invalid-country error) when the value can't be parsed as a phone number, rather
/// than forwarding it and letting the API reject it opaquely.
fn to_api_phone(raw: &str, region: &str, role: Role) -> Result<api::Phone, String> {
    let region_id = region.parse::<phonenumber::country::Id>().ok();
    let number = phonenumber::parse(region_id, raw).map_err(|_| {
        format!(
            "{} contact in contacts.toml has an unrecognized phone {:?} \
             (expected a phone number like +1.4805551212)",
            role.label(),
            raw,
        )
    })?;
    // `NationalNumber`'s Display preserves significant leading zeros (e.g. Italy),
    // so this is correct for every region, not just those whose national number is
    // a plain integer.
    Ok(api::Phone {
        country_code: Some(number.code().value().to_string()),
        national_number: Some(number.national().to_string()),
        extension_number: None,
    })
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

    /// The v3 API contact for a role: `None` when the role is absent (→ omit
    /// from the request → account default), or an error when the configured
    /// country or phone is invalid.
    pub fn to_api(&self, role: Role) -> Result<Option<api::Contact>, String> {
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
# organization, address2. A role you uncomment must have all required fields or
# the file will fail to load.

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
        assert_eq!(registrant.first_name, "Ada");
        assert_eq!(registrant.last_name, "Lovelace");
        assert_eq!(registrant.address.city, "Tempe");
        assert_eq!(registrant.address.state.as_deref(), Some("AZ"));
        assert_eq!(registrant.address.postal_code.as_deref(), Some("85281"));
        // Lower-case "us" in the file resolves to the uppercased ISO code.
        assert_eq!(registrant.address.country_code.as_str(), "US");
        assert_eq!(registrant.email.as_str(), "ada@example.com");
        // An absent role resolves to None (→ account default).
        assert!(file.to_api(Role::Admin).expect("ok").is_none());
    }

    #[test]
    fn to_api_rejects_malformed_country() {
        // A non-two-letter code is rejected early with a clear message. (ISO
        // membership beyond the two-letter shape is validated server-side now that
        // the strict country enum is gone from the generated client.)
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
country = "USA"
"#;
        let file: ContactsFile = toml::from_str(toml).expect("parses");
        let err = file
            .to_api(Role::Registrant)
            .expect_err("USA is not a two-letter code");
        assert!(err.contains("invalid country"));
        assert!(err.contains("registrant"));
    }

    /// Build a single-registrant `ContactsFile` from a `[registrant]` TOML body.
    fn registrant(body: &str) -> api::Contact {
        let toml = format!("[registrant]\n{body}");
        let file: ContactsFile = toml::from_str(&toml).expect("parses");
        file.to_api(Role::Registrant)
            .expect("valid")
            .expect("present")
    }

    const GB_BASE: &str = r#"name_first = "Jane"
name_last = "Doe"
email = "jane@example.com"
address1 = "10 Downing St"
city = "London"
state = "London"
postal_code = "SW1A 2AA"
country = "GB"
"#;

    #[test]
    fn gb_phone_common_formats_parse_to_structured_phone() {
        // International (no separator), international (spaced), and bare local —
        // all the same UK number — parse to countryCode `44` + national `7793601890`.
        for input in ["+447793601890", "+44 7793 601890", "07793 601890"] {
            let c = registrant(&format!("{GB_BASE}phone = \"{input}\"\n"));
            assert_eq!(
                c.phone.country_code.as_deref(),
                Some("44"),
                "input {input:?}"
            );
            assert_eq!(
                c.phone.national_number.as_deref(),
                Some("7793601890"),
                "input {input:?}"
            );
        }
    }

    #[test]
    fn us_phone_parses_to_structured_phone() {
        let body = r#"name_first = "Ada"
name_last = "Lovelace"
email = "ada@example.com"
phone = "(480) 555-1212"
address1 = "1 Main"
city = "Tempe"
state = "AZ"
postal_code = "85281"
country = "US"
"#;
        let c = registrant(body);
        assert_eq!(c.phone.country_code.as_deref(), Some("1"));
        assert_eq!(c.phone.national_number.as_deref(), Some("4805551212"));
    }

    #[test]
    fn already_dotted_phone_is_parsed() {
        let c = registrant(&format!("{GB_BASE}phone = \"+1.4805551212\"\n"));
        assert_eq!(c.phone.country_code.as_deref(), Some("1"));
        assert_eq!(c.phone.national_number.as_deref(), Some("4805551212"));
    }

    #[test]
    fn unparseable_phone_errors_with_role_and_example() {
        let toml = format!("[registrant]\n{GB_BASE}phone = \"not-a-phone\"\n");
        let file: ContactsFile = toml::from_str(&toml).expect("parses");
        let err = file
            .to_api(Role::Registrant)
            .expect_err("garbage phone is rejected");
        assert!(err.contains("registrant"), "got: {err}");
        assert!(err.contains("phone"), "got: {err}");
        assert!(err.contains("+1.4805551212"), "got: {err}");
    }

    #[test]
    fn blank_address2_is_omitted() {
        // The #33 regression: `address2 = ""` must not be sent (would 422 with
        // LENGTH_UNDER). A whitespace-only value is likewise omitted.
        for blank in ["\"\"", "\"   \""] {
            let c = registrant(&format!(
                "{GB_BASE}phone = \"+44.7793601890\"\naddress2 = {blank}\n"
            ));
            assert_eq!(c.address.line2, None, "blank {blank}");
        }
    }

    #[test]
    fn non_empty_address2_is_preserved() {
        let c = registrant(&format!(
            "{GB_BASE}phone = \"+44.7793601890\"\naddress2 = \"Flat 2\"\n"
        ));
        assert_eq!(c.address.line2.as_deref(), Some("Flat 2"));
    }

    #[test]
    fn blank_optional_org_is_omitted() {
        // v3's Contact carries only `organization` among the optional text fields;
        // job_title/name_middle/fax remain accepted in the file (for compatibility)
        // but are not sent. A blank organization must be omitted, not sent as "".
        let c = registrant(&format!(
            "{GB_BASE}phone = \"+44.7793601890\"\norganization = \"\"\njob_title = \"\"\nname_middle = \"\"\nfax = \"\"\n"
        ));
        assert_eq!(c.organization, None);
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
