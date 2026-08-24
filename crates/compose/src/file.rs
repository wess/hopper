//! The shapes a compose file is allowed to take on disk.
//!
//! Compose is generous about syntax — a field is a string or a list, a map or
//! a list of `KEY=VALUE`, a port is a number or a string or a whole map — so
//! most of this module is the untagged enums that accept both and normalize to
//! one form.
//!
//! Every struct keeps an `extra` catch-all. A key Hopper does not implement has
//! to be *reported*, not dropped: a stack that comes up quietly missing its
//! `cap_add` is worse than one that refuses, because you find out in
//! production rather than here.

use serde::Deserialize;
use std::collections::BTreeMap;

/// Anything left over after the fields we understand.
pub type Extra = BTreeMap<String, serde_yaml::Value>;

/// A field compose accepts as one string or a list of them.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    pub fn into_vec(self) -> Vec<String> {
        match self {
            Self::One(s) => vec![s],
            Self::Many(v) => v,
        }
    }
}

/// A field compose accepts as a `KEY: value` map or a `KEY=value` list.
///
/// YAML types the values, so `MYSQL_PORT: 3306` arrives as a number and
/// `DEBUG: true` as a bool. Both have to reach the container as strings.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum MapOrList {
    Map(BTreeMap<String, Option<serde_yaml::Value>>),
    List(Vec<String>),
}

impl MapOrList {
    /// Normalize to ordered `(key, value)` pairs.
    ///
    /// A map entry with no value (`- FOO:` or `FOO:`) means "take it from the
    /// environment Hopper is running in", which is compose's pass-through
    /// form; it comes back as `None` so the caller can do that lookup.
    pub fn into_pairs(self) -> Vec<(String, Option<String>)> {
        match self {
            Self::Map(m) => m
                .into_iter()
                .map(|(k, v)| (k, v.and_then(|v| scalar(&v))))
                .collect(),
            Self::List(items) => items
                .into_iter()
                .map(|item| match item.split_once('=') {
                    Some((k, v)) => (k.trim().to_string(), Some(v.to_string())),
                    None => (item.trim().to_string(), None),
                })
                .collect(),
        }
    }
}

/// A YAML scalar as the string a container will actually see.
///
/// Anything that is not a scalar (a nested map in an environment block) has no
/// sensible string form and is dropped by the caller with a warning.
pub fn scalar(v: &serde_yaml::Value) -> Option<String> {
    match v {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        // An explicit null is compose's "inherit from my environment".
        serde_yaml::Value::Null => None,
        _ => None,
    }
}

/// The whole file.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct File {
    /// `name:` — the project name, when the file names itself.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub services: BTreeMap<String, Service>,
    #[serde(default)]
    pub networks: BTreeMap<String, Option<NetworkDef>>,
    #[serde(default)]
    pub volumes: BTreeMap<String, Option<VolumeDef>>,
    /// `version:` is obsolete in the Compose Specification and ignored, but it
    /// is still in most files in the wild, so it is named rather than left to
    /// the catch-all where it would produce a pointless warning.
    #[serde(default)]
    pub version: Option<serde_yaml::Value>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Service {
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub build: Option<Build>,
    #[serde(default)]
    pub container_name: Option<String>,
    #[serde(default)]
    pub command: Option<OneOrMany>,
    #[serde(default)]
    pub entrypoint: Option<OneOrMany>,
    #[serde(default)]
    pub environment: Option<MapOrList>,
    #[serde(default)]
    pub env_file: Option<EnvFile>,
    #[serde(default)]
    pub ports: Vec<PortEntry>,
    #[serde(default)]
    pub expose: Vec<serde_yaml::Value>,
    #[serde(default)]
    pub volumes: Vec<VolumeEntry>,
    #[serde(default)]
    pub networks: Option<ServiceNetworks>,
    #[serde(default)]
    pub depends_on: Option<DependsOn>,
    #[serde(default)]
    pub restart: Option<String>,
    #[serde(default)]
    pub healthcheck: Option<Healthcheck>,
    #[serde(default)]
    pub labels: Option<MapOrList>,
    #[serde(default)]
    pub working_dir: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub hostname: Option<String>,
    #[serde(default)]
    pub profiles: Vec<String>,
    #[serde(default)]
    pub tty: Option<bool>,
    #[serde(default)]
    pub deploy: Option<Deploy>,
    /// The pre-Specification resource keys, still common in older files.
    #[serde(default)]
    pub mem_limit: Option<serde_yaml::Value>,
    #[serde(default)]
    pub cpus: Option<serde_yaml::Value>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `build:` as either a context path or the long form.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Build {
    Long(BuildSpec),
    Context(String),
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BuildSpec {
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub dockerfile: Option<String>,
    #[serde(default)]
    pub args: Option<MapOrList>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `env_file:` as a path, a list of paths, or the long form with `required`.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum EnvFile {
    One(String),
    Many(Vec<EnvFileEntry>),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum EnvFileEntry {
    Long { path: String, #[serde(default)] required: Option<bool> },
    Path(String),
}

impl EnvFile {
    /// `(path, required)` pairs. Compose defaults `required` to true.
    pub fn into_paths(self) -> Vec<(String, bool)> {
        match self {
            Self::One(p) => vec![(p, true)],
            Self::Many(items) => items
                .into_iter()
                .map(|e| match e {
                    EnvFileEntry::Path(p) => (p, true),
                    EnvFileEntry::Long { path, required } => (path, required.unwrap_or(true)),
                })
                .collect(),
        }
    }
}

/// A `ports:` entry. The long form is tried first because a bare string can
/// never satisfy it, while `serde_yaml::Value` would swallow a map.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum PortEntry {
    Long(LongPort),
    Short(serde_yaml::Value),
}

#[derive(Clone, Debug, Deserialize)]
pub struct LongPort {
    pub target: serde_yaml::Value,
    #[serde(default)]
    pub published: Option<serde_yaml::Value>,
    #[serde(default)]
    pub host_ip: Option<String>,
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
}

/// A `volumes:` entry on a service.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum VolumeEntry {
    Long(LongVolume),
    Short(String),
}

#[derive(Clone, Debug, Deserialize)]
pub struct LongVolume {
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    pub target: String,
    #[serde(default)]
    pub read_only: Option<bool>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A service's `networks:`, as a list or a map of per-network options.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum ServiceNetworks {
    List(Vec<String>),
    Map(BTreeMap<String, Option<serde_yaml::Value>>),
}

impl ServiceNetworks {
    pub fn names(&self) -> Vec<String> {
        match self {
            Self::List(v) => v.clone(),
            Self::Map(m) => m.keys().cloned().collect(),
        }
    }
}

/// `depends_on:` as a plain list or a map carrying conditions.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum DependsOn {
    List(Vec<String>),
    Map(BTreeMap<String, DependsSpec>),
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct DependsSpec {
    #[serde(default)]
    pub condition: Option<String>,
    #[serde(default)]
    pub required: Option<bool>,
}

impl DependsOn {
    /// `(service, condition)` pairs, condition empty when not given.
    pub fn into_pairs(self) -> Vec<(String, Option<String>)> {
        match self {
            Self::List(v) => v.into_iter().map(|s| (s, None)).collect(),
            Self::Map(m) => m.into_iter().map(|(k, v)| (k, v.condition)).collect(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Healthcheck {
    #[serde(default)]
    pub test: Option<OneOrMany>,
    #[serde(default)]
    pub disable: Option<bool>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Deploy {
    #[serde(default)]
    pub replicas: Option<u32>,
    #[serde(default)]
    pub resources: Option<DeployResources>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct DeployResources {
    #[serde(default)]
    pub limits: Option<DeployLimits>,
    #[serde(default)]
    pub reservations: Option<DeployLimits>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct DeployLimits {
    #[serde(default)]
    pub cpus: Option<serde_yaml::Value>,
    #[serde(default)]
    pub memory: Option<serde_yaml::Value>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct NetworkDef {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub external: Option<External>,
    #[serde(default)]
    pub driver: Option<String>,
    #[serde(default)]
    pub internal: Option<bool>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct VolumeDef {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub external: Option<External>,
    #[serde(default)]
    pub driver: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `external:` is a bool in the Specification, but older files write
/// `external: { name: shared }` and both still appear.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum External {
    Flag(bool),
    Named { name: String },
}

impl External {
    pub fn is_external(&self) -> bool {
        match self {
            Self::Flag(b) => *b,
            Self::Named { .. } => true,
        }
    }

    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Flag(_) => None,
            Self::Named { name } => Some(name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(yaml: &str) -> Service {
        serde_yaml::from_str(yaml).expect("service parses")
    }

    #[test]
    fn a_command_reads_as_a_string_or_a_list() {
        let s = service("command: bundle exec rails s");
        assert_eq!(
            s.command.unwrap().into_vec(),
            vec!["bundle exec rails s".to_string()]
        );
        let s = service("command: [rails, server]");
        assert_eq!(
            s.command.unwrap().into_vec(),
            vec!["rails".to_string(), "server".to_string()]
        );
    }

    #[test]
    fn environment_reads_as_a_map_or_a_list() {
        let s = service("environment:\n  A: one\n  B: 2\n");
        let pairs = s.environment.unwrap().into_pairs();
        assert_eq!(pairs[0], ("A".into(), Some("one".into())));
        // YAML numbers have to reach the container as strings.
        assert_eq!(pairs[1], ("B".into(), Some("2".into())));

        let s = service("environment:\n  - A=one\n  - PASSTHROUGH\n");
        let pairs = s.environment.unwrap().into_pairs();
        assert_eq!(pairs[0], ("A".into(), Some("one".into())));
        // No `=` means "take it from my own environment".
        assert_eq!(pairs[1], ("PASSTHROUGH".into(), None));
    }

    #[test]
    fn an_environment_value_of_true_is_not_lost_to_yaml_typing() {
        let s = service("environment:\n  DEBUG: true\n");
        assert_eq!(
            s.environment.unwrap().into_pairs()[0],
            ("DEBUG".into(), Some("true".into()))
        );
    }

    #[test]
    fn ports_read_in_every_form_they_are_written_in() {
        let s = service("ports:\n  - 8080:80\n  - 3000\n  - target: 5432\n    published: 5433\n");
        assert_eq!(s.ports.len(), 3);
        assert!(matches!(s.ports[0], PortEntry::Short(_)));
        assert!(matches!(s.ports[1], PortEntry::Short(_)));
        // The long form must not be swallowed by the permissive short arm.
        assert!(matches!(s.ports[2], PortEntry::Long(_)));
    }

    #[test]
    fn volumes_read_short_and_long() {
        let s = service(
            "volumes:\n  - ./src:/app:ro\n  - type: volume\n    source: db\n    target: /var/lib\n",
        );
        assert!(matches!(s.volumes[0], VolumeEntry::Short(_)));
        assert!(matches!(s.volumes[1], VolumeEntry::Long(_)));
    }

    #[test]
    fn depends_on_reads_as_a_list_or_a_condition_map() {
        let s = service("depends_on: [db]");
        assert_eq!(s.depends_on.unwrap().into_pairs(), vec![("db".into(), None)]);

        let s = service("depends_on:\n  db:\n    condition: service_healthy\n");
        assert_eq!(
            s.depends_on.unwrap().into_pairs(),
            vec![("db".into(), Some("service_healthy".into()))]
        );
    }

    #[test]
    fn an_unknown_service_key_is_kept_rather_than_dropped() {
        // It becomes a warning later; losing it silently is the failure mode
        // this catch-all exists to prevent.
        let s = service("image: nginx\ncap_add: [NET_ADMIN]\n");
        assert!(s.extra.contains_key("cap_add"));
    }

    #[test]
    fn the_obsolete_version_key_is_not_mistaken_for_an_unknown_one() {
        let f: File = serde_yaml::from_str("version: '3.8'\nservices: {}\n").unwrap();
        assert!(f.extra.is_empty(), "version must not produce a warning");
    }

    #[test]
    fn external_reads_as_a_flag_or_a_name() {
        let f: File =
            serde_yaml::from_str("networks:\n  a:\n    external: true\n  b:\n    external:\n      name: shared\n")
                .unwrap();
        let a = f.networks["a"].as_ref().unwrap();
        assert!(a.external.as_ref().unwrap().is_external());
        let b = f.networks["b"].as_ref().unwrap();
        assert_eq!(b.external.as_ref().unwrap().name(), Some("shared"));
    }

    #[test]
    fn a_network_declared_with_no_body_is_still_declared() {
        let f: File = serde_yaml::from_str("networks:\n  app:\n").unwrap();
        assert!(f.networks.contains_key("app"));
        assert!(f.networks["app"].is_none());
    }
}
