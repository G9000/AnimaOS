//! Bounded protocol shared by owner hosts and paired read-only nodes.
use anima_core::{prepare_external_tool, ExternalToolDescriptor};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const NODE_PROTOCOL: &str = "anima-node-v1";
pub const NODE_META_KEY: &str = "animaNode";
pub const MAX_NODE_READ_BYTES: usize = 32 * 1024;
pub const MAX_NODE_WIRE_BYTES: usize = 128 * 1024;
pub const MAX_NODE_ENTRIES: usize = 128;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeManifest {
    pub protocol: String,
    pub identity: String,
    pub generation: u64,
    pub root_binding: String,
    pub tools: Vec<ExternalToolDescriptor>,
}

impl NodeManifest {
    pub fn minimal(identity: String, generation: u64, root_binding: String) -> Self {
        let empty =
            serde_json::json!({"type":"object","properties":{},"additionalProperties":false});
        let read = serde_json::json!({"type":"object","properties":{"path":{"type":"string","minLength":1,"maxLength":1024}},"required":["path"],"additionalProperties":false});
        let tools = [
            ("node_system_info", "Read the paired node operating system and architecture", empty.clone()),
            ("node_list_root", "List up to 128 ordinary entries in the owner-selected node root", empty),
            ("node_read_file", "Read at most 32768 bytes of an ordinary UTF-8 file below the owner-selected node root", read),
        ].into_iter().map(|(name, description, schema)| prepare_external_tool(name, description, schema, None).expect("fixed node schema")).collect();
        Self {
            protocol: NODE_PROTOCOL.into(),
            identity,
            generation,
            root_binding,
            tools,
        }
    }
    pub fn digest(&self) -> Result<String, &'static str> {
        self.validate()?;
        let bytes = serde_jcs::to_vec(self).map_err(|_| "invalid manifest")?;
        Ok(digest_bytes(&bytes))
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.protocol != NODE_PROTOCOL
            || !valid_text(&self.identity)
            || self.generation == 0
            || self.generation > 9_007_199_254_740_991
            || !valid_digest(&self.root_binding)
            || self.tools.len() != 3
        {
            return Err("invalid manifest");
        }
        let expected = Self::minimal(
            self.identity.clone(),
            self.generation,
            self.root_binding.clone(),
        );
        if expected.tools != self.tools {
            return Err("unsupported node capability");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeInvocation {
    pub identity: String,
    pub manifest_digest: String,
    pub generation: u64,
    pub agent_id: String,
    pub run_id: String,
    pub session_id: String,
    pub tool_call_id: String,
    pub descriptor_digest: String,
    pub deadline_unix_ms: u64,
}
impl NodeInvocation {
    pub fn validate(&self, manifest: &NodeManifest, now_ms: u64) -> Result<(), &'static str> {
        if self.identity != manifest.identity
            || self.generation != manifest.generation
            || self.manifest_digest != manifest.digest()?
            || !valid_digest(&self.descriptor_digest)
            || [
                &self.agent_id,
                &self.run_id,
                &self.session_id,
                &self.tool_call_id,
            ]
            .iter()
            .any(|v| !valid_text(v))
            || self.deadline_unix_ms <= now_ms
            || self.deadline_unix_ms - now_ms > 30_000
        {
            return Err("node invocation binding rejected");
        }
        Ok(())
    }
    pub fn replay_key(&self) -> String {
        digest_bytes(
            &serde_jcs::to_vec(&(
                &self.agent_id,
                &self.run_id,
                &self.session_id,
                &self.tool_call_id,
            ))
            .expect("strings serialize"),
        )
    }
}
pub fn valid_text(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.chars().all(|c| c.is_ascii_graphic() || c == ' ')
}
pub fn valid_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|v| {
        v.len() == 64
            && v.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
pub fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paired_devices_manifest_binds_root_and_rejects_capability_widening() {
        let manifest = NodeManifest::minimal("fixture-node".into(), 1, digest_bytes(b"root-a"));
        let before = manifest.digest().unwrap();
        let changed = NodeManifest::minimal("fixture-node".into(), 1, digest_bytes(b"root-b"));
        assert_ne!(before, changed.digest().unwrap());
        let mut widened = manifest;
        widened.tools[0]
            .description
            .push_str(" and execute shell commands");
        assert!(widened.validate().is_err());
    }
    #[test]
    fn paired_devices_invocation_replay_identity_is_stable_and_binding_expires() {
        let manifest = NodeManifest::minimal("fixture-node".into(), 1, digest_bytes(b"root-a"));
        let mut call = NodeInvocation {
            identity: manifest.identity.clone(),
            manifest_digest: manifest.digest().unwrap(),
            generation: 1,
            agent_id: "agent".into(),
            run_id: "run".into(),
            session_id: "session".into(),
            tool_call_id: "call".into(),
            descriptor_digest: manifest.tools[0].descriptor_digest.clone(),
            deadline_unix_ms: 20_000,
        };
        assert!(call.validate(&manifest, 10_000).is_ok());
        let key = call.replay_key();
        call.deadline_unix_ms += 1;
        assert_eq!(key, call.replay_key());
        call.generation = 2;
        assert!(call.validate(&manifest, 10_000).is_err());
        call.generation = 1;
        assert!(call.validate(&manifest, 30_000).is_err());
    }
}
