//! `--distribution-type` flag shared by `gddy platform app init` and `update`.

use platform_app_client::{create_application, update_application};

/// Who can discover and install an application. `CUSTOM` is a deprecated
/// registry value that is never accepted as input, so it has no variant here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(super) enum DistributionTypeArg {
    /// Listed in the marketplace and installable by anyone.
    Public,
    /// Visible to and installable by the owning organization only.
    Private,
}

impl From<DistributionTypeArg> for create_application::DistributionType {
    fn from(value: DistributionTypeArg) -> Self {
        match value {
            DistributionTypeArg::Public => Self::PUBLIC,
            DistributionTypeArg::Private => Self::PRIVATE,
        }
    }
}

impl From<DistributionTypeArg> for update_application::DistributionType {
    fn from(value: DistributionTypeArg) -> Self {
        match value {
            DistributionTypeArg::Public => Self::PUBLIC,
            DistributionTypeArg::Private => Self::PRIVATE,
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::ValueEnum;

    use super::DistributionTypeArg;

    #[test]
    fn accepts_public_and_private_case_insensitively() {
        assert_eq!(
            DistributionTypeArg::from_str("public", true).expect("public"),
            DistributionTypeArg::Public
        );
        assert_eq!(
            DistributionTypeArg::from_str("PRIVATE", true).expect("PRIVATE"),
            DistributionTypeArg::Private
        );
    }

    #[test]
    fn rejects_custom_and_unknown_values() {
        DistributionTypeArg::from_str("custom", true).expect_err("custom is not an input value");
        DistributionTypeArg::from_str("unlisted", true).expect_err("unknown value");
    }

    #[test]
    fn maps_onto_both_generated_enums() {
        assert_eq!(
            super::create_application::DistributionType::from(DistributionTypeArg::Private),
            super::create_application::DistributionType::PRIVATE
        );
        assert_eq!(
            super::update_application::DistributionType::from(DistributionTypeArg::Public),
            super::update_application::DistributionType::PUBLIC
        );
    }
}
