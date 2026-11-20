//! The parsed form of a zone file.

use std::fmt;

use crate::name::Name;
use crate::rdata::RData;

/// One resource record.
///
/// Two records are equal when their name, TTL and data are; [`Record::line`] is
/// where a record came from, not what it is, so it is not compared.
#[derive(Debug, Clone, Eq)]
pub struct Record {
    /// The name the record is for.
    pub name: Name,
    /// Seconds a resolver may cache the record.
    pub ttl: u32,
    /// The type and data.
    pub data: RData,
    /// The 1-based line the record starts on in the text it was read from, so a
    /// caller can point at it in its own error messages. 0 for a record that was
    /// not read from text.
    pub line: usize,
}

impl PartialEq for Record {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.ttl == other.ttl && self.data == other.data
    }
}

/// The records of a zone file, in file order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Zone {
    /// The origin the file started with: the one given in
    /// [`ParseOptions`](crate::ParseOptions) or, failing that, its first
    /// `$ORIGIN`. [`Zone::to_zone_file`] writes names relative to it.
    pub origin: Option<Name>,
    /// The records.
    pub records: Vec<Record>,
}

impl Zone {
    /// Write the zone as a master file that [`parse`](crate::parse) reads back to
    /// the same records.
    pub fn to_zone_file(&self) -> String {
        crate::writer::write_zone(self)
    }
}

impl fmt::Display for Zone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_zone_file())
    }
}
