//! Domain Connect template types (ported from dc-template-cli)

use serde::{Deserialize, Serialize};

/// Domain Connect template structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Template {
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
    pub records: Vec<Record>,
}

/// A single DNS record in the template
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
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
    pub ttl: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub essential: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spf_rules: Option<String>,
}

impl Template {
    /// Create a basic template from minimal inputs
    pub fn from_basic_inputs(
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
            records: Vec::new(),
        }
    }

    /// Add a simple A record
    #[allow(dead_code)]
    pub fn add_a_record(&mut self, host: String, ip_variable: String) {
        self.add_a_record_with_options(host, ip_variable, None, None);
    }

    /// Add a simple A record with groupId and essential options
    pub fn add_a_record_with_options(
        &mut self,
        host: String,
        ip_variable: String,
        group_id: Option<String>,
        essential: Option<String>,
    ) {
        self.records.push(Record {
            record_type: "A".to_string(),
            group_id,
            host: Some(host),
            name: None,
            points_to: Some(format!("%{}%", ip_variable)),
            target: None,
            service: None,
            protocol: None,
            ttl: Some(3600),
            priority: None,
            weight: None,
            port: None,
            data: None,
            essential,
            spf_rules: None,
        });
    }

    /// Add a CNAME record
    #[allow(dead_code)]
    pub fn add_cname_record(&mut self, host: String, target: String) {
        self.add_cname_record_with_options(host, target, None, None);
    }

    /// Add a CNAME record with groupId and essential options
    pub fn add_cname_record_with_options(
        &mut self,
        host: String,
        target: String,
        group_id: Option<String>,
        essential: Option<String>,
    ) {
        self.records.push(Record {
            record_type: "CNAME".to_string(),
            group_id,
            host: Some(host),
            name: None,
            points_to: Some(target),
            target: None,
            service: None,
            protocol: None,
            ttl: Some(3600),
            priority: None,
            weight: None,
            port: None,
            data: None,
            essential,
            spf_rules: None,
        });
    }

    /// Add an AAAA record (IPv6)
    #[allow(dead_code)]
    pub fn add_aaaa_record(&mut self, host: String, ipv6_variable: String) {
        self.add_aaaa_record_with_options(host, ipv6_variable, None, None);
    }

    /// Add an AAAA record with groupId and essential options
    pub fn add_aaaa_record_with_options(
        &mut self,
        host: String,
        ipv6_variable: String,
        group_id: Option<String>,
        essential: Option<String>,
    ) {
        self.records.push(Record {
            record_type: "AAAA".to_string(),
            group_id,
            host: Some(host),
            name: None,
            points_to: Some(format!("%{}%", ipv6_variable)),
            target: None,
            service: None,
            protocol: None,
            ttl: Some(3600),
            priority: None,
            weight: None,
            port: None,
            data: None,
            essential,
            spf_rules: None,
        });
    }

    /// Add an MX record (mail exchanger)
    #[allow(dead_code)]
    pub fn add_mx_record(&mut self, host: String, mail_server: String, priority: i32) {
        self.add_mx_record_with_options(host, mail_server, priority, None, None);
    }

    /// Add an MX record with groupId and essential options
    pub fn add_mx_record_with_options(
        &mut self,
        host: String,
        mail_server: String,
        priority: i32,
        group_id: Option<String>,
        essential: Option<String>,
    ) {
        self.records.push(Record {
            record_type: "MX".to_string(),
            group_id,
            host: Some(host),
            name: None,
            points_to: Some(mail_server),
            target: None,
            service: None,
            protocol: None,
            ttl: Some(3600),
            priority: Some(priority),
            weight: None,
            port: None,
            data: None,
            essential,
            spf_rules: None,
        });
    }

    /// Add a TXT record
    #[allow(dead_code)]
    pub fn add_txt_record(&mut self, host: String, data: String) {
        self.add_txt_record_with_options(host, data, None, None);
    }

    /// Add a TXT record with groupId and essential options
    pub fn add_txt_record_with_options(
        &mut self,
        host: String,
        data: String,
        group_id: Option<String>,
        essential: Option<String>,
    ) {
        self.records.push(Record {
            record_type: "TXT".to_string(),
            group_id,
            host: Some(host),
            name: None,
            points_to: None,
            target: None,
            service: None,
            protocol: None,
            ttl: Some(3600),
            priority: None,
            weight: None,
            port: None,
            data: Some(data),
            essential,
            spf_rules: None,
        });
    }

    /// Add an SRV record (service)
    #[allow(dead_code)]
    #[allow(clippy::too_many_arguments)]
    pub fn add_srv_record(
        &mut self,
        service: String,
        protocol: String,
        name: String,
        target: String,
        priority: i32,
        weight: i32,
        port: i32,
    ) {
        self.add_srv_record_with_options(
            service, protocol, name, target, priority, weight, port, None, None,
        );
    }

    /// Add an SRV record with groupId and essential options
    #[allow(clippy::too_many_arguments)]
    pub fn add_srv_record_with_options(
        &mut self,
        service: String,
        protocol: String,
        name: String,
        target: String,
        priority: i32,
        weight: i32,
        port: i32,
        group_id: Option<String>,
        essential: Option<String>,
    ) {
        self.records.push(Record {
            record_type: "SRV".to_string(),
            group_id,
            host: None,
            name: Some(name),
            points_to: None,
            target: Some(target),
            service: Some(service),
            protocol: Some(protocol),
            ttl: Some(3600),
            priority: Some(priority),
            weight: Some(weight),
            port: Some(port),
            data: None,
            essential,
            spf_rules: None,
        });
    }

    /// Add an SPFM record (SPF Macro - Domain Connect specific)
    #[allow(dead_code)]
    pub fn add_spfm_record(&mut self, host: String, spf_rules: String) {
        self.add_spfm_record_with_options(host, spf_rules, None, None);
    }

    /// Add an SPFM record with groupId and essential options
    pub fn add_spfm_record_with_options(
        &mut self,
        host: String,
        spf_rules: String,
        group_id: Option<String>,
        essential: Option<String>,
    ) {
        self.records.push(Record {
            record_type: "SPFM".to_string(),
            group_id,
            host: Some(host),
            name: None,
            points_to: None,
            target: None,
            service: None,
            protocol: None,
            ttl: Some(3600),
            priority: None,
            weight: None,
            port: None,
            data: None,
            essential,
            spf_rules: Some(spf_rules),
        });
    }

    /// Add a REDIR301 record (301 redirect - Domain Connect specific)
    #[allow(dead_code)]
    pub fn add_redir301_record(&mut self, host: String, target: String) {
        self.add_redir301_record_with_options(host, target, None, None);
    }

    /// Add a REDIR301 record with groupId and essential options
    pub fn add_redir301_record_with_options(
        &mut self,
        host: String,
        target: String,
        group_id: Option<String>,
        essential: Option<String>,
    ) {
        self.records.push(Record {
            record_type: "REDIR301".to_string(),
            group_id,
            host: Some(host),
            name: None,
            points_to: None,
            target: Some(target),
            service: None,
            protocol: None,
            ttl: None,
            priority: None,
            weight: None,
            port: None,
            data: None,
            essential,
            spf_rules: None,
        });
    }

    /// Add an APEXCNAME record (CNAME at apex - Domain Connect specific)
    #[allow(dead_code)]
    pub fn add_apexcname_record(&mut self, host: String, points_to: String) {
        self.add_apexcname_record_with_options(host, points_to, None, None);
    }

    /// Add an APEXCNAME record with groupId and essential options
    pub fn add_apexcname_record_with_options(
        &mut self,
        host: String,
        points_to: String,
        group_id: Option<String>,
        essential: Option<String>,
    ) {
        self.records.push(Record {
            record_type: "APEXCNAME".to_string(),
            group_id,
            host: Some(host),
            name: None,
            points_to: Some(points_to),
            target: None,
            service: None,
            protocol: None,
            ttl: Some(3600),
            priority: None,
            weight: None,
            port: None,
            data: None,
            essential,
            spf_rules: None,
        });
    }

    /// Add a CAA record (Certificate Authority Authorization)
    #[allow(dead_code)]
    pub fn add_caa_record(&mut self, host: String, data: String) {
        self.add_caa_record_with_options(host, data, None, None);
    }

    /// Add a CAA record with groupId and essential options
    pub fn add_caa_record_with_options(
        &mut self,
        host: String,
        data: String,
        group_id: Option<String>,
        essential: Option<String>,
    ) {
        self.records.push(Record {
            record_type: "CAA".to_string(),
            group_id,
            host: Some(host),
            name: None,
            points_to: None,
            target: None,
            service: None,
            protocol: None,
            ttl: Some(3600),
            priority: None,
            weight: None,
            port: None,
            data: Some(data),
            essential,
            spf_rules: None,
        });
    }

    /// Convert to pretty-printed JSON
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}
