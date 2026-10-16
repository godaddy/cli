//! The machinery behind the scope declarations: [`ScopeInfo`], the doc-comment
//! helper and the `declare_scopes!` macro that turns `declarations.rs` into
//! constants, `ALL`, `DIRECTIVES`, `SCOPE_REGISTRY` and `DEFAULT_OAUTH_SCOPES`.
//! Contributors adding a scope want `declarations.rs`, not this file.

/// A requestable scope, its human description, and whether it is requested at
/// login by default. Backs `gddy auth scope`. Which commands use a scope is
/// derived live via `command_scopes`.
pub struct ScopeInfo {
    pub scope: &'static str,
    pub description: &'static str,
    pub default: bool,
}

/// Strips the single leading space `///` leaves on a doc attribute's text, so a
/// `/// Read domains` comment yields the description `"Read domains"`.
pub(super) const fn doc_text(doc: &'static str) -> &'static str {
    match doc.as_bytes() {
        [b' ', rest @ ..] => match core::str::from_utf8(rest) {
            Ok(text) => text,
            // Unreachable: dropping a leading ASCII space keeps valid UTF-8 valid.
            Err(_) => doc,
        },
        _ => doc,
    }
}

/// Declares each scope once and derives everything else from that declaration:
/// the scope constants, `ALL`, `DIRECTIVES` and `SCOPE_REGISTRY`. A scope
/// therefore cannot exist without being registered, described and given a login
/// policy.
///
/// Each entry is a one-line `///` doc comment — it is the scope's description
/// for `gddy auth scope` and the constant's rustdoc — followed by
/// `NAME => "scope",`. Plain `//` comments are discarded before a macro sees
/// them, so use `///` for the description and `//` for anything else. Permission
/// scopes are grouped by login policy: `default` ones are requested at login,
/// `step_up` ones only when a command that declares them runs. Directive scopes
/// are always requested at login.
macro_rules! declare_scopes {
    (
        permissions {
            default {
                $( #[doc = $dd:literal] $dn:ident => $dv:literal, )*
            }
            step_up {
                $( #[doc = $sd:literal] $sn:ident => $sv:literal, )*
            }
        }
        directives {
            $( #[doc = $rd:literal] $rn:ident => $rv:literal, )*
        }
    ) => {
        $( #[doc = $dd] pub const $dn: &str = $dv; )*
        $( #[doc = $sd] pub const $sn: &str = $sv; )*
        $( #[doc = $rd] pub const $rn: &str = $rv; )*

        /// Every `resource:action` permission scope the CLI may request. Auto-derived
        /// from the `permissions` block of [`declare_scopes!`] — keep the client's
        /// registration in sync with this list.
        ///
        /// NOT the complete set of scopes requiring OAuth client registration:
        /// the directive scopes in [`DIRECTIVES`] (e.g. [`OFFLINE_ACCESS`]) aren't
        /// `resource:action` permissions and are deliberately excluded, but still must
        /// be registered on the client server-side. Diff the client's configuration
        /// against `ALL` *plus* [`DIRECTIVES`], not `ALL` alone.
        pub const ALL: &[&str] = &[ $($dn,)* $($sn,)* ];

        /// Every standalone directive scope — the scopes that require OAuth client
        /// registration but, not being `resource:action` permissions, live outside
        /// [`ALL`]. Diff the client's configuration against `ALL` *plus* this list.
        pub const DIRECTIVES: &[&str] = &[ $($rn),* ];

        /// Every requestable scope, described for `gddy auth scope`, and the single
        /// source of truth for which scopes the login request carries: `default`
        /// entries make up [`DEFAULT_OAUTH_SCOPES`]; `step_up` entries are requested
        /// when a command that declares them runs.
        ///
        /// A scope is step-up only when the action it exists for spends money,
        /// can take a site offline, deletes a whole billable resource, or is raw
        /// data access. Editing scopes that also permit deleting a sub-resource
        /// (`dns delete`, `hosting secret delete`) stay defaults: the authorization
        /// server offers no finer split, and prompting for `domains.dns:update`
        /// would put consent back into every site setup.
        ///
        /// Ordered: default permissions, step-up permissions, then directives.
        pub const SCOPE_REGISTRY: &[$crate::scopes::ScopeInfo] = &[
            $( $crate::scopes::ScopeInfo { scope: $dn, description: $crate::scopes::declare::doc_text($dd), default: true }, )*
            $( $crate::scopes::ScopeInfo { scope: $sn, description: $crate::scopes::declare::doc_text($sd), default: false }, )*
            $( $crate::scopes::ScopeInfo { scope: $rn, description: $crate::scopes::declare::doc_text($rd), default: true }, )*
        ];

        /// Scopes requested at login: the `default` permissions plus the
        /// directives — exactly the [`SCOPE_REGISTRY`] entries with `default: true`,
        /// in registry order. Each scope step-up is a browser round trip through the
        /// consent screen, so the login grant is wide on purpose: a first-time
        /// session that buys a domain, buys hosting and deploys a site sees a
        /// consent screen at login and at each purchase, and nowhere else.
        pub const DEFAULT_OAUTH_SCOPES: &[&str] = &[ $($dn,)* $($rn,)* ];
    };
}
