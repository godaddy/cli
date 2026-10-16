//! Internationalized domain names: Unicode for people, punycode for the wire.
//!
//! Commands accept a name in either form and send the ASCII (punycode) form to
//! the API ([`to_ascii`]). Names shown to the user are Unicode by default and
//! ASCII when the user asks for it ([`DomainForm`]).

use serde_json::Value;

use crate::error::GddyError;

/// Prefix of an ASCII-compatible-encoded (punycode) label, any case.
const ACE_PREFIX: &str = "xn--";

/// The ASCII (punycode) form of a domain name or DNS record name.
///
/// Pure-ASCII input is returned untouched, so names the API already accepts —
/// including `@`, `*` and `_dmarc`-style labels — never change. Anything else
/// is run through IDNA, which also lowercases it.
pub(crate) fn to_ascii(name: &str) -> Result<String, GddyError> {
    if name.is_ascii() {
        return Ok(name.to_owned());
    }
    idna::domain_to_ascii(name).map_err(|_| {
        GddyError::validation(format!("{name:?} is not a valid internationalized name"))
            .with_fix("Remove any characters that can't appear in a domain name.")
    })
}

/// The Unicode form of `name`. Labels that are not valid punycode are left as
/// they are, so display never fails.
pub(crate) fn to_unicode(name: &str) -> String {
    if !has_ace_label(name) {
        return name.to_owned();
    }
    match idna::domain_to_unicode(name) {
        (unicode, Ok(())) => unicode,
        (_, Err(_)) => name.to_owned(),
    }
}

fn has_ace_label(name: &str) -> bool {
    name.split('.').any(|label| {
        label
            .get(..ACE_PREFIX.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(ACE_PREFIX))
    })
}

/// Response fields that hold a domain name or a list of host names, across the
/// domain and hosting APIs.
const DOMAIN_FIELDS: &[&str] = &[
    "domain",
    "idnDomain",
    "unicodeDomain",
    "nameServers",
    "nameservers",
    "hostname",
];

/// Fields of a DNS command's output that hold a zone or a record name.
const DNS_FIELDS: &[&str] = &["domain", "name"];

/// Which form of a domain name to show.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum DomainForm {
    #[default]
    Unicode,
    Punycode,
}

impl DomainForm {
    pub(crate) fn from_flag(punycode: bool) -> Self {
        if punycode {
            Self::Punycode
        } else {
            Self::Unicode
        }
    }

    /// `name` in this form. A name that cannot be converted is shown as-is.
    pub(crate) fn render(self, name: &str) -> String {
        match self {
            Self::Unicode => to_unicode(name),
            Self::Punycode => to_ascii(name).unwrap_or_else(|_| name.to_owned()),
        }
    }

    /// Convert, in place, every string under a key in `keys` — at any depth,
    /// including strings inside arrays under such a key — to this form.
    pub(crate) fn convert_fields(self, value: &mut Value, keys: &[&str]) {
        match value {
            Value::Object(map) => {
                for (key, child) in map.iter_mut() {
                    if keys.contains(&key.as_str()) {
                        self.convert_all(child);
                    } else {
                        self.convert_fields(child, keys);
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.convert_fields(item, keys);
                }
            }
            _ => {}
        }
    }

    /// [`Self::convert_fields`] over the standard domain-name response fields.
    pub(crate) fn convert_domain_fields(self, value: &mut Value) {
        self.convert_fields(value, DOMAIN_FIELDS);
    }

    /// [`Self::convert_fields`] over the zone and record-name fields of DNS output.
    pub(crate) fn convert_dns_fields(self, value: &mut Value) {
        self.convert_fields(value, DNS_FIELDS);
    }

    fn convert_all(self, value: &mut Value) {
        match value {
            Value::String(s) => *s = self.render(s),
            Value::Array(items) => items.iter_mut().for_each(|v| self.convert_all(v)),
            _ => {}
        }
    }
}

/// `--punycode`, shared by every command that shows domain names.
#[derive(Debug, Clone, Copy, Default, clap::Args)]
pub(crate) struct DomainFormArgs {
    /// Show domain names in their ASCII (punycode) form, e.g. xn--mnchen-3ya.de, instead of Unicode.
    #[arg(long)]
    punycode: bool,
}

impl DomainFormArgs {
    pub(crate) fn form(&self) -> DomainForm {
        DomainForm::from_flag(self.punycode)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn to_ascii_encodes_unicode_and_leaves_ascii_alone() {
        assert_eq!(
            to_ascii("münchen.de").expect("valid name"),
            "xn--mnchen-3ya.de"
        );
        assert_eq!(
            to_ascii("xn--mnchen-3ya.de").expect("valid name"),
            "xn--mnchen-3ya.de"
        );
        assert_eq!(to_ascii("Example.COM").expect("valid name"), "Example.COM");
        for special in ["@", "*", "_dmarc", "www"] {
            assert_eq!(to_ascii(special).expect("valid name"), special);
        }
    }

    #[test]
    fn to_ascii_encodes_only_the_unicode_labels_of_a_dns_name() {
        assert_eq!(
            to_ascii("*.münchen").expect("valid name"),
            "*.xn--mnchen-3ya"
        );
    }

    #[test]
    fn to_ascii_rejects_names_idna_disallows() {
        assert!(to_ascii("\u{301}abc.com").is_err());
    }

    #[test]
    fn to_unicode_decodes_punycode_labels() {
        assert_eq!(to_unicode("xn--mnchen-3ya.de"), "münchen.de");
        assert_eq!(to_unicode("XN--MNCHEN-3YA.de"), "münchen.de");
        assert_eq!(to_unicode("www.xn--mnchen-3ya.de"), "www.münchen.de");
    }

    #[test]
    fn to_unicode_leaves_plain_and_undecodable_names_alone() {
        assert_eq!(to_unicode("example.com"), "example.com");
        assert_eq!(to_unicode("münchen.de"), "münchen.de");
        assert_eq!(to_unicode("@"), "@");
        assert_eq!(to_unicode("xn--.com"), "xn--.com");
        assert_eq!(to_unicode("xn--a-.com"), "xn--a-.com");
    }

    #[test]
    fn render_honors_the_requested_form() {
        assert_eq!(
            DomainForm::Unicode.render("xn--mnchen-3ya.de"),
            "münchen.de"
        );
        assert_eq!(
            DomainForm::Punycode.render("münchen.de"),
            "xn--mnchen-3ya.de"
        );
        assert_eq!(DomainForm::Punycode.render("example.com"), "example.com");
    }

    #[test]
    fn convert_fields_walks_nested_values_and_string_arrays() {
        let mut v = json!({
            "domain": "xn--mnchen-3ya.de",
            "status": "xn--mnchen-3ya.de",
            "nameServers": ["ns1.xn--mnchen-3ya.de", "ns2.example.com"],
            "items": [{"name": "xn--mnchen-3ya", "type": "A"}],
            "count": 3,
        });
        DomainForm::Unicode.convert_fields(&mut v, &["domain", "nameServers", "name"]);
        assert_eq!(
            v,
            json!({
                "domain": "münchen.de",
                "status": "xn--mnchen-3ya.de",
                "nameServers": ["ns1.münchen.de", "ns2.example.com"],
                "items": [{"name": "münchen", "type": "A"}],
                "count": 3,
            })
        );
    }
}
