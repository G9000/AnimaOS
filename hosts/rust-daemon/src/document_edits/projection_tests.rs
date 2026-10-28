use super::projection::*;
use anima_document_content::{OfficeFormat, OfficeInventory, OfficeTarget, OfficeTargetValue};

#[test]
fn document_edits_inventory_pages_are_complete_bounded_and_hash_bound() {
    let inventory = OfficeInventory {
        version: 1,
        format: OfficeFormat::Docx,
        targets: (0..100)
            .map(|n| OfficeTarget {
                id: format!("p{n}"),
                label: "Body".into(),
                editable: true,
                value: OfficeTargetValue::Text {
                    text: "x".repeat(300),
                },
            })
            .collect(),
        retained_hyperlinks: vec![],
        limitations: vec!["Content preview only".into()],
    };
    let source = SourceLabel {
        kind: "workspace".into(),
        display_name: "report.docx".into(),
    };
    let hash = "a".repeat(64);
    let page = inventory_page(&inventory, &hash, source.clone(), "scope", None).unwrap();
    assert!(serde_json::to_vec(&page).unwrap().len() <= 12 * 1024);
    assert!(!page.coverage.complete);
    let cursor = page.next_cursor.as_deref().unwrap();
    let next = inventory_page(&inventory, &hash, source.clone(), "scope", Some(cursor)).unwrap();
    assert_ne!(page.targets[0].id, next.targets[0].id);
    assert!(inventory_page(
        &inventory,
        &"b".repeat(64),
        source.clone(),
        "scope",
        Some(cursor)
    )
    .is_err());
    assert!(inventory_page(&inventory, &hash, source, "other", Some(cursor)).is_err());
}

#[test]
fn document_edits_single_oversized_target_is_refused_without_truncation() {
    let inventory = OfficeInventory {
        version: 1,
        format: OfficeFormat::Docx,
        targets: vec![OfficeTarget {
            id: "p".into(),
            label: "Body".into(),
            editable: true,
            value: OfficeTargetValue::Text {
                text: "x".repeat(16 * 1024),
            },
        }],
        retained_hyperlinks: vec![],
        limitations: vec![],
    };
    assert!(inventory_page(
        &inventory,
        &"a".repeat(64),
        SourceLabel {
            kind: "workspace".into(),
            display_name: "a.docx".into()
        },
        "scope",
        None
    )
    .is_err());
}
