//! Networks.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ipam {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub subnet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gateway: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Network {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub scope: String,
    pub internal: bool,
    pub attachable: bool,
    pub ipam: Vec<Ipam>,
    pub containers: usize,
    pub created: String,
    pub labels: BTreeMap<String, String>,
}

impl Network {
    /// Docker's own networks, which must never be offered for removal.
    pub fn is_builtin(&self) -> bool {
        matches!(self.name.as_str(), "bridge" | "host" | "none")
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkCreateInput {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub driver: Option<String>,
    #[serde(default)]
    pub internal: bool,
    #[serde(default)]
    pub attachable: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub subnet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gateway: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_networks_are_protected() {
        let net = |name: &str| Network {
            name: name.into(),
            ..Default::default()
        };
        assert!(net("bridge").is_builtin());
        assert!(net("host").is_builtin());
        assert!(net("none").is_builtin());
        assert!(!net("myapp_default").is_builtin());
    }
}
