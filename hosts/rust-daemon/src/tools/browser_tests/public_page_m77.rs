use super::*;

const HEADING: &str = "Herman Melville - Moby-Dick";
const PARAGRAPH_SHA256: &str = "336b128e83163516a5da437a53c3e8b95ac565fa9a79fb2e721e1bb4a3c1bb2a";
const PARAGRAPH_BYTES: usize = 3568;
const PIN: &str = "3.225.83.162";
const CASE_ID: &str = "123e4567-e89b-42d3-a456-426614174000";
const NOW: u64 = 1_800_000_000_000;

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct M77ChildContract {
    version: u8,
    milestone: String,
    case_id: String,
    nonce: String,
    expires_at_ms: u64,
    public_ipv4_pin: String,
    operator_config_sha256: String,
    heading: String,
    paragraph_sha256: String,
    paragraph_utf8_bytes: usize,
}

struct M77Admission {
    contract: M77ChildContract,
    _config_guard: std::fs::File,
    _directory_guards: Vec<std::fs::File>,
}

fn m77_parse_child_contract(
    bytes: &[u8],
    now_ms: u64,
    expected_nonce: &str,
) -> Result<M77ChildContract, &'static str> {
    if bytes.is_empty() || bytes.len() > 4096 {
        return Err("invalid M77 contract");
    }
    let contract: M77ChildContract =
        serde_json::from_slice(bytes).map_err(|_| "invalid M77 contract")?;
    if contract.version != 1
        || contract.milestone != "M77"
        || !m64_uuid_v4(&contract.case_id)
        || !m64_lower_hex(&contract.nonce, 64)
        || contract.nonce != expected_nonce
        || !m64_lower_hex(&contract.operator_config_sha256, 64)
        || now_ms > 9_007_199_254_740_991
        || contract.expires_at_ms > 9_007_199_254_740_991
        || contract.expires_at_ms <= now_ms
        || contract.expires_at_ms - now_ms > 180_000
        || contract.public_ipv4_pin != PIN
        || contract.heading != HEADING
        || contract.paragraph_sha256 != PARAGRAPH_SHA256
        || contract.paragraph_utf8_bytes != PARAGRAPH_BYTES
        || validate_scope(&m77_scope()).is_err()
    {
        return Err("invalid M77 contract");
    }
    Ok(contract)
}

fn m77_check_operator_config(
    contract: &M77ChildContract,
    bytes: &[u8],
) -> Result<(), &'static str> {
    if bytes.is_empty()
        || bytes.len() > 64 * 1024
        || digest(bytes) != contract.operator_config_sha256
    {
        return Err("invalid M77 config");
    }
    let config: Value = serde_json::from_slice(bytes).map_err(|_| "invalid M77 config")?;
    let expected = json!([{"id":"m77-public-page", "template":m77_scope()}]);
    if config.get("schemaVersion") != Some(&json!(1)) || config.get("policies") != Some(&expected) {
        return Err("invalid M77 config");
    }
    Ok(())
}

fn m77_admit_child(
    case_dir: &std::path::Path,
    contract_path: &std::path::Path,
    now_ms: u64,
    expected_nonce: &str,
) -> Result<M77Admission, &'static str> {
    use std::io::{Read, Write};
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    if !case_dir.is_absolute()
        || contract_path != case_dir.join("case-contract.json")
        || case_dir.components().any(|part| {
            matches!(
                part,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        })
    {
        return Err("invalid M77 path");
    }
    let mut directory_guards = Vec::new();
    // Retain the checked directory identities as well as the config file.
    for path in case_dir.ancestors() {
        let metadata = std::fs::symlink_metadata(path).map_err(|_| "invalid M77 directory")?;
        if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
            return Err("invalid M77 directory");
        }
        let guard = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(path)
            .map_err(|_| "invalid M77 directory")?;
        if guard
            .metadata()
            .map_err(|_| "invalid M77 directory")?
            .file_attributes()
            & 0x400
            != 0
        {
            return Err("invalid M77 directory");
        }
        directory_guards.push(guard);
    }
    let canonical_case = std::fs::canonicalize(case_dir).map_err(|_| "invalid M77 directory")?;
    let read = |name: &str, maximum: u64| -> Result<(Vec<u8>, std::fs::File), &'static str> {
        let path = case_dir.join(name);
        let file = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .custom_flags(0x0020_0000)
            .open(&path)
            .map_err(|_| "invalid M77 file")?;
        let metadata = file.metadata().map_err(|_| "invalid M77 file")?;
        if !metadata.is_file()
            || metadata.file_attributes() & 0x400 != 0
            || metadata.len() > maximum
            || std::fs::canonicalize(&path)
                .map_err(|_| "invalid M77 file")?
                .parent()
                != Some(canonical_case.as_path())
        {
            return Err("invalid M77 file");
        }
        let mut bytes = Vec::new();
        (&file)
            .take(maximum + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "invalid M77 file")?;
        if bytes.is_empty() || bytes.len() as u64 != metadata.len() || bytes.len() as u64 > maximum
        {
            return Err("invalid M77 file");
        }
        Ok((bytes, file))
    };
    let (bytes, _contract_guard) = read("case-contract.json", 4096)?;
    let contract = m77_parse_child_contract(&bytes, now_ms, expected_nonce)?;
    if case_dir.file_name().and_then(|s| s.to_str()) != Some(contract.case_id.as_str()) {
        return Err("invalid M77 directory");
    }
    let (bytes, config_guard) = read("operator-config.json", 64 * 1024)?;
    m77_check_operator_config(&contract, &bytes)?;
    let mut marker = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(case_dir.join("child-started.json"))
        .map_err(|_| "M77 case already consumed")?;
    let bytes = serde_json::to_vec(&json!({"caseId":contract.case_id,"nonce":contract.nonce}))
        .map_err(|_| "invalid M77 marker")?;
    marker
        .write_all(&bytes)
        .and_then(|_| marker.sync_all())
        .map_err(|_| "invalid M77 marker")?;
    Ok(M77Admission {
        contract,
        _config_guard: config_guard,
        _directory_guards: directory_guards,
    })
}

fn m77_paragraph(observation: &BrowserObservation) -> String {
    observation
        .nodes
        .iter()
        .filter(|node| node.role == "text")
        .flat_map(|node| node.label.split_whitespace())
        .collect::<Vec<_>>()
        .join(" ")
}

fn m77_observation_matches(
    observation: &BrowserObservation,
    heading: &str,
    paragraph_sha256: &str,
    paragraph_utf8_bytes: usize,
) -> bool {
    let paragraph = m77_paragraph(observation);
    observation.route_id == "page"
        && observation.document_generation > 0
        && observation.artifacts.is_empty()
        && (observation.omitted_nodes == 0 || observation.shortened)
        && observation
            .nodes
            .iter()
            .filter(|node| node.role == "heading" && node.label == heading)
            .count()
            == 1
        && !paragraph.is_empty()
        && paragraph.len() == paragraph_utf8_bytes
        && digest(paragraph.as_bytes()) == paragraph_sha256
}

fn m77_scope() -> BrowserScope {
    let mut scope = m64_public_scope(PIN);
    scope.id = "m77-page-scope".into();
    scope.origin = "https://httpbin.org".into();
    scope.routes[0].path = "/html".into();
    scope
}

fn m77_test_config() -> Value {
    let mut config = m64_test_config();
    config["policies"] = json!([{"id":"m77-public-page", "template":m77_scope()}]);
    config
}

fn m77_contract(config: &[u8]) -> Value {
    json!({
        "version":1, "milestone":"M77", "caseId":CASE_ID,
        "nonce":"a".repeat(64), "expiresAtMs":NOW+60_000,
        "publicIpv4Pin":PIN, "operatorConfigSha256":digest(config),
        "heading":HEADING, "paragraphSha256":PARAGRAPH_SHA256,
        "paragraphUtf8Bytes":PARAGRAPH_BYTES
    })
}

#[test]
fn m77_valid_contract_and_exact_operator_policy_are_admitted_without_launch() {
    let bytes = serde_json::to_vec(&m77_test_config()).unwrap();
    let contract = m77_parse_child_contract(
        &serde_json::to_vec(&m77_contract(&bytes)).unwrap(),
        NOW,
        &"a".repeat(64),
    )
    .expect("the new exact M77 contract is supported");
    assert_eq!(contract.case_id, CASE_ID);
    assert_eq!(contract.heading, HEADING);
    assert_eq!(contract.paragraph_sha256, PARAGRAPH_SHA256);
    assert_eq!(contract.paragraph_utf8_bytes, PARAGRAPH_BYTES);
    assert!(m77_check_operator_config(&contract, &bytes).is_ok());
}

#[test]
fn m77_malformed_contract_and_changed_policy_are_refused() {
    let bytes = serde_json::to_vec(&m77_test_config()).unwrap();
    let valid = m77_contract(&bytes);
    for (key, value) in [
        ("version", json!(2)),
        ("milestone", json!("M64")),
        ("caseId", json!("not-a-uuid")),
        ("nonce", json!("A".repeat(64))),
        ("expiresAtMs", json!(NOW)),
        ("expiresAtMs", json!(NOW + 180_001)),
        ("expiresAtMs", json!(9_007_199_254_740_992u64)),
        ("publicIpv4Pin", json!("127.0.0.1")),
        ("publicIpv4Pin", json!("03.225.83.162")),
        ("operatorConfigSha256", json!("f".repeat(63))),
        ("heading", json!("Example Domain")),
        ("paragraphSha256", json!("b".repeat(64))),
        ("paragraphUtf8Bytes", json!(3567)),
        ("unexpected", json!(true)),
    ] {
        let mut changed = valid.clone();
        changed[key] = value;
        assert!(
            m77_parse_child_contract(&serde_json::to_vec(&changed).unwrap(), NOW, &"a".repeat(64))
                .is_err(),
            "{key}"
        );
    }
    assert!(
        m77_parse_child_contract(&serde_json::to_vec(&valid).unwrap(), NOW, &"b".repeat(64))
            .is_err()
    );
    assert!(m77_parse_child_contract(&vec![b' '; 4097], NOW, &"a".repeat(64)).is_err());
    // Direct deserialization isolates config refusal from contract admission.
    let contract: M77ChildContract = serde_json::from_value(valid).unwrap();
    assert!(m77_check_operator_config(&contract, b"{}").is_err());
    for (key, value) in [
        ("origin", json!("https://example.com")),
        ("routes", json!([{"id":"page","method":"GET","path":"/"}])),
        ("inputIds", json!(["input"])),
        ("publicIpv4Pin", json!("93.184.216.34")),
    ] {
        let mut changed = m77_test_config();
        changed["policies"][0]["template"][key] = value;
        let changed = serde_json::to_vec(&changed).unwrap();
        let mut rebound = contract.clone();
        rebound.operator_config_sha256 = digest(&changed);
        assert!(
            m77_check_operator_config(&rebound, &changed).is_err(),
            "{key}"
        );
    }
}

#[test]
fn m77_admission_locks_config_and_publishes_one_exclusive_marker() {
    let root = tempfile::tempdir().unwrap();
    let case_dir = root.path().join("live").join(CASE_ID);
    std::fs::create_dir_all(&case_dir).unwrap();
    let config_path = case_dir.join("operator-config.json");
    let contract_path = case_dir.join("case-contract.json");
    let bytes = serde_json::to_vec(&m77_test_config()).unwrap();
    std::fs::write(&config_path, &bytes).unwrap();
    std::fs::write(
        &contract_path,
        serde_json::to_vec(&m77_contract(&bytes)).unwrap(),
    )
    .unwrap();
    let admission = m77_admit_child(&case_dir, &contract_path, NOW, &"a".repeat(64))
        .expect("valid isolated M77 files are admitted without launching a worker");
    assert_eq!(admission.contract.case_id, CASE_ID);
    let marker: Value =
        serde_json::from_slice(&std::fs::read(case_dir.join("child-started.json")).unwrap())
            .unwrap();
    assert_eq!(marker["caseId"], CASE_ID);
    assert_eq!(marker["nonce"], "a".repeat(64));
    assert!(std::fs::write(&config_path, b"replacement").is_err());
    assert!(std::fs::rename(&config_path, case_dir.join("replacement.json")).is_err());
    assert!(m77_admit_child(&case_dir, &contract_path, NOW, &"a".repeat(64)).is_err());
    drop(admission);
}

#[test]
fn m77_missing_changed_or_foreign_child_files_do_not_consume_case() {
    let root = tempfile::tempdir().unwrap();
    let case_dir = root.path().join("live").join(CASE_ID);
    std::fs::create_dir_all(&case_dir).unwrap();
    let contract_path = case_dir.join("case-contract.json");
    let bytes = serde_json::to_vec(&m77_test_config()).unwrap();
    assert!(m77_admit_child(&case_dir, &contract_path, NOW, &"a".repeat(64)).is_err());
    std::fs::write(
        &contract_path,
        serde_json::to_vec(&m77_contract(&bytes)).unwrap(),
    )
    .unwrap();
    std::fs::write(case_dir.join("operator-config.json"), b"{}").unwrap();
    assert!(m77_admit_child(&case_dir, &contract_path, NOW, &"a".repeat(64)).is_err());
    std::fs::write(case_dir.join("operator-config.json"), bytes).unwrap();
    assert!(m77_admit_child(
        &case_dir,
        &root.path().join("case-contract.json"),
        NOW,
        &"a".repeat(64)
    )
    .is_err());
    assert!(!case_dir.join("child-started.json").exists());
}

fn m77_observation() -> BrowserObservation {
    BrowserObservation {
        current_url: None,
        document_generation: 1,
        route_id: "page".into(),
        nodes: vec![
            BrowserNode {
                node: BrowserNodeRef {
                    token: "heading-1".into(),
                },
                role: "heading".into(),
                label: HEADING.into(),
            },
            BrowserNode {
                node: BrowserNodeRef {
                    token: "text-1".into(),
                },
                role: "text".into(),
                label: "A complete\n synthetic   paragraph.".into(),
            },
        ],
        shortened: false,
        omitted_nodes: 0,
        artifacts: vec![],
    }
}

#[test]
fn m77_oracle_matches_whole_normalized_text_despite_structural_omissions() {
    let text = "A complete synthetic paragraph.";
    let mut observation = m77_observation();
    assert!(m77_observation_matches(
        &observation,
        HEADING,
        &digest(text.as_bytes()),
        text.len()
    ));
    observation.shortened = true;
    observation.omitted_nodes = 2;
    assert!(
        m77_observation_matches(&observation, HEADING, &digest(text.as_bytes()), text.len()),
        "unnamed structural omissions do not remove the complete target text"
    );
}

#[test]
fn m77_oracle_rejects_wrong_roles_duplicates_and_partial_target_text() {
    let text = "A complete synthetic paragraph.";
    let check = |observation: &BrowserObservation| {
        m77_observation_matches(observation, HEADING, &digest(text.as_bytes()), text.len())
    };
    let original = m77_observation();
    let mut value = original.clone();
    value.route_id = "other".into();
    assert!(!check(&value));
    let mut value = original.clone();
    value.document_generation = 0;
    assert!(!check(&value));
    let mut value = original.clone();
    value.nodes[0].role = "text".into();
    assert!(!check(&value));
    let mut value = original.clone();
    value.nodes[0].label = "Ordinary page label".into();
    assert!(!check(&value));
    let mut value = original.clone();
    value.nodes[1].role = "link".into();
    assert!(!check(&value));
    let mut value = original.clone();
    value.nodes.push(value.nodes[0].clone());
    assert!(!check(&value));
    let mut value = original.clone();
    value.nodes[1].label = "A complete synthetic".into();
    value.shortened = true;
    value.omitted_nodes = 1;
    assert!(!check(&value));
    let mut value = original.clone();
    value.nodes[1].label = format!("{text} Additional text.");
    assert!(!check(&value));
    let mut value = original.clone();
    value.nodes.remove(1);
    assert!(!check(&value));
    let mut value = original.clone();
    value.omitted_nodes = 1;
    assert!(!check(&value));
    assert!(!m77_observation_matches(
        &original,
        HEADING,
        &digest(text.as_bytes()),
        text.len() + 1
    ));
    assert!(!m77_observation_matches(
        &original,
        HEADING,
        &"0".repeat(64),
        text.len()
    ));
}

#[tokio::test]
#[ignore = "root-only one-shot M77 HTTPS content case; requires a fresh admitted release"]
async fn m77_windows_public_page_content_oracle() {
    use futures::FutureExt;
    use std::io::Write;
    use std::panic::AssertUnwindSafe;
    let started = tokio::time::Instant::now();
    let body_deadline = started + Duration::from_secs(160);
    let cleanup_deadline = started + Duration::from_secs(178);
    let case_dir = std::path::PathBuf::from(
        std::env::var_os("M77_CASE_DIR").expect("M77 case directory required"),
    );
    let contract_path = std::path::PathBuf::from(
        std::env::var_os("M77_CHILD_CONTRACT_FILE").expect("M77 child contract required"),
    );
    let nonce = std::env::var("M77_NONCE").expect("M77 nonce required");
    assert!(std::env::var_os("ANIMA_BROWSER_ACCEPTANCE_CONFIG_FILE").is_none());
    assert!(std::env::var_os("ANIMAOS_RS_BROWSER_CONFIG_FILE").is_none());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    // No setup, worker or browser is constructed before exclusive admission.
    // Keep the config and directory handles alive through every cleanup path.
    let admitted = m77_admit_child(&case_dir, &contract_path, now, &nonce)
        .expect("M77 frozen child contract admitted once");
    let model = ScriptedModel::new(vec![]);
    let mut cleanup_service = None;
    let mut lifecycle = None;
    let mut tool_tasks: Vec<Option<tokio::task::JoinHandle<TaskResult<Content>>>> = Vec::new();
    let mut result = json!({
        "version":1,"milestone":"M77","caseId":admitted.contract.case_id,"nonce":admitted.contract.nonce,
        "passed":false,"oracleMatched":false,"headingMatched":false,
        "paragraphSha256":null,"paragraphUtf8Bytes":null,"documentGeneration":null,"routeId":null,
        "shortened":null,"omittedNodes":null,"actionsUsed":null,"requestsUsed":null,"successfulReceipts":null,
        "modelCalls":0,"grantsUnchanged":false,"scopeClosed":false,"physicalLeaseHeld":true,
        "serviceShutdownJoined":false,"toolTasksSettled":false,"cleanupUncertain":true,
        "privateRoot":null,"nodePid":null
    });
    let outcome = tokio::time::timeout_at(
        body_deadline,
        AssertUnwindSafe(async {
            let setup = browser::config::load_startup_setup(&case_dir.join("operator-config.json"))
                .expect("M77 installed program hashes and policy revalidate");
            let base = MetadataFixture::with_tools_model(
                60_000,
                model.clone(),
                &["browser_navigate", "browser_observe"],
            )
            .await;
            let service = browser::BrowserService::new_with_factory(
                &base.state,
                base.coordinator.control_plane_transactions(),
                setup.policies.clone(),
                setup.factory.clone(),
            );
            cleanup_service = Some(service.clone());
            base.state.write().await.browser_service = Some(service.clone());
            let coordinator = base.coordinator.clone().with_browser(service.clone());
            let agent_id = base.run.agent_id.clone();
            let agent = base.state.read().await.get_agent(&agent_id).unwrap().state;
            lifecycle = Some((
                base.state.clone(),
                agent_id.clone(),
                base.run.run_id.clone(),
                agent.config.tools.clone(),
            ));
            base.state.write().await.approvals.set_policy(
                &agent_id,
                ApprovalPolicy::default().with(RiskClass::Exec, PolicyAction::Ask),
            );
            service
                .save(
                    &agent_id,
                    &base.run.run_id,
                    SaveBrowserScope {
                        profile_id: None,
                        policy_id: "m77-public-page".into(),
                        selected_input_ids: vec![],
                        enabled: true,
                        expected_version: None,
                    },
                )
                .await
                .expect("one ordinary run-owned public scope selected");
            let cancel = base
                .state
                .read()
                .await
                .live
                .runs()
                .control(&base.run.run_id)
                .unwrap()
                .cancel;
            let context = gated_context(&coordinator, &base.run, cancel.clone())
                .await
                .with_browser(Some(service.clone()))
                .with_action_journal(RunActionJournal::new(
                    &base.state,
                    coordinator.control_plane_transactions(),
                    base.run.clone(),
                    cancel,
                    None,
                ));
            let navigation = m64_allow(
                &context,
                &agent,
                &base.run,
                &coordinator,
                call(
                    "m77-navigate",
                    "browser_navigate",
                    json!({"routeId":"page"}),
                ),
                &mut tool_tasks,
            )
            .await;
            if let Some(witness) = service.private_witness(&base.run.run_id) {
                result["privateRoot"] = json!(witness.private_root);
                result["nodePid"] = json!(witness.node_pid);
            }
            let (_, navigation) = evidence(navigation);
            assert_eq!(navigation.route_id, "page");
            let (_, observed) = evidence(
                m64_allow(
                    &context,
                    &agent,
                    &base.run,
                    &coordinator,
                    call(
                        "m77-observe",
                        "browser_observe",
                        json!({"screenshot":false}),
                    ),
                    &mut tool_tasks,
                )
                .await,
            );
            let paragraph = m77_paragraph(&observed);
            let heading_matched = observed
                .nodes
                .iter()
                .filter(|node| node.role == "heading" && node.label == admitted.contract.heading)
                .count()
                == 1;
            let matched = m77_observation_matches(
                &observed,
                &admitted.contract.heading,
                &admitted.contract.paragraph_sha256,
                admitted.contract.paragraph_utf8_bytes,
            );
            // These are actual observation values, captured even when the oracle fails.
            result["oracleMatched"] = json!(matched);
            result["headingMatched"] = json!(heading_matched);
            result["paragraphSha256"] = json!(digest(paragraph.as_bytes()));
            result["paragraphUtf8Bytes"] = json!(paragraph.len());
            result["documentGeneration"] = json!(observed.document_generation);
            result["routeId"] = json!(observed.route_id);
            result["shortened"] = json!(observed.shortened);
            result["omittedNodes"] = json!(observed.omitted_nodes);
            let scope = service
                .get(&agent_id, &base.run.run_id)
                .await
                .unwrap()
                .unwrap();
            let successes = base
                .state
                .read()
                .await
                .runs
                .get(&base.run.run_id)
                .unwrap()
                .action_receipts
                .iter()
                .filter(|receipt| receipt.status == ActionStatus::Succeeded)
                .count();
            result["actionsUsed"] = json!(scope.actions_used);
            result["requestsUsed"] = json!(scope.requests_used);
            result["successfulReceipts"] = json!(successes);
            assert!(
                matched,
                "M77 complete target content differs from frozen oracle"
            );
            assert_eq!(scope.origin, "https://httpbin.org");
            assert!(scope.selected_input_ids.is_empty() && scope.artifacts.is_empty());
            assert_eq!(
                (scope.actions_used, scope.requests_used, successes),
                (2, 1, 2)
            );
            assert!(result["nodePid"].as_u64().is_some_and(|pid| pid > 0));
            let stopped = service
                .stop(&agent_id, &base.run.run_id, scope.version)
                .await
                .unwrap();
            result["scopeClosed"] = json!(stopped.state == BrowserScopeState::Closed);
            result["physicalLeaseHeld"] = json!(stopped.physical_lease_held);
            assert_eq!(stopped.state, BrowserScopeState::Closed);
            assert!(!stopped.physical_lease_held);
        })
        .catch_unwind(),
    )
    .await;

    // Retain shutdown's JoinHandle, and reserve the final portion of the single
    // case deadline for task joins and the immutable result, even after panic.
    let shutdown_joined = if let Some(service) = &cleanup_service {
        if let Some((_, _, run, _)) = &lifecycle {
            if let Some(witness) = service.private_witness(run) {
                result["privateRoot"] = json!(witness.private_root);
                result["nodePid"] = json!(witness.node_pid);
            }
        }
        let service = service.clone();
        let mut shutdown = tokio::spawn(async move {
            service.shutdown().await;
        });
        let deadline =
            (tokio::time::Instant::now() + Duration::from_secs(12)).min(cleanup_deadline);
        match tokio::time::timeout_at(deadline, &mut shutdown).await {
            Ok(joined) => joined.is_ok(),
            Err(_) => {
                shutdown.abort();
                let _ = tokio::time::timeout_at(cleanup_deadline, &mut shutdown).await;
                false
            }
        }
    } else {
        false
    };
    let mut tasks_settled = true;
    for slot in &mut tool_tasks {
        if let Some(mut task) = slot.take() {
            match tokio::time::timeout_at(cleanup_deadline, &mut task).await {
                Ok(joined) => {
                    if joined.is_err() {
                        tasks_settled = false;
                    }
                }
                Err(_) => {
                    tasks_settled = false;
                    task.abort();
                    let _ = tokio::time::timeout_at(cleanup_deadline, &mut task).await;
                }
            }
        }
    }
    // Final witnesses are gathered after shutdown and every owned tool join.
    // A failed/expired witness query remains uncertainty, never inferred success.
    let final_witness = tokio::time::timeout_at(cleanup_deadline, async {
        if let (Some(service), Some((state, agent, run, grants))) = (&cleanup_service, &lifecycle) {
            let scope = service.get(agent, run).await.ok().flatten();
            let guard = state.read().await;
            let unchanged = guard
                .agents
                .get(agent)
                .is_some_and(|value| value.config().tools == *grants);
            let successes = guard.runs.get(run).map(|value| {
                value
                    .action_receipts
                    .iter()
                    .filter(|receipt| receipt.status == ActionStatus::Succeeded)
                    .count()
            });
            Some((scope, unchanged, successes))
        } else {
            None
        }
    })
    .await;
    let witness_available = matches!(&final_witness, Ok(Some((Some(_), _, _))));
    if let Ok(Some((scope, unchanged, successes))) = final_witness {
        result["grantsUnchanged"] = json!(unchanged);
        result["successfulReceipts"] = json!(successes);
        if let Some(scope) = scope {
            result["scopeClosed"] = json!(scope.state == BrowserScopeState::Closed);
            result["physicalLeaseHeld"] = json!(scope.physical_lease_held);
            result["actionsUsed"] = json!(scope.actions_used);
            result["requestsUsed"] = json!(scope.requests_used);
        }
    }
    let cleanup_uncertain = !shutdown_joined
        || !tasks_settled
        || !witness_available
        || result["scopeClosed"] != json!(true)
        || result["physicalLeaseHeld"] != json!(false);
    result["modelCalls"] = json!(model.requests().len());
    result["serviceShutdownJoined"] = json!(shutdown_joined);
    result["toolTasksSettled"] = json!(tasks_settled);
    result["cleanupUncertain"] = json!(cleanup_uncertain);
    let passed = matches!(&outcome, Ok(Ok(())))
        && !cleanup_uncertain
        && result["oracleMatched"] == json!(true)
        && result["headingMatched"] == json!(true)
        && result["actionsUsed"] == json!(2)
        && result["requestsUsed"] == json!(1)
        && result["successfulReceipts"] == json!(2)
        && result["modelCalls"] == json!(0)
        && result["grantsUnchanged"] == json!(true);
    result["passed"] = json!(passed);
    let write_result = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(case_dir.join("case-result.json"))?;
        file.write_all(&serde_json::to_vec(&result)?)?;
        file.sync_all()
    })();
    // Preserve the original body panic after the failed result has been attempted.
    if let Ok(Err(panic)) = outcome {
        std::panic::resume_unwind(panic);
    }
    write_result.expect("M77 writes one immutable post-cleanup result");
    assert!(
        passed,
        "M77 content case or physical cleanup did not satisfy its frozen contract"
    );
}
