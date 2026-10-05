//! Database credentials from a Managed WordPress tunnel mint, kept off the
//! terminal. The mint returns WordPress's own database login once; the CLI
//! writes it to a MySQL option file only the current user can read and prints
//! the path, so the password never reaches the event stream, the scrollback,
//! or another process's argument list. The file and its directory are removed
//! when the tunnel exits.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::error::GddyError;

/// WordPress's database login for the tunnelled variant. `Debug` is written by
/// hand so the password cannot reach a log line through `{:?}`.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct DbCredentials {
    pub(super) user: String,
    pub(super) password: String,
    pub(super) name: String,
}

impl std::fmt::Debug for DbCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbCredentials")
            .field("user", &self.user)
            .field("password", &"<redacted>")
            .field("name", &self.name)
            .finish()
    }
}

impl DbCredentials {
    /// Read `database: { user, password, name }` from the mint response.
    /// `None` when the service sent no `database` object; an object with a
    /// missing or empty field is a broken contract and fails.
    pub(super) fn from_mint(resp: &Value) -> Result<Option<Self>, GddyError> {
        let Some(db) = resp.get("database").filter(|v| !v.is_null()) else {
            return Ok(None);
        };
        let field = |key: &str| {
            db.get(key)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| {
                    GddyError::network(format!(
                        "database tunnel mint response has no 'database.{key}'"
                    ))
                    .with_fix("Retry; if it persists, contact support.")
                })
        };
        Ok(Some(Self {
            user: field("user")?,
            password: field("password")?,
            name: field("name")?,
        }))
    }
}

/// A private MySQL option file in a directory of its own. Dropping it removes
/// both, so every exit path that unwinds through `run_tunnel` cleans up; a
/// killed process leaves them behind in the user-only temp directory.
pub(super) struct OptionFile {
    dir: PathBuf,
    path: PathBuf,
    user: String,
    database: String,
}

impl OptionFile {
    /// Write `creds` for a client connecting to `host:port` (the local
    /// listener). The directory is created `0700` and the file `0600` before
    /// any secret is written, so there is no window where another user can
    /// read it.
    pub(super) fn write(creds: &DbCredentials, host: &str, port: u16) -> Result<Self, GddyError> {
        let dir = std::env::temp_dir().join(format!("gddy-db-tunnel-{}", uuid::Uuid::new_v4()));
        create_private_dir(&dir).map_err(|e| write_error(&dir, &e))?;
        let file = Self {
            path: dir.join("my.cnf"),
            dir,
            user: creds.user.clone(),
            database: creds.name.clone(),
        };
        let mut handle =
            create_private_file(&file.path).map_err(|e| write_error(&file.path, &e))?;
        handle
            .write_all(render_option_file(creds, host, port).as_bytes())
            .map_err(|e| write_error(&file.path, &e))?;
        Ok(file)
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    /// The command a MySQL client runs to use this file. `--defaults-extra-file`
    /// must come first on the `mysql` command line.
    pub(super) fn mysql_command(&self) -> String {
        format!(
            "mysql --defaults-extra-file={} --ssl-mode=REQUIRED",
            shell_quote(&self.path.to_string_lossy())
        )
    }
}

impl Drop for OptionFile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn write_error(path: &Path, err: &std::io::Error) -> GddyError {
    GddyError::config(format!(
        "could not write the database option file {}: {err}",
        path.display()
    ))
    .with_fix("Check that the system temp directory is writable (set TMPDIR to another directory), then retry.")
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new().mode(0o700).create(dir)
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    fs::create_dir(dir)
}

#[cfg(unix)]
fn create_private_file(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_private_file(path: &Path) -> std::io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

/// `[client]` is read by every MySQL tool; `database` sits under `[mysql]`
/// because `mysqldump` and friends reject it as an unknown option. TCP is
/// forced so a `localhost` default never turns into a Unix-socket connection
/// to some other server on this machine.
fn render_option_file(creds: &DbCredentials, host: &str, port: u16) -> String {
    format!(
        "# Written by `gddy db tunnel`; removed when the tunnel exits.\n\
         [client]\n\
         user={}\n\
         password={}\n\
         host={}\n\
         port={port}\n\
         protocol=TCP\n\
         \n\
         [mysql]\n\
         database={}\n",
        option_value(&creds.user),
        option_value(&creds.password),
        option_value(host),
        option_value(&creds.name),
    )
}

/// Quote an option-file value. MySQL strips matching outer quotes and then
/// unescapes `\\`, `\"`, `\'`, `\n`, `\r`, `\t` and `\b`; quoting also keeps a
/// `#` in a password from starting a comment.
fn option_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Single-quote a path for a POSIX shell when it holds anything beyond the
/// characters a shell passes through unchanged.
fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | ':' | '\\'))
    {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// The `hint` event telling the operator how to connect. With an option file it
/// names the file and the non-secret parts of the login — never the password;
/// without one the operator supplies the login.
pub(super) fn connect_hint(
    listen_host: &str,
    port: u16,
    option_file: Option<&OptionFile>,
) -> Value {
    match option_file {
        Some(file) => json!({
            "type": "hint",
            "message": format!(
                "Connect with: {} (the login is in that file, readable only by you, and is removed when the tunnel exits)",
                file.mysql_command()
            ),
            "command": file.mysql_command(),
            "optionsFile": file.path().to_string_lossy(),
            "user": file.user,
            "database": file.database,
        }),
        None => json!({
            "type": "hint",
            "message": format!(
                "Connect a MySQL client with TLS so the local hop is encrypted, e.g.: mysql --ssl-mode=REQUIRED -h {listen_host} -P {port} -u <user> -p",
            ),
        }),
    }
}

/// The host a local client should dial for a listener bound to `listen_host`.
/// A wildcard bind is reached over loopback, and `localhost` becomes
/// `127.0.0.1` so the client cannot fall back to a Unix socket.
pub(super) fn client_host(listen_host: &str) -> String {
    match listen_host.parse::<std::net::IpAddr>() {
        Ok(ip) if ip.is_unspecified() => "127.0.0.1".to_owned(),
        Ok(ip) => ip.to_string(),
        Err(_) if listen_host.eq_ignore_ascii_case("localhost") => "127.0.0.1".to_owned(),
        Err(_) => listen_host.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn creds() -> DbCredentials {
        DbCredentials {
            user: "wp_user".to_owned(),
            password: "p\"a'ss\\w#rd".to_owned(),
            name: "wp_db".to_owned(),
        }
    }

    #[test]
    fn reads_credentials_from_mint() {
        let resp = json!({ "database": { "user": "u", "password": "p", "name": "n" } });
        let got = DbCredentials::from_mint(&resp)
            .expect("parse")
            .expect("present");
        assert_eq!(
            (got.user.as_str(), got.password.as_str(), got.name.as_str()),
            ("u", "p", "n")
        );
    }

    #[test]
    fn absent_database_is_none_but_partial_database_fails() {
        assert!(
            DbCredentials::from_mint(&json!({}))
                .expect("parse")
                .is_none()
        );
        assert!(
            DbCredentials::from_mint(&json!({ "database": null }))
                .expect("parse")
                .is_none()
        );
        for partial in [
            json!({ "database": { "user": "u", "name": "n" } }),
            json!({ "database": { "user": "", "password": "p", "name": "n" } }),
            json!({ "database": { "user": "u", "password": 7, "name": "n" } }),
        ] {
            assert!(DbCredentials::from_mint(&partial).is_err(), "{partial}");
        }
    }

    #[test]
    fn debug_never_shows_the_password() {
        let shown = format!("{:?}", creds());
        assert!(!shown.contains("w#rd"), "{shown}");
        assert!(shown.contains("<redacted>"));
    }

    #[test]
    fn option_values_are_quoted_and_escaped() {
        assert_eq!(option_value("plain"), "\"plain\"");
        assert_eq!(option_value("p\"a'ss\\w#rd"), r#""p\"a\'ss\\w#rd""#);
        assert_eq!(option_value("a\nb\tc"), r#""a\nb\tc""#);
    }

    #[test]
    fn option_file_forces_tcp_and_keeps_database_out_of_client_group() {
        let rendered = render_option_file(&creds(), "127.0.0.1", 3307);
        let (client, mysql) = rendered.split_once("[mysql]").expect("mysql group");
        assert!(client.contains("[client]\nuser=\"wp_user\"\n"));
        assert!(client.contains("host=\"127.0.0.1\"\nport=3307\nprotocol=TCP\n"));
        assert!(!client.contains("database="));
        assert!(mysql.contains("database=\"wp_db\""));
    }

    #[test]
    fn option_file_is_private_and_removed_on_drop() {
        let file = OptionFile::write(&creds(), "127.0.0.1", 3306).expect("write");
        let path = file.path().to_path_buf();
        let dir = path.parent().expect("dir").to_path_buf();
        let contents = fs::read_to_string(&path).expect("read");
        assert!(contents.contains(&option_value(&creds().password)));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| fs::metadata(p).expect("meta").permissions().mode() & 0o777;
            assert_eq!(mode(&path), 0o600);
            assert_eq!(mode(&dir), 0o700);
        }
        let command = file.mysql_command();
        assert!(command.starts_with("mysql --defaults-extra-file="));
        assert!(command.ends_with(" --ssl-mode=REQUIRED"));
        assert!(!command.contains("w#rd"));
        drop(file);
        assert!(!dir.exists(), "directory must be removed on drop");
    }

    #[test]
    fn connect_hint_names_the_option_file_and_never_carries_the_password() {
        let file = OptionFile::write(&creds(), "127.0.0.1", 3306).expect("option file");
        let hint = connect_hint("127.0.0.1", 3306, Some(&file));
        assert!(!hint.to_string().contains("w#rd"), "{hint}");
        assert_eq!(hint["user"], "wp_user");
        assert_eq!(hint["database"], "wp_db");
        assert_eq!(hint["command"], file.mysql_command());

        let generic = connect_hint("127.0.0.1", 3307, None);
        let message = generic["message"].as_str().expect("message");
        assert!(message.contains("-P 3307 -u <user> -p"), "{message}");
        assert!(generic.get("optionsFile").is_none());
    }

    #[test]
    fn shell_quote_only_when_needed() {
        assert_eq!(shell_quote("/tmp/gddy-x/my.cnf"), "/tmp/gddy-x/my.cnf");
        assert_eq!(shell_quote("/tmp/a b/my.cnf"), "'/tmp/a b/my.cnf'");
        assert_eq!(shell_quote("/tmp/it's"), r"'/tmp/it'\''s'");
    }

    #[test]
    fn client_host_dials_loopback_for_wildcard_and_localhost() {
        assert_eq!(client_host("0.0.0.0"), "127.0.0.1");
        assert_eq!(client_host("::"), "127.0.0.1");
        assert_eq!(client_host("LOCALHOST"), "127.0.0.1");
        assert_eq!(client_host("127.0.0.1"), "127.0.0.1");
        assert_eq!(client_host("::1"), "::1");
        assert_eq!(client_host("192.168.1.10"), "192.168.1.10");
    }
}
