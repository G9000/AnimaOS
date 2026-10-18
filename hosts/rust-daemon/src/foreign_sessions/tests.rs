use std::io::Cursor;

use super::parse::{
    collapse_tool_lines, merge_turns, parse_session, strip_wrappers, ForeignTurn,
    LEADING_ASSISTANT_STUB,
};
use super::walk::{walk_source, Roots};
use super::*;

fn claude(lines: &[serde_json::Value]) -> ParsedSession {
    parse(ForeignSource::ClaudeCode, lines)
}

fn codex(lines: &[serde_json::Value]) -> ParsedSession {
    parse(ForeignSource::Codex, lines)
}

fn parse(source: ForeignSource, lines: &[serde_json::Value]) -> ParsedSession {
    let text = lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    parse_session(source, Cursor::new(text.into_bytes())).0
}

fn texts(parsed: &ParsedSession) -> Vec<(TurnRole, &str)> {
    parsed
        .turns
        .iter()
        .map(|turn| (turn.role, turn.text.as_str()))
        .collect()
}

fn claude_line(kind: &str, content: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "type": kind,
        "cwd": "/work/project",
        "sessionId": "claude-session-1",
        "timestamp": "2026-01-02T03:04:05.000Z",
        "message": {"role": kind, "content": content}
    })
}

#[test]
fn claude_code_logs_keep_typed_turns_and_render_tools() {
    let parsed = claude(&[
        serde_json::json!({"type": "summary", "summary": "Fix the login bug"}),
        claude_line(
            "user",
            serde_json::json!("<command-name>/clear</command-name>"),
        ),
        claude_line(
            "user",
            serde_json::json!([
                {"type": "text", "text": "<system-reminder>be careful</system-reminder>"},
                {"type": "text", "text": "Why does login fail?"},
                {"type": "image", "source": {}}
            ]),
        ),
        claude_line(
            "assistant",
            serde_json::json!([
                {"type": "thinking", "thinking": "secret reasoning"},
                {"type": "text", "text": "Let me look."},
                {"type": "tool_use", "name": "Read", "input": {}}
            ]),
        ),
        claude_line(
            "user",
            serde_json::json!([{"type": "tool_result", "content": "file body"}]),
        ),
        claude_line(
            "assistant",
            serde_json::json!([{"type": "text", "text": "Found it."}]),
        ),
        {
            let mut sidechain = claude_line("user", serde_json::json!("sub-agent prompt"));
            sidechain["isSidechain"] = true.into();
            sidechain
        },
        {
            let mut meta = claude_line("user", serde_json::json!("caveat text"));
            meta["isMeta"] = true.into();
            meta
        },
        serde_json::json!("not an object"),
    ]);
    assert_eq!(
        texts(&parsed),
        [
            (TurnRole::User, "Why does login fail?\n\n[image]"),
            (
                TurnRole::Assistant,
                "Let me look.\n\n[ran tool: Read]\n\nFound it."
            ),
        ]
    );
    assert_eq!(parsed.cwd.as_deref(), Some("/work/project"));
    assert_eq!(
        parsed.foreign_session_id.as_deref(),
        Some("claude-session-1")
    );
    assert_eq!(parsed.title_guess.as_deref(), Some("Fix the login bug"));
    assert_eq!(parsed.turns[0].at_ms, Some(1_767_323_045_000));
}

#[test]
fn claude_code_custom_titles_win_and_garbage_lines_are_skipped() {
    let text = format!(
        "\u{feff}{}\nnot json\n{}\n{}\n",
        claude_line("user", serde_json::json!("Hello there\nsecond line")),
        serde_json::json!({"type": "custom-title", "customTitle": "Greeting"}),
        serde_json::json!({"type": "summary", "summary": "Older summary"}),
    );
    let parsed = parse_session(ForeignSource::ClaudeCode, Cursor::new(text.into_bytes())).0;
    assert_eq!(
        texts(&parsed),
        [(TurnRole::User, "Hello there\nsecond line")]
    );
    assert_eq!(parsed.title_guess.as_deref(), Some("Greeting"));

    let untitled = claude(&[claude_line(
        "user",
        serde_json::json!("  \n Plan the offsite\nmore"),
    )]);
    assert_eq!(untitled.title_guess.as_deref(), Some("Plan the offsite"));
}

#[test]
fn codex_rollouts_read_meta_messages_and_tool_calls() {
    let parsed = codex(&[
        serde_json::json!({"type": "session_meta", "timestamp": "2026-01-02T03:04:05Z",
            "payload": {"id": "codex-1", "cwd": "/work/codex", "source": "vscode"}}),
        serde_json::json!({"type": "response_item", "payload": {"type": "message", "role": "developer",
            "content": [{"type": "input_text", "text": "developer rules"}]}}),
        serde_json::json!({"type": "response_item", "payload": {"type": "message", "role": "user",
            "content": [{"type": "input_text", "text": "<environment_context>\n<cwd>/x</cwd>\n</environment_context>"}]}}),
        serde_json::json!({"type": "response_item", "payload": {"type": "message", "role": "user",
            "content": [{"type": "input_text", "text": "# AGENTS.md instructions for /work\n\nrules"}]}}),
        serde_json::json!({"type": "response_item", "timestamp": "2026-01-02T03:05:00Z",
            "payload": {"type": "message", "role": "user",
            "content": [{"type": "input_text", "text": "Files:\n- a.rs\n## My request:\nRename the parser\n"}]}}),
        serde_json::json!({"type": "response_item", "payload": {"type": "reasoning", "summary": []}}),
        serde_json::json!({"type": "response_item", "payload": {"type": "function_call", "name": "shell", "arguments": "{}"}}),
        serde_json::json!({"type": "response_item", "payload": {"type": "function_call_output", "output": "ok"}}),
        serde_json::json!({"type": "response_item", "payload": {"type": "local_shell_call"}}),
        serde_json::json!({"type": "response_item", "payload": {"type": "message", "role": "assistant",
            "content": [{"type": "output_text", "text": "Renamed."}]}}),
        serde_json::json!({"type": "event_msg", "payload": {"type": "token_count"}}),
    ]);
    assert_eq!(
        texts(&parsed),
        [
            (
                TurnRole::User,
                "Files:\n- a.rs\n## My request:\nRename the parser"
            ),
            (TurnRole::Assistant, "[ran tools: shell ×2]\n\nRenamed."),
        ]
    );
    assert_eq!(parsed.cwd.as_deref(), Some("/work/codex"));
    assert_eq!(parsed.foreign_session_id.as_deref(), Some("codex-1"));
    assert_eq!(parsed.title_guess.as_deref(), Some("Rename the parser"));
    assert!(!parsed.subagent);

    let spawned = codex(&[serde_json::json!({"type": "session_meta",
        "payload": {"session_id": "child", "source": {"subagent": {"other": "guardian"}}}})]);
    assert!(spawned.subagent);
    assert_eq!(spawned.foreign_session_id.as_deref(), Some("child"));
}

#[test]
fn wrappers_are_stripped_and_unclosed_ones_drop_the_text() {
    assert_eq!(
        strip_wrappers(
            "<system-reminder>x</system-reminder>\n<command-name>/a</command-name> typed"
        ),
        "typed"
    );
    assert_eq!(strip_wrappers("<system-reminder>never closed"), "");
    assert_eq!(strip_wrappers("<SYSTEM-REMINDER>x</system-reminder>"), "");
    assert_eq!(
        strip_wrappers("<div>html stays</div>"),
        "<div>html stays</div>"
    );
    assert_eq!(
        strip_wrappers("<system-reminders>not a wrapper"),
        "<system-reminders>not a wrapper"
    );
}

#[test]
fn turns_merge_into_strict_alternation_with_a_leading_user_stub() {
    let turn = |role, text: &str, at_ms| ForeignTurn {
        role,
        text: text.into(),
        at_ms,
    };
    let merged = merge_turns(vec![
        turn(TurnRole::Assistant, "a1", None),
        turn(TurnRole::Assistant, "a2", Some(5)),
        turn(TurnRole::User, "   ", Some(6)),
        turn(TurnRole::User, "u1", Some(7)),
        turn(TurnRole::User, "u2", Some(8)),
    ]);
    assert_eq!(
        merged,
        vec![
            turn(TurnRole::User, LEADING_ASSISTANT_STUB, Some(5)),
            turn(TurnRole::Assistant, "a1\n\na2", Some(5)),
            turn(TurnRole::User, "u1\n\nu2", Some(7)),
        ]
    );
}

#[test]
fn secrets_are_redacted() {
    let text = "keys sk-ant-api03-AbCdEfGhIjKlMnOp and sk-proj-AbCdEfGh12345678 ghp_abcdefghijklmnop1234 \
        xoxb-123456789012-abcdefghijkl AKIAIOSFODNN7EXAMPLE\nAuthorization: Bearer opaqueAccessValue77\n\
        password=hunter2hunter2 token=plaintokenvalue api_key=plainkeyvalue\n\
        -----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAA\n-----END OPENSSH PRIVATE KEY-----\nafter";
    let redacted = redact_secrets(text);
    for secret in [
        "AbCdEfGhIjKlMnOp",
        "sk-proj-AbCdEfGh12345678",
        "ghp_abcdefghijklmnop1234",
        "xoxb-123456789012",
        "AKIAIOSFODNN7EXAMPLE",
        "opaqueAccessValue77",
        "hunter2hunter2",
        "plaintokenvalue",
        "plainkeyvalue",
        "b3BlbnNzaC1rZXktdjEAAAAA",
    ] {
        assert!(!redacted.contains(secret), "{secret} survived: {redacted}");
    }
    assert!(redacted.contains("[redacted private key]\nafter"));
    assert!(redacted.starts_with("keys [redacted]"));
    assert_eq!(
        redact_secrets("-----BEGIN RSA PRIVATE KEY-----\nMIIabc"),
        "[redacted private key]"
    );
    assert_eq!(redact_secrets("plain words stay"), "plain words stay");
}

fn alternating(count: usize) -> ParsedSession {
    ParsedSession {
        turns: (0..count)
            .map(|index| ForeignTurn {
                role: if index % 2 == 0 {
                    TurnRole::User
                } else {
                    TurnRole::Assistant
                },
                text: format!("turn {index}"),
                at_ms: Some(1_000 + index as u64),
            })
            .collect(),
        ..ParsedSession::default()
    }
}

#[test]
fn imports_keep_the_newest_2000_messages_after_a_note() {
    let prepared = prepare_messages(&alternating(2500), 1_000_000);
    assert_eq!(prepared.len(), MAX_IMPORT_MESSAGES);
    assert_eq!(prepared[0].text, EARLIER_MESSAGES_NOTE);
    assert_eq!(prepared[0].role, TurnRole::User);
    assert_eq!(prepared[1].role, TurnRole::Assistant);
    assert_eq!(prepared.last().unwrap().text, "turn 2499");
    assert_eq!(prepared[1].text, "turn 501");
    assert!(prepared
        .windows(2)
        .all(|pair| pair[0].role != pair[1].role && pair[0].at_ms <= pair[1].at_ms));

    // An odd count leaves an assistant turn first among the kept ones too.
    let odd = prepare_messages(&alternating(2501), 1_000_000);
    assert_eq!(odd.len(), MAX_IMPORT_MESSAGES - 1);
    assert_eq!(odd[1].role, TurnRole::Assistant);
    assert_eq!(odd.last().unwrap().text, "turn 2500");

    let small = prepare_messages(&alternating(3), 1_000_000);
    assert_eq!(small.len(), 3);
    assert_eq!(small[0].text, "turn 0");
}

#[test]
fn messages_are_cut_redacted_and_timed_before_the_import() {
    let mut parsed = alternating(4);
    parsed.turns[0].text = format!("token=plaintokenvalue {}", "é".repeat(20_000));
    parsed.turns[1].at_ms = None;
    parsed.turns[2].at_ms = Some(500);
    parsed.turns[3].at_ms = Some(9_999_999);
    let prepared = prepare_messages(&parsed, 5_000);
    assert!(prepared[0].text.len() <= MAX_MESSAGE_BYTES);
    assert!(prepared[0].text.ends_with('…'));
    assert!(prepared[0].text.starts_with("token=[redacted] "));
    assert_eq!(
        prepared
            .iter()
            .map(|message| message.at_ms)
            .collect::<Vec<_>>(),
        [1_000, 1_000, 1_000, 4_999]
    );
    assert_eq!(
        prepared
            .iter()
            .map(|message| message.timed)
            .collect::<Vec<_>>(),
        [true, false, true, true]
    );

    let messages = transcript_messages("agent", "chat:x", &prepared, 5_000);
    assert_eq!(crate::history::message_ordinal(&messages[3].id), 3);
    assert!(messages.iter().all(|message| message.room_id == "chat:x"));
    assert_eq!(messages[1].role, anima_core::MessageRole::Assistant);
}

#[test]
fn the_hot_tail_starts_at_a_user_turn_unless_the_store_is_ephemeral() {
    let prepared = prepare_messages(&alternating(451), 1_000_000);
    let messages = transcript_messages("agent", "chat:x", &prepared, 1_000_000);
    assert_eq!(hot_start(&messages, true), 0);
    let start = hot_start(&messages, false);
    assert_eq!(start, 252);
    assert_eq!(messages[start].role, anima_core::MessageRole::User);
    assert_eq!(hot_start(&messages[..150], false), 0);
}

#[test]
fn import_titles_are_labelled_and_redacted() {
    let mut parsed = alternating(2);
    parsed.title_guess = Some(format!(
        "Use sk-proj-AbCdEfGh12345678 for {}",
        "x".repeat(100)
    ));
    let title = import_title(&parsed);
    assert!(
        title.starts_with("Imported · Use [redacted] for "),
        "{title}"
    );
    assert!(title.chars().count() <= "Imported · ".chars().count() + 60);
    parsed.title_guess = None;
    assert_eq!(import_title(&parsed), "Imported conversation");
    assert_eq!(
        display_title(&parsed, ForeignSource::Codex),
        "Codex session"
    );
}

#[test]
fn walks_find_logs_skip_escaping_links_and_page_newest_first() {
    let temp = tempfile::tempdir().unwrap();
    let claude_root = temp.path().join("claude").join("projects");
    let project = claude_root.join("-work-project");
    std::fs::create_dir_all(project.join("subagents")).unwrap();
    let line = claude_line("user", serde_json::json!("Hello")).to_string();
    std::fs::write(project.join("one.jsonl"), &line).unwrap();
    std::fs::write(project.join("notes.txt"), &line).unwrap();
    std::fs::write(project.join("subagents").join("deep.jsonl"), &line).unwrap();
    std::fs::write(project.join("empty.jsonl"), "").unwrap();
    let outside = temp.path().join("outside.jsonl");
    std::fs::write(&outside, &line).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, project.join("escape.jsonl")).unwrap();
        std::os::unix::fs::symlink(project.join("one.jsonl"), project.join("alias.jsonl")).unwrap();
        std::fs::create_dir_all(temp.path().join("linked")).unwrap();
        std::fs::write(temp.path().join("linked").join("x.jsonl"), &line).unwrap();
        std::os::unix::fs::symlink(temp.path().join("linked"), claude_root.join("linked")).unwrap();
    }
    let found = walk_source(ForeignSource::ClaudeCode, &claude_root);
    let mut names = found
        .iter()
        .map(|candidate| {
            candidate
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, ["empty.jsonl", "one.jsonl"]);
    let one = found
        .iter()
        .find(|candidate| candidate.path.ends_with("one.jsonl"))
        .unwrap();
    assert_eq!(one.handle, walk::handle_of(&one.path));
    assert_eq!(one.handle.len(), 64);

    let codex_root = temp.path().join("codex").join("sessions");
    let day = codex_root.join("2026").join("01").join("02");
    std::fs::create_dir_all(&day).unwrap();
    std::fs::write(
        day.join("rollout-a.jsonl"),
        serde_json::json!({"type": "session_meta", "payload": {"id": "c-1", "cwd": "/c"}}).to_string()
            + "\n"
            + &serde_json::json!({"type": "response_item", "payload": {"type": "message", "role": "user",
                "content": [{"type": "input_text", "text": "Codex hello"}]}})
            .to_string(),
    )
    .unwrap();
    std::fs::write(day.join("other.jsonl"), &line).unwrap();
    let roots = Roots {
        claude_code: Some(claude_root.clone()),
        codex: Some(codex_root),
    };
    let page = list_page(&roots, None, 0);
    // The empty log is listed by the walk but left out of its page.
    assert_eq!(page.sessions.len(), 2);
    assert_eq!(page.next_offset, None);
    let codex_row = page
        .sessions
        .iter()
        .find(|row| row.source == ForeignSource::Codex)
        .unwrap();
    assert_eq!(codex_row.title, "Codex hello");
    assert_eq!(codex_row.cwd.as_deref(), Some("/c"));
    assert_eq!(codex_row.foreign_session_id.as_deref(), Some("c-1"));
    assert_eq!(
        list_page(&roots, Some(ForeignSource::ClaudeCode), 0)
            .sessions
            .len(),
        1
    );

    assert_eq!(resolve(&roots, &one.handle).unwrap().path, one.path);
    assert_eq!(
        resolve(&roots, &"0".repeat(64)),
        Err(ForeignError::NotFound)
    );
    assert_eq!(
        resolve(&roots, "../etc/passwd"),
        Err(ForeignError::NotFound)
    );
    let outside_handle = walk::handle_of(&std::fs::canonicalize(&outside).unwrap());
    assert_eq!(
        resolve(&roots, &outside_handle),
        Err(ForeignError::NotFound)
    );

    // The size cap is checked before anything is read.
    let big = project.join("big.jsonl");
    std::fs::File::create(&big)
        .unwrap()
        .set_len(MAX_FILE_BYTES + 1)
        .unwrap();
    let big = walk_source(ForeignSource::ClaudeCode, &claude_root)
        .into_iter()
        .find(|candidate| candidate.path.ends_with("big.jsonl"))
        .unwrap();
    assert_eq!(load(&big), Err(ForeignError::TooLarge));
    let empty = found
        .iter()
        .find(|candidate| candidate.path.ends_with("empty.jsonl"))
        .unwrap();
    assert_eq!(load(empty), Err(ForeignError::Empty));
    assert_eq!(load(one).unwrap().turns.len(), 1);
}

#[test]
fn provenance_dedupes_on_the_foreign_id_else_the_log() {
    let candidate = Candidate {
        handle: "a".repeat(64),
        source: ForeignSource::ClaudeCode,
        path: "/x".into(),
        modified_ms: 0,
        size_bytes: 0,
    };
    let mut parsed = alternating(2);
    parsed.foreign_session_id = Some("same".into());
    let first = provenance(&candidate, &parsed);
    let moved = provenance(
        &Candidate {
            handle: "b".repeat(64),
            ..candidate.clone()
        },
        &parsed,
    );
    assert!(first.same_conversation(&moved));
    let mut codex = moved.clone();
    codex.tool = "codex".into();
    assert!(!first.same_conversation(&codex));
    parsed.foreign_session_id = None;
    let anonymous = provenance(&candidate, &parsed);
    assert!(!first.same_conversation(&anonymous));
    assert!(anonymous.same_conversation(&provenance(&candidate, &parsed)));
    assert!(!anonymous.same_conversation(&provenance(
        &Candidate {
            handle: "b".repeat(64),
            ..candidate
        },
        &parsed
    )));
}

#[test]
fn runs_of_tool_lines_fold_into_one_counted_line() {
    let text = "Looking.\n\n[ran tool: Bash]\n\n[ran tool: Read]\n\n[ran tool: Bash]\n\n[ran tool: mcp__Claude_Browser__browser_batch]\n\nFound it.\n\n[ran tool: Edit]";
    assert_eq!(
        collapse_tool_lines(text),
        "Looking.\n\n[ran tools: Bash ×2, Read, browser_batch]\n\nFound it.\n\n[ran tool: Edit]"
    );
    assert_eq!(collapse_tool_lines("plain"), "plain");
}
