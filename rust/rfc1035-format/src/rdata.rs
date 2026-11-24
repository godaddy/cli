//! The data of a resource record.

use std::net::{Ipv4Addr, Ipv6Addr};

use crate::name::Name;

/// The data of one resource record, by type.
///
/// New variants may be added in a minor release, so match with a wildcard arm.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RData {
    /// `A`: an IPv4 address.
    A(Ipv4Addr),
    /// `AAAA`: an IPv6 address.
    Aaaa(Ipv6Addr),
    /// `NS`: an authoritative name server.
    Ns(Name),
    /// `CNAME`: the canonical name this name is an alias of.
    Cname(Name),
    /// `MX`: a mail exchanger.
    Mx {
        /// Lower values are preferred.
        preference: u16,
        /// The mail server.
        exchange: Name,
    },
    /// `TXT`: one or more character-strings. Their concatenation is the value;
    /// the split is kept so a file can be written back as it was read. Each
    /// string is at most 255 bytes: the parser splits a longer one.
    Txt(Vec<Vec<u8>>),
    /// `SRV`: the host and port of a service.
    Srv {
        /// Lower values are tried first.
        priority: u16,
        /// Share among records of equal priority.
        weight: u16,
        /// The TCP or UDP port.
        port: u16,
        /// The host providing the service.
        target: Name,
    },
    /// `CAA`: which certificate authorities may issue for the name.
    Caa {
        /// Flag bits; bit 7 is the "critical" flag.
        flags: u8,
        /// The property, such as `issue` or `iodef`.
        tag: String,
        /// The property value.
        value: Vec<u8>,
    },
    /// `TLSA`: a certificate association for DANE.
    Tlsa {
        /// Which certificate in the chain is matched.
        usage: u8,
        /// Which part of the certificate is matched.
        selector: u8,
        /// How the data is compared with the selected part.
        matching_type: u8,
        /// The certificate data or its digest.
        data: Vec<u8>,
    },
    /// `HTTPS`: service binding for HTTPS origins (RFC 9460).
    Https(Svcb),
    /// `SVCB`: general service binding (RFC 9460).
    Svcb(Svcb),
    /// `SOA`: the start of authority of a zone.
    Soa(Soa),
    /// A type registered with
    /// [`ParseOptions::with_custom_type`](crate::ParseOptions::with_custom_type).
    Custom {
        /// The mnemonic in upper case, such as `ALIAS`.
        type_name: String,
        /// The data, in the shape the type was registered with.
        value: CustomValue,
    },
}

impl RData {
    /// The type mnemonic as it is written in a zone file.
    pub fn type_name(&self) -> &str {
        match self {
            Self::A(_) => "A",
            Self::Aaaa(_) => "AAAA",
            Self::Ns(_) => "NS",
            Self::Cname(_) => "CNAME",
            Self::Mx { .. } => "MX",
            Self::Txt(_) => "TXT",
            Self::Srv { .. } => "SRV",
            Self::Caa { .. } => "CAA",
            Self::Tlsa { .. } => "TLSA",
            Self::Https(_) => "HTTPS",
            Self::Svcb(_) => "SVCB",
            Self::Soa(_) => "SOA",
            Self::Custom { type_name, .. } => type_name,
        }
    }
}

/// The data of an `HTTPS` or `SVCB` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Svcb {
    /// 0 makes the record an alias (AliasMode); higher values rank service
    /// alternatives, lower first.
    pub priority: u16,
    /// The service host, or the root name (`.`) for "this name".
    pub target: Name,
    /// The service parameters as written (`alpn=h2,h3 port=8443`), not
    /// interpreted. Empty when there are none.
    pub params: String,
}

/// The data of an `SOA` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Soa {
    /// The primary name server.
    pub mname: Name,
    /// The responsible person's mailbox, written as a name.
    pub rname: Name,
    /// The zone's version number.
    pub serial: u32,
    /// Seconds between a secondary's checks for a new serial.
    pub refresh: u32,
    /// Seconds a secondary waits before retrying a failed refresh.
    pub retry: u32,
    /// Seconds after which a secondary stops answering if it cannot refresh.
    pub expire: u32,
    /// Seconds a negative answer may be cached.
    pub minimum: u32,
}

/// How the data of a custom record type is written in a zone file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CustomShape {
    /// A single domain name. `@` and relative names are resolved against the
    /// origin, exactly as for `CNAME`.
    Name,
    /// One or more words or quoted strings, kept as written.
    Words,
}

/// The data of a custom record type.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CustomValue {
    /// Data registered as [`CustomShape::Name`].
    Name(Name),
    /// Data registered as [`CustomShape::Words`], each word as it would be
    /// written (quoted strings keep their quotes).
    Words(Vec<String>),
}
