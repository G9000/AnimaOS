use super::*;

fn blob(bytes: &[u8]) -> CheckpointBlob {
    CheckpointBlob {
        reference: BlobReference {
            sha256: checkpoint_digest(bytes),
            byte_count: bytes.len() as u64,
        },
        bytes: bytes.to_vec(),
    }
}
fn metadata(permission: ContentBasicPermissionsV1) -> MetadataSnapshot {
    MetadataSnapshot {
        required: vec![MetadataKind::BasicPermissions],
        attributes: vec![MetadataAttribute {
            kind: MetadataKind::BasicPermissions,
            encoding_version: 1,
            bytes: serde_json::to_vec(&permission).unwrap(),
        }],
    }
}
fn file(object: &CheckpointBlob, permission: ContentBasicPermissionsV1) -> FileState {
    FileState::File {
        content: object.reference.clone(),
        metadata: metadata(permission),
    }
}
fn windows(object: &CheckpointBlob) -> FileState {
    file(
        object,
        ContentBasicPermissionsV1::Windows { readonly: false },
    )
}
fn entry(path: &str, state: FileState) -> CheckpointEntry {
    CheckpointEntry {
        path: path.into(),
        state,
    }
}
fn checkpoint(object: &CheckpointBlob, semantics: PathSemantics) -> FilesystemCheckpoint {
    let scope = CheckpointScope {
        owner_id: "owner".into(),
        workspace_id: "workspace".into(),
        environment_id: "private-test".into(),
        path_semantics: semantics,
        root_identity_digest: "1".repeat(64),
        policy_digest: checkpoint_digest(b"authentic configured ContentBasicV1 policy"),
    };
    let permission = match semantics {
        PathSemantics::WindowsInsensitive => ContentBasicPermissionsV1::Windows { readonly: false },
        PathSemantics::PosixSensitive => ContentBasicPermissionsV1::Posix { mode: 0o640 },
    };
    FilesystemCheckpoint {
        version: 1,
        checkpoint_id: "basic-cp".into(),
        sequence: 1,
        scope_digest: checkpoint_scope_digest(&scope),
        scope,
        entries: vec![entry("target", file(object, permission))],
    }
}
fn change(before: FileState, after: FileState, disposition: MutationDisposition) -> MutationChange {
    MutationChange {
        path: "target".into(),
        disposition,
        observed_preimage: before,
        acknowledged_postimage: Some(after),
    }
}
fn action(sequence: u64, change: MutationChange) -> MutationAction {
    MutationAction {
        sequence,
        action_id: format!("action-{sequence}"),
        native_run_id: "native-run".into(),
        native_tool_call_id: format!("native-call-{sequence}"),
        request_digest: checkpoint_digest(format!("request-{sequence}").as_bytes()),
        outcome: MutationActionOutcome::Acknowledged,
        changes: vec![change],
    }
}
fn evidence(
    cp: &FilesystemCheckpoint,
    actions: Vec<MutationAction>,
) -> (MutationLineage, ExpectedLineageHead) {
    let manifest_digest = checkpoint_manifest_digest(cp);
    let input = LineageHeadDigestInput {
        version: 1,
        checkpoint_id: cp.checkpoint_id.clone(),
        checkpoint_sequence: cp.sequence,
        manifest_digest: manifest_digest.clone(),
        scope_digest: cp.scope_digest.clone(),
        action_count: actions.len() as u32,
        last_sequence: actions.len() as u64,
        action_digests: actions.iter().map(checkpoint_action_digest).collect(),
    };
    let head_digest = checkpoint_lineage_head_digest(&input);
    let expected = ExpectedLineageHead {
        checkpoint_id: cp.checkpoint_id.clone(),
        checkpoint_sequence: cp.sequence,
        manifest_digest: manifest_digest.clone(),
        scope_digest: cp.scope_digest.clone(),
        action_count: input.action_count,
        last_sequence: input.last_sequence,
        head_digest: head_digest.clone(),
        action_bindings: actions
            .iter()
            .map(|action| ExpectedActionBinding {
                sequence: action.sequence,
                action_id: action.action_id.clone(),
                native_run_id: action.native_run_id.clone(),
                native_tool_call_id: action.native_tool_call_id.clone(),
                request_digest: action.request_digest.clone(),
                action_digest: checkpoint_action_digest(action),
            })
            .collect(),
    };
    (
        MutationLineage {
            version: 1,
            checkpoint_id: cp.checkpoint_id.clone(),
            checkpoint_sequence: cp.sequence,
            manifest_digest,
            scope_digest: cp.scope_digest.clone(),
            action_count: input.action_count,
            last_sequence: input.last_sequence,
            actions,
            head_digest,
        },
        expected,
    )
}
fn plan(
    cp: &FilesystemCheckpoint,
    objects: &[CheckpointBlob],
    current: FileState,
    actions: Vec<MutationAction>,
) -> Result<RestoreProposal, CheckpointError> {
    let (lineage, expected) = evidence(cp, actions);
    prepare_content_basic_v1_safe_restore_with_lineage(
        cp,
        &cp.scope,
        objects,
        &[entry("target", current)],
        &lineage,
        &expected,
    )
}
fn meta_mut(cp: &mut FilesystemCheckpoint) -> &mut MetadataSnapshot {
    match &mut cp.entries[0].state {
        FileState::File { metadata, .. } => metadata,
        _ => unreachable!(),
    }
}

#[test]
fn fp08_basic_profile_accepts_real_permissions_without_synthetic_time_and_keeps_legacy_strict() {
    let object = blob(b"\xff\0exact\r\n");
    let cp = checkpoint(&object, PathSemantics::WindowsInsensitive);
    assert_eq!(
        validate_content_basic_v1_filesystem_checkpoint(&cp, &cp.scope, &[object.clone()]),
        Ok(())
    );
    assert_eq!(
        validate_filesystem_checkpoint(&cp, &cp.scope, &[object.clone()]),
        Err(CheckpointError::InvalidMetadata)
    );
    assert_eq!(
        serde_json::to_vec(&ContentBasicPermissionsV1::Windows { readonly: false }).unwrap(),
        br#"{"platform":"windows","readonly":false}"#
    );
    let state = &cp.entries[0].state;
    let (lineage, expected) = evidence(&cp, vec![]);
    assert_eq!(
        prepare_safe_restore_with_lineage(
            &cp,
            &cp.scope,
            &[object],
            &[entry("target", state.clone())],
            &lineage,
            &expected
        ),
        Err(CheckpointError::InvalidMetadata)
    );
}

#[test]
fn fp08_basic_profile_rejects_extra_unknown_duplicate_or_noncanonical_metadata() {
    let object = blob(b"content");
    let valid = checkpoint(&object, PathSemantics::WindowsInsensitive);
    assert_eq!(
        validate_content_basic_v1_filesystem_checkpoint(&valid, &valid.scope, &[object.clone()]),
        Ok(())
    );
    for variation in 0..7 {
        let mut bad = valid.clone();
        let metadata = meta_mut(&mut bad);
        match variation {
            0 => metadata.attributes.push(MetadataAttribute {
                kind: MetadataKind::ModificationTime,
                encoding_version: 1,
                bytes: vec![0],
            }),
            1 => metadata.required.push(MetadataKind::ModificationTime),
            2 => metadata.attributes.push(metadata.attributes[0].clone()),
            3 => metadata.attributes[0].encoding_version = 2,
            4 => {
                metadata.attributes[0].bytes =
                    br#"{"platform":"windows","readonly":false,"extra":1}"#.to_vec()
            }
            5 => {
                metadata.attributes[0].bytes =
                    br#"{ "platform":"windows","readonly":false}"#.to_vec()
            }
            _ => metadata.required.clear(),
        }
        assert_eq!(
            validate_content_basic_v1_filesystem_checkpoint(&bad, &bad.scope, &[object.clone()]),
            Err(CheckpointError::InvalidMetadata)
        );
    }
    assert!(
        serde_json::from_str::<ContentBasicPermissionsV1>(r#"{"platform":"other","mode":0}"#)
            .is_err()
    );
}

#[test]
fn fp08_basic_profile_binds_permission_platform_to_authentic_expected_scope() {
    let object = blob(b"posix bytes");
    for semantics in [
        PathSemantics::PosixSensitive,
        PathSemantics::WindowsInsensitive,
    ] {
        let valid = checkpoint(&object, semantics);
        assert_eq!(
            validate_content_basic_v1_filesystem_checkpoint(
                &valid,
                &valid.scope,
                &[object.clone()]
            ),
            Ok(())
        );
        let mut bad = valid.clone();
        meta_mut(&mut bad).attributes[0].bytes = serde_json::to_vec(&match semantics {
            PathSemantics::WindowsInsensitive => ContentBasicPermissionsV1::Posix { mode: 0o640 },
            PathSemantics::PosixSensitive => ContentBasicPermissionsV1::Windows { readonly: false },
        })
        .unwrap();
        assert_eq!(
            validate_content_basic_v1_filesystem_checkpoint(&bad, &valid.scope, &[object.clone()]),
            Err(CheckpointError::InvalidMetadata)
        );
        let mut foreign = valid.scope.clone();
        foreign.policy_digest = checkpoint_digest(b"different configured profile");
        assert_eq!(
            validate_content_basic_v1_filesystem_checkpoint(&valid, &foreign, &[object.clone()]),
            Err(CheckpointError::InvalidScope)
        );
    }
    let mut bad = checkpoint(&object, PathSemantics::PosixSensitive);
    meta_mut(&mut bad).attributes[0].bytes =
        serde_json::to_vec(&ContentBasicPermissionsV1::Posix { mode: 0o10000 }).unwrap();
    assert_eq!(
        validate_content_basic_v1_filesystem_checkpoint(&bad, &bad.scope, &[object]),
        Err(CheckpointError::InvalidMetadata)
    );
}

#[test]
fn fp08_basic_profile_lineage_restores_exact_content_and_preserves_dirty_or_uncertain_current() {
    let original = blob(b"before\r\n\0");
    let after = blob(b"after");
    let dirty = blob(b"external");
    let cp = checkpoint(&original, PathSemantics::WindowsInsensitive);
    let objects = vec![original.clone(), after.clone(), dirty.clone()];
    let mutation = action(
        1,
        change(
            windows(&original),
            windows(&after),
            MutationDisposition::Replaced,
        ),
    );
    let proposal = plan(&cp, &objects, windows(&after), vec![mutation.clone()])
        .expect("genuine basic-profile lineage acceptance");
    assert_eq!(
        proposal.operations,
        vec![RestoreOperation {
            path: "target".into(),
            expected_current: windows(&after),
            desired: windows(&original)
        }]
    );
    let conflict = plan(&cp, &objects, windows(&dirty), vec![mutation.clone()]).unwrap();
    assert!(conflict.operations.is_empty());
    assert_eq!(conflict.conflicts[0].reason, RestoreConflictReason::Dirty);
    let mut uncertain = mutation;
    uncertain.outcome = MutationActionOutcome::Uncertain;
    uncertain.changes[0].acknowledged_postimage = None;
    let conflict = plan(&cp, &objects, windows(&after), vec![uncertain]).unwrap();
    assert!(conflict.operations.is_empty());
    assert_eq!(
        conflict.conflicts[0].reason,
        RestoreConflictReason::Uncertain
    );
}

#[test]
fn fp08_basic_profile_lineage_created_chain_requires_unbroken_preimages_before_delete() {
    let original = blob(b"creation");
    let after = blob(b"replacement");
    let external = blob(b"external intermediate");
    let mut cp = checkpoint(&original, PathSemantics::WindowsInsensitive);
    cp.entries[0].state = FileState::Absent;
    let objects = vec![original.clone(), after.clone(), external.clone()];
    let created = action(
        1,
        change(
            FileState::Absent,
            windows(&original),
            MutationDisposition::Created,
        ),
    );
    let replaced = action(
        2,
        change(
            windows(&original),
            windows(&after),
            MutationDisposition::Replaced,
        ),
    );
    let proposal = plan(
        &cp,
        &objects,
        windows(&after),
        vec![created.clone(), replaced.clone()],
    )
    .expect("unbroken genuine Created chain");
    assert_eq!(proposal.operations[0].desired, FileState::Absent);
    let mut broken = replaced;
    broken.changes[0].observed_preimage = windows(&external);
    let conflict = plan(&cp, &objects, windows(&after), vec![created, broken]).unwrap();
    assert!(conflict.operations.is_empty());
    assert_eq!(
        conflict.conflicts[0].reason,
        RestoreConflictReason::LineageDiscontinuity
    );
}

#[test]
fn fp08_basic_profile_lineage_requires_original_expected_head_and_bounds_all_input() {
    let original = blob(b"before");
    let after = blob(b"after");
    let cp = checkpoint(&original, PathSemantics::WindowsInsensitive);
    let objects = vec![original.clone(), after.clone()];
    let (lineage, expected) = evidence(
        &cp,
        vec![action(
            1,
            change(
                windows(&original),
                windows(&after),
                MutationDisposition::Replaced,
            ),
        )],
    );
    let current = vec![entry("target", windows(&after))];
    assert!(prepare_content_basic_v1_safe_restore_with_lineage(
        &cp, &cp.scope, &objects, &current, &lineage, &expected
    )
    .is_ok());
    let mut wrong = expected.clone();
    wrong.action_bindings[0].native_tool_call_id = "unrelated".into();
    assert_eq!(
        prepare_content_basic_v1_safe_restore_with_lineage(
            &cp, &cp.scope, &objects, &current, &lineage, &wrong
        ),
        Err(CheckpointError::InvalidWitness)
    );
    let mut oversized = lineage;
    oversized.actions[0].changes[0].observed_preimage = FileState::File {
        content: original.reference.clone(),
        metadata: MetadataSnapshot {
            required: vec![MetadataKind::BasicPermissions],
            attributes: vec![MetadataAttribute {
                kind: MetadataKind::BasicPermissions,
                encoding_version: 1,
                bytes: vec![b'x'; MAX_CHECKPOINT_MANIFEST_BYTES],
            }],
        },
    };
    assert_eq!(
        prepare_content_basic_v1_safe_restore_with_lineage(
            &cp,
            &cp.scope,
            &[],
            &current,
            &oversized,
            &expected
        ),
        Err(CheckpointError::Budget)
    );
}
