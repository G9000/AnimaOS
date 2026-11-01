use super::*;

fn original() -> Vec<u8> {
    b"\xef\xbb\xbf---\r\nname: URL notes\r\ndescription: Preserve original public source\r\nmetadata:\r\n  untrusted: true\r\n---\r\n\r\nUse the documented setup.\r\n".to_vec()
}

type FakeReply = (String, Result<(String, Vec<u8>), SkillError>);
struct FakeHttp(Mutex<VecDeque<FakeReply>>);
impl PublicHttp for FakeHttp {
    fn fetch(&self, url: Url, limit: usize) -> BoxFuture<'_, Result<Fetched, SkillError>> {
        Box::pin(async move {
            let (expected, reply) = self
                .0
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected source request");
            assert_eq!(url.as_str(), expected);
            let (final_url, bytes) = reply?;
            if bytes.len() > limit {
                return Err(unavailable("source byte limit"));
            }
            Ok(Fetched {
                final_url: Url::parse(&final_url).unwrap(),
                bytes,
            })
        })
    }
}
fn http(replies: Vec<FakeReply>) -> FakeHttp {
    FakeHttp(Mutex::new(replies.into()))
}
fn reply(request: &str, final_url: &str, bytes: Vec<u8>) -> FakeReply {
    (request.into(), Ok((final_url.into(), bytes)))
}
async fn url_package() -> UrlPackage {
    let url = "https://public.example.com/notes/SKILL.md";
    acquire_url(
        Url::parse(url).unwrap(),
        &[],
        &http(vec![reply(url, url, original())]),
    )
    .await
    .unwrap()
}

#[test]
fn source_requests_are_strict_camel_case_and_bounded() {
    let inspection: SourceInspectionRequest = serde_json::from_value(
        json!({"source":"example/skills", "supportingPaths":[], "maxChars":42}),
    )
    .unwrap();
    inspection.validate().unwrap();
    assert_eq!(inspection.max_chars, Some(42));
    assert!(serde_json::from_value::<SourceInspectionRequest>(
        json!({"source":"example/skills", "max_chars":42})
    )
    .is_err());
    assert!(serde_json::from_value::<SourceImportRequest>(
        json!({"source":"example/skills", "installer":"run"})
    )
    .is_err());
    for request in [
        SourceInspectionRequest::default(),
        SourceInspectionRequest {
            source: Some("http://public.example.com/SKILL.md".into()),
            ..Default::default()
        },
        SourceInspectionRequest {
            source: Some("example/skills".into()),
            max_chars: Some(MAX_PAGE_CHARS + 1),
            ..Default::default()
        },
        SourceInspectionRequest {
            inspection_id: Some("skill_inspection_fixture".into()),
            file_path: Some("../secret".into()),
            ..Default::default()
        },
    ] {
        assert!(request.validate().is_err(), "{request:?}");
    }
    assert!(SourceImportRequest {
        source: Some("example/skills".into()),
        slug: Some("BAD".into()),
        ..Default::default()
    }
    .validate()
    .is_err());
}

#[test]
fn source_normalizes_github_repo_tree_blob_and_explicit_slash_revision() {
    for (source, revision, expected_path) in [
        ("example/skills/notes", None, "notes"),
        ("https://github.com/example/skills", None, ""),
        (
            "https://github.com/example/skills/blob/main/README.md",
            Some("main"),
            "",
        ),
        (
            "https://github.com/example/skills/tree/feature/setup/notes",
            Some("feature/setup"),
            "notes",
        ),
        (
            "https://github.com/example/skills/blob/feature/setup/notes/SKILL.md",
            Some("feature/setup"),
            "notes",
        ),
    ] {
        let Selection::GitHub(candidates) = selection(source, revision, None).unwrap() else {
            panic!("GitHub source");
        };
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].repository, "example/skills");
        assert_eq!(candidates[0].path, expected_path);
        assert_eq!(candidates[0].revision.as_deref(), revision);
    }
    let Selection::GitHub(candidates) = selection(
        "https://github.com/example/skills/tree/feature/setup/notes",
        None,
        None,
    )
    .unwrap() else {
        panic!("GitHub source");
    };
    assert!(candidates
        .iter()
        .any(
            |candidate| candidate.revision.as_deref() == Some("feature/setup")
                && candidate.path == "notes"
        ));
    assert!(selection(
        "https://github.com/example/skills/tree/feature/setup/notes",
        Some("wrong/ref"),
        None
    )
    .is_err());
}

#[test]
fn direct_source_rejects_downgrades_private_hosts_traversal_credentials_and_encoded_escapes() {
    for url in [
        "http://public.example.com/SKILL.md",
        "https://127.0.0.1/SKILL.md",
        "https://10.0.0.1/SKILL.md",
        "https://[::1]/SKILL.md",
        "https://metadata.goog/SKILL.md",
        "https://internal/SKILL.md",
        "https://a.local/SKILL.md",
        "https://u:p@public.example.com/SKILL.md",
        "https://public.example.com/a/../SKILL.md",
        "https://public.example.com/%2e%2e/SKILL.md",
        "https://public.example.com/SKILL.md?token=x",
        "https://public.example.com/SKILL.md#part",
        "https://public.example.com:8080/SKILL.md",
        "https://public.example.com/README.md",
    ] {
        assert!(checked_document_url(url).is_err(), "{url}");
    }
    assert!(checked_document_url("https://public.example.com/notes/SKILL.md").is_ok());
    let root = Url::parse("https://public.example.com/notes/").unwrap();
    let document = root.join("SKILL.md").unwrap();
    assert!(referenced_paths("[escape](../outside.py)", &root, &document).is_err());
    assert!(referenced_paths("[escape](%2e%2e/outside.py)", &root, &document).is_err());
    assert_eq!(
        referenced_paths("[setup](./scripts/run.py#usage)", &root, &document).unwrap(),
        BTreeSet::from(["scripts/run.py".into()])
    );
}

#[tokio::test]
async fn url_acquisition_uses_final_root_captures_required_files_and_preserves_binary_bytes() {
    let requested = "https://public.example.com/original/SKILL.md";
    let final_url = "https://cdn.example.com/package/SKILL.md";
    let bytes = String::from_utf8(original())
        .unwrap()
        .replace(
            "Use the documented setup.",
            "Use [setup](scripts/setup.py) and [picture](assets/demo.png).",
        )
        .into_bytes();
    let binary = vec![0x89, b'P', b'N', b'G', 0, 0xff];
    let fake = http(vec![
        reply(requested, final_url, bytes.clone()),
        reply(
            "https://cdn.example.com/package/assets/demo.png",
            "https://cdn.example.com/package/assets/demo.png",
            binary.clone(),
        ),
        reply(
            "https://cdn.example.com/package/requirements.txt",
            "https://cdn.example.com/package/requirements.txt",
            b"httpx==0.28.1\n".to_vec(),
        ),
        reply(
            "https://cdn.example.com/package/scripts/setup.py",
            "https://cdn.example.com/package/scripts/setup.py",
            b"print('setup documented')\n".to_vec(),
        ),
    ]);
    let package = acquire_url(
        Url::parse(requested).unwrap(),
        &["requirements.txt".into()],
        &fake,
    )
    .await
    .unwrap();
    assert_eq!(package.source.requested_url, requested);
    assert_eq!(package.source.source_url, final_url);
    assert_eq!(package.source.coverage, "referenced_files");
    assert_eq!(package.bytes, bytes);
    assert_eq!(package.files[0].bytes().unwrap().as_ref(), binary);
    assert_eq!(
        package.source.package_hash,
        skill_package_hash(&bytes, &package.files)
    );
    assert_eq!(package.source.files[0].sha256, skill_hash(&bytes));
    assert!(fake.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn url_acquisition_follows_transitive_markdown_relative_to_final_documents_once() {
    let url = "https://public.example.com/package/SKILL.md";
    let main = String::from_utf8(original())
        .unwrap()
        .replace(
            "Use the documented setup.",
            "Use [setup](references/setup.md). [Usage](SKILL.md#usage)",
        )
        .into_bytes();
    let setup = b"Follow [installer](../../scripts/install.sh). See [self](setup.md#usage), [home](../../SKILL.md#usage), and [extra](../extra.md).".to_vec();
    let extra =
        b"Back to [setup](nested/setup.md) and [installer](../scripts/install.sh).".to_vec();
    let installer = b"printf 'documented install\\n'\n".to_vec();
    let fake = http(vec![
        reply(url, url, main.clone()),
        reply(
            "https://public.example.com/package/references/setup.md",
            "https://public.example.com/package/docs/nested/setup.md",
            setup.clone(),
        ),
        reply(
            "https://public.example.com/package/docs/extra.md",
            "https://public.example.com/package/docs/extra.md",
            extra,
        ),
        reply(
            "https://public.example.com/package/docs/nested/setup.md",
            "https://public.example.com/package/docs/nested/setup.md",
            setup,
        ),
        reply(
            "https://public.example.com/package/scripts/install.sh",
            "https://public.example.com/package/scripts/install.sh",
            installer.clone(),
        ),
    ]);
    let package = acquire_url(Url::parse(url).unwrap(), &[], &fake)
        .await
        .unwrap();
    assert_eq!(package.bytes, main);
    assert_eq!(package.files.len(), 4);
    let install = package
        .files
        .iter()
        .find(|file| file.path == "scripts/install.sh")
        .unwrap();
    assert_eq!(install.bytes().unwrap().as_ref(), installer.as_slice());
    assert!(package
        .source
        .files
        .iter()
        .any(|file| file.path == "references/setup.md"
            && file.source_url.ends_with("/docs/nested/setup.md")));
    assert!(
        fake.0.lock().unwrap().is_empty(),
        "cycles must not repeat file downloads"
    );
}

#[tokio::test]
async fn url_acquisition_ignores_self_navigation_but_requires_missing_transitive_files() {
    let url = "https://public.example.com/package/SKILL.md";
    let navigation = String::from_utf8(original())
        .unwrap()
        .replace(
            "Use the documented setup.",
            "[Usage](SKILL.md#usage), [home](./SKILL.md), [here](#usage).",
        )
        .into_bytes();
    let fake = http(vec![reply(url, url, navigation)]);
    assert!(acquire_url(Url::parse(url).unwrap(), &[], &fake)
        .await
        .unwrap()
        .files
        .is_empty());
    assert!(fake.0.lock().unwrap().is_empty());
    let main = String::from_utf8(original())
        .unwrap()
        .replace("Use the documented setup.", "[Setup](references/setup.md)")
        .into_bytes();
    let missing = http(vec![
        reply(url, url, main),
        reply(
            "https://public.example.com/package/references/setup.md",
            "https://public.example.com/package/references/setup.md",
            b"[Install](../scripts/missing.sh)".to_vec(),
        ),
        (
            "https://public.example.com/package/scripts/missing.sh".into(),
            Err(unavailable("missing required installer")),
        ),
    ]);
    assert!(acquire_url(Url::parse(url).unwrap(), &[], &missing)
        .await
        .is_err());
    assert!(missing.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn url_transitive_discovery_rejects_root_escape_and_global_file_overflow_before_requests() {
    let url = "https://public.example.com/package/SKILL.md";
    let main = String::from_utf8(original())
        .unwrap()
        .replace("Use the documented setup.", "[Setup](references/setup.md)")
        .into_bytes();
    for target in [
        "../../outside.sh",
        "../%2e%2e/outside.sh",
        "../scripts\\outside.sh",
    ] {
        let fake = http(vec![
            reply(url, url, main.clone()),
            reply(
                "https://public.example.com/package/references/setup.md",
                "https://public.example.com/package/references/setup.md",
                format!("[Install]({target})").into_bytes(),
            ),
        ]);
        assert!(
            acquire_url(Url::parse(url).unwrap(), &[], &fake)
                .await
                .is_err(),
            "{target}"
        );
        assert!(fake.0.lock().unwrap().is_empty());
    }
    let links: String = (0..MAX_SKILL_SUPPORTING_FILES)
        .map(|index| format!("[File](../scripts/{index}.sh)\n"))
        .collect();
    let fake = http(vec![
        reply(url, url, main),
        reply(
            "https://public.example.com/package/references/setup.md",
            "https://public.example.com/package/references/setup.md",
            links.into_bytes(),
        ),
    ]);
    assert!(acquire_url(Url::parse(url).unwrap(), &[], &fake)
        .await
        .is_err());
    assert!(
        fake.0.lock().unwrap().is_empty(),
        "global file budget must stop before child downloads"
    );
}

#[tokio::test]
async fn url_acquisition_fails_atomically_for_missing_files_unsafe_redirects_and_partial_packages()
{
    let url = "https://public.example.com/notes/SKILL.md";
    for final_url in [
        "http://public.example.com/SKILL.md",
        "https://127.0.0.1/SKILL.md",
    ] {
        assert!(acquire_url(
            Url::parse(url).unwrap(),
            &[],
            &http(vec![reply(url, final_url, original())])
        )
        .await
        .is_err());
    }
    let missing = http(vec![
        reply(url, url, original()),
        (
            "https://public.example.com/notes/scripts/setup.py".into(),
            Err(unavailable("not found")),
        ),
    ]);
    assert!(acquire_url(
        Url::parse(url).unwrap(),
        &["scripts/setup.py".into()],
        &missing
    )
    .await
    .is_err());
    let outside = http(vec![
        reply(url, url, original()),
        reply(
            "https://public.example.com/notes/scripts/setup.py",
            "https://public.example.com/outside.py",
            b"print('outside')".to_vec(),
        ),
    ]);
    assert!(acquire_url(
        Url::parse(url).unwrap(),
        &["scripts/setup.py".into()],
        &outside
    )
    .await
    .is_err());
    let excessive = http(vec![reply(url, url, vec![b'x'; MAX_SKILL_FILE_BYTES + 1])]);
    assert!(acquire_url(Url::parse(url).unwrap(), &[], &excessive)
        .await
        .is_err());
}

#[tokio::test]
async fn url_draft_stays_inactive_in_free_survives_restart_and_keeps_original_manifest_bytes() {
    use super::super::test_support::{service, temp_workspace, with_workspace};
    let root = temp_workspace("url-source-custody");
    let mut daemon = with_workspace(crate::state::DaemonState::new(), &root);
    let agent = daemon
        .create_agent(crate::agent_runs::test_support::companion_config(
            "URL owner",
        ))
        .unwrap()
        .state
        .id;
    daemon.start_free(&agent);
    let state = Arc::new(tokio::sync::RwLock::new(daemon));
    let package = url_package().await;
    let draft = service(&state)
        .import_package_from_url(
            package.bytes.clone(),
            None,
            package.files,
            package.source.clone(),
        )
        .await
        .unwrap();
    assert!(draft.is_pending());
    assert!(service(&state).index().await.is_empty());
    assert!(!root.join("skills").exists());
    let snapshot = state.read().await.control_plane_snapshot();
    let value = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(
        value["skillDrafts"][0]["urlSource"]["coverage"],
        "referenced_files"
    );
    let mut restarted = with_workspace(crate::state::DaemonState::new(), &root);
    restarted.restore_control_plane_snapshot(snapshot).unwrap();
    let state = Arc::new(tokio::sync::RwLock::new(restarted));
    let skills = service(&state);
    skills
        .approve_draft(&draft.id, super::super::DraftApproval::default())
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(root.join("skills/url-notes/SKILL.md")).unwrap(),
        package.bytes
    );
    let mut tampered = draft.clone();
    tampered.url_source.as_mut().unwrap().package_hash = "f".repeat(64);
    assert!(github::approved_bytes(&tampered, &tampered.body).is_err());
    tampered = draft;
    tampered.url_source.as_mut().unwrap().files[0].sha256 = "f".repeat(64);
    assert!(super::super::registry::SkillRegistry::validate(&[], &[tampered]).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn url_import_cannot_replace_a_base_changed_during_review() {
    use super::super::test_support::{content, service, temp_workspace, with_workspace};
    let root = temp_workspace("url-source-stale");
    let state = Arc::new(tokio::sync::RwLock::new(with_workspace(
        crate::state::DaemonState::new(),
        &root,
    )));
    let skills = service(&state);
    skills
        .save("url-notes", content("Original notes"))
        .await
        .unwrap();
    let package = url_package().await;
    let draft = skills
        .import_package_from_url(package.bytes, None, package.files, package.source)
        .await
        .unwrap();
    skills
        .save("url-notes", content("Later owner notes"))
        .await
        .unwrap();
    assert!(skills
        .approve_draft(&draft.id, super::super::DraftApproval::default())
        .await
        .unwrap_err()
        .message()
        .contains("base changed"));
    assert_eq!(
        skills.load_slug("url-notes").await.unwrap().name,
        "Later owner notes"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn captured_pages_and_imports_recheck_owner_policy_stop_workspace_profile_and_expiry() {
    use super::super::test_support::{service, temp_workspace, with_workspace};
    use crate::approvals::{ApprovalPolicy, PolicyAction, RiskClass};
    use crate::runs::{RunLink, RunRecord, RunSource, RunStart};
    use crate::sessions::{SessionKind, SessionOrigin, SessionRecord, TitleSource};
    let root = temp_workspace("url-source-live-scope");
    let mut daemon = with_workspace(crate::state::DaemonState::new(), &root);
    let agent = daemon
        .create_agent(crate::agent_runs::test_support::companion_config(
            "Source owner",
        ))
        .unwrap()
        .state
        .id;
    daemon.approvals.set_policy(
        &agent,
        ApprovalPolicy::default()
            .with(RiskClass::Network, PolicyAction::Allow)
            .with(RiskClass::Write, PolicyAction::Allow),
    );
    daemon.sessions.insert(SessionRecord::new(
        &agent,
        "chat:source-owner",
        SessionKind::Chat,
        SessionOrigin::Api,
        "Review source".into(),
        TitleSource::FirstMessage,
        1,
    ));
    let record = RunRecord::running(
        RunStart {
            agent_id: agent.clone(),
            session_id: "chat:source-owner".into(),
            source: RunSource::Api,
            source_ref: None,
            idempotency_key: None,
            text: "Review source".into(),
            model: "fixture".into(),
            provider: None,
            parent_run_id: None,
        },
        1,
    );
    let run = RunLink {
        agent_id: agent.clone(),
        session_id: record.session_id.clone(),
        run_id: record.id.clone(),
    };
    let control = daemon.live.runs().register(&record.id);
    daemon.runs.insert(record);
    let state = Arc::new(tokio::sync::RwLock::new(daemon));
    let skills = service(&state);
    let package = url_package().await;
    let capture = insert_capture(CapturedSource {
        id: format!("skill_inspection_{}", uuid::Uuid::new_v4()),
        state: Arc::downgrade(&state),
        root: root.clone(),
        profile_paths: vec![],
        agent_id: agent.clone(),
        session_id: run.session_id.clone(),
        requested_source: package.source.requested_url.clone(),
        requested_revision: None,
        requested_path: None,
        supporting_paths: vec![],
        captured_at_ms: now_millis(),
        expires: Instant::now() + CAPTURE_TTL,
        data: CapturedData::Url(package),
    })
    .unwrap();
    let request = SourceInspectionRequest {
        inspection_id: Some(capture.id.clone()),
        file_path: Some("SKILL.md".into()),
        max_chars: Some(10),
        ..Default::default()
    };
    let call = anima_core::ToolCall {
        id: "inspect".into(),
        name: "inspect_skill_source".into(),
        args: Default::default(),
    };
    let first = skills
        .inspect_source(request.clone(), run.clone(), call.clone(), true)
        .await
        .unwrap();
    let second = skills
        .inspect_source(
            SourceInspectionRequest {
                offset: 10,
                ..request.clone()
            },
            run.clone(),
            call.clone(),
            true,
        )
        .await
        .unwrap();
    assert_eq!(first["inspectionId"], second["inspectionId"]);
    assert_eq!(first["sha256"], second["sha256"]);
    assert_ne!(first["text"], second["text"]);
    assert!(skills
        .inspect_source(
            SourceInspectionRequest {
                source: Some("https://other.example.com/SKILL.md".into()),
                ..request.clone()
            },
            run.clone(),
            call.clone(),
            true
        )
        .await
        .is_err());
    let other_state = Arc::new(tokio::sync::RwLock::new(with_workspace(
        crate::state::DaemonState::new(),
        &root,
    )));
    assert!(service(&other_state)
        .inspect_source(request.clone(), run.clone(), call.clone(), true)
        .await
        .is_err());
    state.write().await.workspace.as_mut().unwrap().root_path = root.join("moved");
    assert!(skills
        .inspect_source(request.clone(), run.clone(), call.clone(), true)
        .await
        .is_err());
    state.write().await.workspace.as_mut().unwrap().root_path = root.clone();
    state.write().await.approvals.set_policy(
        &agent,
        ApprovalPolicy::default().with(RiskClass::Network, PolicyAction::Deny),
    );
    assert!(skills
        .inspect_source(request.clone(), run.clone(), call.clone(), true)
        .await
        .is_err());
    state.write().await.approvals.set_policy(
        &agent,
        ApprovalPolicy::default()
            .with(RiskClass::Network, PolicyAction::Allow)
            .with(RiskClass::Write, PolicyAction::Allow),
    );
    let mut expired = (*capture).clone();
    expired.id = format!("skill_inspection_{}", uuid::Uuid::new_v4());
    expired.expires = Instant::now() - Duration::from_secs(1);
    let expired = insert_capture(expired).unwrap();
    assert!(skills
        .inspect_source(
            SourceInspectionRequest {
                inspection_id: Some(expired.id.clone()),
                ..Default::default()
            },
            run.clone(),
            call.clone(),
            true
        )
        .await
        .is_err());
    // A caller dropped while queued must revoke only that call, even though
    // its owner run is still live. Wait for the detached worker's owned mutex
    // acquisition, rather than relying on one scheduling yield.
    let transaction = skills.transactions.clone().lock_owned().await;
    let importer = skills.clone();
    let held = Arc::strong_count(&skills.transactions);
    let importing_run = run.clone();
    let importing_id = capture.id.clone();
    let waiting = tokio::spawn(async move {
        importer
            .import_source(
                SourceImportRequest {
                    inspection_id: Some(importing_id),
                    ..Default::default()
                },
                importing_run,
                anima_core::ToolCall {
                    id: "drop-import".into(),
                    name: "import_skill".into(),
                    args: Default::default(),
                },
                true,
            )
            .await
    });
    wait_for_source_transaction(&skills, held).await;
    waiting.abort();
    assert!(waiting.await.unwrap_err().is_cancelled());
    drop(transaction);
    // Mutex acquisition is FIFO, so this waits behind the abandoned worker.
    drop(skills.transactions.clone().lock_owned().await);
    assert!(state.read().await.skills.pending_drafts().is_empty());

    let transaction = skills.transactions.clone().lock_owned().await;
    let importer = skills.clone();
    let held = Arc::strong_count(&skills.transactions);
    let importing_run = run.clone();
    let importing_id = capture.id.clone();
    let budget = Duration::from_millis(250);
    let waiting = tokio::spawn(async move {
        importer
            .import_source_with_budget(
                SourceImportRequest {
                    inspection_id: Some(importing_id),
                    ..Default::default()
                },
                importing_run,
                anima_core::ToolCall {
                    id: "deadline-import".into(),
                    name: "import_skill".into(),
                    args: Default::default(),
                },
                true,
                budget,
            )
            .await
    });
    wait_for_source_transaction(&skills, held).await;
    tokio::time::sleep(budget + Duration::from_millis(50)).await;
    let replied_before_transaction = waiting.is_finished();
    drop(transaction);
    assert!(waiting.await.unwrap().is_err());
    assert!(
        !replied_before_transaction,
        "a mutation timeout must not detach queued work"
    );
    assert!(state.read().await.skills.pending_drafts().is_empty());
    let transaction = skills.transactions.clone().lock_owned().await;
    let importer = skills.clone();
    let importing_run = run.clone();
    let importing_id = capture.id.clone();
    let waiting = tokio::spawn(async move {
        importer
            .import_source(
                SourceImportRequest {
                    inspection_id: Some(importing_id),
                    ..Default::default()
                },
                importing_run,
                anima_core::ToolCall {
                    id: "import".into(),
                    name: "import_skill".into(),
                    args: Default::default(),
                },
                true,
            )
            .await
    });
    tokio::task::yield_now().await;
    control.cancel.cancel();
    drop(transaction);
    assert!(waiting.await.unwrap().is_err());
    assert!(state.read().await.skills.pending_drafts().is_empty());
    assert!(skills
        .inspect_source(request, run, call, true)
        .await
        .is_err());
    std::fs::remove_dir_all(root).unwrap();
}

async fn wait_for_source_transaction(skills: &SkillService, held: usize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        // The scoped service, locked worker and pending owned lock each add
        // one reference after the test-side clones were counted.
        while Arc::strong_count(&skills.transactions) < held + 3 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("source import reached the held transaction");
}
