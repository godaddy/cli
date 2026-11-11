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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub txt_conflict_matching_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub txt_conflict_matching_prefix: Option<String>,
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
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
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
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
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
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
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
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
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
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
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
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
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
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
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
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
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
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
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
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
        });
    }

    /// Convert to pretty-printed JSON
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_basic_inputs_creates_valid_template() {
        let template = Template::from_basic_inputs(
            "acme.com".to_string(),
            "Acme Corp".to_string(),
            "email".to_string(),
            "Email Service".to_string(),
            Some("Email configuration".to_string()),
        );

        assert_eq!(template.provider_id, "acme.com");
        assert_eq!(template.provider_name, "Acme Corp");
        assert_eq!(template.service_id, "email");
        assert_eq!(template.service_name, "Email Service");
        assert_eq!(template.version, Some(1));
        assert_eq!(
            template.description,
            Some("Email configuration".to_string())
        );
        assert!(template.records.is_empty());
    }

    #[test]
    fn add_a_record_with_options_creates_correct_record() {
        let mut template = Template::from_basic_inputs(
            "test.com".to_string(),
            "Test".to_string(),
            "web".to_string(),
            "Web".to_string(),
            None,
        );

        template.add_a_record_with_options(
            "@".to_string(),
            "ip".to_string(),
            Some("web".to_string()),
            Some("OnApply".to_string()),
        );

        assert_eq!(template.records.len(), 1);
        let record = &template.records[0];
        assert_eq!(record.record_type, "A");
        assert_eq!(record.host, Some("@".to_string()));
        assert_eq!(record.points_to, Some("%ip%".to_string()));
        assert_eq!(record.ttl, Some(3600));
        assert_eq!(record.group_id, Some("web".to_string()));
        assert_eq!(record.essential, Some("OnApply".to_string()));
    }

    #[test]
    fn add_mx_record_with_priority() {
        let mut template = Template::from_basic_inputs(
            "test.com".to_string(),
            "Test".to_string(),
            "email".to_string(),
            "Email".to_string(),
            None,
        );

        template.add_mx_record_with_options(
            "@".to_string(),
            "mail.test.com".to_string(),
            20,
            Some("mail".to_string()),
            None,
        );

        assert_eq!(template.records.len(), 1);
        let record = &template.records[0];
        assert_eq!(record.record_type, "MX");
        assert_eq!(record.points_to, Some("mail.test.com".to_string()));
        assert_eq!(record.priority, Some(20));
        assert_eq!(record.group_id, Some("mail".to_string()));
    }

    #[test]
    fn add_srv_record_with_all_fields() {
        let mut template = Template::from_basic_inputs(
            "test.com".to_string(),
            "Test".to_string(),
            "voip".to_string(),
            "VoIP".to_string(),
            None,
        );

        template.add_srv_record_with_options(
            "_sip".to_string(),
            "_tcp".to_string(),
            "@".to_string(),
            "sip.test.com".to_string(),
            10,
            5,
            5060,
            Some("voip".to_string()),
            None,
        );

        assert_eq!(template.records.len(), 1);
        let record = &template.records[0];
        assert_eq!(record.record_type, "SRV");
        assert_eq!(record.service, Some("_sip".to_string()));
        assert_eq!(record.protocol, Some("_tcp".to_string()));
        assert_eq!(record.name, Some("@".to_string()));
        assert_eq!(record.target, Some("sip.test.com".to_string()));
        assert_eq!(record.priority, Some(10));
        assert_eq!(record.weight, Some(5));
        assert_eq!(record.port, Some(5060));
    }

    #[test]
    fn add_txt_record_with_data() {
        let mut template = Template::from_basic_inputs(
            "test.com".to_string(),
            "Test".to_string(),
            "verify".to_string(),
            "Verify".to_string(),
            None,
        );

        template.add_txt_record_with_options(
            "@".to_string(),
            "v=spf1 ~all".to_string(),
            None,
            Some("Always".to_string()),
        );

        assert_eq!(template.records.len(), 1);
        let record = &template.records[0];
        assert_eq!(record.record_type, "TXT");
        assert_eq!(record.data, Some("v=spf1 ~all".to_string()));
        assert_eq!(record.essential, Some("Always".to_string()));
    }

    #[test]
    fn add_spfm_record() {
        let mut template = Template::from_basic_inputs(
            "test.com".to_string(),
            "Test".to_string(),
            "email".to_string(),
            "Email".to_string(),
            None,
        );

        template.add_spfm_record_with_options(
            "@".to_string(),
            "include:_spf.test.com".to_string(),
            None,
            None,
        );

        assert_eq!(template.records.len(), 1);
        let record = &template.records[0];
        assert_eq!(record.record_type, "SPFM");
        assert_eq!(record.spf_rules, Some("include:_spf.test.com".to_string()));
    }

    #[test]
    fn add_redir301_record() {
        let mut template = Template::from_basic_inputs(
            "test.com".to_string(),
            "Test".to_string(),
            "redirect".to_string(),
            "Redirect".to_string(),
            None,
        );

        template.add_redir301_record_with_options(
            "old".to_string(),
            "https://new.test.com".to_string(),
            None,
            None,
        );

        assert_eq!(template.records.len(), 1);
        let record = &template.records[0];
        assert_eq!(record.record_type, "REDIR301");
        assert_eq!(record.target, Some("https://new.test.com".to_string()));
    }

    #[test]
    fn json_serialization_uses_camel_case() {
        let mut template = Template::from_basic_inputs(
            "test.com".to_string(),
            "Test Company".to_string(),
            "service".to_string(),
            "Service Name".to_string(),
            None,
        );

        template.multi_instance = Some(true);
        template.host_required = Some(true);

        let json = template.to_json_pretty().expect("serialization failed");

        assert!(json.contains("\"providerId\""));
        assert!(json.contains("\"providerName\""));
        assert!(json.contains("\"serviceId\""));
        assert!(json.contains("\"serviceName\""));
        assert!(json.contains("\"multiInstance\""));
        assert!(json.contains("\"hostRequired\""));
        assert!(!json.contains("provider_id"));
        assert!(!json.contains("multi_instance"));
    }

    #[test]
    fn json_serialization_omits_none_fields() {
        let template = Template::from_basic_inputs(
            "test.com".to_string(),
            "Test".to_string(),
            "service".to_string(),
            "Service".to_string(),
            None,
        );

        let json = template.to_json_pretty().expect("serialization failed");

        assert!(!json.contains("\"logoUrl\""));
        assert!(!json.contains("\"syncBlock\""));
        assert!(!json.contains("\"shared\""));
        assert!(!json.contains("\"description\""));
    }

    #[test]
    fn multiple_records_of_same_type() {
        let mut template = Template::from_basic_inputs(
            "test.com".to_string(),
            "Test".to_string(),
            "email".to_string(),
            "Email".to_string(),
            None,
        );

        template.add_mx_record("@".to_string(), "mail1.test.com".to_string(), 10);
        template.add_mx_record("@".to_string(), "mail2.test.com".to_string(), 20);
        template.add_txt_record("@".to_string(), "v=spf1 ~all".to_string());
        template.add_txt_record("_dmarc".to_string(), "v=DMARC1; p=quarantine".to_string());

        assert_eq!(template.records.len(), 4);
        assert_eq!(
            template
                .records
                .iter()
                .filter(|r| r.record_type == "MX")
                .count(),
            2
        );
        assert_eq!(
            template
                .records
                .iter()
                .filter(|r| r.record_type == "TXT")
                .count(),
            2
        );
    }

    #[test]
    fn ipv6_variable_wrapping() {
        let mut template = Template::from_basic_inputs(
            "test.com".to_string(),
            "Test".to_string(),
            "web".to_string(),
            "Web".to_string(),
            None,
        );

        template.add_aaaa_record("@".to_string(), "IP6".to_string());

        let record = &template.records[0];
        assert_eq!(record.points_to, Some("%IP6%".to_string()));
    }
}
