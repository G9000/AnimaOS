//! Stable, storage-neutral individual identity. Registration grants no capabilities.
use crate::MemoryPortError;
use serde::{Deserialize, Serialize};

macro_rules! identity_id {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String")]
        pub struct $name(String);
        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, MemoryPortError> {
                Ok(Self(valid_id(value.into())?))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl TryFrom<String> for $name {
            type Error = MemoryPortError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }
    };
}
identity_id!(OwnerId);
identity_id!(IndividualId);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityBinding {
    owner_id: OwnerId,
    individual_id: IndividualId,
}

impl IdentityBinding {
    pub fn new(
        owner: impl Into<String>,
        individual: impl Into<String>,
    ) -> Result<Self, MemoryPortError> {
        Ok(Self {
            owner_id: OwnerId::new(owner)?,
            individual_id: IndividualId::new(individual)?,
        })
    }
    pub fn owner_id(&self) -> &OwnerId {
        &self.owner_id
    }
    pub fn individual_id(&self) -> &IndividualId {
        &self.individual_id
    }
}

/// An explicit host save/world mapping, separate from identity and filesystem authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ResidentWire")]
pub struct WorldResidentBinding {
    identity: IdentityBinding,
    world_id: String,
    resident_id: String,
}
impl WorldResidentBinding {
    pub fn new(
        identity: IdentityBinding,
        world_id: impl Into<String>,
        resident_id: impl Into<String>,
    ) -> Result<Self, MemoryPortError> {
        Ok(Self {
            identity,
            world_id: valid_id(world_id.into())?,
            resident_id: valid_id(resident_id.into())?,
        })
    }
    pub fn identity(&self) -> &IdentityBinding {
        &self.identity
    }
    pub fn world_id(&self) -> &str {
        &self.world_id
    }
    pub fn resident_id(&self) -> &str {
        &self.resident_id
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResidentWire {
    identity: IdentityBinding,
    world_id: String,
    resident_id: String,
}
impl TryFrom<ResidentWire> for WorldResidentBinding {
    type Error = MemoryPortError;
    fn try_from(wire: ResidentWire) -> Result<Self, Self::Error> {
        Self::new(wire.identity, wire.world_id, wire.resident_id)
    }
}
fn valid_id(value: String) -> Result<String, MemoryPortError> {
    if value.is_empty()
        || value.len() > 256
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(MemoryPortError::invalid(
            "identity IDs must be 1..256 bytes without padding or control characters",
        ));
    }
    Ok(value)
}
