//! The host sources `get host` enumerates (spec §9.1, ADR-0103).
//!
//! Two files say which hosts this shell knows besides the links it holds: the OpenSSH client
//! configuration, because the ssh transport of ADR-0037 runs `ssh <host>` and that is where
//! `ssh` learns what a host name means; and the shell's own host file under its configuration
//! directory, which `add host`, `set host` and `remove host` write. The first is read and never
//! written — it is OpenSSH's file, and rewriting it would lose everything the shell does not
//! understand. The second is the shell's, and is rewritten whole.

use std::path::{Path, PathBuf};

use ono_core::ErrorCode;
use ono_value::ErrorValue;
use serde::{Deserialize, Serialize};

/// The name of the shell's own host file inside its configuration directory.
const OWN_FILE: &str = "hosts.json";

/// One host as a source records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostEntry {
    /// The host's name, the word `link host` takes.
    pub name: String,
    /// An address or DNS name, when the source records one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    /// The port, when the source records one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// The login user, when the source records one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

impl HostEntry {
    /// An entry with only a name.
    #[must_use]
    pub fn named(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            address: None,
            port: None,
            user: None,
        }
    }
}

/// The shell's own host file, as written.
#[derive(Debug, Default, Serialize, Deserialize)]
struct OwnFile {
    version: u32,
    #[serde(default)]
    hosts: Vec<HostEntry>,
}

/// Where the sources live for one session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostSources {
    /// `~/.ssh/config`, read-only.
    pub ssh_config: Option<PathBuf>,
    /// The shell's own host file, read and written.
    pub own: Option<PathBuf>,
    /// The pinned host keys of spec §21.5, read and written (ADR-0355).
    pub trust_store: Option<PathBuf>,
    /// The client keys this machine authorizes, read and written (v0.4.1 §9.2, ADR-0468).
    ///
    /// The mirror of `trust_store`: that file says which machines this shell will link *to*,
    /// this one says which clients its listening agent will serve.
    pub authorized_clients: Option<PathBuf>,
    /// The configuration directory this shell's own peer identity lives in (v0.4.1 §8.1).
    ///
    /// The directory rather than the file, because §8.2's migration ladder reads two names in it.
    pub config_dir: Option<PathBuf>,
}

impl HostSources {
    /// The sources an environment points at: `~/.ssh/config` under `HOME`, and the shell's
    /// host file under the configuration directory of ADR-0010 (`ONO_CONFIG_DIR`, then
    /// `XDG_CONFIG_HOME/ono`, then `~/.config/ono`).
    #[must_use]
    pub fn from_environment<'a>(environment: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let environment: Vec<(&str, &str)> = environment.into_iter().collect();
        let home = environment
            .iter()
            .find(|(name, _)| *name == "HOME")
            .map(|(_, value)| PathBuf::from(*value));
        let config_dir = crate::config::config_dir_from_environment(environment);
        Self {
            ssh_config: home.map(|home| home.join(".ssh").join("config")),
            own: config_dir
                .as_ref()
                .map(|directory| directory.join(OWN_FILE)),
            trust_store: config_dir
                .as_ref()
                .map(|directory| directory.join(crate::trust::STORE_FILE)),
            authorized_clients: config_dir
                .as_ref()
                .map(|directory| directory.join(crate::trust::AUTHORIZED_CLIENTS_FILE)),
            config_dir,
        }
    }

    /// The hosts the shell's own file records, in file order. A file that does not exist is an
    /// empty source, not a failure.
    ///
    /// # Errors
    ///
    /// A file that exists but cannot be read or is not the shell's format.
    pub fn own_hosts(&self) -> Result<Vec<HostEntry>, ErrorValue> {
        let Some(path) = &self.own else {
            return Ok(Vec::new());
        };
        read_own(path).map(|file| file.hosts)
    }

    /// The `Host` entries of the OpenSSH client configuration, in file order. A missing file is
    /// an empty source.
    ///
    /// # Errors
    ///
    /// A file that exists but cannot be read.
    pub fn ssh_hosts(&self) -> Result<Vec<HostEntry>, ErrorValue> {
        let Some(path) = &self.ssh_config else {
            return Ok(Vec::new());
        };
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(parse_ssh_config(&text)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(ErrorValue::new(
                ErrorCode::IoPermissionDenied,
                format!("cannot read {}: {error}", path.display()),
            )),
        }
    }

    /// Rewrites the shell's own host file with `hosts`, creating the directory on the way.
    ///
    /// # Errors
    ///
    /// No configuration directory is known, or the file cannot be written.
    pub fn write_own(&self, hosts: Vec<HostEntry>) -> Result<(), ErrorValue> {
        let Some(path) = &self.own else {
            return Err(ErrorValue::new(
                ErrorCode::IoNotFound,
                "no configuration directory is known, so there is nowhere to record a host",
            )
            .with_help("set `HOME`, `XDG_CONFIG_HOME` or `ONO_CONFIG_DIR` (ADR-0010)"));
        };
        if let Some(directory) = path.parent() {
            std::fs::create_dir_all(directory).map_err(|error| {
                ErrorValue::new(
                    ErrorCode::IoPermissionDenied,
                    format!("cannot create {}: {error}", directory.display()),
                )
            })?;
        }
        let file = OwnFile { version: 1, hosts };
        let text = serde_json::to_string_pretty(&file).map_err(|error| {
            ErrorValue::new(
                ErrorCode::ProviderSchemaViolation,
                format!("the host file cannot be encoded: {error}"),
            )
        })?;
        std::fs::write(path, text + "\n").map_err(|error| {
            ErrorValue::new(
                ErrorCode::IoPermissionDenied,
                format!("cannot write {}: {error}", path.display()),
            )
        })
    }
}

fn read_own(path: &Path) -> Result<OwnFile, ErrorValue> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|error| {
            ErrorValue::new(
                ErrorCode::ProviderSchemaViolation,
                format!("{} is not a host file: {error}", path.display()),
            )
            .with_help("the file is `{\"version\": 1, \"hosts\": [{\"name\": …}]}`")
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(OwnFile::default()),
        Err(error) => Err(ErrorValue::new(
            ErrorCode::IoPermissionDenied,
            format!("cannot read {}: {error}", path.display()),
        )),
    }
}

/// The `Host` blocks of an OpenSSH client configuration.
///
/// Only the keywords that say where a host is are read — `HostName`, `Port`, `User` — and only
/// for literal host names: a pattern (`*`, `?`, a negation) names a class of hosts, not one.
/// `Match` blocks end the current `Host` block and are otherwise skipped, and `Include` is
/// not followed. Nothing here is a full parser of that file, and nothing needs to be: the
/// shell asks which hosts exist, and OpenSSH itself applies the rest when the ssh transport
/// runs.
#[must_use]
pub fn parse_ssh_config(text: &str) -> Vec<HostEntry> {
    let mut entries: Vec<HostEntry> = Vec::new();
    // The entries the current `Host` line opened, by index.
    let mut current: Vec<usize> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (keyword, rest) = match line.split_once(['=', ' ', '\t']) {
            Some((keyword, rest)) => (keyword.trim(), rest.trim_start_matches(['=', ' ', '\t'])),
            None => (line, ""),
        };
        let rest = rest.trim();
        match keyword.to_ascii_lowercase().as_str() {
            "host" => {
                current.clear();
                for pattern in rest.split_whitespace() {
                    if pattern.contains(['*', '?', '!']) {
                        continue;
                    }
                    current.push(entries.len());
                    entries.push(HostEntry::named(pattern));
                }
            }
            "match" => current.clear(),
            "hostname" => {
                for index in &current {
                    entries[*index].address = Some(rest.to_owned());
                }
            }
            "port" => {
                let port = rest.parse::<u16>().ok();
                for index in &current {
                    entries[*index].port = port;
                }
            }
            "user" => {
                for index in &current {
                    entries[*index].user = Some(rest.to_owned());
                }
            }
            _ => {}
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_read_literal_host_blocks_and_skip_patterns() {
        let parsed = parse_ssh_config(
            "# comment\nHost devbox\n    HostName 10.4.2.11\n    User deploy\n    Port 2222\n\n\
             Host *.example\n    User nobody\n\nHost a b\n  HostName=shared\nMatch all\n  Port 1\n",
        );
        assert_eq!(
            parsed,
            vec![
                HostEntry {
                    name: "devbox".into(),
                    address: Some("10.4.2.11".into()),
                    port: Some(2222),
                    user: Some("deploy".into()),
                },
                HostEntry {
                    name: "a".into(),
                    address: Some("shared".into()),
                    port: None,
                    user: None,
                },
                HostEntry {
                    name: "b".into(),
                    address: Some("shared".into()),
                    port: None,
                    user: None,
                },
            ]
        );
    }
}
