use anima_document_content::{
    check_office_document_text, inspect_office_document, prepare_office_edit, OfficeEditSpec,
    OfficeFormat,
};
use serde_json::{json, Value};

fn html_format() -> OfficeFormat {
    serde_json::from_value(json!("html")).expect("HTML uses the additive document format")
}

fn edit(start: usize, end: usize, expected: &str, replacement: &str) -> Value {
    json!({"type":"html_replace_text","start_utf8":start,"end_utf8":end,
        "expected_text":expected,"new_text":replacement})
}

fn spec(operations: Vec<Value>) -> OfficeEditSpec {
    serde_json::from_value(json!({"operations":operations})).expect("typed HTML operation")
}

#[test]
fn html_editing_preserves_untouched_source_and_escapes_literal_replacement() {
    let source = "<!DOCTYPE html>\r\n<html><body><p class='unchanged'>First &amp; 猫</p><p>First</p></body></html>";
    let start = source.rfind("First").unwrap();
    let result = prepare_office_edit(
        html_format(),
        source.as_bytes(),
        &spec(vec![edit(start, start + 5, "First", "<new> & 猫")]),
    )
    .expect("exact text edit should prepare");
    let expected = format!(
        "{}&lt;new&gt; &amp; 猫{}",
        &source[..start],
        &source[start + 5..]
    );
    assert_eq!(result.bytes(), expected.as_bytes());
    let review = result.review();
    assert_eq!(review.changes.len(), 1);
    assert_eq!(review.changes[0].before, "First");
    assert_eq!(review.changes[0].after, "&lt;new&gt; &amp; 猫");
    assert_eq!(review.changes[0].operation_index, Some(0));
    assert!(review.preservation.unchanged_bytes_verified);
    assert!(!review.preservation.container_normalized);
    assert_eq!(review.before.format, html_format());
    assert_eq!(review.after.format, html_format());
}

#[test]
fn html_inventory_exposes_raw_entities_and_absolute_utf8_offsets() {
    let source = "<p>猫 &amp; dog</p><p>dog</p>";
    let inventory = inspect_office_document(html_format(), source.as_bytes()).unwrap();
    let first = serde_json::to_value(&inventory.targets[0]).unwrap();
    assert_eq!(first["id"], "html:3:16");
    assert_eq!(first["value"]["kind"], "html_text");
    assert_eq!(first["value"]["start_utf8"], 3);
    assert_eq!(first["value"]["end_utf8"], 16);
    assert_eq!(first["value"]["text"], "猫 &amp; dog");
    assert!(inventory.targets.iter().all(|target| target.editable));
    assert!(inventory
        .limitations
        .iter()
        .any(|text| text.contains("source")));
}

#[test]
fn html_editing_rejects_stale_multibyte_overlapping_or_markup_selection() {
    let source = "<p title='attribute'>猫 &amp; dog</p><script>danger</script><!-- hidden -->";
    let start = source.find('猫').unwrap();
    for operation in [
        edit(start, start + 3, "dog", "new"),
        edit(start + 1, start + 3, "猫", "new"),
        edit(1, 2, "p", "new"),
        edit(
            source.find("attribute").unwrap(),
            source.find("attribute").unwrap() + 9,
            "attribute",
            "new",
        ),
        edit(
            source.find("danger").unwrap(),
            source.find("danger").unwrap() + 6,
            "danger",
            "new",
        ),
        edit(
            source.find("hidden").unwrap(),
            source.find("hidden").unwrap() + 6,
            "hidden",
            "new",
        ),
    ] {
        assert!(
            prepare_office_edit(
                html_format(),
                source.as_bytes(),
                &spec(vec![operation.clone()])
            )
            .is_err(),
            "{operation}"
        );
    }
    assert!(prepare_office_edit(
        html_format(),
        source.as_bytes(),
        &spec(vec![
            edit(start, start + 3, "猫", "one"),
            edit(start, start + 3, "猫", "two")
        ])
    )
    .is_err());
}

#[test]
fn html_inventory_excludes_active_and_ambiguous_contexts() {
    let source = "<html><head><style>.x{color:red}</style></head><body><p>Shown</p><script>script text</script><template><p>template text</p></template><svg><text>foreign text</text></svg><textarea>raw text</textarea></body></html>";
    let inventory = inspect_office_document(html_format(), source.as_bytes()).unwrap();
    assert_eq!(inventory.targets.len(), 1);
    assert_eq!(
        serde_json::to_value(&inventory.targets[0]).unwrap()["value"]["text"],
        "Shown"
    );
    for source in [
        "<p title='unterminated>Text</p>",
        "<p>Text",
        "<p>Text</div>",
        "<script>missing end",
    ] {
        assert!(
            inspect_office_document(html_format(), source.as_bytes()).is_err(),
            "{source}"
        );
    }
}

#[test]
fn html_editing_enforces_source_candidate_and_selection_bounds() {
    assert!(inspect_office_document(html_format(), &vec![b'a'; 1024 * 1024 + 1]).is_err());
    assert!(inspect_office_document(html_format(), b"\xff").is_err());
    let source = format!("<p>{}</p>", "a".repeat(8193));
    assert!(prepare_office_edit(
        html_format(),
        source.as_bytes(),
        &spec(vec![edit(3, 8196, &"a".repeat(8193), "new")])
    )
    .is_err());
    assert!(prepare_office_edit(
        html_format(),
        b"<p>old</p>",
        &spec(vec![edit(3, 6, "old", &"x".repeat(8193))])
    )
    .is_err());
}

#[test]
fn html_privacy_visitor_checks_retained_script_and_attribute_bytes() {
    for source in [
        "<p title='private-marker'>Public</p>",
        "<script>private-marker</script><p>Public</p>",
        "<!--private-marker--><p>Public</p>",
    ] {
        assert!(
            !check_office_document_text(html_format(), source.as_bytes(), |text| !text
                .contains("private-marker"))
            .unwrap()
        );
    }
    assert!(check_office_document_text(html_format(), b"<p>Public</p>", |_| true).unwrap());
}

#[test]
fn html_inventory_accepts_boolean_attributes_and_chunks_large_raw_runs() {
    let source = format!(
        "<p hidden data-label='kept'>{}</p>",
        "猫 &amp; ".repeat(500)
    );
    let inventory = inspect_office_document(html_format(), source.as_bytes()).unwrap();
    assert!(inventory.targets.len() > 1);
    let mut reconstructed = String::new();
    for target in inventory.targets {
        let value = serde_json::to_value(target.value).unwrap();
        reconstructed.push_str(value["text"].as_str().unwrap());
        assert!(serde_json::to_vec(&value).unwrap().len() < 12 * 1024);
    }
    assert_eq!(reconstructed, "猫 &amp; ".repeat(500));
}

#[test]
fn html_edits_refuse_split_entities_and_candidates_above_one_mib() {
    assert!(prepare_office_edit(
        html_format(),
        b"<p>A &amp; B</p>",
        &spec(vec![edit(7, 9, "mp", "x")])
    )
    .is_err());
    let source = format!("<p>a</p><!--{}-->", "x".repeat(1024 * 1024 - 15));
    assert_eq!(source.len(), 1024 * 1024);
    assert!(inspect_office_document(html_format(), source.as_bytes()).is_ok());
    assert!(prepare_office_edit(
        html_format(),
        source.as_bytes(),
        &spec(vec![edit(3, 4, "a", "longer")])
    )
    .is_err());
}
