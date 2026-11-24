//! Reads and writes the RFC 1035 master file format, usually called a zone file
//! or BIND zone file.
//!
//! The crate covers what is needed to move records between providers and tools:
//! `$ORIGIN` and `$TTL`, relative and absolute names, `@`, owner names inherited
//! from the line above, BIND-style times (`1h30m`), `;` comments, `( )` across
//! lines, and quoted strings with escapes. It knows `A`, `AAAA`, `NS`, `CNAME`,
//! `MX`, `TXT`, `SRV`, `CAA`, `TLSA`, `HTTPS`, `SVCB` and `SOA`. Other types
//! are rejected unless the caller registers them (see below); nothing is skipped
//! silently.
//!
//! Not supported, and reported as [`ErrorKind::Unsupported`]: `$INCLUDE`,
//! `$GENERATE`, classes other than `IN`, and the RFC 3597 generic forms
//! (`TYPE123`, `\# 4 c0000201`).
//!
//! # Reading
//!
//! ```
//! use rfc1035_format::{parse, Name, ParseOptions, RData};
//!
//! let text = r#"
//! $ORIGIN example.com.
//! $TTL 1h
//! @    IN MX 10 mail
//! www  300 A 192.0.2.1
//! "#;
//! let zone = parse(text, &ParseOptions::default())?;
//! assert_eq!(zone.records.len(), 2);
//! assert_eq!(zone.records[1].ttl, 300);
//! assert!(matches!(zone.records[0].data, RData::Mx { preference: 10, .. }));
//! # Ok::<(), rfc1035_format::ParseError>(())
//! ```
//!
//! # Non-standard record types
//!
//! Some providers add types of their own. Register one with its shape and it is
//! read, and written back, like any other:
//!
//! ```
//! use rfc1035_format::{parse, CustomShape, ParseOptions};
//!
//! let options = ParseOptions::default().with_custom_type("ALIAS", CustomShape::Name);
//! let zone = parse("$ORIGIN example.com.\n@ 600 ALIAS target.example.net.\n", &options)?;
//! assert_eq!(zone.records[0].data.type_name(), "ALIAS");
//! # Ok::<(), rfc1035_format::ParseError>(())
//! ```
//!
//! # Writing
//!
//! [`Zone::to_zone_file`] writes every record in full, so the output does not
//! depend on `$TTL` or on the line above, and [`parse`] reads it back to the same
//! records.
//!
//! # Untrusted input
//!
//! Input size and record count are limited (see [`ParseOptions`]). The parser
//! never reads files and never panics on malformed text.

mod error;
mod lexer;
mod name;
mod options;
mod parser;
mod rdata;
mod rdata_parse;
#[cfg(test)]
mod test_support;
mod ttl;
mod writer;
mod zone;

pub use error::{ErrorKind, ParseError};
pub use name::Name;
pub use options::ParseOptions;
pub use rdata::{CustomShape, CustomValue, RData, Soa, Svcb};
pub use zone::{Record, Zone};

/// Read the zone file in `input`.
///
/// # Errors
///
/// The first problem found, with its line and column: text that is not a master
/// file, a value out of range, a record type that is not built in or registered,
/// a feature this crate does not support, or input over a limit in `options`.
pub fn parse(input: &str, options: &ParseOptions) -> Result<Zone, ParseError> {
    parser::parse(input, options)
}
