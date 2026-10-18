use super::{Containment, Custody};
use process_wrap::tokio::{ChildWrapper, CommandWrap};
use std::{path::PathBuf, process::Stdio, time::Duration};

fn node() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join(if cfg!(windows) { "node.exe" } else { "node" }))
        .find(|p| p.is_file())
        .expect("Node fixture runtime")
}

/// Launches `script` with Node under custody in `root`.
fn launch(root: &std::path::Path, script: &str) -> (Custody, Box<dyn ChildWrapper>) {
    std::fs::write(root.join("fixture.js"), script).unwrap();
    let (mut custody, mut native) = Custody::prepare(&node(), vec!["fixture.js".into()]);
    native
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut command = CommandWrap::from(native);
    custody.wrap(&mut command);
    let child = command.spawn().unwrap();
    custody.attach(child.as_ref());
    (custody, child)
}

async fn wait_for(path: PathBuf) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !path.is_file() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fixture started");
}

async fn stays_still(path: PathBuf) -> bool {
    let before = std::fs::read(&path).unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    before == std::fs::read(&path).unwrap()
}

const GRANDCHILD: &str = r#"const cp=require('child_process');cp.spawn(process.execPath,['-e',`setInterval(()=>require('fs').writeFileSync('heartbeat',String(Date.now())),20)`],{stdio:'ignore'});setInterval(()=>{},1000)"#;

#[test]
fn containment_names_the_platform_mechanism() {
    let (custody, _) = Custody::prepare(&node(), Vec::new());
    let expected = if cfg!(windows) {
        vec![Containment::JobObject]
    } else {
        vec![Containment::Cgroup, Containment::ProcessGroup]
    };
    assert!(expected.contains(&custody.containment()));
}

#[tokio::test]
async fn retire_kills_descendants_before_reporting_the_tree_gone() {
    let root = tempfile::tempdir().unwrap();
    let (custody, mut child) = launch(root.path(), GRANDCHILD);
    wait_for(root.path().join("heartbeat")).await;
    assert!(custody.retire(&mut child).await);
    assert!(stays_still(root.path().join("heartbeat")).await);
}

#[tokio::test]
async fn retire_after_a_normal_exit_is_not_cleanup_uncertainty() {
    let root = tempfile::tempdir().unwrap();
    let (custody, mut child) = launch(root.path(), "process.exit(0)");
    let status = child.inner_mut().wait().await.unwrap();
    assert!(status.success());
    assert!(!custody.join_failed(status.code().unwrap_or(-1), false));
    assert!(custody.retire(&mut child).await);
}

#[tokio::test]
#[cfg(unix)]
async fn unix_setsid_escape_is_killed_only_with_a_cgroup() {
    use nix::{sys::signal, unistd::Pid};
    let root = tempfile::tempdir().unwrap();
    let (custody, mut child) = launch(
        root.path(),
        r#"const c=require('child_process').spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{detached:true,stdio:'ignore'});require('fs').writeFileSync('escaped-pid',String(c.pid));setInterval(()=>{},1000)"#,
    );
    wait_for(root.path().join("escaped-pid")).await;
    let cgroup = custody.containment() == Containment::Cgroup;
    let retired = custody.retire(&mut child).await;
    let pid: i32 = std::fs::read_to_string(root.path().join("escaped-pid"))
        .unwrap()
        .parse()
        .unwrap();
    // A killed cgroup member may stay a zombie briefly until init reaps it.
    let mut alive = true;
    for _ in 0..100 {
        alive = signal::kill(Pid::from_raw(pid), None).is_ok();
        if !alive || !cgroup {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // A plain process group cannot reach a descendant that left it. Clean the
    // fixture up either way.
    let _ = signal::kill(Pid::from_raw(pid), signal::Signal::SIGKILL);
    assert!(retired);
    assert_eq!(alive, !cgroup, "cgroup={cgroup}");
}
