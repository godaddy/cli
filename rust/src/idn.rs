//! Internationalized domain names: Unicode for people, punycode for the wire.
//!
//! Commands accept a name in either form and send the ASCII (punycode) form to
//! the API ([`to_ascii`]). Names shown to the user are Unicode by default and
//! ASCII when the user asks for it ([`DomainForm`]).

use cli_engine::CommandContext;
use serde_json::Value;

use cli_engine::DetailedError;

use crate::error::GddyError;
use crate::prefs::Prefs;

/// Prefix of an ASCII-compatible-encoded (punycode) label, any case.
const ACE_PREFIX: &str = "xn--";

/// The ASCII (punycode) form of a domain name or DNS record name.
///
/// The result is always lowercase, since domain names are case-insensitive.
/// Pure-ASCII input is otherwise returned as typed, so names the API already
/// accepts — including `@`, `*` and `_dmarc`-style labels — never change.
/// Anything else is run through IDNA.
pub(crate) fn to_ascii(name: &str) -> Result<String, GddyError> {
    if name.is_ascii() {
        return Ok(name.to_ascii_lowercase());
    }
    idna::domain_to_ascii(name).map_err(|_| {
        GddyError::validation(format!("{name:?} is not a valid internationalized name"))
            .with_fix("Remove any characters that can't appear in a domain name.")
    })
}

/// Clap value-parser for a domain name, record name or hostname argument:
/// sends the ASCII (lowercase, punycode) form. The error keeps the `fix`, since
/// clap shows only a message.
pub(crate) fn parse_name_arg(raw: &str) -> Result<String, String> {
    to_ascii(raw).map_err(|e| match e.error_fix() {
        Some(fix) => format!("{e}. {fix}"),
        None => e.to_string(),
    })
}

/// The Unicode form of `name`. Labels that are not valid punycode are left as
/// they are, so display never fails.
pub(crate) fn to_unicode(name: &str) -> String {
    if !has_ace_label(name) {
        return name.to_owned();
    }
    match idna::domain_to_unicode(name) {
        (unicode, Ok(())) => unicode,
        (_, Err(_)) => name.to_owned(),
    }
}

fn has_ace_label(name: &str) -> bool {
    name.split('.').any(|label| {
        label
            .get(..ACE_PREFIX.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(ACE_PREFIX))
    })
}

/// Which form of a domain name to show.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum DomainForm {
    #[default]
    Unicode,
    Punycode,
}

impl DomainForm {
    pub(crate) fn from_flag(punycode: bool) -> Self {
        if punycode {
            Self::Punycode
        } else {
            Self::Unicode
        }
    }

    /// `name` in this form. A name that cannot be converted is shown as-is.
    pub(crate) fn render(self, name: &str) -> String {
        match self {
            Self::Unicode => to_unicode(name),
            Self::Punycode => to_ascii(name).unwrap_or_else(|_| name.to_owned()),
        }
    }

    /// Convert to this form the value at each dotted path in `paths` (for
    /// example `"domain"` or `"resolved.nameServers"`). A path names a string
    /// or an array of strings reached through objects only; a path that is not
    /// present, or leads anywhere else, is left alone. Callers list exactly
    /// the fields their command's output holds domain names in, and loop over
    /// list items themselves.
    pub(crate) fn convert_fields(self, value: &mut Value, paths: &[&str]) {
        for path in paths {
            let target = path
                .split('.')
                .try_fold(&mut *value, |node, key| node.get_mut(key));
            if let Some(target) = target {
                self.convert_names(target);
            }
        }
    }

    fn convert_names(self, value: &mut Value) {
        match value {
            Value::String(s) => *s = self.render(s),
            Value::Array(items) => items.iter_mut().for_each(|v| self.convert_names(v)),
            _ => {}
        }
    }
}

/// `--punycode`, shared by every command that shows domain names.
///
/// Precedence: the flag, then `GDDY_PUNYCODE`, then the saved `punycode`
/// preference, then Unicode.
#[derive(Debug, Clone, Copy, Default, clap::Args)]
pub(crate) struct DomainFormArgs {
    /// Show domain names in their ASCII (punycode) form, e.g. xn--mnchen-3ya.de, instead of
    /// Unicode. Use `--punycode=false` to override a saved preference.
    #[arg(
        long,
        env = "GDDY_PUNYCODE",
        value_name = "BOOL",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "true",
        value_parser = clap::builder::BoolishValueParser::new(),
    )]
    punycode: Option<bool>,
}

impl DomainFormArgs {
    /// The form to show names in. The config file is only read when neither the
    /// flag nor the environment variable decided, so an explicit choice works
    /// even while the file is broken.
    pub(crate) fn form(&self, ctx: &CommandContext) -> Result<DomainForm, GddyError> {
        let punycode = match self.punycode {
            Some(explicit) => explicit,
            None => Prefs::load(ctx)?.punycode,
        };
        Ok(DomainForm::from_flag(punycode))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn to_ascii_encodes_unicode_and_leaves_ascii_alone() {
        assert_eq!(
            to_ascii("münchen.de").expect("valid name"),
            "xn--mnchen-3ya.de"
        );
        assert_eq!(
            to_ascii("xn--mnchen-3ya.de").expect("valid name"),
            "xn--mnchen-3ya.de"
        );
        for special in ["@", "*", "_dmarc", "www"] {
            assert_eq!(to_ascii(special).expect("valid name"), special);
        }
    }

    #[test]
    fn to_ascii_lowercases_ascii_and_unicode_names_alike() {
        assert_eq!(
            to_ascii("WWW.Example.COM").expect("valid name"),
            "www.example.com"
        );
        assert_eq!(
            to_ascii("MÜNCHEN.de").expect("valid name"),
            "xn--mnchen-3ya.de"
        );
    }

    #[test]
    fn to_ascii_encodes_only_the_unicode_labels_of_a_dns_name() {
        assert_eq!(
            to_ascii("*.münchen").expect("valid name"),
            "*.xn--mnchen-3ya"
        );
    }

    #[test]
    fn to_ascii_rejects_names_idna_disallows() {
        assert!(to_ascii("\u{301}abc.com").is_err());
    }

    #[test]
    fn parse_name_arg_sends_ascii_and_keeps_the_fix_in_its_error() {
        assert_eq!(
            parse_name_arg("MÜNCHEN.de").expect("valid name"),
            "xn--mnchen-3ya.de"
        );
        let err = parse_name_arg("\u{301}abc.com").expect_err("invalid name");
        assert!(
            err.contains("is not a valid internationalized name"),
            "{err}"
        );
        assert!(err.contains("Remove any characters"), "{err}");
    }

    #[test]
    fn to_unicode_decodes_punycode_labels() {
        assert_eq!(to_unicode("xn--mnchen-3ya.de"), "münchen.de");
        assert_eq!(to_unicode("XN--MNCHEN-3YA.de"), "münchen.de");
        assert_eq!(to_unicode("www.xn--mnchen-3ya.de"), "www.münchen.de");
    }

    #[test]
    fn to_unicode_leaves_plain_and_undecodable_names_alone() {
        assert_eq!(to_unicode("example.com"), "example.com");
        assert_eq!(to_unicode("münchen.de"), "münchen.de");
        assert_eq!(to_unicode("@"), "@");
        assert_eq!(to_unicode("xn--.com"), "xn--.com");
        assert_eq!(to_unicode("xn--a-.com"), "xn--a-.com");
    }

    #[test]
    fn render_honors_the_requested_form() {
        assert_eq!(
            DomainForm::Unicode.render("xn--mnchen-3ya.de"),
            "münchen.de"
        );
        assert_eq!(
            DomainForm::Punycode.render("münchen.de"),
            "xn--mnchen-3ya.de"
        );
        assert_eq!(DomainForm::Punycode.render("example.com"), "example.com");
    }

    #[test]
    fn convert_fields_converts_only_the_listed_paths() {
        let mut v = json!({
            "domain": "xn--mnchen-3ya.de",
            "idnDomain": "xn--mnchen-3ya.de",
            "status": "xn--mnchen-3ya.de",
            "nameServers": ["ns1.xn--mnchen-3ya.de", "ns2.example.com"],
            "resolved": {
                "nameServers": ["ns1.xn--mnchen-3ya.de"],
                "name": "xn--mnchen-3ya",
            },
            "items": [{"domain": "xn--mnchen-3ya.de"}],
            "count": 3,
        });
        DomainForm::Unicode.convert_fields(
            &mut v,
            &[
                "domain",
                "nameServers",
                "resolved.nameServers",
                "missing.path",
            ],
        );
        assert_eq!(
            v,
            json!({
                "domain": "münchen.de",
                "idnDomain": "xn--mnchen-3ya.de",
                "status": "xn--mnchen-3ya.de",
                "nameServers": ["ns1.münchen.de", "ns2.example.com"],
                "resolved": {
                    "nameServers": ["ns1.münchen.de"],
                    "name": "xn--mnchen-3ya",
                },
                "items": [{"domain": "xn--mnchen-3ya.de"}],
                "count": 3,
            })
        );
    }

    #[test]
    fn convert_fields_ignores_paths_that_do_not_lead_to_names() {
        let mut v = json!({"domain": 7, "nameServers": {"a": "xn--mnchen-3ya.de"}, "items": ["xn--mnchen-3ya.de"]});
        let before = v.clone();
        DomainForm::Unicode.convert_fields(&mut v, &["domain", "nameServers.a.b", "items.x"]);
        assert_eq!(v, before);
    }

    mod precedence {
        use cli_engine::{
            Cli, CliConfig, CommandResult, CommandSpec, GroupSpec, Module, RuntimeCommandSpec,
            RuntimeGroupSpec,
        };

        use serde_json::json;

        use super::super::*;
        use crate::environments::test_support::{ENV_LOCK, EnvGuard};

        #[derive(Debug, Clone, clap::Args)]
        struct ShowArgs {
            #[command(flatten)]
            domain_format: DomainFormArgs,
        }

        /// A CLI with one command reporting which form `DomainFormArgs` chose.
        fn cli() -> Cli {
            let module = Module::new("Test", |_ctx| {
                RuntimeGroupSpec::new(GroupSpec::new("t", "test")).with_command(
                    RuntimeCommandSpec::new_typed_with_context::<ShowArgs, _, _, _>(
                        CommandSpec::from_args::<ShowArgs>("show", "show").no_auth(true),
                        |ctx, args: ShowArgs| async move {
                            let form = args.domain_format.form(&ctx)?;
                            Ok(CommandResult::new(
                                json!({ "punycode": form == DomainForm::Punycode }),
                            ))
                        },
                    ),
                )
            });
            Cli::new(
                CliConfig::new("gddy", "test", "gddy")
                    .with_config_schema::<Prefs>()
                    .with_startup_args(Vec::<&str>::new())
                    .with_module(module),
            )
        }

        /// Runs `gddy t show <flag>` with the given saved config and env var.
        async fn run(config: Option<&str>, env: Option<&str>, flag: Option<&str>) -> (i32, String) {
            let _lock = ENV_LOCK.lock().await;
            let dir = tempfile::tempdir().expect("tempdir");
            let _xdg = EnvGuard::set("XDG_CONFIG_HOME", &dir.path().display().to_string());
            let _env = match env {
                Some(value) => EnvGuard::set("GDDY_PUNYCODE", value),
                None => EnvGuard::unset("GDDY_PUNYCODE"),
            };
            if let Some(contents) = config {
                let path = dir.path().join("gddy");
                std::fs::create_dir_all(&path).expect("mkdir");
                std::fs::write(path.join("config.toml"), contents).expect("write config");
            }
            let mut args = vec!["gddy", "t", "show"];
            args.extend(flag);
            let out = cli().run(args).await;
            (out.exit_code, out.rendered)
        }

        async fn punycode(config: Option<&str>, env: Option<&str>, flag: Option<&str>) -> bool {
            let (code, rendered) = run(config, env, flag).await;
            assert_eq!(
                code, 0,
                "config={config:?} env={env:?} flag={flag:?}: {rendered}"
            );
            rendered.contains("\"punycode\": true")
        }

        const ON: Option<&str> = Some("punycode = true\n");
        const OFF: Option<&str> = Some("punycode = false\n");

        #[tokio::test]
        async fn defaults_to_unicode() {
            assert!(!punycode(None, None, None).await);
        }

        #[tokio::test]
        async fn saved_preference_applies() {
            assert!(punycode(ON, None, None).await);
            assert!(!punycode(OFF, None, None).await);
        }

        #[tokio::test]
        async fn env_beats_saved_preference() {
            assert!(!punycode(ON, Some("false"), None).await);
            assert!(punycode(OFF, Some("true"), None).await);
            assert!(punycode(OFF, Some("1"), None).await, "boolish env values");
        }

        #[tokio::test]
        async fn flag_beats_env_and_saved_preference() {
            assert!(punycode(OFF, Some("false"), Some("--punycode")).await);
            assert!(!punycode(ON, Some("true"), Some("--punycode=false")).await);
        }

        #[tokio::test]
        async fn explicit_choice_works_while_the_config_file_is_broken() {
            let broken = Some("not = valid = toml");
            assert!(punycode(broken, None, Some("--punycode")).await);
            assert!(punycode(broken, Some("true"), None).await);
        }

        #[tokio::test]
        async fn broken_config_is_an_error_not_a_silent_default() {
            for broken in [Some("not = valid = toml"), Some("punycode = \"banana\"\n")] {
                let (code, rendered) = run(broken, None, None).await;
                assert_ne!(code, 0, "{broken:?}: {rendered}");
                assert!(rendered.contains("fix"), "{broken:?}: {rendered}");
            }
        }

        /// Set only in the child process spawned by `invalid_env_value_is_rejected`.
        const CHILD: &str = "GDDY_TEST_INVALID_PUNYCODE_ENV";

        /// An invalid `GDDY_PUNYCODE` makes every command that reads it fail to
        /// parse, so it must never be set in this process, where other tests
        /// parse such commands without holding `ENV_LOCK`. Run it in a child
        /// test process instead.
        #[test]
        fn invalid_env_value_is_rejected() {
            let (_, module) = module_path!()
                .split_once("::")
                .expect("crate-qualified path");
            let status = std::process::Command::new(std::env::current_exe().expect("test binary"))
                .args([
                    "--exact",
                    &format!("{module}::invalid_env_value_is_rejected_in_child"),
                ])
                .env(CHILD, "1")
                .env("GDDY_PUNYCODE", "banana")
                .output()
                .expect("spawn test binary");
            assert!(
                status.status.success(),
                "child failed:\n{}\n{}",
                String::from_utf8_lossy(&status.stdout),
                String::from_utf8_lossy(&status.stderr)
            );
            // A filter that matched nothing would also exit 0.
            assert!(
                String::from_utf8_lossy(&status.stdout).contains("1 passed"),
                "child test did not run"
            );
        }

        /// Runs only when spawned by `invalid_env_value_is_rejected`, which has
        /// set the invalid value in this process's environment.
        #[tokio::test]
        async fn invalid_env_value_is_rejected_in_child() {
            if std::env::var_os(CHILD).is_none() {
                return;
            }
            let out = cli().run(["gddy", "t", "show"]).await;
            assert_ne!(out.exit_code, 0, "{}", out.rendered);
        }
    }
}
