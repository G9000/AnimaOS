//! Pure protocol/factory/file proofs plus an explicit root-only real Node Job witness.
//! The ignored helper is not browser compatibility or page/egress acceptance.
use super::*;
use anima_service_adapters::browser::{
    BrowserArtifactKind, BrowserLimits, BrowserMethod, BrowserRoute,
};
use std::path::{Path, PathBuf};

fn scope() -> BrowserScope {
    BrowserScope {
        id: "scope_1".into(),
        revision: 1,
        origin: "http://127.0.0.1:43220".into(),
        public_ipv4_pin: None,
        authenticated: false,
        interactive: None,
        routes: vec![BrowserRoute {
            id: "home".into(),
            method: BrowserMethod::Get,
            path: "/".into(),
        }],
        input_ids: vec!["input_1".into()],
        limits: BrowserLimits {
            actions: 40,
            requests: 80,
            scope_ms: 120000,
            frame_bytes: 65536,
            wire_bytes: 1048576,
            observation_nodes: 128,
            observation_text_bytes: 8192,
            input_bytes: 16384,
            output_bytes: 16384,
            screenshot_bytes: 262144,
        },
    }
}
fn prepared(installation: &VerifiedBrowserInstallation) -> PreparedBrowserTask {
    let bytes: Arc<[u8]> = Arc::from(&b"quantity,max_delivery_days\n3,2\n"[..]);
    PreparedBrowserTask {
        profile: None,
        scope: scope(),
        policy_id: "policy_1".into(),
        policy_digest: "a".repeat(64),
        installation_id: installation.id.clone(),
        installation_digest: installation.digest.clone(),
        expires_at_ms: now() + 30000,
        inputs: vec![VerifiedInput {
            id: "input_1".into(),
            sha256: super::super::files::hash(&bytes),
            bytes,
        }],
        outputs: vec![
            OutputSlot {
                id: "csv_1".into(),
                kind: BrowserArtifactKind::Csv,
            },
            OutputSlot {
                id: "csv_2".into(),
                kind: BrowserArtifactKind::Csv,
            },
            OutputSlot {
                id: "shot_1".into(),
                kind: BrowserArtifactKind::Screenshot,
            },
            OutputSlot {
                id: "shot_2".into(),
                kind: BrowserArtifactKind::Screenshot,
            },
        ],
    }
}
fn installation(root: &Path) -> VerifiedBrowserInstallation {
    let node = root.join("node.exe");
    let worker = root.join("worker.mjs");
    let browser = root.join("browser.exe");
    for path in [&node, &worker, &browser] {
        std::fs::write(path, b"pure verification fixture; never executed").unwrap();
    }
    let hash = super::super::files::hash(b"pure verification fixture; never executed");
    VerifiedBrowserInstallation {
        id: "installation_1".into(),
        digest: "b".repeat(64),
        node_executable: node,
        node_sha256: hash.clone(),
        worker_script: worker,
        worker_sha256: hash.clone(),
        browser_executable: browser,
        browser_sha256: hash,
    }
}
fn code<T>(result: Result<T, BrowserError>) -> BrowserErrorCode {
    match result {
        Err(failure) => failure.code,
        Ok(_) => panic!("expected a bounded static failure"),
    }
}
fn control() -> BrowserControl {
    BrowserControl {
        cancel: anima_core::CancelSignal::new(),
        deadline: std::time::Instant::now() + Duration::from_secs(10),
    }
}
fn budget(maximum: usize) -> Arc<Budget> {
    Arc::new(Budget {
        used: AtomicUsize::new(0),
        maximum,
        inner: Weak::new(),
    })
}

#[test]
fn m46_transport_strict_json_rejects_recursive_duplicates_null_unsafe_ids_and_limits() {
    for input in [
        br#"{"id":1,"id":2}"#.as_slice(),
        br#"{"nested":{"key":1,"k\u0065y":2}}"#,
        br#"{"node":{"value":null}}"#,
        br#"{"id":9007199254740992}"#,
        b"{} true",
        &[0xff],
    ] {
        assert!(strict_decode::<Value>(input).is_err());
    }
    let deep = format!("{}0{}", "[".repeat(9), "]".repeat(9));
    assert_eq!(
        code(strict_decode::<Value>(deep.as_bytes())),
        BrowserErrorCode::LimitsExceeded
    );
    let nodes = serde_json::to_vec(&vec![true; 2048]).unwrap();
    assert_eq!(
        code(strict_decode::<Value>(&nodes)),
        BrowserErrorCode::LimitsExceeded
    );
    assert_eq!(
        code(strict_decode::<Value>(&vec![b' '; 65537])),
        BrowserErrorCode::LimitsExceeded
    );
    assert_eq!(
        canonical_digest(&scope()).unwrap(),
        scope_digest(&scope()).unwrap()
    );
}
#[tokio::test]
async fn m46_transport_partial_frames_eof_bad_magic_and_size_fail_before_payload_allocation() {
    use tokio::io::AsyncWriteExt;
    let (mut writer, reader) = tokio::io::duplex(65536);
    let (tx, mut rx) = mpsc::channel(2);
    let stop = CancellationToken::new();
    let bytes = encoded(1, "open", json!({}), 65536).unwrap();
    let ledger = budget(1048576);
    let pump = tokio::spawn(stdout_pump(reader, tx, ledger.clone(), stop.clone(), 65536));
    for piece in bytes.chunks(3) {
        writer.write_all(piece).await.unwrap();
    }
    let frame = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!((frame.id, frame.op.as_str()), (1, "open"));
    assert_eq!(ledger.used.load(Ordering::SeqCst), bytes.len());
    drop(writer);
    tokio::time::timeout(Duration::from_secs(1), pump)
        .await
        .unwrap()
        .unwrap();
    assert!(stop.is_cancelled());
    for bytes in [
        b"ABW1".to_vec(),
        [b"ABW1".as_slice(), &u32::MAX.to_be_bytes()].concat(),
        b"NOPE0000".to_vec(),
    ] {
        let (mut writer, reader) = tokio::io::duplex(64);
        let (tx, _rx) = mpsc::channel(2);
        let stop = CancellationToken::new();
        let pump = tokio::spawn(stdout_pump(
            reader,
            tx,
            budget(1048576),
            stop.clone(),
            65536,
        ));
        writer.write_all(&bytes).await.unwrap();
        drop(writer);
        tokio::time::timeout(Duration::from_secs(1), pump)
            .await
            .unwrap()
            .unwrap();
        assert!(stop.is_cancelled());
    }
}
#[tokio::test]
async fn m46_transport_stderr_is_discarded_with_a_fixed_total_bound() {
    for size in [65536usize, 65537] {
        let (mut writer, reader) = tokio::io::duplex(65537);
        let stop = CancellationToken::new();
        let pump = tokio::spawn(stderr_pump(reader, stop.clone(), Weak::new()));
        writer.write_all(&vec![b'x'; size]).await.unwrap();
        drop(writer);
        tokio::time::timeout(Duration::from_secs(1), pump)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stop.is_cancelled(), size > 65536);
    }
}
#[test]
fn m46_transport_reply_identity_order_and_shared_wire_budget_are_strict() {
    let ledger = budget(10);
    ledger.consume(6).unwrap();
    assert!(ledger.consume(5).is_err());
    assert_eq!(
        ledger.used.load(Ordering::SeqCst),
        11,
        "rejected raw bytes remain counted"
    );
    assert_eq!(
        code(encoded(
            MAX_BROWSER_SAFE_INTEGER + 1,
            "open",
            json!({}),
            65536
        )),
        BrowserErrorCode::Protocol
    );
    assert_eq!(
        code(encoded(1, "open", json!({"x":"a".repeat(65536)}), 65536)),
        BrowserErrorCode::LimitsExceeded
    );
    let ledger = budget(1048576);
    ledger.consume(1000).unwrap();
    let wrong = Envelope {
        protocol: 1,
        id: 2,
        op: "open".into(),
        payload: json!({"ok":true,"result":{}}),
    };
    assert_eq!(
        code(reply_payload(
            wrong,
            1,
            "open",
            &scope(),
            BrowserCounters::default(),
            &ledger
        )),
        BrowserErrorCode::Protocol
    );
    let invalid = Envelope {
        protocol: 1,
        id: 1,
        op: "open".into(),
        payload: json!({"ok":false,"error":{"code":"denied","effect":"notDispatched"},"counters":{"actionsUsed":0,"requestsUsed":0,"wireBytesUsed":0},"unknown":true}),
    };
    assert_eq!(
        code(reply_payload(
            invalid,
            1,
            "open",
            &scope(),
            BrowserCounters::default(),
            &ledger
        )),
        BrowserErrorCode::Protocol
    );
    let old = BrowserCounters {
        actions_used: 2,
        requests_used: 3,
        wire_bytes_used: 100,
    };
    assert!(validate_counters(&scope(), old, BrowserCounters::default(), 1000).is_err());
}
#[test]
fn m46_transport_expected_witness_never_uses_newer_cached_document() {
    let old = BrowserDocumentWitness {
        document_id: "doc_1".into(),
        document_generation: 1,
        observation_digest: "a".repeat(64),
    };
    let newer = BrowserDocumentWitness {
        document_generation: 2,
        ..old.clone()
    };
    let click = BrowserOperation::Click {
        node: anima_service_adapters::browser::BrowserNodeRef {
            token: "node_1".into(),
        },
    };
    assert_eq!(
        code(admitted_witness(&click, &Some(old.clone()), &Some(newer))),
        BrowserErrorCode::Stale
    );
    admitted_witness(&click, &Some(old.clone()), &Some(old)).unwrap();
    assert!(admitted_witness(&click, &None, &None).is_err());
    admitted_witness(
        &BrowserOperation::Observe { screenshot: false },
        &None,
        &None,
    )
    .unwrap();
}
#[tokio::test]
async fn m46_transport_bind_is_pure_immutable_and_close_before_open_is_permanent() {
    let root = tempfile::tempdir().unwrap();
    let install = installation(root.path());
    let runtime = Arc::new(BrowserPhysicalRuntime::default());
    let task = prepared(&install);
    let factory = PlaywrightTransportFactory::new(install.clone(), runtime.clone());
    std::fs::remove_file(&install.worker_script).unwrap();
    let bound = factory.bind(task.clone()).unwrap();
    assert_eq!(runtime.child_count(), 0);
    assert_eq!(bound.handle.close().await, BrowserClose::Closed);
    let stopped = bound.port.open(task.scope.clone(), control()).await;
    assert_eq!(code(stopped), BrowserErrorCode::Cancelled);
    assert_eq!(runtime.child_count(), 0);
    let mut changed = task.clone();
    changed.inputs[0].bytes = Arc::from(&b"changed"[..]);
    assert_eq!(code(factory.bind(changed)), BrowserErrorCode::Invalid);
    let bound = factory.bind(task.clone()).unwrap();
    let mut scope = task.scope;
    scope.revision += 1;
    assert_eq!(
        code(bound.port.open(scope, control()).await),
        BrowserErrorCode::Stale
    );
    assert_eq!(runtime.child_count(), 0);
}
#[test]
fn m46_transport_private_outputs_are_fixed_hashed_bounded_and_link_guarded() {
    let root = tempfile::tempdir().unwrap();
    let install = installation(root.path());
    let task = prepared(&install);
    let private = PrivateFiles::create(&task.scope, &task.inputs, &task.outputs).unwrap();
    let data = b"bounded generated CSV\n";
    let output = &private.outputs[0];
    std::fs::write(&output.file_path, data).unwrap();
    let artifact = BrowserArtifact {
        id: output.id.clone(),
        kind: output.kind,
        bytes: data.len() as u64,
        sha256: super::super::files::hash(data),
    };
    assert_eq!(
        private
            .read_output(&task.scope, &artifact)
            .unwrap()
            .bytes
            .as_ref(),
        data
    );
    assert_eq!(
        task.inputs[0].bytes.as_ref(),
        b"quantity,max_delivery_days\n3,2\n"
    );
    let mut wrong = artifact.clone();
    wrong.sha256 = "f".repeat(64);
    assert!(private.read_output(&task.scope, &wrong).is_err());
    wrong = artifact.clone();
    wrong.id = "foreign_slot".into();
    assert!(private.read_output(&task.scope, &wrong).is_err());
    std::fs::remove_file(&output.file_path).unwrap();
    let alias = root.path().join("alias.csv");
    std::fs::write(&alias, data).unwrap();
    std::fs::hard_link(&alias, &output.file_path).unwrap();
    assert!(private.read_output(&task.scope, &artifact).is_err());
}
#[test]
fn m46_transport_synthetic_quarantine_keeps_the_shared_slot_and_owned_directory() {
    let root = tempfile::tempdir().unwrap();
    let install = installation(root.path());
    let task = prepared(&install);
    let runtime = Arc::new(BrowserPhysicalRuntime::default());
    let permit = runtime.permit.clone().try_acquire_owned().unwrap();
    let files = Arc::new(PrivateFiles::create(&task.scope, &task.inputs, &task.outputs).unwrap());
    let path = files.root().to_owned();
    drop(Lease {
        runtime: runtime.clone(),
        reservation: Some(Reservation {
            _permit: permit,
            files,
            _profile: None,
        }),
        inner: Weak::new(),
    });
    assert_eq!(runtime.child_count(), 1);
    assert!(runtime.permit.clone().try_acquire_owned().is_err());
    assert!(path.is_dir());
}
#[cfg(windows)]
#[test]
fn browser_profile_quarantine_keeps_os_lease_after_all_transport_owners_drop() {
    for explicit_quarantine in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let store =
            super::super::profiles::ProfileStore::for_test(directory.path().join("profiles"));
        let install = installation(directory.path());
        let mut task = prepared(&install);
        task.scope.authenticated = true;
        let created = store
            .create(&workspace, "owner", &task.policy_id, &task.scope.origin)
            .unwrap();
        // This fixture tests real exclusive OS handle lifetime, not authentication.
        std::fs::write(
            directory
                .path()
                .join("profiles")
                .join(format!("{}.sealed", created.id)),
            b"sealed-fixture",
        )
        .unwrap();
        let ready = store.finish(&workspace, &created).unwrap();
        let profile = store
            .acquire(&workspace, &ready, false, "home".into())
            .unwrap();
        let profile_weak = Arc::downgrade(&profile);
        task.profile = Some(profile.clone());
        let runtime = Arc::new(BrowserPhysicalRuntime::default());
        let files =
            Arc::new(PrivateFiles::create(&task.scope, &task.inputs, &task.outputs).unwrap());
        let private_path = files.root().to_owned();
        let inner = Arc::new(Inner {
            digest: scope_digest(&task.scope).unwrap(),
            task: task.clone(),
            installation: Arc::new(install),
            runtime: runtime.clone(),
            state: StdMutex::new(State::default()),
            opened: AtomicBool::new(true),
            stop: CancellationToken::new(),
            closed: Notify::new(),
        });
        let bound = BoundBrowserTask {
            port: Arc::new(BoundPort(inner.clone())),
            handle: Arc::new(Handle(inner.clone())),
        };
        let lease = Lease {
            runtime: runtime.clone(),
            reservation: Some(Reservation {
                _permit: runtime.permit.clone().try_acquire_owned().unwrap(),
                files,
                _profile: task.profile.clone(),
            }),
            inner: Arc::downgrade(&inner),
        };
        if explicit_quarantine {
            lease.quarantine(None, Vec::new());
        } else {
            drop(lease);
        }
        assert_eq!(
            bound.handle.snapshot().close,
            Some(BrowserClose::Quarantined)
        );
        drop(bound);
        drop(inner);
        drop(task);
        drop(profile);
        assert!(private_path.is_dir());
        assert_eq!(runtime.child_count(), 1);
        // A fresh factory has its own free global permit. It must still be
        // unable to reuse this profile while the original owner is quarantined.
        let independent_runtime = BrowserPhysicalRuntime::default();
        assert_eq!(independent_runtime.child_count(), 0);
        let independent_store = store.clone();
        let competing = independent_store.acquire(&workspace, &ready, false, "home".into());
        assert!(
            matches!(competing, Err(super::super::BrowserServiceError::Busy)),
            "profile OS lease escaped quarantine after transport owners dropped (explicit={explicit_quarantine})"
        );
        assert!(profile_weak.upgrade().is_some());
    }
}

#[test]
fn m46_transport_startup_config_is_strict_explicit_and_verifies_exact_program_hashes() {
    let root = tempfile::tempdir().unwrap();
    let i = installation(root.path());
    let mut template = scope();
    template.input_ids.clear();
    let config = json!({"schemaVersion":1,"installation":{"id":i.id,"nodeExecutable":i.node_executable,"nodeSha256":i.node_sha256,
        "workerScript":i.worker_script,"workerSha256":i.worker_sha256,"browserExecutable":i.browser_executable,"browserSha256":i.browser_sha256},
        "policies":[{"id":"policy_1","template":template}]});
    let path = root.path().join("runtime.json");
    std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    let setup = super::super::config::load_startup_setup(&path).unwrap();
    assert!(setup.policies.installation.is_some());
    let duplicate = serde_json::to_string(&config).unwrap().replace(
        "\"schemaVersion\":1",
        "\"schemaVersion\":1,\"schemaVersion\":1",
    );
    std::fs::write(&path, duplicate).unwrap();
    assert_eq!(
        code(super::super::config::load_startup_setup(&path)),
        BrowserErrorCode::Invalid
    );
    std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    std::fs::write(&i.worker_script, b"changed after verification").unwrap();
    assert_eq!(
        code(super::super::config::load_startup_setup(&path)),
        BrowserErrorCode::Unavailable
    );
}
#[cfg(not(windows))]
#[tokio::test]
async fn m46_transport_unsupported_host_rejects_before_program_lookup_or_reservation() {
    let root = tempfile::tempdir().unwrap();
    let i = installation(root.path());
    let task = prepared(&i);
    let runtime = Arc::new(BrowserPhysicalRuntime::default());
    let factory = PlaywrightTransportFactory::new(i.clone(), runtime.clone());
    std::fs::remove_file(&i.node_executable).unwrap();
    let bound = factory.bind(task.clone()).unwrap();
    assert_eq!(
        code(bound.port.open(task.scope, control()).await),
        BrowserErrorCode::Unavailable
    );
    assert_eq!(runtime.child_count(), 0);
}
#[cfg(windows)]
#[tokio::test]
#[ignore = "root-only isolated real Node helper; no browser/compatibility claim"]
async fn m46_windows_owned_node_helper_drop_and_descendant_held_pipes_join_the_job() {
    let node = PathBuf::from(
        std::env::var_os("ANIMAOS_BROWSER_TEST_NODE")
            .expect("root supplies an explicit absolute verified Node path"),
    );
    assert!(node.is_absolute() && node.is_file());
    for (dropped, distinct_execute_cancel) in [(false, false), (true, false), (false, true)] {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("pid.json");
        let heartbeat = root.path().join("heartbeat");
        let executed = root.path().join("executed.json");
        let child = format!(
            "const fs=require('node:fs');let n=0;setInterval(()=>fs.writeFileSync({},String(++n)),20);",
            serde_json::to_string(&heartbeat).unwrap()
        );
        let script = format!(
            r#"import fs from 'node:fs';import {{spawn}} from 'node:child_process';
const child=spawn(process.execPath,['-e',{}],{{stdio:['ignore','inherit','inherit']}});
fs.writeFileSync({},JSON.stringify({{pid:process.pid,childPid:child.pid,image:process.execPath,startedAtMs:Date.now(),cwd:process.cwd(),envNames:Object.keys(process.env)}}));
let buffered=Buffer.alloc(0),wire=0,actions=0,pending;function send(id,op,result,failed=false){{let out,body;const payload=failed?{{ok:false,error:result,counters:{{actionsUsed:actions,requestsUsed:0,wireBytesUsed:wire}}}}:{{ok:true,result}};const counters=failed?payload.counters:result.counters;for(let n=0;n<8;n++){{body=Buffer.from(JSON.stringify({{protocol:1,id,op,payload}}));out=Buffer.alloc(8+body.length);out.write('ABW1');out.writeUInt32BE(body.length,4);body.copy(out,8);if(counters.wireBytesUsed===wire+out.length)break;counters.wireBytesUsed=wire+out.length;}}wire+=out.length;process.stdout.write(out);}}
process.stdin.on('data',chunk=>{{wire+=chunk.length;buffered=Buffer.concat([buffered,chunk]);while(buffered.length>=8){{const size=buffered.readUInt32BE(4);if(buffered.length<8+size)return;const m=JSON.parse(buffered.subarray(8,8+size));buffered=buffered.subarray(8+size);if(m.op==='open'){{if({})continue;send(m.id,m.op,{{contextId:'context_1',pageId:'page_1',counters:{{actionsUsed:actions,requestsUsed:0,wireBytesUsed:wire}}}});}}else if(m.op==='execute'){{pending=m.id;actions++;fs.writeFileSync({},JSON.stringify({{id:pending,actions}}));}}else if(m.op==='close'){{if(pending)send(pending,'execute',{{code:'cancelled',effect:'mayHaveOccurred'}},true);send(m.id,m.op,{{engineClosed:true,counters:{{actionsUsed:actions,requestsUsed:0,wireBytesUsed:wire}}}});process.exit(0);}}}}}});"#,
            serde_json::to_string(&child).unwrap(),
            serde_json::to_string(&marker).unwrap(),
            dropped,
            serde_json::to_string(&executed).unwrap()
        );
        let worker = root.path().join("helper.mjs");
        std::fs::write(&worker, &script).unwrap();
        let node_hash = super::super::files::hash(&std::fs::read(&node).unwrap());
        let i = VerifiedBrowserInstallation {
            id: "node_helper".into(),
            digest: "b".repeat(64),
            node_executable: node.clone(),
            node_sha256: node_hash.clone(),
            worker_script: worker,
            worker_sha256: super::super::files::hash(script.as_bytes()),
            browser_executable: node.clone(),
            browser_sha256: node_hash,
        };
        let task = prepared(&i);
        let runtime = Arc::new(BrowserPhysicalRuntime::default());
        let factory = PlaywrightTransportFactory::new(i, runtime.clone());
        let bound = factory.bind(task.clone()).unwrap();
        let port = bound.port.clone();
        let opening_control = control();
        let opening_cancel = opening_control.cancel.clone();
        let open = tokio::spawn(async move { port.open(task.scope, opening_control).await });
        tokio::time::timeout(Duration::from_secs(3), async {
            while !marker.is_file() || !heartbeat.is_file() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let pid: Value = serde_json::from_slice(&std::fs::read(&marker).unwrap()).unwrap();
        assert!(pid["childPid"].as_u64().is_some_and(|pid| pid > 0));
        assert!(pid["startedAtMs"].as_u64().is_some());
        let witness = bound
            .handle
            .private_witness()
            .expect("actual wrapped Node spawn supplies a private witness");
        assert_eq!(pid["pid"].as_u64(), Some(u64::from(witness.node_pid)));
        assert!(witness.spawn_started_at_ms <= witness.spawn_returned_at_ms);
        assert!(pid["startedAtMs"].as_u64().unwrap() >= witness.spawn_started_at_ms);
        assert_eq!(witness.node_executable, node);
        assert_eq!(witness.browser_executable, node);
        assert!(witness.private_root.is_dir());
        assert_eq!(
            PathBuf::from(pid["cwd"].as_str().unwrap()),
            witness.private_root
        );
        assert!(pid["envNames"]
            .as_array()
            .unwrap()
            .iter()
            .all(|name| matches!(
                name.as_str(),
                Some("SystemRoot" | "WINDIR" | "TEMP" | "TMP")
            )));
        let before = std::fs::read_to_string(&heartbeat).unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_ne!(
            std::fs::read_to_string(&heartbeat).unwrap(),
            before,
            "actual descendant must hold inherited pipes while alive"
        );
        let session = if dropped {
            open.abort();
            let _ = open.await;
            None
        } else {
            Some(open.await.unwrap().unwrap())
        };
        if distinct_execute_cancel {
            let pre_cancelled = control();
            pre_cancelled.cancel.cancel();
            let pre_result = bound
                .handle
                .execute_admitted(
                    BrowserOperation::Observe { screenshot: false },
                    None,
                    pre_cancelled,
                )
                .await;
            let pre_sent_nothing = !executed.exists();
            let active_control = control();
            let active_cancel = active_control.cancel.clone();
            let handle = bound.handle.clone();
            let mut execute = tokio::spawn(async move {
                handle
                    .execute_admitted(
                        BrowserOperation::Observe { screenshot: false },
                        None,
                        active_control,
                    )
                    .await
            });
            let reached_worker = tokio::time::timeout(Duration::from_secs(3), async {
                while !executed.is_file() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await;
            active_cancel.cancel();
            // A caller await is bounded, but the owned process/IO must continue
            // retirement independently. Never abort the physical owner to pass.
            let prompt = tokio::time::timeout(Duration::from_millis(700), &mut execute)
                .await
                .ok();
            let automatic_join = tokio::time::timeout(Duration::from_secs(3), async {
                while bound.handle.snapshot().close.is_none() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .is_ok();
            let before_explicit_close = bound.handle.snapshot();
            let prompt_observed = prompt.is_some();
            // Always join cleanup BEFORE the expected RED assertion, including
            // the unfixed late-cancel path, so no child survives a test failure.
            let closed = tokio::time::timeout(Duration::from_secs(5), bound.handle.close()).await;
            let result = match prompt {
                Some(result) => result,
                None => execute.await,
            };
            let after = bound.handle.snapshot();
            let after_result = bound
                .handle
                .execute_admitted(
                    BrowserOperation::Observe { screenshot: false },
                    None,
                    control(),
                )
                .await;
            let stopped = std::fs::read_to_string(&heartbeat).unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert_eq!(closed.unwrap(), BrowserClose::Closed);
            assert_eq!(bound.handle.close().await, BrowserClose::Closed);
            assert_eq!(runtime.child_count(), 0);
            assert!(!witness.private_root.exists());
            assert_eq!(std::fs::read_to_string(&heartbeat).unwrap(), stopped);
            assert!(
                !opening_cancel.is_cancelled(),
                "Open control A must remain live"
            );
            assert!(
                reached_worker.is_ok(),
                "the real Node peer must receive Execute before cancelling B"
            );
            assert!(pre_sent_nothing);
            assert_eq!(
                pre_result.err().unwrap(),
                error(BrowserErrorCode::Cancelled)
            );
            assert!(
                prompt_observed,
                "late Execute control B cancellation must settle promptly while A is live"
            );
            assert!(
                automatic_join,
                "late B cancellation must itself retire and join the owned Job/IO"
            );
            assert!(
                before_explicit_close.close.is_some() || before_explicit_close.physical_lease_held,
                "a caller result cannot release an unjoined physical lease"
            );
            assert_eq!(after.close, Some(BrowserClose::Closed));
            assert!(!after.physical_lease_held);
            assert_eq!(
                result.unwrap().err().unwrap(),
                unknown(error(BrowserErrorCode::Cancelled))
            );
            assert_eq!(
                after_result.err().unwrap().effect,
                BrowserEffect::NotDispatched
            );
            let observed: Value =
                serde_json::from_slice(&std::fs::read(&executed).unwrap()).unwrap();
            assert_eq!(
                observed["actions"], 1,
                "pre-cancelled and retired calls send zero additional Execute frames"
            );
            drop(session);
            continue;
        }
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), bound.handle.close())
                .await
                .unwrap(),
            BrowserClose::Closed
        );
        assert_eq!(bound.handle.close().await, BrowserClose::Closed);
        drop(session);
        assert_eq!(runtime.child_count(), 0);
        assert!(
            !witness.private_root.exists(),
            "joined cleanup removes the exact owned private directory"
        );
        let stopped = std::fs::read_to_string(&heartbeat).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(std::fs::read_to_string(&heartbeat).unwrap(), stopped);
    }
}
