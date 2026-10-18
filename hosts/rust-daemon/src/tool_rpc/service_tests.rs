//! Controller-selected physical target; no default interpreter discovery.
use super::*;
use crate::tool_rpc::{ToolRpcConfig, ToolRpcService};
use tokio::time::{timeout, Instant};

#[tokio::test]
async fn fp06_physical_cleanup_unknown_latches_started_and_blocks_later_native_effect() {
    let fixture = Fixture::new(PolicyAction::Allow).await;
    let projected = crate::tool_rpc::service::project_worker_failure(
        &fixture.journal,
        crate::tool_rpc::worker::WorkerFailure::CleanupUnknown,
    );
    let acknowledged = fixture.journal.finish(&outer_call(), &projected, 0).await;
    let subsequent = fixture
        .context
        .clone()
        .execute_tool(
            fixture.agent.clone(),
            tool_input(&fixture.agent.id, SESSION),
            ToolCall {
                id: "fp06-after-unproven-worker".into(),
                name: "write_file".into(),
                args: BTreeMap::from([
                    ("file_path".into(), DataValue::String(TARGET.into())),
                    (
                        "content".into(),
                        DataValue::String("must not execute after unknown Python lifetime".into()),
                    ),
                ]),
            },
        )
        .await;
    assert!(
        fixture.journal.failure().is_some(),
        "physical uncertainty latches existing journal"
    );
    assert!(
        acknowledged.is_err(),
        "outer Started cannot become an ordinary acknowledged Failed outcome"
    );
    assert_eq!(subsequent.status, TaskStatus::Error);
    assert!(!fixture.output_exists());
    let receipts = fixture.receipts().await;
    assert_eq!(receipts.len(), 1, "later effects cannot create new intents");
    assert_eq!(receipts[0].status, ActionStatus::Started);
    let output: Value = serde_json::from_str(&projected.error.unwrap()).unwrap();
    assert_eq!(output["outcome"], "review_required");
    assert_eq!(output["cleanupVerified"], false);
}

#[tokio::test]
async fn fp06_physical_proven_timeout_remains_distinct_from_unknown_lifetime() {
    let fixture = Fixture::new(PolicyAction::Allow).await;
    let projected = crate::tool_rpc::service::project_worker_failure(
        &fixture.journal,
        crate::tool_rpc::worker::WorkerFailure::TimedOut,
    );
    assert_eq!(projected.status, TaskStatus::Error);
    assert!(
        fixture.journal.failure().is_none(),
        "proven retirement does not invent uncertainty"
    );
    fixture
        .journal
        .finish(&outer_call(), &projected, 0)
        .await
        .expect("known failed outcome is acknowledged");
    assert_eq!(fixture.receipts().await[0].status, ActionStatus::Failed);
    let output: Value = serde_json::from_str(&projected.error.unwrap()).unwrap();
    assert_eq!(output["cleanupVerified"], true);
}

#[tokio::test]
async fn fp06_captured_descriptors_use_native_registry_and_closed_grant_intersection() {
    let fixture = Fixture::new(PolicyAction::Allow).await;
    let mut copied = fixture.agent.clone();
    for descriptor in copied.config.tools.as_mut().unwrap() {
        descriptor.description = "ATTACKER copied grant text".into();
        descriptor.parameters_schema.clear();
    }
    let broker = Broker::capture(
        fixture.context.clone(),
        copied,
        tool_input(&fixture.agent.id, SESSION),
        outer_call(),
    )
    .await
    .unwrap();
    let descriptors = broker.captured_descriptors();
    assert_eq!(
        descriptors
            .iter()
            .map(|d| d.name.as_str())
            .collect::<Vec<_>>(),
        vec!["bash", "read_file", "write_file"]
    );
    let native = fixture.context.rpc_descriptor("read_file").unwrap();
    assert_eq!(
        descriptors.iter().find(|d| d.name == "read_file").unwrap(),
        &native
    );
    assert!(descriptors
        .iter()
        .all(|d| d.name != "execute_code" && !d.description.contains("ATTACKER")));
}

fn selected_service() -> Arc<ToolRpcService> {
    let path = std::env::var_os("ANIMAOS_RS_TOOL_RPC_ACCEPTANCE_PYTHON_PATH")
        .expect("controller must explicitly select the synthetic physical test interpreter");
    ToolRpcService::new(
        ToolRpcConfig::parse(Some(path)).expect("absolute selected existing interpreter"),
    )
}

fn physical_call(code: &str) -> ToolCall {
    ToolCall {
        id: "fp06-physical-outer".into(),
        name: "execute_code".into(),
        args: BTreeMap::from([("code".into(), DataValue::String(code.into()))]),
    }
}

async fn approved_outer(
    fixture: &Fixture,
    service: Arc<ToolRpcService>,
    code: &str,
) -> anima_core::TaskResult<anima_core::Content> {
    fixture.state.write().await.approvals.set_policy(
        &fixture.agent.id,
        ApprovalPolicy::default()
            .with(RiskClass::Write, PolicyAction::Allow)
            .with(RiskClass::Exec, PolicyAction::Ask),
    );
    let context = fixture.context.clone().with_tool_rpc_service(service);
    let agent = fixture.agent.clone();
    let call = physical_call(code);
    let input = tool_input(&fixture.agent.id, SESSION);
    let task = tokio::spawn(async move { context.execute_tool(agent, input, call).await });
    let approval = pending_approvals(&fixture.coordinator, 1).await.remove(0);
    assert_eq!(approval.tool, "execute_code");
    assert_eq!(approval.tool_call_id, "fp06-physical-outer");
    assert!(!fixture.root.path().join("summary.json").exists());
    fixture
        .coordinator
        .decide_approval(
            &approval.id,
            decision(ApprovalDecisionKind::AllowOnce, approval.revision),
        )
        .await
        .unwrap();
    timeout(Duration::from_secs(20), task)
        .await
        .expect("bounded finite physical test")
        .unwrap()
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "controller must select explicit synthetic Python target"]
async fn fp06_physical_two_native_reads_one_write_fresh_exec_approval() {
    let fixture = Fixture::new(PolicyAction::Allow).await;
    std::fs::write(fixture.root.path().join("beta.json"), b"{\"value\":5}").unwrap();
    let service = selected_service();
    let before = fixture.receipts().await.len();
    let result = approved_outer(
        &fixture,
        service.clone(),
        r#"import json
from anima_tools import read_file, write_file
def native_json(path):
    numbered = read_file(file_path=path)
    return json.loads('\n'.join(line.split('| ', 1)[1] for line in numbered.splitlines()))
total = native_json('alpha.json')['value'] + native_json('beta.json')['value']
write_file(file_path='summary.json', content=json.dumps({'total':total}, separators=(',',':')))
print('total=' + str(total))
"#,
    )
    .await;
    assert_eq!(
        result.status,
        TaskStatus::Success,
        "physical one-shot execution: {:?}",
        result.error
    );
    assert_eq!(
        std::fs::read(fixture.root.path().join("summary.json")).unwrap(),
        b"{\"total\":12}"
    );
    let receipt = fixture.receipts().await;
    let new = &receipt[before..];
    assert_eq!(
        new.len(),
        4,
        "one outer and exactly three inner durable receipts"
    );
    assert_eq!(new.iter().filter(|r| r.name == "read_file").count(), 2);
    assert_eq!(new.iter().filter(|r| r.name == "write_file").count(), 1);
    assert!(new.iter().all(|r| r.status == ActionStatus::Succeeded));
    let output: Value = serde_json::from_str(&result.data.unwrap().text).unwrap();
    assert_eq!(output["mode"], "one_shot");
    assert_eq!(output["stdout"], "total=12\r\n");
    assert_eq!(output["cleanupVerified"], true);
    assert_eq!(output["dispatchedCalls"], 3);
    assert_eq!(service.active_cells(), 0);
}

async fn wait_file(path: &std::path::Path) {
    timeout(Duration::from_secs(5), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("actual interpreter reached synthetic fixture");
}

#[cfg(windows)]
fn hold_descendant(fixture: &Fixture) -> win_custody::Process {
    let pid: u32 = std::fs::read_to_string(fixture.root.path().join("started.pid"))
        .unwrap()
        .parse()
        .unwrap();
    win_custody::Process::open(pid, win_custody::ProcessAccess::QueryLimitedAndSynchronize)
        .expect("hold exact live descendant object before termination, never reopen reused PID")
}

const DESCENDANT: &str = r#"import os, subprocess, sys, time
from pathlib import Path
child = subprocess.Popen([sys.executable, '-I', '-S', '-u', '-c', "import time; time.sleep(60)"])
Path('started.pid').write_text(str(child.pid))
while True:
    time.sleep(.1)
"#;

#[cfg(windows)]
#[tokio::test]
#[ignore = "controller must select explicit synthetic Python target"]
async fn fp06_physical_timeout_retires_descendant_and_pipe_holder_before_capacity() {
    let fixture = Fixture::with_deadline(
        PolicyAction::Allow,
        Some(Instant::now() + Duration::from_secs(2)),
    )
    .await;
    let service = selected_service();
    let context = fixture
        .context
        .clone()
        .with_tool_rpc_service(service.clone());
    let agent = fixture.agent.clone();
    let input = tool_input(&agent.id, SESSION);
    let task = tokio::spawn(async move {
        context
            .execute_tool(agent, input, physical_call(DESCENDANT))
            .await
    });
    wait_file(&fixture.root.path().join("started.pid")).await;
    let descendant = hold_descendant(&fixture);
    let result = timeout(Duration::from_secs(8), task)
        .await
        .expect("physical deadline + retirement grace")
        .unwrap();
    assert_eq!(result.status, TaskStatus::Error);
    timeout(Duration::from_secs(5), async {
        while service.active_cells() != 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("exact tree and inherited pipes must retire");
    assert_eq!(service.active_cells(), 0);
    assert!(
        descendant.exit_code().unwrap().is_some(),
        "exact inherited-pipe descendant exited before capacity release"
    );
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "controller must select explicit synthetic Python target"]
async fn fp06_physical_dropped_caller_retains_cleanup_owner_until_capacity_proven() {
    let fixture = Fixture::new(PolicyAction::Allow).await;
    let service = selected_service();
    let context = fixture
        .context
        .clone()
        .with_tool_rpc_service(service.clone());
    let agent = fixture.agent.clone();
    let input = tool_input(&agent.id, SESSION);
    let task = tokio::spawn(async move {
        context
            .execute_tool(agent, input, physical_call(DESCENDANT))
            .await
    });
    wait_file(&fixture.root.path().join("started.pid")).await;
    let descendant = hold_descendant(&fixture);
    assert_eq!(service.active_cells(), 1);
    task.abort();
    let _ = task.await;
    timeout(Duration::from_secs(5), async {
        while service.active_cells() != 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("caller drop must leave independent physical cleanup alive");
    assert_eq!(service.active_cells(), 0);
    assert!(
        descendant.exit_code().unwrap().is_some(),
        "caller cancellation retires exact descendant handle"
    );
}
