use crate::{
    fs::{confine, slice_lines},
    log::{EventLog, LOG_CAP},
    AcpEvent,
};

fn message(text: &str) -> AcpEvent {
    AcpEvent::AgentMessage { text: text.into() }
}

#[test]
fn chunks_merge_until_a_reader_has_seen_them() {
    let mut log = EventLog::new();
    log.push_chunk(message("Hel"), None);
    log.push_chunk(message("lo"), None);
    let (events, cursor) = log.since(0);
    assert_eq!(events, vec![message("Hello")]);
    // Already delivered: later text starts a new event instead of vanishing.
    log.push_chunk(message(" world"), None);
    let (events, _) = log.since(cursor);
    assert_eq!(events, vec![message(" world")]);
}

#[test]
fn chunks_of_different_kinds_or_messages_stay_apart() {
    let mut log = EventLog::new();
    log.push_chunk(message("a"), Some("m1"));
    log.push_chunk(message("b"), Some("m2"));
    log.push_chunk(AcpEvent::AgentThought { text: "t".into() }, Some("m2"));
    log.push(AcpEvent::Stderr { text: "e1".into() });
    log.push(AcpEvent::Stderr { text: "e2".into() });
    let (events, _) = log.since(0);
    assert_eq!(
        events,
        vec![
            message("a"),
            message("b"),
            AcpEvent::AgentThought { text: "t".into() },
            AcpEvent::Stderr {
                text: "e1\ne2".into()
            },
        ]
    );
}

#[test]
fn log_drops_oldest_events_past_its_budget() {
    let mut log = EventLog::new();
    let big = "x".repeat(60 * 1024);
    for _ in 0..40 {
        log.push(AcpEvent::Stderr { text: big.clone() });
        log.push(AcpEvent::TurnEnded {
            stop_reason: "end_turn".into(),
        });
    }
    assert!(log.dropped() > 0);
    let (events, next) = log.since(0);
    assert_eq!(next, 80);
    assert_eq!(events.len() as u64, 80 - log.dropped());
    let kept: usize = events
        .iter()
        .map(|e| match e {
            AcpEvent::Stderr { text } => text.len(),
            _ => 0,
        })
        .sum();
    assert!(kept <= LOG_CAP);
}

#[test]
fn oversized_text_is_truncated() {
    let mut log = EventLog::new();
    log.push(AcpEvent::Stderr {
        text: "y".repeat(LOG_CAP * 2),
    });
    let (events, _) = log.since(0);
    let AcpEvent::Stderr { text } = &events[0] else {
        panic!("stderr event");
    };
    assert!(text.len() < LOG_CAP / 2 && text.ends_with("[truncated]"));
}

#[test]
fn confine_allows_new_paths_inside_and_refuses_escapes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    assert_eq!(
        confine(&root, &root.join("a/b/new.txt")).unwrap(),
        root.join("a/b/new.txt")
    );
    assert_eq!(
        confine(&root, std::path::Path::new("rel.txt")).unwrap(),
        root.join("rel.txt")
    );
    assert_eq!(
        confine(&root, &root.join("a/../b.txt")).unwrap(),
        root.join("b.txt")
    );
    assert!(confine(&root, &root.join("../outside.txt")).is_err());
    assert!(confine(&root, &root.join("a/../../outside.txt")).is_err());
    assert!(confine(&root, &dir.path().join("outside.txt")).is_err());
}

#[cfg(unix)]
#[test]
fn confine_refuses_symlinks_that_leave_the_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let outside = dir.path().join("outside");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();
    std::os::unix::fs::symlink(outside.join("missing"), root.join("dangling")).unwrap();
    std::os::unix::fs::symlink(root.join("real"), root.join("inner")).unwrap();
    std::fs::create_dir(root.join("real")).unwrap();
    assert!(confine(&root, &root.join("escape/x.txt")).is_err());
    assert!(confine(&root, &root.join("escape")).is_err());
    assert!(confine(&root, &root.join("dangling")).is_err());
    assert_eq!(
        confine(&root, &root.join("inner/x.txt")).unwrap(),
        root.join("real/x.txt")
    );
}

#[test]
fn line_and_limit_are_one_based_line_windows() {
    let text = "a\nb\nc\nd\n".to_string();
    assert_eq!(slice_lines(text.clone(), None, None), text);
    assert_eq!(slice_lines(text.clone(), Some(2), Some(2)), "b\nc\n");
    assert_eq!(slice_lines(text.clone(), Some(4), None), "d\n");
    assert_eq!(slice_lines(text, None, Some(1)), "a\n");
}
