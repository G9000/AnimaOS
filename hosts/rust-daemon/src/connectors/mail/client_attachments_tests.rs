use super::*;

#[test]
fn gmail_distinguishes_named_attachments_from_message_body_and_bounds_descriptors() {
    let mut parts = vec![json!({"partId":"body","mimeType":"text/plain","body":{"data":"Ym9keQ"}})];
    parts.extend((0..12).map(|number| json!({"partId":format!("p{number}"),"filename":format!("file{number}.txt"),"mimeType":"text/plain","body":{"size":4,"data":"dGVzdA"}})));
    let mut found = Vec::new();
    collect_gmail_parts(
        &json!({"mimeType":"multipart/mixed","parts":parts}),
        0,
        &mut found,
    )
    .unwrap();
    assert_eq!(found.len(), 12);
    assert!(found
        .iter()
        .all(|part| part["filename"].as_str().unwrap().starts_with("file")));
}

#[test]
fn supported_attachment_types_require_matching_extensions() {
    assert!(supported("invoice.PDF", "application/pdf"));
    assert!(supported("notes.csv", "text/csv"));
    assert!(!supported("invoice.exe", "application/pdf"));
    assert!(!supported("photo.jpg", "image/jpeg"));
}
