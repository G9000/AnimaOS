//! Explicit basic-content profile. The authentic host binds this choice into
//! expected scope/policy; no model input can select a weaker validation profile.
use super::*;

/// Attribute encoding_version=1 contains exactly serde_json::to_vec of this
/// closed type, in declared field order. No timestamps are inferred or encoded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "platform", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContentBasicPermissionsV1 {
    Windows { readonly: bool },
    Posix { mode: u32 },
}

pub fn validate_content_basic_v1_filesystem_checkpoint(
    checkpoint: &FilesystemCheckpoint,
    expected_scope: &CheckpointScope,
    objects: &[CheckpointBlob],
) -> Result<(), CheckpointError> {
    validate_filesystem_checkpoint_with_profile(
        checkpoint,
        expected_scope,
        objects,
        ValidationProfile::ContentBasicV1(expected_scope.path_semantics),
    )
}

pub fn prepare_content_basic_v1_safe_restore_with_lineage(
    checkpoint: &FilesystemCheckpoint,
    expected_scope: &CheckpointScope,
    objects: &[CheckpointBlob],
    current: &[CheckpointEntry],
    lineage: &MutationLineage,
    expected_head: &ExpectedLineageHead,
) -> Result<RestoreProposal, CheckpointError> {
    lineage::prepare_safe_restore_with_lineage_profile(
        checkpoint,
        expected_scope,
        objects,
        current,
        lineage,
        expected_head,
        ValidationProfile::ContentBasicV1(expected_scope.path_semantics),
    )
}

pub(super) fn validate_metadata(
    metadata: &MetadataSnapshot,
    semantics: PathSemantics,
) -> Result<(), CheckpointError> {
    if metadata.required != [MetadataKind::BasicPermissions] || metadata.attributes.len() != 1 {
        return Err(CheckpointError::InvalidMetadata);
    }
    let attribute = &metadata.attributes[0];
    if attribute.kind != MetadataKind::BasicPermissions || attribute.encoding_version != 1 {
        return Err(CheckpointError::InvalidMetadata);
    }
    let permission: ContentBasicPermissionsV1 =
        serde_json::from_slice(&attribute.bytes).map_err(|_| CheckpointError::InvalidMetadata)?;
    match (&permission, semantics) {
        (ContentBasicPermissionsV1::Windows { .. }, PathSemantics::WindowsInsensitive) => {}
        (ContentBasicPermissionsV1::Posix { mode }, PathSemantics::PosixSensitive)
            if *mode <= 0o7777 => {}
        _ => return Err(CheckpointError::InvalidMetadata),
    }
    if serde_json::to_vec(&permission).map_err(|_| CheckpointError::InvalidMetadata)?
        != attribute.bytes
    {
        return Err(CheckpointError::InvalidMetadata);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
