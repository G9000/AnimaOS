use std::{
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use serde::Serialize;
use tokio::{sync::Semaphore, task::JoinHandle};

use super::{ImageOperation, ImageStatus};
use crate::attachments::files::read_attachment_bytes;

pub(crate) const MAX_OBSERVATION_BYTES: u64 = 16 * 1024 * 1024;
const MAX_CONCURRENT_READERS: usize = 2;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImageObservation {
    operation_id: String,
    version_id: String,
    attachment_id: String,
    observed_at_ms: u64,
    limit: &'static str,
    #[serde(flatten)]
    outcome: ImageObservationOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
enum ImageObservationOutcome {
    BytesMatched { sha256: String, byte_length: u64 },
    Unavailable { reason: BytesUnavailable },
    NotChecked { reason: NotCheckedReason },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum BytesUnavailable {
    BytesUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum NotCheckedReason {
    BudgetExceeded,
    ReaderBusy,
}

fn observe_saved_image(
    root: Option<&Path>,
    operation: &ImageOperation,
    remaining_bytes: &mut u64,
) -> Option<ImageObservation> {
    if operation.status != ImageStatus::Succeeded {
        return None;
    }
    let (version, output) = operation.version.as_ref().zip(operation.output.as_ref())?;
    if version.attachment_id != output.id {
        return None;
    }
    let allowance = output.size.saturating_add(1);
    let outcome = if allowance > *remaining_bytes {
        ImageObservationOutcome::NotChecked {
            reason: NotCheckedReason::BudgetExceeded,
        }
    } else {
        *remaining_bytes -= allowance;
        match root.and_then(|root| read_attachment_bytes(root, output).ok()) {
            Some(bytes) => ImageObservationOutcome::BytesMatched {
                sha256: output.sha256.clone(),
                byte_length: bytes.len() as u64,
            },
            None => ImageObservationOutcome::Unavailable {
                reason: BytesUnavailable::BytesUnavailable,
            },
        }
    };
    observation(operation, outcome)
}

fn observation(
    operation: &ImageOperation,
    outcome: ImageObservationOutcome,
) -> Option<ImageObservation> {
    if operation.status != ImageStatus::Succeeded {
        return None;
    }
    let (version, output) = operation.version.as_ref().zip(operation.output.as_ref())?;
    if version.attachment_id != output.id {
        return None;
    }
    Some(ImageObservation {
        operation_id: operation.id.clone(),
        version_id: version.id.clone(),
        attachment_id: output.id.clone(),
        observed_at_ms: anima_core::primitives::now_millis(),
        limit: "saved_bytes_only",
        outcome,
    })
}

fn spawn_read_with_slots<T: Send + 'static>(
    readers: Arc<Semaphore>,
    read: impl FnOnce() -> T + Send + 'static,
) -> Option<JoinHandle<T>> {
    let permit = readers.try_acquire_owned().ok()?;
    Some(tokio::task::spawn_blocking(move || {
        // The physical reader owns capacity even if its async caller is cancelled.
        let _permit = permit;
        read()
    }))
}

pub(crate) async fn observe_saved_images(
    root: Option<PathBuf>,
    operations: &[ImageOperation],
) -> Vec<Option<ImageObservation>> {
    static READERS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    let readers =
        Arc::clone(READERS.get_or_init(|| Arc::new(Semaphore::new(MAX_CONCURRENT_READERS))));
    observe_saved_images_with_readers(root, operations, readers).await
}

async fn observe_saved_images_with_readers(
    root: Option<PathBuf>,
    operations: &[ImageOperation],
    readers: Arc<Semaphore>,
) -> Vec<Option<ImageObservation>> {
    let captured = operations.to_vec();
    let read = spawn_read_with_slots(readers, move || {
        let mut remaining_bytes = MAX_OBSERVATION_BYTES;
        captured
            .iter()
            .map(|operation| observe_saved_image(root.as_deref(), operation, &mut remaining_bytes))
            .collect::<Vec<_>>()
    });
    let fallback = match read {
        Some(read) => match read.await {
            Ok(observations) => return observations,
            Err(_) => ImageObservationOutcome::Unavailable {
                reason: BytesUnavailable::BytesUnavailable,
            },
        },
        None => ImageObservationOutcome::NotChecked {
            reason: NotCheckedReason::ReaderBusy,
        },
    };
    operations
        .iter()
        .map(|operation| observation(operation, fallback.clone()))
        .collect()
}

pub(crate) async fn observed_values(
    root: Option<PathBuf>,
    operations: &[ImageOperation],
) -> Vec<serde_json::Value> {
    let observations = observe_saved_images(root, operations).await;
    public_values(operations, observations)
}

fn public_values(
    operations: &[ImageOperation],
    observations: Vec<Option<ImageObservation>>,
) -> Vec<serde_json::Value> {
    operations
        .iter()
        .zip(observations)
        .map(|(operation, observation)| {
            let mut value = operation.public_value();
            if let Some(observation) = observation {
                value["observation"] =
                    serde_json::to_value(observation).expect("host image observation serializes");
            }
            value
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attachments::{files::save_upload, AttachmentKind, AttachmentRecord};
    use crate::images::ImageVersion;

    async fn read_values(
        root: Option<PathBuf>,
        operations: &[ImageOperation],
    ) -> Vec<serde_json::Value> {
        let observations = observe_saved_images_with_readers(
            root,
            operations,
            Arc::new(Semaphore::new(MAX_CONCURRENT_READERS)),
        )
        .await;
        public_values(operations, observations)
    }

    fn fixture() -> (tempfile::TempDir, ImageOperation) {
        let temp = tempfile::tempdir().expect("image observation workspace");
        let mut bytes = Vec::new();
        {
            let encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder
                .write_header()
                .expect("PNG header")
                .write_image_data(&[255])
                .expect("PNG pixel");
        }
        let saved = save_upload(temp.path(), "2026-10-30", "image.png", &bytes)
            .expect("saved image fixture");
        let output = AttachmentRecord {
            id: "attachment".into(),
            agent_id: "agent".into(),
            session_id: "chat:image".into(),
            name: "image.png".into(),
            mime: "image/png".into(),
            size: bytes.len() as u64,
            kind: AttachmentKind::Image,
            path: saved.path,
            created_at_ms: 1,
            workspace_root: saved.workspace_root,
            sha256: saved.sha256,
        };
        let operation = ImageOperation {
            id: "operation".into(),
            run_id: "run".into(),
            tool_call_id: "call".into(),
            clear_generation: 0,
            prompt: "A bird".into(),
            source: None,
            status: ImageStatus::Succeeded,
            created_at_ms: 1,
            output: Some(output),
            version: Some(ImageVersion {
                id: "version".into(),
                attachment_id: "attachment".into(),
                parent_attachment_id: None,
                width: 1024,
                height: 1024,
                model: "gpt-image-2.5-sunburst".into(),
                created_at_ms: 1,
            }),
            usage: None,
            credential_id: "fixture".into(),
            approval_id: "approval".into(),
        };
        (temp, operation)
    }

    #[tokio::test]
    async fn saved_image_observation_matches_exact_bytes_and_names_its_limits() {
        let (temp, operation) = fixture();
        let output = operation.output.as_ref().expect("fixture output");
        let value = read_values(
            Some(temp.path().to_path_buf()),
            std::slice::from_ref(&operation),
        )
        .await;
        let observation = &value[0]["observation"];
        assert_eq!(observation["state"], "bytes_matched");
        assert_eq!(observation["operationId"], operation.id);
        assert_eq!(observation["attachmentId"], output.id);
        assert_eq!(observation["versionId"], "version");
        assert_eq!(observation["sha256"], output.sha256);
        assert_eq!(observation["byteLength"], output.size);
        assert_eq!(observation["limit"], "saved_bytes_only");
        assert!(observation["observedAtMs"].as_u64().expect("time") > 0);
    }

    #[tokio::test]
    async fn missing_or_changed_image_bytes_do_not_rewrite_historical_success() {
        for deleted in [false, true] {
            let (temp, operation) = fixture();
            let output = operation.output.as_ref().expect("fixture output");
            let path = temp.path().join(&output.path);
            if deleted {
                std::fs::remove_file(&path).expect("remove fixture image");
            } else {
                let mut bytes = std::fs::read(&path).expect("read fixture image");
                bytes[0] ^= 1;
                std::fs::write(&path, bytes).expect("tamper fixture image");
            }
            let values = read_values(
                Some(temp.path().to_path_buf()),
                std::slice::from_ref(&operation),
            )
            .await;
            assert_eq!(values[0]["status"], "succeeded");
            assert_eq!(values[0]["observation"]["state"], "unavailable");
            assert_eq!(values[0]["observation"]["reason"], "bytes_unavailable");
            assert_eq!(operation.status, ImageStatus::Succeeded);
        }
    }

    #[tokio::test]
    async fn bounded_image_observation_does_not_infer_remote_or_visual_success() {
        let (temp, mut operation) = fixture();
        let mut budget = 0;
        let observation = observe_saved_image(Some(temp.path()), &operation, &mut budget)
            .expect("budget observation");
        assert!(matches!(
            observation.outcome,
            ImageObservationOutcome::NotChecked { .. }
        ));
        assert_eq!(budget, 0);
        let other = tempfile::tempdir().expect("other workspace");
        let values = read_values(
            Some(other.path().to_path_buf()),
            std::slice::from_ref(&operation),
        )
        .await;
        assert_eq!(values[0]["observation"]["state"], "unavailable");
        operation.status = ImageStatus::OutcomeUnknown;
        operation.output = None;
        operation.version = None;
        let values = read_values(Some(temp.path().to_path_buf()), &[operation]).await;
        assert_eq!(values[0]["status"], "outcome_unknown");
        assert!(values[0].get("observation").is_none());
    }

    #[test]
    fn saved_image_observation_enforces_exact_and_cumulative_read_budget() {
        let (temp, operation) = fixture();
        let output = operation.output.as_ref().expect("fixture output");
        let mut remaining_bytes = output.size + 1;
        let first = observe_saved_image(Some(temp.path()), &operation, &mut remaining_bytes)
            .expect("first saved byte observation");
        assert!(matches!(
            first.outcome,
            ImageObservationOutcome::BytesMatched { byte_length, .. } if byte_length == output.size
        ));
        assert_eq!(remaining_bytes, 0, "the exact byte allowance is consumed");
        std::fs::remove_file(temp.path().join(&output.path)).expect("remove observed fixture");
        let next = observe_saved_image(Some(temp.path()), &operation, &mut remaining_bytes)
            .expect("exhausted budget observation");
        assert_eq!(
            next.outcome,
            ImageObservationOutcome::NotChecked {
                reason: NotCheckedReason::BudgetExceeded,
            },
            "an exhausted pass must skip another physical read"
        );
        assert_eq!(remaining_bytes, 0);
        assert_eq!(operation.status, ImageStatus::Succeeded);
    }

    #[tokio::test]
    async fn occupied_image_readers_report_not_checked_for_the_exact_saved_target() {
        let (temp, operation) = fixture();
        let readers = Arc::new(Semaphore::new(1));
        let occupied = readers.clone().try_acquire_owned().expect("occupy reader");
        let observations = observe_saved_images_with_readers(
            Some(temp.path().to_path_buf()),
            std::slice::from_ref(&operation),
            readers,
        )
        .await;
        let observation = observations[0].as_ref().expect("busy observation");
        let value = serde_json::to_value(observation).expect("serialize observation");
        assert_eq!(value["state"], "not_checked");
        assert_eq!(value["reason"], "reader_busy");
        assert_eq!(value["operationId"], operation.id);
        assert_eq!(value["versionId"], "version");
        assert_eq!(value["attachmentId"], "attachment");
        assert_eq!(value["limit"], "saved_bytes_only");
        assert!(value["observedAtMs"].as_u64().expect("time") > 0);
        assert_eq!(operation.status, ImageStatus::Succeeded);
        drop(occupied);
    }

    #[tokio::test]
    async fn image_reader_saturation_and_waiter_abort_retain_physical_capacity() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::Duration;

        let readers = Arc::new(Semaphore::new(1));
        let entries = Arc::new(AtomicUsize::new(0));
        let read_entries = entries.clone();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let read = spawn_read_with_slots(readers.clone(), move || {
            read_entries.fetch_add(1, Ordering::SeqCst);
            let _ = entered_tx.send(());
            let _ = release_rx.recv();
        })
        .expect("start physical read");
        let waiter = tokio::spawn(async move { read.await });
        tokio::time::timeout(Duration::from_secs(5), entered_rx)
            .await
            .expect("reader entered promptly")
            .expect("entry signal");

        let assert_saturated = || {
            let extra_entries = entries.clone();
            assert!(
                spawn_read_with_slots(readers.clone(), move || {
                    extra_entries.fetch_add(1, Ordering::SeqCst);
                })
                .is_none(),
                "saturated readers must reject extra physical work"
            );
            assert_eq!(readers.available_permits(), 0);
            assert_eq!(entries.load(Ordering::SeqCst), 1);
        };
        assert_saturated();
        waiter.abort();
        assert!(waiter.await.expect_err("cancelled waiter").is_cancelled());
        assert_saturated();
        release_tx.send(()).expect("release physical read");
        let retired = tokio::time::timeout(Duration::from_secs(5), readers.clone().acquire_owned())
            .await
            .expect("physical reader retires")
            .expect("reader semaphore open");
        drop(retired);
        let resumed_entries = entries.clone();
        spawn_read_with_slots(readers, move || {
            resumed_entries.fetch_add(1, Ordering::SeqCst);
        })
        .expect("capacity restored after physical retirement")
        .await
        .expect("resumed reader");
        assert_eq!(entries.load(Ordering::SeqCst), 2);
    }
}
