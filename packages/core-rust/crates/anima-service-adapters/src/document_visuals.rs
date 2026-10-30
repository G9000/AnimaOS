//! Source-bound page requests carry no pixels and grant no source access.
use anima_core::{Content, DataValue};
use serde::{Deserialize, Serialize};

pub const DOCUMENT_VISUALS_KEY: &str = "documentVisualPagesV1";
pub const WORKSPACE_DOCUMENT_VISUALS_KEY: &str = "workspaceDocumentVisualPagesV1";
/// Covers bounded page images, recognized text, and page/source labels.
pub const DOCUMENT_PAGE_CONTEXT_TOKENS: u64 = 16_384;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisualDocumentSelection {
    pub attachment_id: String,
    pub source_sha256: String,
    pub pages: Vec<u32>,
}

pub fn selections(content: &Content) -> Vec<VisualDocumentSelection> {
    let Some(DataValue::String(json)) = content
        .metadata
        .as_ref()
        .and_then(|map| map.get(DOCUMENT_VISUALS_KEY))
    else {
        return Vec::new();
    };
    if json.len() > 2048 {
        return Vec::new();
    }
    let Ok(values) = serde_json::from_str::<Vec<VisualDocumentSelection>>(json) else {
        return Vec::new();
    };
    if values.len() > 2
        || values.iter().any(|value| {
            value.attachment_id.is_empty()
                || value.attachment_id.len() > 128
                || !value
                    .attachment_id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                || value.source_sha256.len() != 64
                || !value
                    .source_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
                || value.pages.is_empty()
                || value.pages.len() > 2
                || value.pages.iter().any(|page| !(1..=100).contains(page))
                || value.pages.windows(2).any(|pages| pages[0] >= pages[1])
        })
    {
        return Vec::new();
    }
    values
}

pub fn reserved_tokens(content: &Content) -> u64 {
    let attachments = selections(content)
        .iter()
        .map(|value| value.pages.len() as u64)
        .sum::<u64>();
    let workspace =
        workspace_selection(content).map_or(0, |selection| selection.pages.len() as u64);
    attachments
        .saturating_add(workspace)
        .saturating_mul(DOCUMENT_PAGE_CONTEXT_TOKENS)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn untrusted_descriptors_are_closed_and_bounded() {
        let mut content = Content::default();
        for json in [
            r#"[{"attachment_id":"a","source_sha256":"bad","pages":[1]}]"#,
            r#"[{"attachment_id":"a","source_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","pages":[2,1]}]"#,
        ] {
            content.metadata = Some(std::collections::BTreeMap::from([(
                DOCUMENT_VISUALS_KEY.into(),
                DataValue::String(json.into()),
            )]));
            assert!(selections(&content).is_empty());
            assert_eq!(reserved_tokens(&content), 0);
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceDocumentSelection {
    pub path: String,
    pub source_sha256: String,
    pub pages: Vec<u32>,
}

pub fn workspace_selection(content: &Content) -> Option<WorkspaceDocumentSelection> {
    let DataValue::String(json) = content
        .metadata
        .as_ref()?
        .get(WORKSPACE_DOCUMENT_VISUALS_KEY)?
    else {
        return None;
    };
    if json.len() > 2048 {
        return None;
    }
    let value: WorkspaceDocumentSelection = serde_json::from_str(json).ok()?;
    if value.path.is_empty()
        || value.path.len() > 1024
        || !value
            .path
            .rsplit_once('.')
            .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("pdf"))
        || value.path.contains(['\\', ':'])
        || value.path.chars().any(char::is_control)
        || value
            .path
            .split('/')
            .any(|part| matches!(part, "" | "." | ".."))
        || value.source_sha256.len() != 64
        || !value
            .source_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || value.pages.is_empty()
        || value.pages.len() > 2
        || value.pages.iter().any(|page| !(1..=100).contains(page))
        || value.pages.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return None;
    }
    Some(value)
}
