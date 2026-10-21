use super::*;
use serde_json::json;

fn local(root: &std::path::Path) -> serde_json::Value {
    json!({"id":"local-test","label":"Test local","backend":{"kind":"local","executable":std::env::current_exe().unwrap(),"arguments":[],"workspaceRoot":root},"maxTimeoutMs":1000})
}

#[test]
fn execution_config_is_fail_closed_and_profiles_are_immutable() {
    let temp = tempfile::tempdir().unwrap();
    let profile = local(temp.path());
    let service =
        ExecutionService::from_json(&json!({"version":1,"targets":[profile.clone()]}).to_string())
            .unwrap();
    assert_eq!(service.targets()[0].id, "local-test");
    assert!(service.target("unconfigured").is_err());
    assert!(ExecutionService::from_json(
        &json!({"version":1,"targets":[profile.clone(),profile]}).to_string()
    )
    .is_err());
}

#[test]
fn execution_request_cannot_override_transport_or_security_configuration() {
    assert!(serde_json::from_value::<ExecutionRequest>(
        json!({"targetId":"x","command":"echo hi","image":"evil"})
    )
    .is_err());
    assert!(serde_json::from_value::<ExecutionRequest>(
        json!({"targetId":"x","command":"echo hi","executable":"evil"})
    )
    .is_err());
    assert!(serde_json::from_value::<ExecutionRequest>(
        json!({"targetId":"x","command":"echo hi","sshHost":"evil"})
    )
    .is_err());
}

#[test]
fn execution_docker_rejects_mutable_images_and_ssh_requires_pinned_files() {
    let temp = tempfile::tempdir().unwrap();
    let mut p = local(temp.path());
    p["backend"] = json!({"kind":"docker","executable":std::env::current_exe().unwrap(),"image":"alpine:latest"});
    assert!(ExecutionService::from_json(&json!({"version":1,"targets":[p]}).to_string()).is_err());
    let mut p = local(temp.path());
    p["backend"] = json!({"kind":"ssh","executable":std::env::current_exe().unwrap(),"host":"example.invalid","user":"owner","port":22,"identityFile":temp.path().join("missing-key"),"knownHostsFile":temp.path().join("missing-pins"),"workerEntrypoint":"/opt/anima"});
    assert!(ExecutionService::from_json(&json!({"version":1,"targets":[p]}).to_string()).is_err());
}

#[test]
fn execution_remote_arguments_isolate_model_data() {
    let command = "echo ; $(touch /tmp/host-injection) --privileged";
    let docker = DockerProfile {
        executable: std::env::current_exe().unwrap(),
        image: format!("example.invalid/worker@sha256:{}", "a".repeat(64)),
        memory_mb: 128,
        cpus: 1,
        pids: 32,
    };
    let request = ExecutionRequest {
        target_id: "docker".into(),
        command: command.into(),
        cwd: ".".into(),
        timeout_ms: 1000,
    };
    let args =
        docker::create_arguments(&docker, &request, "anima-execution-test", "token").unwrap();
    assert_eq!(args.last().unwrap(), command);
    for required in [
        "--network=none",
        "--read-only",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges",
        "--user=65534:65534",
        "--pull=never",
    ] {
        assert!(args.iter().any(|arg| arg == required));
    }
    assert!(!args
        .iter()
        .any(|arg| arg == "--privileged" || arg == "--volume" || arg == "--mount"));
    assert!(docker::create_arguments(
        &docker,
        &ExecutionRequest {
            cwd: "../../etc".into(),
            ..request
        },
        "anima-execution-test",
        "token"
    )
    .is_err());
}

#[tokio::test]
async fn execution_unknown_target_never_falls_back_to_local_execution() {
    let service = ExecutionService::default();
    let result = service
        .execute(
            ExecutionRequest {
                target_id: "missing".into(),
                command: "echo must-not-run".into(),
                cwd: ".".into(),
                timeout_ms: 1000,
            },
            None,
        )
        .await;
    assert!(result.unwrap_err().contains("not configured"));
}

#[tokio::test]
async fn execution_authority_revoked_while_waiting_admits_no_transport() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let root = tempfile::tempdir().unwrap();
    let service = Arc::new(
        ExecutionService::from_json(
            &json!({"version":1,"targets":[shell_profile(root.path())]}).to_string(),
        )
        .unwrap(),
    );
    let entered = Arc::new(tokio::sync::Semaphore::new(0));
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let revoked = Arc::new(AtomicBool::new(false));
    let scope = ExecutionScope {
        agent_id: "owner".into(),
        authority: {
            let entered = entered.clone();
            let release = release.clone();
            let revoked = revoked.clone();
            Arc::new(move || {
                let entered = entered.clone();
                let release = release.clone();
                let revoked = revoked.clone();
                Box::pin(async move {
                    entered.add_permits(1);
                    release.acquire().await.unwrap().forget();
                    if revoked.load(Ordering::SeqCst) {
                        Err("Grant was revoked before dispatch".into())
                    } else {
                        Ok(())
                    }
                })
            })
        },
    };
    let active = service.clone();
    let task = tokio::spawn(async move {
        active
            .execute_scoped(
                ExecutionRequest {
                    target_id: "local-test".into(),
                    command: "require('fs').writeFileSync('must-not-exist','x')".into(),
                    cwd: ".".into(),
                    timeout_ms: 1000,
                },
                None,
                scope,
            )
            .await
    });
    entered.acquire().await.unwrap().forget();
    assert_eq!(service.in_flight_count(), 1);
    revoked.store(true, Ordering::SeqCst);
    release.add_permits(1);
    assert!(task.await.unwrap().unwrap_err().contains("revoked"));
    assert_eq!(service.in_flight_count(), 0);
    assert!(!root.path().join("must-not-exist").exists());
}

#[tokio::test]
async fn execution_shutdown_cancels_admitted_work_and_refuses_new_work() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let root = tempfile::tempdir().unwrap();
    let service = Arc::new(
        ExecutionService::from_json(
            &json!({"version":1,"targets":[shell_profile(root.path())]}).to_string(),
        )
        .unwrap(),
    );
    let entered = Arc::new(tokio::sync::Semaphore::new(0));
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let first = Arc::new(AtomicBool::new(true));
    let scope = ExecutionScope {
        agent_id: "owner".into(),
        authority: {
            let entered = entered.clone();
            let release = release.clone();
            let first = first.clone();
            Arc::new(move || {
                let entered = entered.clone();
                let release = release.clone();
                let first = first.clone();
                Box::pin(async move {
                    if first.swap(false, Ordering::SeqCst) {
                        entered.add_permits(1);
                        release.acquire().await.unwrap().forget();
                    }
                    Ok(())
                })
            })
        },
    };
    let request = ExecutionRequest {
        target_id: "local-test".into(),
        command: "require('fs').writeFileSync('must-not-exist','x')".into(),
        cwd: ".".into(),
        timeout_ms: 1000,
    };
    let active = service.clone();
    let queued = request.clone();
    let task = tokio::spawn(async move { active.execute_scoped(queued, None, scope).await });
    entered.acquire().await.unwrap().forget();
    service.request_shutdown();
    assert_eq!(service.in_flight_count(), 1);
    assert!(service
        .execute(request, None)
        .await
        .unwrap_err()
        .contains("shutting down"));
    release.add_permits(1);
    let receipt = task.await.unwrap().unwrap();
    assert_eq!(receipt.status, ExecutionStatus::Cancelled);
    service.shutdown().await;
    assert_eq!(service.in_flight_count(), 0);
    assert!(!root.path().join("must-not-exist").exists());
    assert!(service
        .receipt_for_agent(&receipt.execution_id, "owner")
        .is_some());
    assert!(service
        .receipt_for_agent(&receipt.execution_id, "other-agent")
        .is_none());
}

fn shell_profile(root: &std::path::Path) -> serde_json::Value {
    let output = std::process::Command::new("node")
        .args(["-p", "process.execPath"])
        .output()
        .unwrap();
    let executable = String::from_utf8(output.stdout).unwrap().trim().to_string();
    let mut profile = local(root);
    profile["backend"]["executable"] = json!(executable);
    profile["backend"]["arguments"] = json!(["-e"]);
    profile
}

#[tokio::test]
#[cfg(windows)]
async fn execution_local_timeout_retires_custody_before_reusing_target() {
    let temp = tempfile::tempdir().unwrap();
    let service = ExecutionService::from_json(
        &json!({"version":1,"targets":[shell_profile(temp.path())]}).to_string(),
    )
    .unwrap();
    let result = service
        .execute(
            ExecutionRequest {
                target_id: "local-test".into(),
                command: "setInterval(()=>{},1000)".into(),
                cwd: ".".into(),
                timeout_ms: 100,
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(result.status, ExecutionStatus::TimedOut);
    assert!(result.cleanup_confirmed);
    let result = service
        .execute(
            ExecutionRequest {
                target_id: "local-test".into(),
                command: "process.stdout.write('selected-local')".into(),
                cwd: ".".into(),
                timeout_ms: 1000,
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(result.status, ExecutionStatus::Succeeded);
    assert_eq!(result.stdout, "selected-local");
}

#[tokio::test]
#[cfg(windows)]
async fn execution_local_cancel_and_cwd_escape_are_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let service = std::sync::Arc::new(
        ExecutionService::from_json(
            &json!({"version":1,"targets":[shell_profile(temp.path())]}).to_string(),
        )
        .unwrap(),
    );
    assert!(service
        .execute(
            ExecutionRequest {
                target_id: "local-test".into(),
                command: "process.exit(0)".into(),
                cwd: "..".into(),
                timeout_ms: 1000
            },
            None
        )
        .await
        .is_err());
    let cancel = anima_core::CancelSignal::new();
    let running = {
        let service = service.clone();
        let signal = cancel.clone();
        tokio::spawn(async move {
            service
                .execute(
                    ExecutionRequest {
                        target_id: "local-test".into(),
                        command: "setInterval(()=>{},1000)".into(),
                        cwd: ".".into(),
                        timeout_ms: 1000,
                    },
                    Some(signal),
                )
                .await
                .unwrap()
        })
    };
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    cancel.cancel();
    let receipt = running.await.unwrap();
    assert_eq!(receipt.status, ExecutionStatus::Cancelled);
    assert!(receipt.cleanup_confirmed);
}

#[test]
fn execution_worker_frames_are_bounded_and_commands_are_json_data() {
    let request = worker::WorkerRequest::Run {
        version: 1,
        execution_id: uuid::Uuid::new_v4().to_string(),
        command: "echo ; $(uname) \" x".into(),
        cwd: ".".into(),
        timeout_ms: 1000,
        target_id: "test".into(),
        receipt_key: "a".repeat(64),
    };
    let mut frame = Vec::new();
    worker::write_frame(&mut frame, &request).unwrap();
    assert_eq!(
        worker::read_frame::<_, worker::WorkerRequest>(&mut frame.as_slice()).unwrap(),
        request
    );
    let mut oversized = (worker::MAX_FRAME_BYTES as u32 + 1).to_be_bytes().to_vec();
    oversized.push(b'{');
    assert!(worker::read_frame::<_, worker::WorkerRequest>(&mut oversized.as_slice()).is_err());
}

fn peer() -> std::path::PathBuf {
    let tests = std::env::current_exe().unwrap();
    tests
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples")
        .join(format!(
            "execution_backend_peer{}",
            std::env::consts::EXE_SUFFIX
        ))
}
fn fake_environment_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}
struct FakeEnvironment(Option<std::ffi::OsString>);
impl FakeEnvironment {
    fn new(root: &std::path::Path) -> Self {
        let previous = std::env::var_os("ANIMA_EXECUTION_FAKE_ROOT");
        std::env::set_var("ANIMA_EXECUTION_FAKE_ROOT", root);
        Self(previous)
    }
}
impl Drop for FakeEnvironment {
    fn drop(&mut self) {
        match self.0.take() {
            Some(value) => std::env::set_var("ANIMA_EXECUTION_FAKE_ROOT", value),
            None => std::env::remove_var("ANIMA_EXECUTION_FAKE_ROOT"),
        }
    }
}
fn docker_service() -> ExecutionService {
    let executable = peer();
    assert!(
        executable.is_file(),
        "Cargo test must build the execution_backend_peer example"
    );
    ExecutionService::from_json(&json!({"version":1,"targets":[{"id":"container","label":"Controlled Docker peer","maxTimeoutMs":2000,"backend":{"kind":"docker","executable":executable,"image":format!("example.invalid/worker@sha256:{}","b".repeat(64))}}]}).to_string()).unwrap()
}

#[tokio::test]
async fn execution_docker_timeout_removes_exact_owned_container_before_permit_reuse() {
    let _lock = fake_environment_lock().lock().await;
    let temp = tempfile::tempdir().unwrap();
    let _environment = FakeEnvironment::new(temp.path());
    std::fs::write(temp.path().join("hold"), b"").unwrap();
    let service = docker_service();
    let request = ExecutionRequest {
        target_id: "container".into(),
        command: "ignored fixture command".into(),
        cwd: ".".into(),
        timeout_ms: 150,
    };
    let receipt = service.execute(request.clone(), None).await.unwrap();
    assert_eq!(receipt.status, ExecutionStatus::TimedOut);
    assert!(receipt.cleanup_confirmed);
    assert!(!temp.path().join("exists").exists());
    let calls = std::fs::read_to_string(temp.path().join("calls.jsonl")).unwrap();
    let calls: Vec<Vec<String>> = calls
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let removal = calls.iter().find(|c| c[0] == "rm").unwrap();
    assert_eq!(removal.last().unwrap(), &"a".repeat(64));
    std::fs::remove_file(temp.path().join("hold")).unwrap();
    assert_eq!(
        service.execute(request, None).await.unwrap().status,
        ExecutionStatus::Succeeded
    );
}

#[tokio::test]
async fn execution_docker_unknown_cleanup_holds_capacity_until_authoritative_absence() {
    let _lock = fake_environment_lock().lock().await;
    let temp = tempfile::tempdir().unwrap();
    let _environment = FakeEnvironment::new(temp.path());
    std::fs::write(temp.path().join("refuse-removal"), b"").unwrap();
    let service = docker_service();
    let request = ExecutionRequest {
        target_id: "container".into(),
        command: "ignored fixture command".into(),
        cwd: ".".into(),
        timeout_ms: 1000,
    };
    let receipt = service.execute(request.clone(), None).await.unwrap();
    assert_eq!(receipt.status, ExecutionStatus::OutcomeUnknown);
    assert!(!receipt.cleanup_confirmed);
    assert_eq!(service.in_flight_count(), 1);
    assert!(service
        .execute(request.clone(), None)
        .await
        .unwrap_err()
        .contains("unreconciled"));
    std::fs::remove_file(temp.path().join("refuse-removal")).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(8), async {
        loop {
            if service
                .receipt(&receipt.execution_id)
                .unwrap()
                .cleanup_confirmed
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    assert!(!temp.path().join("exists").exists());
    assert_eq!(service.in_flight_count(), 0);
    assert_eq!(
        service.execute(request, None).await.unwrap().status,
        ExecutionStatus::Succeeded
    );
}

#[tokio::test]
async fn execution_docker_refuses_image_declared_volumes_before_container_creation() {
    let _lock = fake_environment_lock().lock().await;
    let root = tempfile::tempdir().unwrap();
    let _environment = FakeEnvironment::new(root.path());
    std::fs::write(root.path().join("declared-volume"), b"").unwrap();
    let result = docker_service()
        .execute(
            ExecutionRequest {
                target_id: "container".into(),
                command: "never dispatched".into(),
                cwd: ".".into(),
                timeout_ms: 1000,
            },
            None,
        )
        .await;
    assert!(result.unwrap_err().contains("declared volumes"));
    assert!(!std::fs::read_to_string(root.path().join("calls.jsonl"))
        .unwrap()
        .contains("\"create\""));
}

#[tokio::test]
async fn execution_shutdown_keeps_unknown_docker_work_visible_until_cleanup() {
    let _lock = fake_environment_lock().lock().await;
    let root = tempfile::tempdir().unwrap();
    let _environment = FakeEnvironment::new(root.path());
    std::fs::write(root.path().join("refuse-removal"), b"").unwrap();
    let service = docker_service();
    let receipt = service
        .execute(
            ExecutionRequest {
                target_id: "container".into(),
                command: "ignored fixture command".into(),
                cwd: ".".into(),
                timeout_ms: 1000,
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(receipt.status, ExecutionStatus::OutcomeUnknown);
    assert_eq!(service.in_flight_count(), 1);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), service.shutdown())
            .await
            .is_err()
    );
    assert_eq!(service.in_flight_count(), 1);
    std::fs::remove_file(root.path().join("refuse-removal")).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(8), service.shutdown())
        .await
        .unwrap();
    assert_eq!(service.in_flight_count(), 0);
    assert!(!root.path().join("exists").exists());
}

#[test]
fn execution_ssh_configuration_has_no_model_command_or_implicit_authentication_arguments() {
    let p = SshProfile {
        executable: peer(),
        host: "owner.example.invalid".into(),
        user: "worker".into(),
        port: 2222,
        identity_file: "/operator/key".into(),
        known_hosts_file: "/operator/pins".into(),
        worker_entrypoint: "/opt/anima".into(),
    };
    let args = ssh::arguments(&p);
    assert_eq!(args.last().unwrap(), "/opt/anima --execution-worker-v1");
    for expected in [
        "-oBatchMode=yes",
        "-oIdentitiesOnly=yes",
        "-oIdentityAgent=none",
        "-oForwardAgent=no",
        "-oStrictHostKeyChecking=yes",
        "-oGlobalKnownHostsFile=none",
    ] {
        assert!(args.iter().any(|a| a == expected));
    }
    assert!(!args
        .iter()
        .any(|a| a.contains("command") || a.contains("receiptKey")));
}

#[test]
fn execution_worker_reconciliation_rejects_forged_or_changed_terminal_receipts() {
    let mut receipt = ExecutionReceipt::unknown("id", "target", "ssh", "test");
    receipt.cleanup_confirmed = true;
    receipt.status = ExecutionStatus::Succeeded;
    let proof = worker::proof(&receipt, "secret");
    assert!(worker::verify(&receipt, "secret", &proof));
    receipt.status = ExecutionStatus::Failed;
    assert!(!worker::verify(&receipt, "secret", &proof));
}

#[tokio::test]
async fn execution_ssh_lost_connection_keeps_outcome_unknown_until_signed_worker_reconciliation() {
    let _lock = fake_environment_lock().lock().await;
    let root = tempfile::tempdir().unwrap();
    let _environment = FakeEnvironment::new(root.path());
    let key = root.path().join("key");
    let pins = root.path().join("pins");
    std::fs::write(&key, "controlled key fixture").unwrap();
    std::fs::write(&pins, "controlled pin fixture").unwrap();
    let service=ExecutionService::from_json(&json!({"version":1,"targets":[{"id":"remote","label":"Controlled SSH peer","maxTimeoutMs":1000,"backend":{"kind":"ssh","executable":peer(),"host":"peer.invalid","user":"worker","port":22,"identityFile":key,"knownHostsFile":pins,"workerEntrypoint":"/opt/anima"}}]}).to_string()).unwrap();
    let command = "echo ; $(must-remain-command-data) --privileged";
    let request = ExecutionRequest {
        target_id: "remote".into(),
        command: command.into(),
        cwd: ".".into(),
        timeout_ms: 500,
    };
    let receipt = service.execute(request.clone(), None).await.unwrap();
    assert_eq!(receipt.status, ExecutionStatus::OutcomeUnknown);
    assert!(!receipt.cleanup_confirmed);
    assert!(service
        .execute(request, None)
        .await
        .unwrap_err()
        .contains("unreconciled"));
    let wire: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("worker-request.json")).unwrap())
            .unwrap();
    assert_eq!(wire["command"], command);
    assert!(!std::fs::read_to_string(root.path().join("calls.jsonl"))
        .unwrap()
        .contains("must-remain-command-data"));
    std::fs::write(root.path().join("allow-status"), b"").unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(8), async {
        loop {
            if service
                .receipt(&receipt.execution_id)
                .unwrap()
                .cleanup_confirmed
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        service.receipt(&receipt.execution_id).unwrap().status,
        ExecutionStatus::Cancelled
    );
}

#[cfg(windows)]
#[test]
fn execution_real_rust_worker_owns_deadline_and_eof_cancellation() {
    use std::process::{Command, Stdio};
    for close_input in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut child = Command::new(peer())
            .arg("--execution-worker-v1")
            .env("ANIMAOS_EXECUTION_WORKER_ROOT", root.path())
            .env("ANIMAOS_EXECUTION_WORKER_STATE_DIR", state.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let key = "a".repeat(64);
        let mut input = child.stdin.take().unwrap();
        let mut output = child.stdout.take().unwrap();
        worker::write_frame(
            &mut input,
            &worker::WorkerRequest::Run {
                version: 1,
                execution_id: id.clone(),
                command: "node -e \"require('fs').writeFileSync('ready','ready');setInterval(()=>{},1000)\"".into(),
                cwd: ".".into(),
                timeout_ms: if close_input { 3000 } else { 150 },
                target_id: "worker".into(),
                receipt_key: key.clone(),
            },
        )
        .unwrap();
        let accepted: worker::WorkerResponse = worker::read_frame(&mut output).unwrap();
        assert!(matches!(accepted, worker::WorkerResponse::Accepted { .. }));
        if close_input {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while !root.path().join("ready").is_file() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(
                root.path().join("ready").is_file(),
                "real worker child must start before EOF cancellation"
            );
            drop(input)
        }
        let terminal: worker::WorkerResponse = worker::read_frame(&mut output).unwrap();
        let worker::WorkerResponse::Terminal { receipt, proof } = terminal else {
            panic!("missing authoritative terminal receipt")
        };
        assert!(worker::verify(&receipt, &key, &proof));
        assert!(receipt.cleanup_confirmed);
        assert_eq!(
            receipt.status,
            if close_input {
                ExecutionStatus::Cancelled
            } else {
                ExecutionStatus::TimedOut
            }
        );
        assert!(child.wait().unwrap().success());
        assert!(!state.path().join("active.json").exists());
        assert!(state.path().join(format!("{id}.terminal.json")).is_file());
    }
}

#[tokio::test]
#[ignore = "requires explicitly selected installed Docker CLI/image and live engine access"]
async fn execution_live_docker_operator_profile_verifies_isolation_timeout_and_cancel() {
    let executable = std::path::PathBuf::from(
        std::env::var_os("ANIMAOS_EXECUTION_LIVE_DOCKER_PATH").expect("explicit Docker CLI path"),
    );
    let image = std::env::var("ANIMAOS_EXECUTION_LIVE_DOCKER_IMAGE")
        .expect("explicit installed image digest");
    let service=std::sync::Arc::new(ExecutionService::from_json(&json!({"version":1,"targets":[{"id":"live-acceptance","label":"Live acceptance","maxTimeoutMs":5000,"backend":{"kind":"docker","executable":executable,"image":image}}]}).to_string()).unwrap());
    let request=ExecutionRequest{target_id:"live-acceptance".into(),command:"test \"$(id -u)\" = 65534 && test ! -S /var/run/docker.sock && test \"$(ls /sys/class/net)\" = lo && test \"$(awk '/CapEff/ {print $2}' /proc/self/status)\" = 0000000000000000 && touch /workspace/probe && ! touch /root-write-test && echo isolation-confirmed".into(),cwd:".".into(),timeout_ms:5000};
    let receipt = service.execute(request.clone(), None).await.unwrap();
    println!("{}", serde_json::to_string(&receipt).unwrap());
    assert_eq!(receipt.status, ExecutionStatus::Succeeded);
    assert!(receipt.stdout.contains("isolation-confirmed"));
    assert!(receipt.cleanup_confirmed);
    let mut sleeping = request.clone();
    sleeping.command = "sleep 30".into();
    sleeping.timeout_ms = 250;
    let timed = service.execute(sleeping.clone(), None).await.unwrap();
    println!("{}", serde_json::to_string(&timed).unwrap());
    assert_eq!(timed.status, ExecutionStatus::TimedOut);
    assert!(timed.cleanup_confirmed);
    sleeping.timeout_ms = 5000;
    let cancel = anima_core::CancelSignal::new();
    let signal = cancel.clone();
    let active = service.clone();
    let task = tokio::spawn(async move { active.execute(sleeping, Some(signal)).await.unwrap() });
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    cancel.cancel();
    let stopped = task.await.unwrap();
    println!("{}", serde_json::to_string(&stopped).unwrap());
    assert_eq!(stopped.status, ExecutionStatus::Cancelled);
    assert!(stopped.cleanup_confirmed);
}
