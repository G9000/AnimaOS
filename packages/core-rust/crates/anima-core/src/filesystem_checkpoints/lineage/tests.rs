use super::*;
use crate::filesystem_checkpoints::{
    CheckpointEntry, MetadataAttribute, MetadataKind, MetadataSnapshot, PathSemantics,
    RestoreConflictReason, RestoreOperation, MAX_CHECKPOINT_CONTENT_BYTES,
    MAX_CHECKPOINT_MANIFEST_BYTES, MAX_CHECKPOINT_STREAM_BYTES,
};
use sha2::{Digest, Sha256};

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn domain_digest(domain: &[u8], value: &impl Serialize) -> String {
    let mut bytes = domain.to_vec();
    bytes.extend(serde_json::to_vec(value).unwrap());
    digest(&bytes)
}

fn scope() -> CheckpointScope {
    CheckpointScope {
        owner_id: "owner-1".into(),
        workspace_id: "workspace-1".into(),
        environment_id: "host-1".into(),
        path_semantics: PathSemantics::WindowsInsensitive,
        root_identity_digest: "1".repeat(64),
        policy_digest: "2".repeat(64),
    }
}

fn object(bytes: &[u8]) -> CheckpointBlob {
    CheckpointBlob {
        reference: super::super::BlobReference {
            sha256: digest(bytes),
            byte_count: bytes.len() as u64,
        },
        bytes: bytes.to_vec(),
    }
}

fn file(blob: &CheckpointBlob, permissions: u8) -> FileState {
    FileState::File {
        content: blob.reference.clone(),
        metadata: MetadataSnapshot {
            required: vec![
                MetadataKind::BasicPermissions,
                MetadataKind::ModificationTime,
            ],
            attributes: vec![
                MetadataAttribute {
                    kind: MetadataKind::BasicPermissions,
                    encoding_version: 1,
                    bytes: vec![permissions, 0, 255],
                },
                MetadataAttribute {
                    kind: MetadataKind::ModificationTime,
                    encoding_version: 1,
                    bytes: vec![1, 2, 3, 4],
                },
            ],
        },
    }
}

fn entry(path: &str, state: FileState) -> CheckpointEntry {
    CheckpointEntry {
        path: path.into(),
        state,
    }
}

fn change(
    path: &str,
    disposition: MutationDisposition,
    before: FileState,
    after: FileState,
) -> MutationChange {
    MutationChange {
        path: path.into(),
        disposition,
        observed_preimage: before,
        acknowledged_postimage: Some(after),
    }
}

fn action(sequence: u64, mut changes: Vec<MutationChange>) -> MutationAction {
    changes.sort_by(|left, right| left.path.cmp(&right.path));
    MutationAction {
        sequence,
        action_id: format!("action-{sequence}"),
        native_run_id: "run-1".into(),
        native_tool_call_id: format!("call-{sequence}"),
        request_digest: digest(format!("request-{sequence}").as_bytes()),
        outcome: MutationActionOutcome::Acknowledged,
        changes,
    }
}

fn head_input(lineage: &MutationLineage) -> LineageHeadDigestInput {
    LineageHeadDigestInput {
        version: lineage.version,
        checkpoint_id: lineage.checkpoint_id.clone(),
        checkpoint_sequence: lineage.checkpoint_sequence,
        manifest_digest: lineage.manifest_digest.clone(),
        scope_digest: lineage.scope_digest.clone(),
        action_count: lineage.action_count,
        last_sequence: lineage.last_sequence,
        action_digests: lineage
            .actions
            .iter()
            .map(|action| domain_digest(b"anima-filesystem-checkpoint-action-v1\0", action))
            .collect(),
    }
}

fn recompute_decoded_head(lineage: &mut MutationLineage) {
    lineage.action_count = lineage.actions.len() as u32;
    lineage.last_sequence = lineage.actions.last().map_or(0, |action| action.sequence);
    lineage.head_digest = domain_digest(
        b"anima-filesystem-checkpoint-lineage-head-v1\0",
        &head_input(lineage),
    );
}

fn expected(lineage: &MutationLineage) -> ExpectedLineageHead {
    ExpectedLineageHead {
        checkpoint_id: lineage.checkpoint_id.clone(),
        checkpoint_sequence: lineage.checkpoint_sequence,
        manifest_digest: lineage.manifest_digest.clone(),
        scope_digest: lineage.scope_digest.clone(),
        action_count: lineage.action_count,
        last_sequence: lineage.last_sequence,
        head_digest: lineage.head_digest.clone(),
        action_bindings: lineage
            .actions
            .iter()
            .map(|action| ExpectedActionBinding {
                sequence: action.sequence,
                action_id: action.action_id.clone(),
                native_run_id: action.native_run_id.clone(),
                native_tool_call_id: action.native_tool_call_id.clone(),
                request_digest: action.request_digest.clone(),
                action_digest: domain_digest(b"anima-filesystem-checkpoint-action-v1\0", action),
            })
            .collect(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Case {
    checkpoint: FilesystemCheckpoint,
    objects: Vec<CheckpointBlob>,
    current: Vec<CheckpointEntry>,
    lineage: MutationLineage,
    expected: ExpectedLineageHead,
}

impl Case {
    fn new(
        mut entries: Vec<CheckpointEntry>,
        objects: Vec<CheckpointBlob>,
        current: Vec<CheckpointEntry>,
        actions: Vec<MutationAction>,
    ) -> Self {
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        let scope = scope();
        let checkpoint = FilesystemCheckpoint {
            version: 1,
            checkpoint_id: "cp-1".into(),
            sequence: 7,
            scope_digest: domain_digest(b"anima-filesystem-checkpoint-scope-v1\0", &scope),
            scope,
            entries,
        };
        let mut lineage = MutationLineage {
            version: 1,
            checkpoint_id: checkpoint.checkpoint_id.clone(),
            checkpoint_sequence: checkpoint.sequence,
            manifest_digest: domain_digest(
                b"anima-filesystem-checkpoint-manifest-v1\0",
                &checkpoint,
            ),
            scope_digest: checkpoint.scope_digest.clone(),
            action_count: 0,
            last_sequence: 0,
            actions,
            head_digest: String::new(),
        };
        recompute_decoded_head(&mut lineage);
        let expected = expected(&lineage);
        Self {
            checkpoint,
            objects,
            current,
            lineage,
            expected,
        }
    }

    fn plan(&self) -> Result<RestoreProposal, CheckpointError> {
        prepare_safe_restore_with_lineage(
            &self.checkpoint,
            &scope(),
            &self.objects,
            &self.current,
            &self.lineage,
            &self.expected,
        )
    }

    // Tests use this only to simulate a host-authenticated journal that really
    // recorded a discontinuity/uncertain step, not to grant decoded data authority.
    fn reseal_authentic_fixture(&mut self) {
        recompute_decoded_head(&mut self.lineage);
        self.expected = expected(&self.lineage);
    }
}

fn creation_case() -> Case {
    let first = object(b"first\r\n");
    let last = object(b"last\n");
    let a = file(&first, 0);
    let b = file(&last, 128);
    Case::new(
        vec![entry("created.bin", FileState::Absent)],
        vec![first, last],
        vec![entry("created.bin", b.clone())],
        vec![
            action(
                1,
                vec![change(
                    "created.bin",
                    MutationDisposition::Created,
                    FileState::Absent,
                    a.clone(),
                )],
            ),
            action(
                2,
                vec![change("created.bin", MutationDisposition::Replaced, a, b)],
            ),
        ],
    )
}

fn assert_conflict(case: &Case, reason: RestoreConflictReason) {
    let plan = case.plan().unwrap();
    assert!(plan.operations.is_empty());
    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.conflicts[0].path, "created.bin");
    assert_eq!(plan.conflicts[0].reason, reason);
}

#[test]
fn fp08_lineage_digest_domains_use_exact_declared_struct_json_bytes() {
    let mut case = creation_case();
    case.checkpoint.entries.clear();
    let scope_hash = &case.checkpoint.scope_digest;
    let manifest_json = format!(
        r#"{{"version":1,"checkpoint_id":"cp-1","sequence":7,"scope":{{"owner_id":"owner-1","workspace_id":"workspace-1","environment_id":"host-1","path_semantics":"windows_insensitive","root_identity_digest":"{}","policy_digest":"{}"}},"scope_digest":"{}","entries":[]}}"#,
        "1".repeat(64),
        "2".repeat(64),
        scope_hash
    );
    assert_eq!(
        serde_json::to_vec(&case.checkpoint).unwrap(),
        manifest_json.as_bytes()
    );
    let mut exact = b"anima-filesystem-checkpoint-manifest-v1\0".to_vec();
    exact.extend(manifest_json.as_bytes());
    assert_eq!(checkpoint_manifest_digest(&case.checkpoint), digest(&exact));
    let mut action = action(
        1,
        vec![MutationChange {
            path: "empty".into(),
            disposition: MutationDisposition::Uncertain,
            observed_preimage: FileState::Absent,
            acknowledged_postimage: None,
        }],
    );
    action.outcome = MutationActionOutcome::Uncertain;
    let action_json = format!(
        r#"{{"sequence":1,"action_id":"action-1","native_run_id":"run-1","native_tool_call_id":"call-1","request_digest":"{}","outcome":"uncertain","changes":[{{"path":"empty","disposition":"uncertain","observed_preimage":{{"kind":"absent"}},"acknowledged_postimage":null}}]}}"#,
        action.request_digest
    );
    assert_eq!(serde_json::to_vec(&action).unwrap(), action_json.as_bytes());
    let mut exact = b"anima-filesystem-checkpoint-action-v1\0".to_vec();
    exact.extend(action_json.as_bytes());
    assert_eq!(checkpoint_action_digest(&action), digest(&exact));
    let input = head_input(&case.lineage);
    let head_json = format!(
        r#"{{"version":1,"checkpoint_id":"cp-1","checkpoint_sequence":7,"manifest_digest":"{}","scope_digest":"{}","action_count":2,"last_sequence":2,"action_digests":["{}","{}"]}}"#,
        input.manifest_digest, input.scope_digest, input.action_digests[0], input.action_digests[1]
    );
    assert_eq!(serde_json::to_vec(&input).unwrap(), head_json.as_bytes());
    let mut exact = b"anima-filesystem-checkpoint-lineage-head-v1\0".to_vec();
    exact.extend(head_json.as_bytes());
    assert_eq!(checkpoint_lineage_head_digest(&input), digest(&exact));
    assert_ne!(
        digest(&exact),
        domain_digest(b"anima-filesystem-checkpoint-action-v1\0", &input)
    );
}

#[test]
fn fp08_lineage_created_then_replaced_can_remove_only_unchanged_creation() {
    let case = creation_case();
    let pristine = case.clone();
    let plan = case.plan().unwrap();
    assert_eq!(
        plan.operations,
        vec![RestoreOperation {
            path: "created.bin".into(),
            expected_current: case.current[0].state.clone(),
            desired: FileState::Absent
        }]
    );
    assert!(plan.conflicts.is_empty());
    assert_eq!(case, pristine);
    let mut dirty = case.clone();
    dirty.current[0].state = FileState::Absent;
    assert_conflict(&dirty, RestoreConflictReason::Dirty);
    let mut missing = case.clone();
    missing.current.clear();
    assert_conflict(&missing, RestoreConflictReason::IncompleteObservation);
}

#[test]
fn fp08_lineage_deleted_then_created_restores_exact_original() {
    let original = object(b"\xff\0original\r\n");
    let created = object(b"");
    let before = file(&original, 128);
    let after = file(&created, 0);
    let case = Case::new(
        vec![entry("file", before.clone())],
        vec![original, created],
        vec![entry("file", after.clone())],
        vec![
            action(
                1,
                vec![change(
                    "file",
                    MutationDisposition::Deleted,
                    before.clone(),
                    FileState::Absent,
                )],
            ),
            action(
                2,
                vec![change(
                    "file",
                    MutationDisposition::Created,
                    FileState::Absent,
                    after.clone(),
                )],
            ),
        ],
    );
    assert_eq!(
        case.plan().unwrap().operations,
        vec![RestoreOperation {
            path: "file".into(),
            expected_current: after,
            desired: before
        }]
    );
    assert!(case.plan().unwrap().conflicts.is_empty());
}

#[test]
fn fp08_lineage_replacements_preserve_exact_first_preimage() {
    let original = object(b"\xff\0first\r\n");
    let middle = object(b"middle\n");
    let last = object(b"last");
    let o = file(&original, 128);
    let a = file(&middle, 0);
    let b = file(&last, 1);
    let case = Case::new(
        vec![entry("file", o.clone())],
        vec![original, middle, last],
        vec![entry("file", b.clone())],
        vec![
            action(
                1,
                vec![change(
                    "file",
                    MutationDisposition::Replaced,
                    o.clone(),
                    a.clone(),
                )],
            ),
            action(
                2,
                vec![change("file", MutationDisposition::Replaced, a, b.clone())],
            ),
        ],
    );
    assert_eq!(
        case.plan().unwrap().operations,
        vec![RestoreOperation {
            path: "file".into(),
            expected_current: b,
            desired: o
        }]
    );
}

#[test]
fn fp08_lineage_interleaved_paths_share_global_action_identity() {
    let blobs: Vec<_> = [b"a1".as_slice(), b"a3", b"b0", b"b1", b"b2", b"unrelated"]
        .into_iter()
        .map(object)
        .collect();
    let (a1, a3, b0, b1, b2, unrelated) = (
        file(&blobs[0], 0),
        file(&blobs[1], 0),
        file(&blobs[2], 128),
        file(&blobs[3], 0),
        file(&blobs[4], 0),
        file(&blobs[5], 0),
    );
    let case = Case::new(
        vec![entry("a", FileState::Absent), entry("b", b0.clone())],
        blobs,
        vec![
            entry("unrelated", unrelated),
            entry("b", b2.clone()),
            entry("a", a3.clone()),
        ],
        vec![
            action(
                1,
                vec![
                    change(
                        "a",
                        MutationDisposition::Created,
                        FileState::Absent,
                        a1.clone(),
                    ),
                    change("b", MutationDisposition::Replaced, b0.clone(), b1.clone()),
                ],
            ),
            action(
                2,
                vec![change("b", MutationDisposition::Replaced, b1, b2.clone())],
            ),
            action(
                3,
                vec![change("a", MutationDisposition::Replaced, a1, a3.clone())],
            ),
        ],
    );
    let before = case.clone();
    let plan = case.plan().unwrap();
    assert_eq!(
        plan.operations,
        vec![
            RestoreOperation {
                path: "a".into(),
                expected_current: a3,
                desired: FileState::Absent
            },
            RestoreOperation {
                path: "b".into(),
                expected_current: b2,
                desired: b0
            }
        ]
    );
    assert!(plan.conflicts.is_empty());
    assert_eq!(case, before);
}

#[test]
fn fp08_lineage_intermediate_external_change_never_borrows_final_ack() {
    let base = creation_case();
    assert_eq!(base.plan().unwrap().operations.len(), 1);
    let external = object(b"owner edit");
    for mismatch in 0..4 {
        let mut case = base.clone();
        case.objects.push(external.clone());
        match mismatch {
            0 => case.lineage.actions[1].changes[0].observed_preimage = file(&external, 0),
            1 => {
                if let FileState::File { metadata, .. } =
                    &mut case.lineage.actions[1].changes[0].observed_preimage
                {
                    metadata.attributes[0].bytes[0] ^= 1;
                }
            }
            2 => {
                case.lineage.actions[1].changes[0].observed_preimage = FileState::Absent;
                case.lineage.actions[1].changes[0].disposition = MutationDisposition::Created;
            }
            _ => {
                case.lineage.actions[0].changes[0].observed_preimage = file(&external, 0);
                case.lineage.actions[0].changes[0].disposition = MutationDisposition::Replaced;
            }
        }
        case.reseal_authentic_fixture();
        assert_conflict(&case, RestoreConflictReason::LineageDiscontinuity);
        case.current[0].state = FileState::Absent;
        assert_conflict(&case, RestoreConflictReason::LineageDiscontinuity);
    }
    let original = object(b"original");
    let external = object(b"external recreation");
    let final_blob = object(b"final");
    let o = file(&original, 0);
    let u = file(&external, 0);
    let last = file(&final_blob, 0);
    let case = Case::new(
        vec![entry("created.bin", o.clone())],
        vec![original, external, final_blob],
        vec![entry("created.bin", last.clone())],
        vec![
            action(
                1,
                vec![change(
                    "created.bin",
                    MutationDisposition::Deleted,
                    o,
                    FileState::Absent,
                )],
            ),
            action(
                2,
                vec![change(
                    "created.bin",
                    MutationDisposition::Replaced,
                    u,
                    last,
                )],
            ),
        ],
    );
    assert_conflict(&case, RestoreConflictReason::LineageDiscontinuity);
}

#[test]
fn fp08_lineage_uncertain_step_cannot_be_repaired_by_later_ack() {
    let base = creation_case();
    assert_eq!(base.plan().unwrap().operations.len(), 1);
    let mut case = base.clone();
    case.lineage.actions[0].outcome = MutationActionOutcome::Uncertain;
    case.lineage.actions[0].changes[0].disposition = MutationDisposition::Uncertain;
    case.lineage.actions[0].changes[0].acknowledged_postimage = None;
    case.reseal_authentic_fixture();
    assert_conflict(&case, RestoreConflictReason::Uncertain);
    case.current[0].state = FileState::Absent;
    assert_conflict(&case, RestoreConflictReason::Uncertain);
    let mut multi = base.clone();
    multi
        .checkpoint
        .entries
        .push(entry("other", FileState::Absent));
    multi.current.push(entry("other", FileState::Absent));
    multi.lineage.actions[0].outcome = MutationActionOutcome::Uncertain;
    multi.lineage.actions[0].changes.push(MutationChange {
        path: "other".into(),
        disposition: MutationDisposition::Uncertain,
        observed_preimage: FileState::Absent,
        acknowledged_postimage: None,
    });
    multi.lineage.manifest_digest = domain_digest(
        b"anima-filesystem-checkpoint-manifest-v1\0",
        &multi.checkpoint,
    );
    multi.reseal_authentic_fixture();
    let plan = multi.plan().unwrap();
    assert!(plan.operations.is_empty());
    assert_eq!(plan.conflicts.len(), 2);
    assert!(plan
        .conflicts
        .iter()
        .all(|conflict| conflict.reason == RestoreConflictReason::Uncertain));
}

#[test]
fn fp08_lineage_rejects_global_identity_or_sequence_lies() {
    let base = creation_case();
    assert_eq!(base.plan().unwrap().operations.len(), 1);
    for mutation in 0..12 {
        let mut case = base.clone();
        match mutation {
            0 => case.lineage.actions.swap(0, 1),
            1 => case.lineage.actions[1].sequence = 3,
            2 => case.lineage.actions[1].sequence = 1,
            3 => case.lineage.actions[1].action_id = case.lineage.actions[0].action_id.clone(),
            4 => {
                case.lineage.actions[1].native_tool_call_id =
                    case.lineage.actions[0].native_tool_call_id.clone()
            }
            5 => {
                let duplicate = case.lineage.actions[0].changes[0].clone();
                case.lineage.actions[0].changes.push(duplicate);
            }
            6 => {
                let mut alias = case.lineage.actions[0].changes[0].clone();
                alias.path = "CREATED.BIN".into();
                case.lineage.actions[0].changes.push(alias);
            }
            7 => case.lineage.scope_digest = "0".repeat(64),
            8 => case.lineage.checkpoint_id = "foreign".into(),
            9 => case.lineage.checkpoint_sequence += 1,
            10 => case.lineage.manifest_digest = "0".repeat(64),
            _ => case.lineage.actions[0].changes[0].observed_preimage = FileState::Absent,
        }
        if mutation == 11 {
            case.lineage.actions[0].changes[0].disposition = MutationDisposition::Replaced;
        }
        case.reseal_authentic_fixture();
        assert!(case.plan().is_err(), "accepted malformed case {mutation}");
    }
}

#[test]
fn fp08_lineage_requires_authentic_complete_head_and_real_preimages() {
    let base = creation_case();
    assert_eq!(base.plan().unwrap().operations.len(), 1);
    for mutation in 0..8 {
        let mut case = base.clone();
        match mutation {
            0 => {
                case.lineage.actions.pop();
                recompute_decoded_head(&mut case.lineage);
            }
            1 => {
                case.lineage.actions[0].changes.clear();
                recompute_decoded_head(&mut case.lineage);
            }
            2 => case.expected.action_bindings[0].native_run_id = "foreign-run".into(),
            3 => case.expected.action_bindings[0].native_tool_call_id = "foreign-call".into(),
            4 => case.expected.action_bindings[0].request_digest = "0".repeat(64),
            5 => case.expected.action_bindings[0].action_digest = "0".repeat(64),
            6 => case.lineage.version = 2,
            _ => case.objects.clear(),
        }
        assert!(
            case.plan().is_err(),
            "accepted unbound/incomplete case {mutation}"
        );
    }
    let mut missing = serde_json::to_value(&base.lineage).unwrap();
    missing["actions"][0]["changes"][0]
        .as_object_mut()
        .unwrap()
        .remove("observed_preimage");
    assert!(serde_json::from_value::<MutationLineage>(missing).is_err());
    let mut unknown = serde_json::to_value(&base.lineage).unwrap();
    unknown["force"] = serde_json::json!(true);
    assert!(serde_json::from_value::<MutationLineage>(unknown).is_err());
    let mut future = serde_json::to_value(&base.lineage).unwrap();
    future["version"] = serde_json::json!(2);
    assert!(
        serde_json::from_value::<MutationLineage>(future).is_err(),
        "future persisted lineage versions fail closed"
    );
    // The old API stays a separate one-witness contract; duplicate old entries
    // still cannot be silently promoted into an ordered, preimage-proven chain.
    let old: Vec<_> = base
        .lineage
        .actions
        .iter()
        .map(|action| super::super::MutationWitness {
            action_id: action.action_id.clone(),
            checkpoint_id: base.checkpoint.checkpoint_id.clone(),
            scope_digest: base.checkpoint.scope_digest.clone(),
            path: action.changes[0].path.clone(),
            disposition: action.changes[0].disposition,
            acknowledged_postimage: action.changes[0].acknowledged_postimage.clone().unwrap(),
        })
        .collect();
    assert!(super::super::prepare_safe_restore(
        &base.checkpoint,
        &scope(),
        &base.objects,
        &base.current,
        &old
    )
    .is_err());
}

#[test]
fn fp08_lineage_bounds_entire_input_before_object_resolution() {
    let base = creation_case();
    assert_eq!(base.plan().unwrap().operations.len(), 1);
    let blob = object(b"");
    let same = file(&blob, 0);
    let actions: Vec<_> = (1..=MAX_CHECKPOINT_LINEAGE_ACTIONS)
        .map(|sequence| {
            action(
                sequence as u64,
                vec![change(
                    "file",
                    MutationDisposition::Replaced,
                    same.clone(),
                    same.clone(),
                )],
            )
        })
        .collect();
    let exact = Case::new(
        vec![entry("file", same.clone())],
        vec![blob.clone()],
        vec![entry("file", same.clone())],
        actions,
    );
    let plan = exact.plan().unwrap();
    assert!(plan.operations.is_empty() && plan.conflicts.is_empty());
    let mut too_many = exact.clone();
    too_many.lineage.actions.push(action(
        51,
        vec![change(
            "file",
            MutationDisposition::Replaced,
            same.clone(),
            same,
        )],
    ));
    too_many.reseal_authentic_fixture();
    assert_eq!(too_many.plan(), Err(CheckpointError::Budget));
    let members: Vec<_> = (0..MAX_CHECKPOINT_LINEAGE_MEMBERS)
        .map(|index| entry(&format!("f{index:05}"), FileState::Absent))
        .collect();
    let changes = members
        .iter()
        .map(|member| MutationChange {
            path: member.path.clone(),
            disposition: MutationDisposition::Uncertain,
            observed_preimage: FileState::Absent,
            acknowledged_postimage: None,
        })
        .collect();
    let mut uncertain = action(1, changes);
    uncertain.outcome = MutationActionOutcome::Uncertain;
    let exact_members = Case::new(members.clone(), vec![], members, vec![uncertain]);
    assert!(
        serde_json::to_vec(&exact_members.lineage).unwrap().len() <= MAX_CHECKPOINT_MANIFEST_BYTES
    );
    let plan = exact_members.plan().unwrap();
    assert!(plan.operations.is_empty());
    assert_eq!(plan.conflicts.len(), MAX_CHECKPOINT_LINEAGE_MEMBERS);
    let mut excess_members = exact_members.clone();
    let mut extra = action(
        2,
        vec![MutationChange {
            path: "f00000".into(),
            disposition: MutationDisposition::Uncertain,
            observed_preimage: FileState::Absent,
            acknowledged_postimage: None,
        }],
    );
    extra.outcome = MutationActionOutcome::Uncertain;
    excess_members.lineage.actions.push(extra);
    excess_members.reseal_authentic_fixture();
    assert_eq!(excess_members.plan(), Err(CheckpointError::Budget));
    let mut encoded = Case::new(
        (0..60)
            .map(|i| entry(&format!("f{i:03}"), FileState::Absent))
            .collect(),
        vec![blob.clone()],
        (0..60)
            .map(|i| entry(&format!("f{i:03}"), FileState::Absent))
            .collect(),
        vec![],
    );
    let changes = encoded
        .checkpoint
        .entries
        .iter()
        .map(|entry| {
            let mut state = file(&blob, 0);
            if let FileState::File { metadata, .. } = &mut state {
                metadata.attributes[0].bytes = vec![255; 40_000];
            }
            MutationChange {
                path: entry.path.clone(),
                disposition: MutationDisposition::Uncertain,
                observed_preimage: state.clone(),
                acknowledged_postimage: Some(state),
            }
        })
        .collect();
    let mut uncertain = action(1, changes);
    uncertain.outcome = MutationActionOutcome::Uncertain;
    encoded.lineage.actions = vec![uncertain];
    encoded.reseal_authentic_fixture();
    assert!(serde_json::to_vec(&encoded.lineage).unwrap().len() > MAX_CHECKPOINT_MANIFEST_BYTES);
    assert_eq!(
        encoded.plan(),
        Err(CheckpointError::Budget),
        "no-op desired/current states cannot excuse oversized encoded evidence"
    );
    let mut declared = base.clone();
    declared.checkpoint.entries[0].state = FileState::Absent;
    declared.current[0].state = FileState::Absent;
    let mut state = file(&blob, 0);
    if let FileState::File { content, .. } = &mut state {
        content.sha256 = "a".repeat(64);
        content.byte_count = MAX_CHECKPOINT_STREAM_BYTES;
    }
    declared.lineage.actions = (1..=4)
        .map(|sequence| {
            let mut step = action(
                sequence,
                vec![MutationChange {
                    path: "created.bin".into(),
                    disposition: MutationDisposition::Uncertain,
                    observed_preimage: state.clone(),
                    acknowledged_postimage: Some(state.clone()),
                }],
            );
            step.outcome = MutationActionOutcome::Uncertain;
            step
        })
        .collect();
    declared.lineage.manifest_digest = domain_digest(
        b"anima-filesystem-checkpoint-manifest-v1\0",
        &declared.checkpoint,
    );
    declared.objects.clear();
    declared.reseal_authentic_fixture();
    assert!(8 * MAX_CHECKPOINT_STREAM_BYTES > MAX_CHECKPOINT_CONTENT_BYTES);
    assert_eq!(
        declared.plan(),
        Err(CheckpointError::Budget),
        "declared observation bytes are bounded before missing object lookup"
    );
}
