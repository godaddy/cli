//! Settings for [`parse`](crate::parse).

use std::collections::BTreeMap;

use crate::error::{ErrorKind, ParseError};
use crate::name::Name;
use crate::rdata::CustomShape;

/// Mnemonics this crate reads itself; a custom type may not reuse one.
pub(crate) const BUILT_IN_TYPES: &[&str] = &[
    "A", "AAAA", "NS", "CNAME", "MX", "TXT", "SRV", "CAA", "TLSA", "HTTPS", "SVCB", "SOA",
];

/// The classes of a master file. A word that is one of these is read as the class of
/// a record, never as its type.
pub(crate) const CLASS_NAMES: &[&str] = &["IN", "CH", "HS", "CS"];

/// Input larger than this is refused unless raised with
/// [`ParseOptions::with_max_input_bytes`].
const DEFAULT_MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
/// More records than this are refused unless raised with
/// [`ParseOptions::with_max_records`].
const DEFAULT_MAX_RECORDS: usize = 100_000;

/// Settings for [`parse`](crate::parse). Start from [`ParseOptions::default`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ParseOptions {
    pub(crate) origin: Option<Name>,
    pub(crate) default_ttl: Option<u32>,
    pub(crate) max_records: usize,
    pub(crate) max_input_bytes: usize,
    pub(crate) custom_types: BTreeMap<String, CustomShape>,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            origin: None,
            default_ttl: None,
            max_records: DEFAULT_MAX_RECORDS,
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            custom_types: BTreeMap::new(),
        }
    }
}

impl ParseOptions {
    /// The origin for names before the file's first `$ORIGIN`. Without one,
    /// `@` and relative names are an error until a `$ORIGIN` line appears.
    #[must_use]
    pub fn with_origin(mut self, origin: Name) -> Self {
        self.origin = Some(origin);
        self
    }

    /// The TTL for a record that gives none and follows no `$TTL` line or earlier
    /// record with a TTL. Without one, such a record is an error.
    #[must_use]
    pub fn with_default_ttl(mut self, ttl: u32) -> Self {
        self.default_ttl = Some(ttl);
        self
    }

    /// The most records to accept (default 100 000). More is a
    /// [`Limit`](ErrorKind::Limit) error.
    #[must_use]
    pub fn with_max_records(mut self, max: usize) -> Self {
        self.max_records = max;
        self
    }

    /// The most input bytes to accept (default 16 MiB). More is a
    /// [`Limit`](ErrorKind::Limit) error.
    #[must_use]
    pub fn with_max_input_bytes(mut self, max: usize) -> Self {
        self.max_input_bytes = max;
        self
    }

    /// Accept a record type this crate does not know, such as GoDaddy's `ALIAS`.
    /// `mnemonic` is matched without regard to case. It must be letters, digits
    /// and hyphens, and not the name of a built-in type; [`parse`](crate::parse)
    /// reports a bad one as an [`InvalidValue`](ErrorKind::InvalidValue) error.
    #[must_use]
    pub fn with_custom_type(mut self, mnemonic: impl AsRef<str>, shape: CustomShape) -> Self {
        self.custom_types
            .insert(mnemonic.as_ref().to_ascii_uppercase(), shape);
        self
    }

    pub(crate) fn validate(&self) -> Result<(), ParseError> {
        for name in self.custom_types.keys() {
            // A word that starts with a digit is read as a time, so a type name
            // may not start with one.
            let well_formed = name.bytes().next().is_some_and(|b| !b.is_ascii_digit())
                && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
            if !well_formed {
                return Err(ParseError::new(
                    ErrorKind::InvalidValue,
                    0,
                    0,
                    format!(
                        "`{name}` is not a valid record type name: use letters, digits and \
                         hyphens, starting with a letter"
                    ),
                ));
            }
            if CLASS_NAMES.contains(&name.as_str()) {
                return Err(ParseError::new(
                    ErrorKind::InvalidValue,
                    0,
                    0,
                    format!("`{name}` is a record class, so it cannot be used as a record type"),
                ));
            }
            if BUILT_IN_TYPES.contains(&name.as_str()) {
                return Err(ParseError::new(
                    ErrorKind::InvalidValue,
                    0,
                    0,
                    format!("`{name}` is a built-in record type and cannot be redefined"),
                ));
            }
        }
        Ok(())
    }
}
