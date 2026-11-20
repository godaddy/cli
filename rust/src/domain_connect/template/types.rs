//! Domain Connect template types.

use serde::{Deserialize, Serialize};

/// A Domain Connect template, written to `<providerId>.<serviceId>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Template {
    pub provider_id: String,
    pub provider_name: String,
    pub service_id: String,
    pub service_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variable_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_pub_key_domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_redirect_domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_block: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_provider_name: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_service_name: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multi_instance: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warn_phishing: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_required: Option<bool>,
    pub records: Vec<Record>,
}

impl Template {
    pub(super) fn from_basic_inputs(
        provider_id: String,
        provider_name: String,
        service_id: String,
        service_name: String,
        description: Option<String>,
    ) -> Self {
        Self {
            provider_id,
            provider_name,
            service_id,
            service_name,
            version: Some(1),
            logo_url: None,
            description,
            variable_description: None,
            sync_pub_key_domain: None,
            sync_redirect_domain: None,
            sync_block: None,
            shared: None,
            shared_provider_name: None,
            shared_service_name: None,
            multi_instance: None,
            warn_phishing: None,
            host_required: None,
            records: Vec::new(),
        }
    }

    pub(super) fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

/// The record types a template can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RecordKind {
    A,
    Aaaa,
    Cname,
    Mx,
    Txt,
    Srv,
    Ns,
    Spfm,
    Redir301,
    Redir302,
    Apexcname,
    Caa,
}

impl RecordKind {
    pub(super) const ALL: [Self; 12] = [
        Self::A,
        Self::Aaaa,
        Self::Cname,
        Self::Mx,
        Self::Txt,
        Self::Srv,
        Self::Ns,
        Self::Spfm,
        Self::Redir301,
        Self::Redir302,
        Self::Apexcname,
        Self::Caa,
    ];

    /// The record's `type` value in the template.
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::Aaaa => "AAAA",
            Self::Cname => "CNAME",
            Self::Mx => "MX",
            Self::Txt => "TXT",
            Self::Srv => "SRV",
            Self::Ns => "NS",
            Self::Spfm => "SPFM",
            Self::Redir301 => "REDIR301",
            Self::Redir302 => "REDIR302",
            Self::Apexcname => "APEXCNAME",
            Self::Caa => "CAA",
        }
    }

    pub(super) const fn description(self) -> &'static str {
        match self {
            Self::A => "IPv4 address",
            Self::Aaaa => "IPv6 address",
            Self::Cname => "alias for another name",
            Self::Mx => "mail server",
            Self::Txt => "text",
            Self::Srv => "service location",
            Self::Ns => "name server",
            Self::Spfm => "SPF rules merged into the domain's SPF record",
            Self::Redir301 => "permanent redirect",
            Self::Redir302 => "temporary redirect",
            Self::Apexcname => "alias at the root of the domain",
            Self::Caa => "certificate authority authorization",
        }
    }
}

/// How a record is treated when a later change conflicts with the template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
pub(super) enum Essential {
    #[value(name = "Always")]
    Always,
    #[value(name = "OnApply")]
    OnApply,
}

/// How conflicts with existing TXT records are detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
pub(super) enum TxtConflictMode {
    #[serde(rename = "None")]
    #[value(name = "None")]
    Off,
    #[value(name = "All")]
    All,
    #[value(name = "Prefix")]
    Prefix,
}

/// A number in a record: a literal, or a `%variable%` filled in when the
/// template is applied. Written to the file as a JSON number or a string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub(super) enum Number {
    Literal(i32),
    Variable(String),
}

/// A single DNS record in the template.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Record {
    #[serde(rename = "type")]
    pub record_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<Number>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<Number>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<Number>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<Number>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub essential: Option<Essential>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spf_rules: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub txt_conflict_matching_mode: Option<TxtConflictMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub txt_conflict_matching_prefix: Option<String>,
}

impl Record {
    pub(super) fn new(kind: RecordKind) -> Self {
        Self {
            record_type: kind.name().to_owned(),
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn template() -> Template {
        Template::from_basic_inputs(
            "acme.com".to_owned(),
            "Acme Corp".to_owned(),
            "email".to_owned(),
            "Email Service".to_owned(),
            None,
        )
    }

    #[test]
    fn from_basic_inputs_starts_at_version_one_with_no_records() {
        let template = template();
        assert_eq!(template.version, Some(1));
        assert!(template.records.is_empty());
    }

    #[test]
    fn template_serializes_camel_case_and_omits_unset_fields() {
        let mut template = template();
        template.multi_instance = Some(true);

        let value = serde_json::to_value(&template).expect("serialize");

        assert_eq!(
            value,
            json!({
                "providerId": "acme.com",
                "providerName": "Acme Corp",
                "serviceId": "email",
                "serviceName": "Email Service",
                "version": 1,
                "multiInstance": true,
                "records": [],
            })
        );
    }

    #[test]
    fn record_serializes_only_the_fields_that_are_set() {
        let record = Record {
            host: Some("@".to_owned()),
            data: Some("v=spf1 ~all".to_owned()),
            ttl: Some(Number::Literal(3600)),
            essential: Some(Essential::OnApply),
            txt_conflict_matching_mode: Some(TxtConflictMode::Off),
            ..Record::new(RecordKind::Txt)
        };

        assert_eq!(
            serde_json::to_value(&record).expect("serialize"),
            json!({
                "type": "TXT",
                "host": "@",
                "data": "v=spf1 ~all",
                "ttl": 3600,
                "essential": "OnApply",
                "txtConflictMatchingMode": "None",
            })
        );
    }

    #[test]
    fn every_record_kind_has_a_distinct_type_name() {
        let mut names: Vec<&str> = RecordKind::ALL.iter().map(|kind| kind.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), RecordKind::ALL.len());
    }
}
