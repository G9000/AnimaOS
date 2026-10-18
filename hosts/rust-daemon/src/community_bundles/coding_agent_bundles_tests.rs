//! The shipped coding-agent bundles under `bundles/coding-agents/`: each
//! manifest inlines its SKILL.md byte for byte, passes the manifest validator
//! against this host, and its skill passes the daemon's SKILL.md parser.
use anima_service_adapters::community_bundles::{
    parse_manifest, validate_manifest, ManifestComponent,
};

struct Shipped {
    id: &'static str,
    slug: &'static str,
    dir: &'static str,
    manifest: &'static str,
    skill: &'static str,
}

macro_rules! shipped {
    ($id:literal, $slug:literal, $dir:literal) => {
        Shipped {
            id: $id,
            slug: $slug,
            dir: $dir,
            manifest: include_str!(concat!(
                "../../../../bundles/coding-agents/",
                $dir,
                "/manifest.json"
            )),
            skill: include_str!(concat!(
                "../../../../bundles/coding-agents/",
                $dir,
                "/SKILL.md"
            )),
        }
    };
}

fn shipped() -> [Shipped; 5] {
    [
        shipped!("anima.coding.claude-code", "claude-code", "claude-code"),
        shipped!("anima.coding.codex", "codex", "codex"),
        shipped!("anima.coding.opencode", "opencode", "opencode"),
        shipped!("anima.coding.gemini-cli", "gemini-cli", "gemini-cli"),
        shipped!("anima.coding.delegate", "coding-agent-handoff", "delegate"),
    ]
}

/// Tool names a skill shows being called as `name {...}`.
fn called_tools(body: &str) -> Vec<String> {
    let mut names = Vec::new();
    for (index, _) in body.match_indices(" {") {
        let start = body[..index]
            .rfind(|c: char| !(c.is_ascii_lowercase() || c == '_'))
            .map_or(0, |at| at + 1);
        let name = &body[start..index];
        let tool = name == "bash"
            || ["bg_", "app_", "acp_"]
                .iter()
                .any(|prefix| name.starts_with(prefix));
        if tool && !names.iter().any(|known| known == name) {
            names.push(name.to_string());
        }
    }
    names
}

#[test]
fn coding_agent_bundles_validate_and_inline_their_skill_files() {
    let runtime = super::service::runtime();
    for bundle in shipped() {
        let manifest = parse_manifest(bundle.manifest.as_bytes())
            .unwrap_or_else(|error| panic!("{}: {error}", bundle.id));
        let valid = validate_manifest(&manifest, &runtime)
            .unwrap_or_else(|error| panic!("{}: {error}", bundle.id));
        assert_eq!(manifest.id, bundle.id);
        assert_eq!(manifest.schema_version, 1);
        assert_eq!(
            manifest.compatibility.required_capabilities,
            ["skills.import-draft.v1"]
        );
        assert!(
            manifest.permissions.external_tools.is_empty(),
            "{}",
            bundle.id
        );
        assert_eq!(valid.source_skill_digests.len(), 1, "{}", bundle.id);

        let [ManifestComponent::Skill { skill_file, .. }] = manifest.components.as_slice() else {
            panic!("{}: expected exactly one skill component", bundle.id);
        };
        assert_eq!(
            skill_file, bundle.skill,
            "{}: manifest is stale; run `node bundles/coding-agents/build.mjs`",
            bundle.id
        );

        let file = crate::skills::parse_skill_file(bundle.skill.as_bytes())
            .unwrap_or_else(|error| panic!("{}: {error}", bundle.id));
        assert_eq!(file.name, bundle.slug);
        assert!(file.description.chars().count() <= 300, "{}", bundle.id);

        let requested = &manifest.permissions.native_tool_names;
        for tool in called_tools(&file.body) {
            assert!(
                requested.contains(&tool),
                "{}: the skill calls {tool} but nativeToolNames omits it",
                bundle.id
            );
        }
        assert!(
            !file.body.contains("tmux"),
            "{}: use the app_* tools, not tmux",
            bundle.id
        );
    }
}

#[test]
fn every_bundle_attributes_its_skill_to_this_repository() {
    for bundle in shipped() {
        let manifest = parse_manifest(bundle.manifest.as_bytes()).unwrap();
        let [entry] = manifest.attribution.as_slice() else {
            panic!("{}: expected one attribution entry", bundle.id);
        };
        assert_eq!(
            entry.source,
            format!("bundles/coding-agents/{}/SKILL.md", bundle.dir),
            "{}",
            bundle.id
        );
        assert!(entry.license.starts_with("MIT"), "{}", bundle.id);
    }
}
