use super::*;
use std::collections::VecDeque;
use std::sync::Mutex;

const COMMIT: &str = "1111111111111111111111111111111111111111";
const ROOT: &str = "2222222222222222222222222222222222222222";
const SUBTREE: &str = "3333333333333333333333333333333333333333";
const MAIN: &str = "4444444444444444444444444444444444444444";
const SUPPORT: &str = "5555555555555555555555555555555555555555";

struct FakeApi(Mutex<VecDeque<(Vec<String>, bool, bool, Vec<u8>)>>);
impl GitHubApi for FakeApi {
    fn read(&self, request: ApiRequest) -> BoxFuture<'_, Result<Vec<u8>, GitHubSkillError>> {
        Box::pin(async move {
            let (segments, raw, recursive, bytes) = self
                .0
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected GitHub request");
            assert_eq!(request.segments, segments);
            assert_eq!(request.raw, raw);
            assert_eq!(request.recursive, recursive);
            if bytes.len() > request.limit {
                return Err(GitHubSkillError::Limits);
            }
            Ok(bytes)
        })
    }
}
fn json(value: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&value).unwrap()
}
fn path(tail: &[&str]) -> Vec<String> {
    ["repos", "example", "skills"]
        .into_iter()
        .chain(tail.iter().copied())
        .map(str::to_string)
        .collect()
}
fn entry(name: &str, mode: &str, kind: &str, sha: &str, size: usize) -> serde_json::Value {
    serde_json::json!({"path":name,"mode":mode,"type":kind,"sha":sha,"size":size})
}
fn skill() -> Vec<u8> {
    super::super::test_support::skill_text("Github notes").into_bytes()
}
fn raw_path(name: &str) -> Vec<String> {
    ["example", "skills", COMMIT, "notes"]
        .into_iter()
        .chain(name.split('/'))
        .map(str::to_string)
        .collect()
}
fn fake(mut entries: Vec<serde_json::Value>, blobs: Vec<(&str, Vec<u8>)>) -> FakeApi {
    let mut downloads = Vec::new();
    for entry in &mut entries {
        if entry["type"] == "blob" {
            if let Some((_, bytes)) = blobs
                .iter()
                .find(|(sha, _)| Some(*sha) == entry["sha"].as_str())
            {
                entry["sha"] = git_blob_hash(bytes).into();
                let name = entry["path"].as_str().unwrap();
                if ![".gitignore", ".gitattributes", ".DS_Store"].contains(&name) {
                    downloads.push((name.to_string(), bytes.clone()));
                }
            }
        }
    }
    downloads.sort_by(
        |(left, _), (right, _)| match (left == "SKILL.md", right == "SKILL.md") {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => left.cmp(right),
        },
    );
    let mut replies = VecDeque::from([
        (
            path(&["commits", "feature/setup"]),
            false,
            false,
            json(serde_json::json!({"sha":COMMIT,"commit":{"tree":{"sha":ROOT}}})),
        ),
        (
            path(&["git", "trees", ROOT]),
            false,
            false,
            json(
                serde_json::json!({"truncated":false,"tree":[entry("notes","040000","tree",SUBTREE,0)]}),
            ),
        ),
        (
            path(&["git", "trees", SUBTREE]),
            false,
            true,
            json(serde_json::json!({"truncated":false,"tree":entries})),
        ),
    ]);
    for (name, bytes) in downloads {
        replies.push_back((raw_path(&name), true, false, bytes));
    }
    FakeApi(Mutex::new(replies))
}
fn request() -> GitHubSkillRequest {
    GitHubSkillRequest {
        repository: "example/skills".into(),
        revision: Some("feature/setup".into()),
        path: "notes".into(),
        slug: None,
    }
}

#[test]
fn github_blob_integrity_uses_git_object_framing() {
    assert_eq!(
        git_blob_hash(b"hello\n"),
        "ce013625030ba8dba906f756967f9e9ca394464a"
    );
}

#[tokio::test]
#[ignore = "downloads the pinned public last30days package; run explicitly for network acceptance"]
async fn github_live_last30days_acquisition_preserves_complete_pinned_package() {
    let request = GitHubSkillRequest {
        repository: "mvanhorn/last30days-skill".into(),
        revision: Some("7efc0ef764765ed9ceff5ce1204462b35b89d680".into()),
        path: "skills/last30days".into(),
        slug: None,
    };
    let api = PublicGitHub::new().unwrap();
    let package = acquire(&request, &api).await.unwrap();
    assert_eq!(package.source.commit, request.revision.unwrap());
    assert_eq!(package.files.len() + 1, 159);
    let total = package.bytes.len()
        + package
            .files
            .iter()
            .map(|file| file.decoded_len().unwrap())
            .sum::<usize>();
    assert_eq!(total, 17_599_252);
    assert!(package.source.ignored_paths.is_empty());
    assert!(package.files.iter().any(|file| file.path == ".skillignore"));
    assert!(package
        .files
        .iter()
        .any(|file| file.path == "scripts/last30days.py"));
    assert!(package
        .files
        .iter()
        .any(|file| file.path == "assets/claude-code-rap.mp3"));
}

#[test]
fn github_request_rejects_credentials_urls_and_path_escapes() {
    for repository in [
        "https://github.com/example/skills",
        "x/y/z",
        "x/..",
        "token@github.com/repo",
        "x/y?token=secret",
        "../y",
    ] {
        let mut request = request();
        request.repository = repository.into();
        assert!(request.validate().is_err(), "{repository}");
    }
    for path in [
        "../notes",
        "/notes",
        "notes/../notes",
        "notes//x",
        "C:\\notes",
        "notes?token=x",
    ] {
        let mut request = request();
        request.path = path.into();
        assert!(request.validate().is_err(), "{path}");
    }
    for revision in [
        "..",
        "x/../y",
        "https://example.com",
        "x\r\ny",
        "x?auth=x",
        "/branch",
    ] {
        let mut request = request();
        request.revision = Some(revision.into());
        assert!(request.validate().is_err(), "{revision}");
    }
    assert!(request().validate().is_ok());
}

#[tokio::test]
async fn github_import_pins_tree_and_blobs_and_keeps_complete_supported_package() {
    let bytes = skill();
    let api = fake(
        vec![
            entry("SKILL.md", "100644", "blob", MAIN, bytes.len()),
            entry("references", "040000", "tree", SUPPORT, 0),
            entry("references/api.md", "100644", "blob", SUPPORT, 10),
            entry("LICENSE", "100644", "blob", SUPPORT, 10),
        ],
        vec![(MAIN, bytes.clone()), (SUPPORT, b"Read this.".to_vec())],
    );
    let package = acquire(&request(), &api).await.unwrap();
    assert_eq!(package.bytes, bytes);
    assert_eq!(package.files[0].path, "LICENSE");
    assert_eq!(package.files[1].path, "references/api.md");
    assert_eq!(package.files[1].content, "Read this.");
    assert_eq!(package.source.commit, COMMIT);
    assert!(package.source.ignored_paths.is_empty());
    assert_eq!(
        package.source.source_url,
        format!("https://github.com/example/skills/tree/{COMMIT}/notes")
    );
    assert_eq!(
        package.source.package_hash,
        anima_core::skill_packages::skill_package_hash(&package.bytes, &package.files)
    );
    assert!(api.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn github_import_refuses_incomplete_or_unsafe_tree_without_fetching_blobs() {
    for bad in [
        entry("scripts/run.py", "120000", "blob", SUPPORT, 4),
        entry("assets", "160000", "commit", SUPPORT, 0),
        entry("references/../escape.md", "100644", "blob", SUPPORT, 4),
        entry("scripts/installer.exe", "100644", "blob", SUPPORT, 4),
    ] {
        let api = fake(
            vec![
                entry("SKILL.md", "100644", "blob", MAIN, skill().len()),
                bad,
            ],
            vec![],
        );
        assert!(acquire(&request(), &api).await.is_err());
        assert!(api.0.lock().unwrap().is_empty());
    }
    let api = fake(
        vec![entry("references/file.md", "100644", "blob", SUPPORT, 1)],
        vec![],
    );
    assert!(acquire(&request(), &api).await.is_err());
}

#[tokio::test]
async fn github_import_keeps_large_complete_package_with_binary_assets_and_root_files() {
    let bytes = skill();
    let image = b"\x89PNG\r\n\x1a\n\0\xffsynthetic fixture".to_vec();
    let mut entries = vec![
        entry("SKILL.md", "100644", "blob", MAIN, bytes.len()),
        entry("README.md", "100644", "blob", SUPPORT, 10),
        entry("assets/cover.png", "100644", "blob", ROOT, image.len()),
        entry(".gitignore", "100644", "blob", SUPPORT, 10),
        entry(".skillignore", "100644", "blob", SUPPORT, 10),
    ];
    for index in 0..80 {
        entries.push(entry(
            &format!("engine/lib/module-{index}.py"),
            "100644",
            "blob",
            SUPPORT,
            10,
        ));
    }
    let api = fake(
        entries,
        vec![
            (MAIN, bytes.clone()),
            (SUPPORT, b"Read this.".to_vec()),
            (ROOT, image.clone()),
        ],
    );
    let package = acquire(&request(), &api).await.unwrap();
    assert_eq!(package.bytes, bytes);
    assert_eq!(package.files.len(), 83);
    let binary = package
        .files
        .iter()
        .find(|file| file.path == "assets/cover.png")
        .unwrap();
    assert_eq!(binary.bytes().unwrap().as_ref(), image);
    assert_eq!(serde_json::to_value(binary).unwrap()["encoding"], "base64");
    assert!(package.files.iter().any(|file| file.path == "README.md"));
    assert!(package.files.iter().any(|file| file.path == ".skillignore"));
    assert_eq!(package.source.ignored_paths, [".gitignore"]);
    assert!(api.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn github_import_rejects_same_length_substitutions_missing_files_and_lfs_pointers() {
    let bytes = skill();
    let api = fake(
        vec![entry("SKILL.md", "100644", "blob", MAIN, bytes.len())],
        vec![(MAIN, bytes.clone())],
    );
    let mut replies = api.0.lock().unwrap();
    let download = replies.back_mut().unwrap();
    download.3[0] ^= 1;
    drop(replies);
    assert!(matches!(
        acquire(&request(), &api).await,
        Err(GitHubSkillError::Protocol)
    ));

    let pointer =
        b"version https://git-lfs.github.com/spec/v1\noid sha256:000\nsize 200000\n".to_vec();
    let api = fake(
        vec![
            entry("SKILL.md", "100644", "blob", MAIN, bytes.len()),
            entry("assets/image.png", "100644", "blob", SUPPORT, pointer.len()),
        ],
        vec![(MAIN, bytes), (SUPPORT, pointer)],
    );
    assert!(matches!(
        acquire(&request(), &api).await,
        Err(GitHubSkillError::Package)
    ));

    struct Missing(FakeApi);
    impl GitHubApi for Missing {
        fn read(&self, request: ApiRequest) -> BoxFuture<'_, Result<Vec<u8>, GitHubSkillError>> {
            if request.raw {
                Box::pin(async { Err(GitHubSkillError::NotPublic) })
            } else {
                self.0.read(request)
            }
        }
    }
    let missing = Missing(fake(
        vec![entry("SKILL.md", "100644", "blob", MAIN, skill().len())],
        vec![],
    ));
    assert!(matches!(
        acquire(&request(), &missing).await,
        Err(GitHubSkillError::NotPublic)
    ));
}

#[tokio::test]
async fn github_import_rejects_file_directory_prefix_collisions_before_downloading() {
    let api = fake(
        vec![
            entry("SKILL.md", "100644", "blob", MAIN, skill().len()),
            entry("engine.py", "100644", "blob", SUPPORT, 1),
            entry("engine.py/module.py", "100644", "blob", SUPPORT, 1),
        ],
        vec![],
    );
    assert!(matches!(
        acquire(&request(), &api).await,
        Err(GitHubSkillError::Package)
    ));
    assert!(api.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn github_import_refuses_oversize_duplicate_binary_text_and_truncated_content() {
    let bytes = skill();
    for entries in [
        vec![entry(
            "SKILL.md",
            "100644",
            "blob",
            MAIN,
            MAX_SKILL_FILE_BYTES + 1,
        )],
        vec![
            entry("SKILL.md", "100644", "blob", MAIN, bytes.len()),
            entry(
                "references/a.md",
                "100644",
                "blob",
                SUPPORT,
                MAX_SKILL_SUPPORTING_FILE_BYTES + 1,
            ),
        ],
        vec![
            entry("SKILL.md", "100644", "blob", MAIN, bytes.len()),
            entry("references/a.md", "100644", "blob", SUPPORT, 1),
            entry("references/A.md", "100644", "blob", SUPPORT, 1),
        ],
    ] {
        let api = fake(entries, vec![]);
        assert!(acquire(&request(), &api).await.is_err());
    }
    let api = fake(
        vec![
            entry("SKILL.md", "100644", "blob", MAIN, bytes.len()),
            entry("references/a.md", "100644", "blob", SUPPORT, 2),
        ],
        vec![(MAIN, bytes.clone()), (SUPPORT, vec![0xff, 0xfe])],
    );
    assert!(acquire(&request(), &api).await.is_err());
    let api = fake(
        vec![entry("SKILL.md", "100644", "blob", MAIN, bytes.len() + 1)],
        vec![(MAIN, bytes)],
    );
    assert!(acquire(&request(), &api).await.is_err());
    let api = FakeApi(Mutex::new(VecDeque::from([
        (
            path(&["commits", "feature/setup"]),
            false,
            false,
            json(serde_json::json!({"sha":COMMIT,"commit":{"tree":{"sha":ROOT}}})),
        ),
        (
            path(&["git", "trees", ROOT]),
            false,
            false,
            json(serde_json::json!({"truncated":true,"tree":[]})),
        ),
    ])));
    assert!(acquire(&request(), &api).await.is_err());
}

#[tokio::test]
async fn github_import_enforces_support_count_and_whole_package_size_before_blob_requests() {
    let mut entries = vec![entry("SKILL.md", "100644", "blob", MAIN, skill().len())];
    for n in 0..=MAX_SKILL_SUPPORTING_FILES {
        entries.push(entry(
            &format!("references/{n}.md"),
            "100644",
            "blob",
            SUPPORT,
            1,
        ));
    }
    let api = fake(entries, vec![]);
    assert!(acquire(&request(), &api).await.is_err());
    let mut entries = vec![entry("SKILL.md", "100644", "blob", MAIN, skill().len())];
    for n in 0..=MAX_SKILL_PACKAGE_BYTES / MAX_SKILL_SUPPORTING_FILE_BYTES {
        entries.push(entry(
            &format!("references/{n}.md"),
            "100644",
            "blob",
            SUPPORT,
            MAX_SKILL_SUPPORTING_FILE_BYTES,
        ));
    }
    let api = fake(entries, vec![]);
    assert!(acquire(&request(), &api).await.is_err());
}

#[test]
fn github_api_urls_encode_refs_and_never_accept_remote_download_locations() {
    let api_request = ApiRequest {
        segments: path(&["commits", "feature/setup"]),
        recursive: false,
        raw: false,
        limit: 1024,
    };
    assert_eq!(
        api_request.url().unwrap().as_str(),
        "https://api.github.com/repos/example/skills/commits/feature%2Fsetup"
    );
    let raw = request().raw(COMMIT, "assets/a b.png", 1024);
    assert_eq!(
        raw.url().unwrap().as_str(),
        format!("https://raw.githubusercontent.com/example/skills/{COMMIT}/notes/assets/a%20b.png")
    );
    assert!(!raw.url().unwrap().as_str().contains("feature"));
}

#[tokio::test]
async fn github_import_defaults_to_head_without_fetching_a_moving_branch_again() {
    let bytes = skill();
    let api = FakeApi(Mutex::new(VecDeque::from([
        (
            path(&["commits", "HEAD"]),
            false,
            false,
            json(serde_json::json!({"sha":COMMIT,"commit":{"tree":{"sha":ROOT}}})),
        ),
        (
            path(&["git", "trees", ROOT]),
            false,
            true,
            json(
                serde_json::json!({"truncated":false,"tree":[entry("SKILL.md","100644","blob",&git_blob_hash(&bytes),bytes.len())]}),
            ),
        ),
        (
            ["example", "skills", COMMIT, "SKILL.md"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            true,
            false,
            bytes.clone(),
        ),
    ])));
    let request = GitHubSkillRequest {
        repository: "example/skills".into(),
        revision: None,
        path: String::new(),
        slug: None,
    };
    let package = acquire(&request, &api).await.unwrap();
    assert_eq!(package.bytes, bytes);
    assert_eq!(package.source.commit, COMMIT);
    assert_eq!(package.source.path, "");
    assert!(api.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn github_source_is_durable_import_data_and_never_activates_itself() {
    use super::super::test_support::{service, temp_workspace, with_workspace};
    use std::sync::Arc;
    use tokio::sync::RwLock;
    let root = temp_workspace("github-source");
    let mut daemon = with_workspace(crate::state::DaemonState::new(), &root);
    let agent = daemon
        .create_agent(crate::agent_runs::test_support::companion_config(
            "import owner",
        ))
        .unwrap()
        .state
        .id;
    daemon.start_free(&agent);
    let state = Arc::new(RwLock::new(daemon));
    let bytes = skill();
    let source = GitHubSkillSource {
        repository: "example/skills".into(),
        commit: COMMIT.into(),
        path: "notes".into(),
        source_url: format!("https://github.com/example/skills/tree/{COMMIT}/notes"),
        package_hash: anima_core::skill_packages::skill_package_hash(&bytes, &[]),
        ignored_paths: vec!["LICENSE".into()],
        original_skill_text: None,
    };
    let draft = service(&state)
        .import_package_from_github(bytes, None, vec![], source.clone())
        .await
        .unwrap();
    assert_eq!(draft.source, super::super::DraftSource::Import);
    assert_eq!(draft.github_source.as_ref(), Some(&source));
    assert!(draft.is_pending());
    assert!(!root.join("skills").exists());
    let json = serde_json::to_value(state.read().await.control_plane_snapshot()).unwrap();
    assert_eq!(json["skillDrafts"][0]["githubSource"]["commit"], COMMIT);
    let mut restarted = with_workspace(crate::state::DaemonState::new(), &root);
    restarted
        .restore_control_plane_snapshot(serde_json::from_value(json).unwrap())
        .unwrap();
    let restored = service(&Arc::new(RwLock::new(restarted)));
    let found = restored.drafts(false).await.unwrap();
    assert_eq!(found[0].draft.github_source.as_ref(), Some(&source));
    assert!(found[0].draft.is_pending());
    assert!(restored.index().await.is_empty());
    restored
        .approve_draft(&draft.id, super::super::DraftApproval::default())
        .await
        .unwrap();
    assert!(root.join("skills/github-notes/SKILL.md").exists());
    assert_eq!(
        restored.load_slug("github-notes").await.unwrap().body,
        super::super::parse_skill_file(&skill()).unwrap().body
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[tokio::test]
async fn github_approval_preserves_original_manifest_and_unknown_metadata() {
    use super::super::test_support::{service, temp_workspace, with_workspace};
    use std::sync::Arc;
    use tokio::sync::RwLock;
    let prefix = "\u{feff}---\r\nname: Metadata notes\r\ndescription: Preserve the original manifest\r\ndeps: [httpx]\r\nmetadata:\r\n  openclaw:\r\n    requires:\r\n      bins: [python3]\r\n---\r\n\r\n";
    let original = format!("{prefix}Use the approved script.\r\n");
    for edited_body in [None, Some("Owner reviewed instructions.\n")] {
        let root = temp_workspace("github-preserve-manifest");
        let state = Arc::new(RwLock::new(with_workspace(
            crate::state::DaemonState::new(),
            &root,
        )));
        let skills = service(&state);
        let files = vec![SkillSupportingFile::from_bytes(
            "requirements.txt".into(),
            b"httpx==0.28.1\n".to_vec(),
        )
        .unwrap()];
        let original_hash = skill_package_hash(original.as_bytes(), &files);
        let source = GitHubSkillSource {
            repository: "example/skills".into(),
            commit: COMMIT.into(),
            path: "notes".into(),
            source_url: format!("https://github.com/example/skills/tree/{COMMIT}/notes"),
            package_hash: original_hash.clone(),
            ignored_paths: vec![],
            original_skill_text: Some(original.clone()),
        };
        let draft = skills
            .import_package_from_github(original.as_bytes().to_vec(), None, files.clone(), source)
            .await
            .unwrap();
        let approved = skills
            .approve_draft(
                &draft.id,
                super::super::DraftApproval {
                    body: edited_body.map(str::to_string),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let installed = std::fs::read(root.join("skills/metadata-notes/SKILL.md")).unwrap();
        let expected = edited_body
            .map(|body| format!("{prefix}{body}"))
            .unwrap_or_else(|| original.clone());
        assert_eq!(installed, expected.as_bytes());
        assert_eq!(
            approved.skill.approved_hash,
            skill_package_hash(&installed, &files)
        );
        if edited_body.is_none() {
            assert_eq!(approved.skill.approved_hash, original_hash);
        }
        let loaded = skills.load_slug("metadata-notes").await.unwrap();
        assert!(loaded
            .body
            .starts_with(edited_body.unwrap_or("Use the approved script.\r\n")));
        assert!(loaded.body.contains("requirements.txt (14 bytes)"));
        assert!(loaded.body.contains(&approved.skill.approved_hash));
        assert!(!loaded.body.contains("deps:"));
        std::fs::remove_dir_all(&root).unwrap();
    }
}

#[tokio::test]
async fn github_import_rejects_mismatched_original_manifest_and_approval_rechecks_provenance() {
    use super::super::test_support::{service, temp_workspace, with_workspace};
    use std::sync::Arc;
    use tokio::sync::RwLock;
    let root = temp_workspace("github-original-fence");
    let state = Arc::new(RwLock::new(with_workspace(
        crate::state::DaemonState::new(),
        &root,
    )));
    let skills = service(&state);
    let bytes = skill();
    let mut source = GitHubSkillSource {
        repository: "example/skills".into(),
        commit: COMMIT.into(),
        path: "notes".into(),
        source_url: format!("https://github.com/example/skills/tree/{COMMIT}/notes"),
        package_hash: skill_package_hash(&bytes, &[]),
        ignored_paths: vec![],
        original_skill_text: Some("different bytes".into()),
    };
    assert!(skills
        .import_package_from_github(bytes.clone(), None, vec![], source.clone())
        .await
        .is_err());
    assert!(state.read().await.skills.pending_drafts().is_empty());
    source.original_skill_text = Some(String::from_utf8(bytes.clone()).unwrap());
    let mut draft = skills
        .import_package_from_github(bytes, None, vec![], source)
        .await
        .unwrap();
    draft.github_source.as_mut().unwrap().package_hash = "f".repeat(64);
    assert!(approved_bytes(&draft, &draft.body).is_err());
    draft.github_source.as_mut().unwrap().package_hash = skill_package_hash(
        draft
            .github_source
            .as_ref()
            .unwrap()
            .original_skill_text
            .as_ref()
            .unwrap()
            .as_bytes(),
        &[],
    );
    draft.name = "Modified name".into();
    assert!(approved_bytes(&draft, &draft.body).is_err());
    std::fs::remove_dir_all(&root).unwrap();
}

#[tokio::test]
async fn github_draft_cannot_overwrite_a_skill_changed_while_waiting_for_review() {
    use super::super::test_support::{content, service, temp_workspace, with_workspace};
    use std::sync::Arc;
    use tokio::sync::RwLock;
    let root = temp_workspace("github-stale");
    let state = Arc::new(RwLock::new(with_workspace(
        crate::state::DaemonState::new(),
        &root,
    )));
    let skills = service(&state);
    skills
        .save("github-notes", content("Original notes"))
        .await
        .unwrap();
    let bytes = skill();
    let source = GitHubSkillSource {
        repository: "example/skills".into(),
        commit: COMMIT.into(),
        path: "notes".into(),
        source_url: format!("https://github.com/example/skills/tree/{COMMIT}/notes"),
        package_hash: anima_core::skill_packages::skill_package_hash(&bytes, &[]),
        ignored_paths: vec![],
        original_skill_text: None,
    };
    let draft = skills
        .import_package_from_github(bytes, Some("github-notes".into()), vec![], source)
        .await
        .unwrap();
    skills
        .save("github-notes", content("Later owner notes"))
        .await
        .unwrap();
    assert!(skills
        .approve_draft(&draft.id, super::super::DraftApproval::default())
        .await
        .unwrap_err()
        .message()
        .contains("base changed"));
    assert_eq!(
        skills.load_slug("github-notes").await.unwrap().name,
        "Later owner notes"
    );
    assert!(skills
        .drafts(false)
        .await
        .unwrap()
        .iter()
        .any(|view| view.draft.id == draft.id && view.draft.is_pending()));
    std::fs::remove_dir_all(&root).unwrap();
}

#[tokio::test]
async fn github_import_rechecks_stop_workspace_and_write_policy_after_transaction_wait() {
    use super::super::test_support::{service, temp_workspace, with_workspace};
    use crate::approvals::{ApprovalPolicy, PolicyAction, RiskClass};
    use crate::runs::{RunLink, RunRecord, RunSource, RunStart};
    use crate::sessions::{SessionKind, SessionOrigin, SessionRecord, TitleSource};
    use std::sync::Arc;
    use tokio::sync::RwLock;
    enum LateChange {
        Stop,
        Workspace,
        WriteDeny,
    }
    for change in [
        LateChange::Stop,
        LateChange::Workspace,
        LateChange::WriteDeny,
    ] {
        let root = temp_workspace("github-owner-fence");
        let mut daemon = with_workspace(crate::state::DaemonState::new(), &root);
        let agent = daemon
            .create_agent(crate::agent_runs::test_support::companion_config(
                "import owner",
            ))
            .unwrap()
            .state
            .id;
        daemon.approvals.set_policy(
            &agent,
            ApprovalPolicy::default().with(RiskClass::Write, PolicyAction::Allow),
        );
        daemon.sessions.insert(SessionRecord::new(
            &agent,
            "chat:github-owner",
            SessionKind::Chat,
            SessionOrigin::Api,
            "Import".into(),
            TitleSource::FirstMessage,
            1,
        ));
        let record = RunRecord::running(
            RunStart {
                agent_id: agent.clone(),
                session_id: "chat:github-owner".into(),
                source: RunSource::Api,
                source_ref: None,
                idempotency_key: None,
                text: "Import".into(),
                model: "fixture".into(),
                provider: None,
                parent_run_id: None,
            },
            1,
        );
        let run = RunLink {
            agent_id: agent.clone(),
            session_id: record.session_id.clone(),
            run_id: record.id.clone(),
        };
        let control = daemon.live.runs().register(&record.id);
        daemon.runs.insert(record);
        let scope = OwnerImportScope {
            run,
            root: root.clone(),
            call: anima_core::ToolCall {
                id: "import".into(),
                name: "import_github_skill".into(),
                args: Default::default(),
            },
            approved: true,
        };
        scope.check(&daemon, &root).unwrap();
        let state = Arc::new(RwLock::new(daemon));
        let mut skills = service(&state);
        skills.owner_import = Some(scope);
        let transaction = skills.transactions.clone().lock_owned().await;
        let bytes = skill();
        let source = GitHubSkillSource {
            repository: "example/skills".into(),
            commit: COMMIT.into(),
            path: "notes".into(),
            source_url: format!("https://github.com/example/skills/tree/{COMMIT}/notes"),
            package_hash: anima_core::skill_packages::skill_package_hash(&bytes, &[]),
            ignored_paths: vec![],
            original_skill_text: None,
        };
        let waiting = tokio::spawn(async move {
            skills
                .import_package_from_github(bytes, None, vec![], source)
                .await
        });
        tokio::task::yield_now().await;
        match change {
            LateChange::Stop => control.cancel.cancel(),
            LateChange::Workspace => {
                state.write().await.workspace.as_mut().unwrap().root_path = root.join("moved")
            }
            LateChange::WriteDeny => {
                state.write().await.approvals.set_policy(
                    &agent,
                    ApprovalPolicy::default().with(RiskClass::Write, PolicyAction::Deny),
                );
            }
        }
        drop(transaction);
        assert!(waiting.await.unwrap().is_err());
        assert!(state.read().await.skills.pending_drafts().is_empty());
        assert!(!root.join("skills").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
