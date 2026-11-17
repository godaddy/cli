//! Record building, fetching, and argument helpers shared by every `dns`
//! subcommand: type/name validation, the `RecordOptions` bundle, converting
//! CLI input into v3 `DnsRecord`s, and the paginated `list_dns_records` fetch.

use cli_engine::{
    Alignment, CliCoreError, CommandResult, NextAction, NextActionParam, TableColumn,
};
use serde_json::{Value, json};

use crate::domain::api_error;
use crate::idn::DomainForm;
use crate::next_action::next_action;

use domains_client::types;

/// DNS record types the CLI can create/replace/delete via v3 (`add`/`set`/`delete`).
/// NS and SOA are registry-managed / read-only, so they're excluded. v3's
/// `DNSRecordType` is otherwise an open string; this list is the CLI's guardrail
/// against typos and includes `CAA` and GoDaddy's `ALIAS` extension.
pub(super) const WRITABLE_TYPES: &[&str] = &[
    "A", "AAAA", "ALIAS", "CAA", "CNAME", "HTTPS", "MX", "SRV", "SVCB", "TLSA", "TXT",
];
/// Record types accepted by the `list` filter — the writable set plus the
/// read-only NS/SOA, which are listable even though they can't be modified.
pub(super) const LISTABLE_TYPES: &[&str] = &[
    "A", "AAAA", "ALIAS", "CAA", "CNAME", "HTTPS", "MX", "NS", "SOA", "SRV", "SVCB", "TLSA", "TXT",
];
/// Default TTL (seconds) for `dns add`/`set` when `--ttl` is omitted (v3 requires a ttl).
pub(super) const DEFAULT_TTL: i64 = 3600;
/// The TTL range (seconds) the API accepts: 10 minutes to 24 hours.
const MIN_TTL: i64 = 600;
const MAX_TTL: i64 = 86_400;
/// The output fields of a DNS write that hold the zone and the record name.
pub(super) const ZONE_AND_NAME: &[&str] = &["domain", "name"];

/// Page size for the paginated v3 list; the handler pages through until every
/// matching record is collected.
const LIST_PAGE_SIZE: std::num::NonZeroU64 =
    std::num::NonZeroU64::new(100).expect("LIST_PAGE_SIZE is nonzero");

/// clap value-parser for a mutating `--type` (`add`/`set`/`delete`): validate
/// against [`WRITABLE_TYPES`] and return the canonical upper-case wire string.
/// The read-only NS/SOA get a clear "managed by GoDaddy" reason. Validating in
/// clap rejects invalid input at parse time — before `--dry-run`/auth short-circuits.
pub(super) fn parse_write_type_arg(raw: &str) -> Result<String, String> {
    let upper = raw.to_ascii_uppercase();
    if WRITABLE_TYPES.contains(&upper.as_str()) {
        return Ok(upper);
    }
    if matches!(upper.as_str(), "NS" | "SOA") {
        Err(format!(
            "{upper} records are managed by GoDaddy and can't be created, replaced, or deleted; \
             writable types: {}",
            WRITABLE_TYPES.join(", ")
        ))
    } else {
        Err(format!(
            "invalid record type {raw:?}; expected one of {}",
            WRITABLE_TYPES.join(", ")
        ))
    }
}

/// clap value-parser for the `list` `--type` filter: [`LISTABLE_TYPES`] (the
/// writable set plus read-only NS/SOA), upper-cased.
pub(super) fn parse_list_type_arg(raw: &str) -> Result<String, String> {
    let upper = raw.to_ascii_uppercase();
    if LISTABLE_TYPES.contains(&upper.as_str()) {
        Ok(upper)
    } else {
        Err(format!(
            "invalid record type {raw:?}; expected one of {}",
            LISTABLE_TYPES.join(", ")
        ))
    }
}

/// clap value-parser for `--service` and `--protocol` (SRV and TLSA). DNS and the
/// API both require these labels to start with an underscore (`_sip`, `_tcp`), so
/// the underscore says nothing the customer could have meant otherwise: `sip` and
/// `_sip` are accepted alike and the underscore is added when it is missing. The
/// result is checked the way the API checks it — letters, digits, `-` or `_` after
/// the underscore, at most 63 characters in all (a DNS label) — and nothing more;
/// any such name is accepted, so there is no list of known services. A bad value
/// would otherwise cost a login and a 400, so catch it here with an example.
pub(super) fn parse_underscore_label(raw: &str) -> Result<String, String> {
    let label = if raw.starts_with('_') {
        raw.to_owned()
    } else {
        format!("_{raw}")
    };
    let valid = label.len() > 1
        && label.len() <= 63
        && label[1..]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if valid {
        Ok(label)
    } else {
        Err(format!(
            "{raw:?} must be letters, digits, `-` or `_`, up to 62 characters (for example \
             `sip` or `tcp`)"
        ))
    }
}

/// Optional record fields shared by `add` and `set`, including the CAA-only
/// `flag`/`tag`, the TLSA-only `usage`/`selector`/`matching_type`, and the
/// HTTPS/SVCB-only `parameters` (SvcParams).
#[derive(Clone)]
pub(super) struct RecordOptions {
    pub(super) ttl: Option<i64>,
    pub(super) priority: Option<i64>,
    pub(super) port: Option<i64>,
    pub(super) weight: Option<i64>,
    pub(super) protocol: Option<String>,
    pub(super) service: Option<String>,
    pub(super) flag: Option<i64>,
    pub(super) tag: Option<String>,
    pub(super) usage: Option<i64>,
    pub(super) selector: Option<i64>,
    pub(super) matching_type: Option<i64>,
    pub(super) parameters: Option<String>,
}

impl RecordOptions {
    pub(super) fn from_write_args(args: &RecordWriteArgs) -> Self {
        RecordOptions {
            ttl: args.ttl,
            priority: args.priority,
            port: args.port,
            weight: args.weight,
            protocol: args.protocol.clone(),
            service: args.service.clone(),
            flag: args.flag,
            tag: args.tag.clone(),
            usage: args.usage,
            selector: args.selector,
            matching_type: args.matching_type,
            parameters: args.parameters.clone(),
        }
    }
}

/// Flags below that clap only requires conditionally, via `required_if_eq`.
/// clap doesn't expose that relation publicly, so `spec_cmd` reads this
/// table by hand. Keep in sync with the attributes below.
#[cfg_attr(not(feature = "dev-portal-spec"), allow(dead_code))]
pub(crate) const CONDITIONALLY_REQUIRED_FLAGS: &[(&str, &str)] = &[
    (
        "priority",
        "required when --type is MX, SRV, HTTPS, or SVCB",
    ),
    ("weight", "required when --type is SRV"),
    ("port", "required when --type is SRV or TLSA"),
    ("service", "required when --type is SRV"),
    ("protocol", "required when --type is SRV or TLSA"),
    ("tag", "required when --type is CAA"),
    ("usage", "required when --type is TLSA"),
    ("selector", "required when --type is TLSA"),
    ("matching-type", "required when --type is TLSA"),
];

/// Shared flags for the mutating commands (`add`/`set`): required type/name and
/// the repeatable `--data`, plus the optional record fields.
#[derive(Debug, Clone, clap::Args)]
pub(super) struct RecordWriteArgs {
    /// Domain whose records to modify (e.g. example.com).
    #[arg(value_name = "DOMAIN", value_parser = crate::idn::parse_name_arg)]
    pub(super) domain: String,

    // `ignore_case` is load-bearing, not cosmetic. The `required_if_eq` predicates
    // below compare against the *raw* `--type` value, not the `parse_write_type_arg`
    // output — so without it `--type tlsa` (accepted, and canonicalized to `TLSA`)
    // never trips the `--usage`/`--selector`/`--matching-type` requirement, and
    // `--type caa` never trips `--tag`.
    /// Record type (A, AAAA, ALIAS, CAA, CNAME, HTTPS, MX, SRV, SVCB, TLSA, TXT).
    #[arg(long = "type", value_name = "TYPE", value_parser = parse_write_type_arg, ignore_case = true)]
    pub(super) record_type: String,

    /// Record name relative to the domain (e.g. www, @ for the apex).
    #[arg(long, value_name = "NAME", value_parser = crate::idn::parse_name_arg)]
    pub(super) name: String,

    #[command(flatten)]
    pub(super) domain_format: crate::idn::DomainFormArgs,

    /// Record value (repeatable for multiple records on the same name). For
    /// TLSA, this is the hex-encoded certificate association data; use
    /// `--usage`/`--selector`/`--matching-type` for the rest.
    #[arg(long, value_name = "VALUE", required = true)]
    pub(super) data: Vec<String>,

    /// Time-to-live in seconds, 600 (10 minutes) to 86400 (24 hours). Defaults
    /// to 3600 when omitted; `set` keeps a record's current TTL instead.
    #[arg(long, value_name = "SECONDS", value_parser = clap::value_parser!(i64).range(MIN_TTL..=MAX_TTL))]
    pub(super) ttl: Option<i64>,

    /// Record priority (MX, SRV, HTTPS, and SVCB only; required for those). For
    /// HTTPS/SVCB, 0 means AliasMode.
    #[arg(
        long,
        value_name = "N",
        value_parser = clap::value_parser!(i64).range(0..=65535),
        required_if_eq_any([
            ("record_type", "MX"),
            ("record_type", "SRV"),
            ("record_type", "HTTPS"),
            ("record_type", "SVCB"),
        ])
    )]
    pub(super) priority: Option<i64>,

    /// Service port (SRV and TLSA only; required for those).
    #[arg(
        long,
        value_name = "PORT",
        value_parser = clap::value_parser!(i64).range(1..=65535),
        required_if_eq_any([("record_type", "SRV"), ("record_type", "TLSA")])
    )]
    pub(super) port: Option<i64>,

    /// Record weight (SRV only; required for SRV).
    #[arg(
        long,
        value_name = "N",
        value_parser = clap::value_parser!(i64).range(0..=65535),
        required_if_eq("record_type", "SRV")
    )]
    pub(super) weight: Option<i64>,

    /// Service protocol, e.g. tcp or udp (SRV and TLSA only; required for those).
    /// A leading underscore is added if you leave it off.
    #[arg(
        long,
        value_name = "PROTO",
        value_parser = parse_underscore_label,
        required_if_eq_any([("record_type", "SRV"), ("record_type", "TLSA")])
    )]
    pub(super) protocol: Option<String>,

    /// Service name, e.g. sip or xmpp (SRV only; required for SRV). A leading
    /// underscore is added if you leave it off.
    #[arg(
        long,
        value_name = "SERVICE",
        value_parser = parse_underscore_label,
        required_if_eq("record_type", "SRV")
    )]
    pub(super) service: Option<String>,

    /// CAA flag byte, 0-255 (CAA only; 0 non-critical, 128 critical). Defaults to 0.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(i64).range(0..=255))]
    pub(super) flag: Option<i64>,

    // A CAA record needs a tag; enforce it at parse time (before auth) — which
    // holds for `--type caa` as well as `--type CAA` only because `record_type`
    // sets `ignore_case`. The reverse guard — flag/tag only valid for CAA —
    // lives in the handler (`validate_caa_fields`), which clap can't express.
    /// CAA property tag, e.g. issue/issuewild/iodef (CAA only; required for CAA).
    #[arg(long, value_name = "TAG", required_if_eq("record_type", "CAA"))]
    pub(super) tag: Option<String>,

    /// TLSA certificate usage, 0-3 (RFC 6698 §2.1.1; TLSA only; required for
    /// TLSA). 0 PKIX-TA, 1 PKIX-EE, 2 DANE-TA, 3 DANE-EE.
    #[arg(long = "usage", value_name = "N", value_parser = clap::value_parser!(i64).range(0..=3), required_if_eq("record_type", "TLSA"))]
    pub(super) usage: Option<i64>,

    /// TLSA selector, 0-1 (RFC 6698 §2.1.2; TLSA only; required for TLSA). 0
    /// full certificate, 1 SubjectPublicKeyInfo.
    #[arg(long = "selector", value_name = "N", value_parser = clap::value_parser!(i64).range(0..=1), required_if_eq("record_type", "TLSA"))]
    pub(super) selector: Option<i64>,

    /// TLSA matching type, 0-2 (RFC 6698 §2.1.3; TLSA only; required for
    /// TLSA). 0 exact match, 1 SHA-256, 2 SHA-512.
    #[arg(long = "matching-type", value_name = "N", value_parser = clap::value_parser!(i64).range(0..=2), required_if_eq("record_type", "TLSA"))]
    pub(super) matching_type: Option<i64>,

    /// SvcParams for HTTPS/SVCB records (RFC 9460), e.g. "alpn=h2,h3
    /// port=8443" (HTTPS/SVCB only).
    #[arg(long, value_name = "PARAMS")]
    pub(super) parameters: Option<String>,
}

/// Validate the CAA-specific fields against the record type. A CAA record needs a
/// `--tag` (`--data` carries the CA domain / value); `--flag`/`--tag` are
/// meaningless for other types. Pure so it's unit-testable and runs before any
/// network call.
pub(super) fn validate_caa_fields(record_type: &str, opts: &RecordOptions) -> Result<(), String> {
    if record_type == "CAA" {
        if opts.tag.as_deref().map(str::trim).unwrap_or("").is_empty() {
            return Err(
                "CAA records require --tag (e.g. issue, issuewild, iodef); --data carries the \
                 CA domain / value"
                    .to_string(),
            );
        }
    } else if opts.flag.is_some() || opts.tag.is_some() {
        return Err(format!(
            "--flag/--tag are only valid for CAA records, not {record_type}"
        ));
    }
    Ok(())
}

/// `dns add` and `dns set` write the record under the `--name` they are given, so
/// for SRV and TLSA that must be the record's own name, not the full DNS name the
/// API also understands as a selector (`_sip._tcp.chat`, `_443._tcp.www`). Written
/// under the full name, a record whose prefix differs from its service and
/// protocol lands at a nested name nobody asked for (and `set` would move an
/// existing record there). Reject it up front and say what to type. Pure so it is
/// unit-testable and runs before any network call.
pub(super) fn validate_record_name(record_type: &str, name: &str) -> Result<(), String> {
    if !COMPOSITE_NAME_TYPES.contains(&record_type) {
        return Ok(());
    }
    let mut labels = name.splitn(3, '.');
    let (Some(first), Some(second)) = (labels.next(), labels.next()) else {
        return Ok(());
    };
    if !(first.starts_with('_') && second.starts_with('_')) {
        return Ok(());
    }
    let own_name = labels.next().unwrap_or("@");
    let flags = if record_type == "SRV" {
        "--service/--protocol"
    } else {
        "--port/--protocol"
    };
    Err(format!(
        "use the record's own name, not the full DNS name: for {record_type} write \
         `--name {own_name}` and give {flags} ({first}.{second} here) separately"
    ))
}

/// Validate the TLSA-specific fields against the record type. `--usage`/
/// `--selector`/`--matching-type` being present when `record_type` is TLSA is
/// enforced by clap (`required_if_eq`, which covers a lower-case `--type` only
/// because the arg sets `ignore_case`); this only guards the reverse — they're
/// meaningless for other types. Pure so it's unit-testable and runs before any
/// network call.
pub(super) fn validate_tlsa_fields(record_type: &str, opts: &RecordOptions) -> Result<(), String> {
    if record_type != "TLSA"
        && (opts.usage.is_some() || opts.selector.is_some() || opts.matching_type.is_some())
    {
        return Err(format!(
            "--usage/--selector/--matching-type are only valid for TLSA records, not {record_type}"
        ));
    }
    Ok(())
}

/// Validate the HTTPS/SVCB-specific `--parameters` (SvcParams) field against
/// the record type — meaningless for anything else. Pure so it's
/// unit-testable and runs before any network call.
pub(super) fn validate_svcb_fields(record_type: &str, opts: &RecordOptions) -> Result<(), String> {
    if opts.parameters.is_some() && !matches!(record_type, "HTTPS" | "SVCB") {
        return Err(format!(
            "--parameters is only valid for HTTPS/SVCB records, not {record_type}"
        ));
    }
    Ok(())
}

/// Build one v3 `DnsRecord` from a `--data` value + shared options. `ttl` defaults
/// to [`DEFAULT_TTL`] (v3 requires it). SRV/MX numerics convert into v3's `u16`
/// and the CAA `flag`/TLSA `usage`/`selector`/`matchingType` into `u8` (clap
/// already bounds all four ranges). TLSA doesn't use `data` — v3 wants its
/// value under `certificateData` instead — so `data`'s value moves there.
pub(super) fn v3_record(
    name: &str,
    ty: &str,
    data: &str,
    opts: &RecordOptions,
) -> types::DnsRecord {
    let to_u16 = |v: Option<i64>| v.and_then(|n| u16::try_from(n).ok());
    let to_u8 = |v: Option<i64>| v.and_then(|n| u8::try_from(n).ok());
    let is_tlsa = ty == "TLSA";
    types::DnsRecord {
        certificate_data: is_tlsa.then(|| data.to_owned()),
        matching_type: is_tlsa
            .then(|| to_u8(opts.matching_type))
            .flatten()
            .map(types::TlsaMatchingType),
        selector: is_tlsa
            .then(|| to_u8(opts.selector))
            .flatten()
            .map(types::TlsaSelector),
        usage: is_tlsa
            .then(|| to_u8(opts.usage))
            .flatten()
            .map(types::TlsaUsage),
        data: (!is_tlsa).then(|| data.to_owned()),
        // The API requires a flag on CAA records; 0 (non-critical) is the usual one.
        flag: opts
            .flag
            .or((ty == "CAA").then_some(0))
            .and_then(|n| u8::try_from(n).ok()),
        name: name.to_owned(),
        parameters: opts.parameters.clone(),
        port: to_u16(opts.port),
        priority: to_u16(opts.priority),
        protocol: opts.protocol.clone(),
        record_id: None,
        service: opts.service.clone(),
        tag: opts.tag.clone(),
        ttl: opts.ttl.unwrap_or(DEFAULT_TTL),
        type_: types::DnsRecordType(ty.to_owned()),
        weight: to_u16(opts.weight),
    }
}

/// Build the v3 `DnsRecord`s for `add` — one per `--data` value (parallel to it).
pub(super) fn v3_records(
    name: &str,
    ty: &str,
    data: &[String],
    opts: &RecordOptions,
) -> Vec<types::DnsRecord> {
    data.iter().map(|d| v3_record(name, ty, d, opts)).collect()
}

/// The user-facing "value" of a fetched v3 `DnsRecord`: `data` for every type
/// except TLSA, which carries its value in `certificateData` instead (see
/// [`v3_record`]'s TLSA branch). Delete/set per-record reporting and the
/// CNAME-conflict message need "the value" regardless of which wire field
/// holds it, so they read through this rather than `data` directly (exact-
/// duplicate/no-op detection compares full records via [`same_content`]
/// instead, since a shared value alone doesn't make two records identical).
/// Checks `type_` rather than just falling back on an absent `data`, so a
/// TLSA record correctly prefers `certificateData` even if the API ever
/// returns both fields populated.
pub(super) fn record_value(rec: &types::DnsRecord) -> Option<&str> {
    if rec.type_.as_str() == "TLSA" {
        rec.certificate_data.as_deref().or(rec.data.as_deref())
    } else {
        rec.data.as_deref().or(rec.certificate_data.as_deref())
    }
}

/// Re-merge a TLSA record's split fields (`usage`/`selector`/`matchingType`/
/// `certificateData`) back into a single presentation-format string, for
/// `dns list` output. DNS itself treats a TLSA record's RDATA as one blob
/// (RFC 6698 §2.1) — `dig`/zone-file output shows
/// `"<usage> <selector> <matching-type> <hex>"` — the v3 API's split into
/// four JSON fields is that API's own modeling, not something DNS does (same
/// story as CAA's `flag`/`tag`/`data` split). The write path (`v3_record`)
/// never populates `data` for TLSA, so `dns list` reconstructs it here for
/// display; the original four fields are left untouched alongside it. Only
/// builds the full presentation string when all three numeric fields are
/// present — the API's schema guarantees that together for a real TLSA
/// record, but substituting 0 (a real, meaningful usage/selector/matching-type
/// value, not a sentinel) for a genuinely absent field would fabricate RDATA
/// that was never actually returned; fall back to the certificate data alone.
pub(super) fn merged_tlsa_data(rec: &types::DnsRecord) -> String {
    match (&rec.usage, &rec.selector, &rec.matching_type) {
        (Some(usage), Some(selector), Some(matching_type)) => format!(
            "{} {} {} {}",
            usage.0,
            selector.0,
            matching_type.0,
            rec.certificate_data.as_deref().unwrap_or(""),
        ),
        _ => rec.certificate_data.clone().unwrap_or_default(),
    }
}

/// The fields that tell apart records of one type at one name (an SRV's service,
/// port and priority; an MX's priority; a CAA's flag and tag; an HTTPS record's
/// parameters). TLSA's four fields are left out because [`merged_tlsa_data`]
/// already folds them into `data`.
pub(super) const DETAIL_FIELDS: &[&str] = &[
    "priority",
    "weight",
    "port",
    "service",
    "protocol",
    "flag",
    "tag",
    "parameters",
];

/// Table columns for [`DETAIL_FIELDS`], each shown only when some listed record
/// has a value for it, so a table of A and TXT records is unchanged while SRV,
/// MX and CAA records bring the fields that distinguish them.
pub(super) fn detail_columns() -> Vec<TableColumn> {
    detail_columns_at("", "")
}

/// [`detail_columns`] for fields held in a nested object: `path` is prepended to
/// each field name (`old.`) and `label` to each header (`Old `).
fn detail_columns_at(path: &str, label: &str) -> Vec<TableColumn> {
    let numeric = ["priority", "weight", "port", "flag"];
    DETAIL_FIELDS
        .iter()
        .map(|field| {
            let header = match *field {
                "priority" => "Priority",
                "weight" => "Weight",
                "port" => "Port",
                "service" => "Service",
                "protocol" => "Protocol",
                "flag" => "Flag",
                "tag" => "Tag",
                _ => "Parameters",
            };
            let column = TableColumn::new(format!("{path}{field}"), format!("{label}{header}"))
                .hide_when_empty(true);
            if numeric.contains(field) {
                column.align(Alignment::Right)
            } else {
                column
            }
        })
        .collect()
}

/// The [`DETAIL_FIELDS`] a record actually has, as JSON members to merge into a
/// row of output.
pub(super) fn detail_values(rec: &types::DnsRecord) -> serde_json::Map<String, Value> {
    let Ok(Value::Object(all)) = serde_json::to_value(rec) else {
        return serde_json::Map::new();
    };
    DETAIL_FIELDS
        .iter()
        .filter_map(|field| all.get(*field).map(|v| ((*field).to_owned(), v.clone())))
        .collect()
}

/// How a record is named in a plan entry and in per-record results
pub(super) fn record_label_value(rec: &types::DnsRecord) -> Option<String> {
    if rec.type_.as_str() == "TLSA" {
        Some(merged_tlsa_data(rec)).filter(|text| !text.is_empty())
    } else {
        record_value(rec).map(str::to_owned)
    }
}

/// A record as it appears in a plan entry's `old` or `new`
pub(super) fn record_snapshot(rec: &types::DnsRecord) -> Value {
    let mut row = detail_values(rec);
    row.insert("type".to_owned(), json!(rec.type_.as_str()));
    row.insert("data".to_owned(), json!(record_label_value(rec)));
    row.insert("ttl".to_owned(), json!(rec.ttl));
    Value::Object(row)
}

/// One entry of the `records` that `dns add`, `dns set` and `dns delete` report,
/// whether the write is only previewed (`--dry-run`) or really done: what happens
/// to one record (`create`, `replace`, `unchanged` or `delete`), the record's id
/// (`null` for a create that has not been made yet), and the record before
/// (`old`) and after (`new`), `null` where there is none.
pub(super) fn change_entry(
    action: &str,
    record_id: Option<&str>,
    old: Option<&types::DnsRecord>,
    new: Option<&types::DnsRecord>,
) -> Value {
    json!({
        "action": action,
        "recordId": record_id,
        "old": old.map(record_snapshot),
        "new": new.map(record_snapshot),
    })
}

/// Table columns for the `records` of a write (see [`change_entry`]): the action, the
/// old and new value, the type-specific fields that tell records apart (only when
/// some entry has them, like [`detail_columns`]), then TTLs and the record id.
/// Column order is priority order — on a narrow terminal the last columns go first
/// — so what identifies a record comes before what merely describes it.
pub(super) fn change_columns() -> Vec<TableColumn> {
    let mut columns = vec![
        TableColumn::new("action", "Action").essential(true),
        TableColumn::new("old.data", "Old").hide_when_empty(true),
        TableColumn::new("new.data", "New").hide_when_empty(true),
    ];
    // What tells one record from another under the same name (an SRV's service and
    // port, a CAA's tag) is never dropped to make room: a delete of several
    // otherwise-identical rows is unreadable without it.
    columns.extend(detail_columns_at("old.", "Old ").into_iter().map(|column| {
        let identifies = matches!(
            column.field.as_str(),
            "old.service" | "old.port" | "old.tag"
        );
        column.essential(identifies)
    }));
    columns.extend(detail_columns_at("new.", "New "));
    columns.extend([
        TableColumn::new("old.ttl", "Old TTL")
            .align(Alignment::Right)
            .hide_when_empty(true),
        TableColumn::new("new.ttl", "New TTL")
            .align(Alignment::Right)
            .hide_when_empty(true),
        TableColumn::new("recordId", "Record ID"),
        TableColumn::new("error", "Error").hide_when_empty(true),
    ]);
    columns
}

/// Whether two v3 records have the same content — every field except `ttl`
/// and `recordId`, neither of which makes two records a different *value*
/// (a TTL-only change was never treated as a new value, and `recordId` only
/// ever exists on a fetched record, never a desired one). Used by
/// duplicate/no-op detection so a record type whose identity spans several
/// fields (CAA's `flag`/`tag`, SRV's `priority`/`weight`/`port`, TLSA's
/// `usage`/`selector`/`matchingType`, HTTPS/SVCB's `priority`/`parameters`)
/// isn't misjudged by comparing only [`record_value`]. Compares via JSON
/// rather than a hand-maintained field list, so a future `DnsRecord` field
/// is covered without another update here.
pub(super) fn same_content(a: &types::DnsRecord, b: &types::DnsRecord) -> bool {
    comparable(a, false) == comparable(b, false)
}

/// The content of a record as the API would hold it, for comparing two records
/// that are meant to be the same. Drops `recordId` and, unless `compare_ttl`,
/// `ttl`; the API stores record names lower-cased, and hostname-valued `data`
/// lower-cased with a trailing dot (`Example.COM` comes back as `example.com.`),
/// so those are compared in that form. CAA `data` (the API lower-cases it, URLs
/// included) and TLSA `certificateData` (hex) are compared without case. Everything
/// else is compared exactly — the API keeps TXT text, AAAA addresses, HTTPS/SVCB
/// `parameters` and SRV `service`/`protocol` as written, so case in them is real.
///
/// Compares via JSON rather than a hand-maintained field list, so a future
/// `DnsRecord` field is covered without another update here. `DnsRecord` is all
/// plain String/Option/newtype fields, so serializing it should never fail —
/// falling back to a placeholder on error would let two unrelated records that
/// both failed to serialize compare as "equal" and silently skip a real change.
pub(super) fn comparable(record: &types::DnsRecord, compare_ttl: bool) -> Value {
    let mut value = serde_json::to_value(record).expect("DnsRecord always serializes to JSON");
    let record_type = record.type_.as_str();
    if let Some(fields) = value.as_object_mut() {
        fields.remove("recordId");
        if !compare_ttl {
            fields.remove("ttl");
        }
        if let Some(Value::String(name)) = fields.get_mut("name") {
            *name = name.to_lowercase();
        }
        if let Some(Value::String(data)) = fields.get_mut("data") {
            match record_type {
                "ALIAS" | "CNAME" | "MX" | "SRV" | "HTTPS" | "SVCB" => {
                    *data = data.trim_end_matches('.').to_lowercase();
                }
                "CAA" => *data = data.to_lowercase(),
                _ => {}
            }
        }
        // Hex digits mean the same in either case.
        if let Some(Value::String(cert)) = fields.get_mut("certificateData") {
            *cert = cert.to_lowercase();
        }
    }
    value
}

/// The record as the API will hold it once written, for a preview to show what a
/// real run will return: names lower-cased, hostname-valued data lower-cased with
/// a trailing dot, CAA data and TLSA hex lower-cased (the rules [`comparable`]
/// compares by). Keeps the TTL and any recordId. A record that does not survive
/// the round trip is returned as given.
pub(super) fn as_stored(record: &types::DnsRecord) -> types::DnsRecord {
    let mut value = comparable(record, true);
    if let Some(fields) = value.as_object_mut() {
        // `comparable` trims the dot off hostnames to compare them; the API keeps it.
        // The root target `.` is all dot, so trimming leaves it empty; that is
        // still a value to restore, unlike a record that had none.
        let had_data = record.data.as_deref().is_some_and(|data| !data.is_empty());
        if let Some(Value::String(data)) = fields.get_mut("data")
            && had_data
            && matches!(
                record.type_.as_str(),
                "ALIAS" | "CNAME" | "MX" | "SRV" | "HTTPS" | "SVCB"
            )
        {
            data.push('.');
        }
    }
    serde_json::from_value::<types::DnsRecord>(value)
        .map(|stored| types::DnsRecord {
            record_id: record.record_id.clone(),
            ..stored
        })
        .unwrap_or_else(|_| record.clone())
}

/// Types the API stores under a composed name (`_service._proto.name` for SRV,
/// `_port._proto.name` for TLSA), while a list response shows only the short
/// `name` plus separate fields. A server-side `name` filter for those types
/// matches the composed form, so filtering by the short name finds nothing.
const COMPOSITE_NAME_TYPES: &[&str] = &["SRV", "TLSA"];

/// List every v3 DNS record for a zone matching the optional `type`/`name`
/// filters, paging through the collection (v3 list is paginated). Shared by
/// `list`, `set`, and `delete` — the latter two need the matching records' ids.
///
/// For SRV and TLSA the `name` is matched here, against each record's short
/// name, rather than sent to the API: its filter only understands the composed
/// name, so it would return nothing for the name the record is listed under.
pub(super) async fn fetch_records(
    client: &domains_client::Client,
    zone: &str,
    type_: Option<&str>,
    name: Option<&str>,
    debug: bool,
) -> Result<Vec<types::DnsRecord>, CliCoreError> {
    let match_name_here = type_.is_some_and(|t| COMPOSITE_NAME_TYPES.contains(&t));
    let server_name = if match_name_here { None } else { name };
    let mut all = Vec::new();
    let mut page: u64 = 1;
    loop {
        let page_nz = std::num::NonZeroU64::new(page).expect("page starts at 1 and only grows");
        let mut req = client
            .list_dns_records()
            .zone(zone)
            .page(page_nz)
            .page_size(LIST_PAGE_SIZE)
            .total_required(true);
        if let Some(t) = type_ {
            req = req.type_(types::DnsRecordType(t.to_owned()));
        }
        if let Some(n) = server_name {
            req = req.name(n);
        }
        let body = match req.send().await {
            Ok(r) => r.into_inner(),
            Err(e) => return Err(api_error("listing DNS records", debug, e).await),
        };
        let items = body.items.unwrap_or_default();
        let got = items.len();
        all.extend(items);
        // Last page when the API's total_pages is reached, or (absent that) a
        // short/empty page came back.
        let last = body
            .total_pages
            .map(|tp| page >= tp.get())
            .unwrap_or(got < LIST_PAGE_SIZE.get() as usize);
        if last || got == 0 {
            break;
        }
        page += 1;
    }
    if match_name_here && let Some(wanted) = name {
        // A record listed under exactly this name wins. Otherwise `chat` and a
        // separate record named `_sip._tcp.chat` would both answer to
        // `_sip._tcp.chat`, and a delete would reach an owner it never named.
        if all
            .iter()
            .any(|record| record.name.eq_ignore_ascii_case(wanted))
        {
            all.retain(|record| record.name.eq_ignore_ascii_case(wanted));
        } else {
            all.retain(|record| listed_name_matches(record, wanted));
        }
    }
    Ok(all)
}

/// The name the API files a record under. For SRV (`_service._proto.name`) and
/// TLSA (`_port._proto.name`) that is the composed owner, while a list response
/// shows only the short `name` plus separate fields; every other type is filed
/// under its own name. At the zone apex (`@`) the composed owner is the prefix
/// alone. `None` for an SRV or TLSA record missing a field the prefix needs.
pub(super) fn composed_owner(record: &types::DnsRecord) -> Option<String> {
    let prefix = match record.type_.as_str() {
        "SRV" => record
            .service
            .as_deref()
            .zip(record.protocol.as_deref())
            .map(|(service, protocol)| format!("{service}.{protocol}")),
        "TLSA" => record
            .port
            .zip(record.protocol.as_deref())
            .map(|(port, protocol)| format!("_{port}.{protocol}")),
        _ => return Some(record.name.clone()),
    }?;
    Some(match record.name.as_str() {
        "@" => prefix,
        name => format!("{prefix}.{name}"),
    })
}

/// Whether `wanted` names `record`: either the short name the record is listed
/// under, or the composed name the API itself understands (see
/// [`composed_owner`]), so a selector written either way finds it. Case does not
/// matter.
fn listed_name_matches(record: &types::DnsRecord, wanted: &str) -> bool {
    record.name.eq_ignore_ascii_case(wanted)
        || composed_owner(record).is_some_and(|owner| owner.eq_ignore_ascii_case(wanted))
}

/// Next action pointing back at `dns list` to verify a write, pre-filled with
/// the domain/type/name the write just touched.
pub(super) fn verify_with_list_action(domain: &str, record_type: &str, name: &str) -> NextAction {
    next_action(
        "dns list <domain> --type <type> --name <name>",
        "Verify the records for this type+name",
    )
    .with_param("domain", NextActionParam::value(domain))
    .with_param("type", NextActionParam::value(record_type))
    .with_param("name", NextActionParam::value(name))
}

/// A mutation's success payload plus the "verify with `dns list`" next action,
/// with the zone and record name shown in `domain_format`.
pub(super) fn verified_result(
    domain_format: DomainForm,
    mut payload: Value,
    domain: &str,
    record_type: &str,
    name: &str,
) -> CommandResult {
    domain_format.convert_fields(&mut payload, ZONE_AND_NAME);
    CommandResult::new(payload).with_next_actions(vec![verify_with_list_action(
        &domain_format.render(domain),
        record_type,
        &domain_format.render(name),
    )])
}

#[cfg(test)]
#[path = "records_tests.rs"]
mod tests;
