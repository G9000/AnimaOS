//! Task-private input copies and fixed output slots. Durable publication belongs
//! to BrowserService and the existing owner attachment store.
use super::{
    records::{digest_valid, valid_id},
    transport::{error, BrowserErrorCode},
};
use crate::tools::workspace_bytes::WorkspaceBytes;
use anima_service_adapters::browser::{
    BrowserArtifact, BrowserArtifactKind, BrowserError, BrowserScope,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};
use tempfile::TempDir;

#[derive(Clone)]
pub(crate) struct VerifiedInput {
    pub(crate) id: String,
    pub(crate) bytes: Arc<[u8]>,
    pub(crate) sha256: String,
}
#[derive(Clone, Debug)]
pub(crate) struct OutputSlot {
    pub(crate) id: String,
    pub(crate) kind: BrowserArtifactKind,
}
pub(crate) struct VerifiedOutput {
    pub(crate) bytes: Arc<[u8]>,
    pub(crate) sha256: String,
}
pub(super) fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub(super) fn validate_slots(
    scope: &BrowserScope,
    inputs: &[VerifiedInput],
    outputs: &[OutputSlot],
) -> Result<(), BrowserError> {
    let mut ids = HashSet::new();
    if inputs.len() != scope.input_ids.len() || outputs.len() != 4 {
        return Err(error(BrowserErrorCode::Invalid));
    }
    for input in inputs {
        if !valid_id(&input.id)
            || !ids.insert(&input.id)
            || !scope.input_ids.contains(&input.id)
            || input.bytes.is_empty()
            || input.bytes.len() > scope.limits.input_bytes as usize
            || !digest_valid(&input.sha256)
            || hash(&input.bytes) != input.sha256
        {
            return Err(error(BrowserErrorCode::Invalid));
        }
    }
    let mut csv = false;
    let mut screenshot = false;
    for output in outputs {
        if !valid_id(&output.id) || !ids.insert(&output.id) {
            return Err(error(BrowserErrorCode::Invalid));
        }
        match output.kind {
            BrowserArtifactKind::Screenshot => screenshot = true,
            _ => csv = true,
        }
    }
    if !csv || !screenshot {
        return Err(error(BrowserErrorCode::Invalid));
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BootstrapInput {
    id: String,
    file_path: String,
    bytes: u64,
    sha256: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BootstrapOutput {
    pub(super) id: String,
    pub(super) kind: BrowserArtifactKind,
    pub(super) file_path: String,
}
pub(super) struct PrivateFiles {
    pub(super) directory: TempDir,
    pub(super) inputs: Vec<BootstrapInput>,
    pub(super) outputs: Vec<BootstrapOutput>,
}
impl PrivateFiles {
    pub(super) fn create(
        scope: &BrowserScope,
        inputs: &[VerifiedInput],
        outputs: &[OutputSlot],
    ) -> Result<Self, BrowserError> {
        validate_slots(scope, inputs, outputs)?;
        let directory = tempfile::Builder::new()
            .prefix("anima-browser-")
            .tempdir()
            .map_err(|_| error(BrowserErrorCode::Unavailable))?;
        let workspace = WorkspaceBytes::open(directory.path())
            .map_err(|_| error(BrowserErrorCode::Unavailable))?;
        let mut copies = Vec::new();
        for (index, input) in inputs.iter().enumerate() {
            let name = format!("input-{index}.csv");
            workspace
                .create_new_bytes(&name, &input.bytes)
                .map_err(|_| error(BrowserErrorCode::Unavailable))?;
            copies.push(BootstrapInput {
                id: input.id.clone(),
                file_path: path_text(&directory.path().join(name))?,
                bytes: input.bytes.len() as u64,
                sha256: input.sha256.clone(),
            });
        }
        let slots = outputs
            .iter()
            .enumerate()
            .map(|(index, output)| {
                let extension = output.kind.extension();
                Ok(BootstrapOutput {
                    id: output.id.clone(),
                    kind: output.kind,
                    file_path: path_text(
                        &directory.path().join(format!("output-{index}.{extension}")),
                    )?,
                })
            })
            .collect::<Result<Vec<_>, BrowserError>>()?;
        Ok(Self {
            directory,
            inputs: copies,
            outputs: slots,
        })
    }
    pub(super) fn root(&self) -> &Path {
        self.directory.path()
    }
    pub(super) fn write_bootstrap(&self, bytes: &[u8]) -> Result<PathBuf, BrowserError> {
        if bytes.len() > 64 * 1024 {
            return Err(error(BrowserErrorCode::LimitsExceeded));
        }
        WorkspaceBytes::open(self.root())
            .and_then(|root| root.create_new_bytes("bootstrap.json", bytes))
            .map_err(|_| error(BrowserErrorCode::Unavailable))?;
        Ok(self.root().join("bootstrap.json"))
    }
    pub(super) fn read_output(
        &self,
        scope: &BrowserScope,
        artifact: &BrowserArtifact,
    ) -> Result<VerifiedOutput, BrowserError> {
        if scope.interactive.is_none()
            && !matches!(
                artifact.kind,
                BrowserArtifactKind::Csv | BrowserArtifactKind::Screenshot
            )
        {
            return Err(error(BrowserErrorCode::Denied));
        }
        let slot = self
            .outputs
            .iter()
            .find(|slot| {
                slot.id == artifact.id
                    && (slot.kind == artifact.kind
                        || scope.interactive.is_some()
                            && slot.kind == BrowserArtifactKind::Csv
                            && artifact.kind != BrowserArtifactKind::Screenshot)
            })
            .ok_or_else(|| error(BrowserErrorCode::Denied))?;
        let maximum = match artifact.kind {
            BrowserArtifactKind::Screenshot => scope.limits.screenshot_bytes,
            _ => scope.limits.output_bytes,
        };
        if artifact.bytes == 0
            || artifact.bytes > u64::from(maximum)
            || !digest_valid(&artifact.sha256)
        {
            return Err(error(BrowserErrorCode::Invalid));
        }
        let relative = Path::new(&slot.file_path)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| error(BrowserErrorCode::Unavailable))?;
        let root =
            WorkspaceBytes::open(self.root()).map_err(|_| error(BrowserErrorCode::Unavailable))?;
        if root
            .instruction_link_count(relative)
            .map_err(|_| error(BrowserErrorCode::Unavailable))?
            != Some(1)
        {
            return Err(error(BrowserErrorCode::Unavailable));
        }
        let bytes = root
            .read_bounded(relative, maximum as usize, artifact.bytes)
            .map_err(|_| error(BrowserErrorCode::Unavailable))?;
        if hash(&bytes) != artifact.sha256
            || root
                .instruction_link_count(relative)
                .map_err(|_| error(BrowserErrorCode::Unavailable))?
                != Some(1)
        {
            return Err(error(BrowserErrorCode::Invalid));
        }
        Ok(VerifiedOutput {
            bytes: bytes.into(),
            sha256: artifact.sha256.clone(),
        })
    }
}
pub(super) fn path_text(path: &Path) -> Result<String, BrowserError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| error(BrowserErrorCode::Invalid))
}

#[cfg(test)]
mod interactive_kind_tests {
    use super::*;
    #[test]
    fn general_output_kinds_cannot_use_a_legacy_csv_slot() {
        let scope = crate::browser::tests::policies(120_000)
            .policies
            .remove("controlled-operator-policy")
            .unwrap()
            .template;
        let private = PrivateFiles::create(
            &scope,
            &[],
            &[
                super::super::transport::OutputSlot {
                    id: "download".into(),
                    kind: BrowserArtifactKind::Csv,
                },
                super::super::transport::OutputSlot {
                    id: "screenshot".into(),
                    kind: BrowserArtifactKind::Screenshot,
                },
                super::super::transport::OutputSlot {
                    id: "download_two".into(),
                    kind: BrowserArtifactKind::Csv,
                },
                super::super::transport::OutputSlot {
                    id: "screenshot_two".into(),
                    kind: BrowserArtifactKind::Screenshot,
                },
            ],
        )
        .unwrap();
        let output = &private.outputs[0];
        let bytes = b"bounded fixture";
        std::fs::write(&output.file_path, bytes).unwrap();
        for kind in [
            BrowserArtifactKind::Text,
            BrowserArtifactKind::Json,
            BrowserArtifactKind::Pdf,
            BrowserArtifactKind::Png,
            BrowserArtifactKind::Jpeg,
        ] {
            let artifact = BrowserArtifact {
                id: output.id.clone(),
                kind,
                bytes: bytes.len() as u64,
                sha256: hash(bytes),
            };
            assert!(private.read_output(&scope, &artifact).is_err());
        }
    }
}
