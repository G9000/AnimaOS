use super::*;
use serde_json::json;

#[test]
fn runtime_serde_attachment_mime_preserves_serde_and_runtime_roundtrips() {
    for (kind, serde_kind, mime) in [
        ("image", "Image", Some("image/png")),
        ("file", "File", None),
        ("url", "Url", None),
    ] {
        let mut legacy =
            json!({"attachment_type": serde_kind, "name":"exact name", "data":"files/exact"});
        let old: Attachment = serde_json::from_value(legacy.clone()).unwrap();
        assert_eq!(serde_json::to_value(&old).unwrap(), legacy);
        if let Some(mime) = mime {
            legacy["mime_type"] = json!(mime);
            let attachment: Attachment = serde_json::from_value(legacy.clone()).unwrap();
            assert_eq!(serde_json::to_value(&attachment).unwrap(), legacy);
        }
        let mut runtime = json!({"type":kind, "name":"exact name", "data":"files/exact"});
        if let Some(mime) = mime {
            runtime["mimeType"] = json!(mime);
        }
        let attachment = attachment_from_json(&runtime).unwrap();
        let content = Content {
            attachments: Some(vec![attachment]),
            ..Content::default()
        };
        let encoded = data_value_to_json(&content_data_value(Some(&content)));
        assert_eq!(encoded["attachments"][0], runtime);
        assert_eq!(content_from_json(&encoded).unwrap(), content);
    }
}

#[test]
fn runtime_serde_attachment_null_mime_is_absent_and_wrong_type_is_rejected() {
    let mut value = json!({"type":"image", "name":"x", "data":"files/x", "mimeType":null});
    let attachment = attachment_from_json(&value).unwrap();
    let content = Content {
        attachments: Some(vec![attachment]),
        ..Content::default()
    };
    let encoded = data_value_to_json(&content_data_value(Some(&content)));
    assert!(encoded["attachments"][0].get("mimeType").is_none());
    value["mimeType"] = json!(42);
    assert!(attachment_from_json(&value).is_none());
}
