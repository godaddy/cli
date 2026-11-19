//! Which upgrades cross a breaking change, and the guide that explains them.
//!
//! `gddy` follows semver, so a breaking change is always a version boundary:
//! a new major version, or while the major version is 0, a new minor version
//! (`0.2.x` to `0.3.x`). `gddy guide upgrades` has one section per boundary,
//! titled `<from> to <to>` (`0.2 to 0.3`, `1 to 2`).

use cli_engine::NextAction;
use semver::Version;

use crate::next_action::next_action;

/// The embedded `gddy guide upgrades`.
pub(super) const UPGRADES_GUIDE: &[u8] = include_bytes!("guides/upgrades.md");

/// Whether moving from `from` to `to` crosses a breaking change: the major
/// version changes, or the major version is 0 and the minor version changes.
/// Patch releases, and minor releases from 1.0 on, never do. Moving backwards
/// or staying put doesn't cross one either.
pub(super) fn crosses_breaking_boundary(from: &Version, to: &Version) -> bool {
    if to <= from {
        return false;
    }
    from.major != to.major || (from.major == 0 && from.minor != to.minor)
}

/// The label of the breaking line a version belongs to, as it appears at the end
/// of a guide section title: `0.2` for every `0.2.x`, `3` for every `3.x.y`.
#[cfg(test)]
pub(super) fn breaking_line(version: &Version) -> String {
    if version.major == 0 {
        format!("0.{}", version.minor)
    } else {
        version.major.to_string()
    }
}

/// A suggestion to read the upgrade guide, when an update crossed a breaking
/// change. Empty for any other update, so patch releases don't nag.
pub(super) fn upgrade_guide_next_actions(
    status: &str,
    previous: Option<&Version>,
    new: Option<&Version>,
) -> Vec<NextAction> {
    match (status, previous, new) {
        ("updated", Some(previous), Some(new)) if crosses_breaking_boundary(previous, new) => {
            vec![next_action(
                "guide upgrades",
                format!("Read what changed between {previous} and {new}"),
            )]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).expect("valid semver")
    }

    #[test]
    fn a_new_minor_version_before_1_0_is_a_breaking_boundary() {
        assert!(crosses_breaking_boundary(&v("0.2.39"), &v("0.3.0")));
        assert!(crosses_breaking_boundary(&v("0.1.16"), &v("0.2.0")));
        // Skipping several lines crosses them all.
        assert!(crosses_breaking_boundary(&v("0.2.39"), &v("0.5.2")));
    }

    #[test]
    fn a_new_major_version_is_a_breaking_boundary() {
        assert!(crosses_breaking_boundary(&v("1.4.2"), &v("2.0.0")));
        assert!(crosses_breaking_boundary(&v("0.9.3"), &v("1.0.0")));
        assert!(crosses_breaking_boundary(&v("1.4.2"), &v("3.1.0")));
    }

    #[test]
    fn patch_releases_never_cross_a_breaking_boundary() {
        assert!(!crosses_breaking_boundary(&v("0.2.38"), &v("0.2.39")));
        assert!(!crosses_breaking_boundary(&v("1.4.2"), &v("1.4.3")));
    }

    #[test]
    fn minor_releases_from_1_0_on_are_not_breaking() {
        assert!(!crosses_breaking_boundary(&v("1.4.2"), &v("1.5.0")));
        assert!(!crosses_breaking_boundary(&v("2.0.0"), &v("2.9.1")));
    }

    #[test]
    fn staying_put_or_going_backwards_crosses_nothing() {
        assert!(!crosses_breaking_boundary(&v("0.3.0"), &v("0.3.0")));
        assert!(!crosses_breaking_boundary(&v("0.3.0"), &v("0.2.39")));
        assert!(!crosses_breaking_boundary(&v("2.0.0"), &v("1.9.0")));
    }

    #[test]
    fn the_breaking_line_names_the_minor_before_1_0_and_the_major_after() {
        assert_eq!(breaking_line(&v("0.2.39")), "0.2");
        assert_eq!(breaking_line(&v("0.10.0")), "0.10");
        assert_eq!(breaking_line(&v("3.1.4")), "3");
    }

    #[test]
    fn updating_across_a_breaking_boundary_suggests_the_upgrade_guide() {
        let actions = upgrade_guide_next_actions("updated", Some(&v("0.2.39")), Some(&v("0.3.0")));
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].command, "gddy guide upgrades");
        assert!(
            actions[0].description.contains("0.2.39") && actions[0].description.contains("0.3.0"),
            "{}",
            actions[0].description
        );
    }

    #[test]
    fn an_update_inside_one_breaking_line_suggests_nothing() {
        let patch = upgrade_guide_next_actions("updated", Some(&v("0.2.38")), Some(&v("0.2.39")));
        assert!(patch.is_empty());
    }

    #[test]
    fn nothing_is_suggested_when_no_update_was_installed() {
        let actions =
            upgrade_guide_next_actions("already up to date", Some(&v("0.3.0")), Some(&v("0.3.0")));
        assert!(actions.is_empty());
        // An unreadable version can't be shown to cross anything.
        assert!(upgrade_guide_next_actions("updated", None, Some(&v("0.3.0"))).is_empty());
    }

    /// The guide has to be there when the suggestion points at it, and has to
    /// cover the line this build belongs to. A release PR that bumps the version
    /// into a new breaking line fails here until its section is written.
    #[test]
    fn the_upgrades_guide_documents_the_breaking_line_of_this_version() {
        let guide = std::str::from_utf8(UPGRADES_GUIDE).expect("the guide is UTF-8");
        let line = breaking_line(&super::super::current_version());
        // The first line has nothing to upgrade from.
        if line == "0.0" || line == "0.1" {
            return;
        }
        let documented = guide
            .lines()
            .filter_map(|text| text.strip_prefix("## "))
            .filter_map(|title| title.trim().split_once(" to "))
            .any(|(_, to)| to == line);
        assert!(
            documented,
            "gddy {line}.x has no section titled `## <previous> to {line}` in \
             rust/src/update/guides/upgrades.md. Add one that says what a user has to change \
             to move up to this version (see .agents/skills/breaking-changes/SKILL.md)."
        );
    }
}
