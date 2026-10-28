use super::*;

#[test]
fn query_centering_maps_unicode_case_expansion_back_to_source_bytes() {
    let text = format!("{} needle {}", "İ".repeat(800), "z".repeat(2000));
    let passage = excerpt(&text, &["needle".to_owned()]);
    assert!(passage.contains("needle"), "{passage}");
    assert!(passage.len() <= MAX_SEARCH_EXCERPT_BYTES);
}

#[test]
fn smaller_context_excerpt_keeps_late_matches_and_byte_boundaries() {
    let text = format!("{} Cobalt harbour", "padding ".repeat(112));
    let passage = excerpt_for_query(&text, "cobalt harbour", 600);
    assert!(passage.contains("Cobalt harbour"));
    assert!(passage.len() <= 600);
    let unicode = format!("{} needle {}", "İ🦀".repeat(160), "é".repeat(700));
    let passage = excerpt_for_query(&unicode, "needle", 600);
    assert!(passage.contains("needle"));
    assert!(passage.len() <= 600);
    assert!(excerpt_for_query(&unicode, "needle", 0).is_empty());
    assert!(excerpt_for_query("🦀", "🦀", 1).is_empty());
}

#[test]
fn query_centering_preserves_contextual_greek_final_sigma() {
    let word = "ὈΔΥΣΣΕΎΣ";
    let text = format!("{} {word} {}", "padding ".repeat(250), "z".repeat(1000));
    let passage = excerpt_for_query(&text, word, 600);
    assert!(passage.contains(word), "{passage}");
    assert!(passage.len() <= 600);
}

fn document(id: &str, text: &str) -> DocumentInput {
    DocumentInput {
        document_id: id.into(),
        relative_path: format!("notes/{id}.txt"),
        title: format!("Document {id}"),
        source_sha256: source_sha256(text.as_bytes()),
        chunks: vec![ChunkInput {
            locator: "line=1".into(),
            text: text.into(),
        }],
    }
}

#[test]
fn persists_ranked_passages_with_exact_revision_provenance_and_opaque_citations() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("knowledge.sqlite");
    let original = document("first", "cobalt harbour project budget 7319");
    let citation = {
        let store = DocumentStore::open(&path).unwrap();
        store
            .replace_source(
                "project-a",
                "folder-a",
                &[original.clone(), document("second", "harbour")],
            )
            .unwrap();
        let hits = store
            .search("project-a", &["folder-a".into()], "cobalt budget", 8)
            .unwrap();
        assert_eq!(hits.len(), 1);
        let hit = &hits[0];
        assert_eq!(hit.document_id, "first");
        assert_eq!(hit.source_sha256, original.source_sha256);
        assert_eq!(
            hit.chunk_sha256,
            source_sha256(original.chunks[0].text.as_bytes())
        );
        assert_eq!(hit.text, original.chunks[0].text);
        assert!(!hit.citation_id.contains("first"));
        hit.citation_id.clone()
    };
    let store = DocumentStore::open(&path).unwrap();
    assert_eq!(
        store
            .citation("project-a", &citation)
            .unwrap()
            .unwrap()
            .relative_path,
        "notes/first.txt"
    );
    assert!(store.citation("project-b", &citation).unwrap().is_none());
    store
        .replace_source("project-a", "folder-a", &[original.clone()])
        .unwrap();
    assert_eq!(
        store
            .search("project-a", &["folder-a".into()], "cobalt", 8)
            .unwrap()[0]
            .citation_id,
        citation
    );
    let changed = document("first", "new revision amethyst");
    store
        .replace_source("project-a", "folder-a", &[changed])
        .unwrap();
    assert!(store.citation("project-a", &citation).unwrap().is_none());
    assert!(store
        .search("project-a", &["folder-a".into()], "cobalt", 8)
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .search("project-a", &["folder-a".into()], "amethyst", 8)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn isolates_corpora_and_allowed_sources_and_removes_fts_rows() {
    let store = DocumentStore::open(":memory:").unwrap();
    for (corpus, source) in [("a", "one"), ("a", "two"), ("b", "one")] {
        store
            .replace_source(corpus, source, &[document("same", "cobalt shared")])
            .unwrap();
    }
    assert!(store.search("a", &[], "cobalt", 20).unwrap().is_empty());
    assert_eq!(
        store
            .search("a", &["one".into()], "cobalt", 20)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store
            .search("a", &["one".into(), "two".into()], "cobalt", 20)
            .unwrap()
            .len(),
        2
    );
    store.remove_source("a", "one").unwrap();
    assert!(store
        .search("a", &["one".into()], "cobalt", 20)
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .search("b", &["one".into()], "cobalt", 20)
            .unwrap()
            .len(),
        1
    );
    store.remove_corpus("a").unwrap();
    assert!(store
        .search("a", &["two".into()], "cobalt", 20)
        .unwrap()
        .is_empty());
}

#[test]
fn failed_replacement_preserves_the_whole_prior_revision() {
    let store = DocumentStore::open(":memory:").unwrap();
    store
        .replace_source("a", "one", &[document("old", "cobalt")])
        .unwrap();
    let mut invalid = document("new", "amethyst");
    invalid.chunks[0].text = "x".repeat(MAX_CHUNK_TEXT_BYTES + 1);
    assert!(store
        .replace_source("a", "one", &[document("valid", "amethyst"), invalid])
        .is_err());
    assert_eq!(
        store
            .search("a", &["one".into()], "cobalt", 20)
            .unwrap()
            .len(),
        1
    );
    assert!(store
        .search("a", &["one".into()], "amethyst", 20)
        .unwrap()
        .is_empty());
    assert!(store
        .replace_source(
            "a",
            "one",
            &[document("same", "one"), document("same", "two")]
        )
        .is_err());
}

#[test]
fn bounds_queries_and_literalizes_fts_operators_and_sql_syntax() {
    let store = DocumentStore::open(":memory:").unwrap();
    store
        .replace_source("a", "one", &[document("doc", "cobalt OR budget")])
        .unwrap();
    for query in [
        "",
        "  ",
        "\"",
        "*",
        "NEAR(",
        "'; DROP TABLE chunks; --",
        "cobalt\" OR *",
    ] {
        let result = store.search("a", &["one".into()], query, 20);
        assert!(result.is_ok(), "query {query:?}: {result:?}");
    }
    assert!(store
        .search("a", &["one".into()], &"a".repeat(513), 8)
        .is_err());
    assert_eq!(
        store
            .search("a", &["one".into()], "cobalt", 999)
            .unwrap()
            .len(),
        1
    );
    assert!(store
        .search("a", &["one".into()], "cobalt", 0)
        .unwrap()
        .is_empty());
}

#[test]
fn rejects_outside_relative_paths_bad_hashes_and_overflow_before_writing() {
    let store = DocumentStore::open(":memory:").unwrap();
    for path in [
        "../outside.txt",
        "/absolute.txt",
        "C:\\secret.txt",
        "notes/../other.txt",
        "notes//bad.txt",
    ] {
        let mut value = document("doc", "cobalt");
        value.relative_path = path.into();
        assert!(
            store.replace_source("a", "one", &[value]).is_err(),
            "{path}"
        );
    }
    let mut value = document("doc", "cobalt");
    value.source_sha256 = "not-a-hash".into();
    assert!(store.replace_source("a", "one", &[value]).is_err());
    assert!(store
        .replace_source(
            "a",
            "one",
            &vec![document("doc", "cobalt"); MAX_SOURCE_DOCUMENTS + 1]
        )
        .is_err());
    let mut value = document("doc", "cobalt");
    value.chunks = (0..MAX_DOCUMENT_CHUNKS + 1)
        .map(|i| ChunkInput {
            locator: format!("line={i}"),
            text: "x".into(),
        })
        .collect();
    assert!(store.replace_source("a", "one", &[value]).is_err());
}

#[test]
fn search_excerpt_and_stored_passage_remain_utf8_bounded() {
    let store = DocumentStore::open(":memory:").unwrap();
    let text = format!("{} cobalt {}", "🦀".repeat(400), "é".repeat(700));
    store
        .replace_source("a", "one", &[document("doc", &text)])
        .unwrap();
    let hit = store
        .search("a", &["one".into()], "cobalt", 8)
        .unwrap()
        .remove(0);
    assert!(hit.excerpt.len() <= MAX_SEARCH_EXCERPT_BYTES);
    assert!(hit.excerpt.contains("cobalt"));
    assert_eq!(hit.text, text);
    assert_eq!(
        store.citation("a", &hit.citation_id).unwrap().unwrap().text,
        text
    );
}

#[test]
fn source_reconciliation_removes_stale_persisted_sources_after_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("knowledge.sqlite");
    let removed_citation = {
        let store = DocumentStore::open(&path).unwrap();
        store
            .replace_source("a", "kept", &[document("doc", "cobalt")])
            .unwrap();
        store
            .replace_source("a", "removed", &[document("doc", "cobalt")])
            .unwrap();
        store
            .replace_source("b", "removed", &[document("doc", "cobalt")])
            .unwrap();
        store.search("a", &["removed".into()], "cobalt", 1).unwrap()[0]
            .citation_id
            .clone()
    };
    let store = DocumentStore::open(&path).unwrap();
    store.retain_sources("a", &["kept".into()]).unwrap();
    assert!(store.citation("a", &removed_citation).unwrap().is_none());
    assert_eq!(
        store
            .search("a", &["kept".into()], "cobalt", 8)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store
            .search("b", &["removed".into()], "cobalt", 8)
            .unwrap()
            .len(),
        1
    );
    store.retain_sources("a", &[]).unwrap();
    assert!(store
        .search("a", &["kept".into()], "cobalt", 8)
        .unwrap()
        .is_empty());
}

#[test]
fn sqlite_failure_rolls_back_prior_source_documents_and_fts_rows() {
    let store = DocumentStore::open(":memory:").unwrap();
    store
        .replace_source("a", "one", &[document("old", "cobalt")])
        .unwrap();
    store
        .connection()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_passage BEFORE INSERT ON chunks WHEN new.document_id='rejected'
         BEGIN SELECT RAISE(ABORT,'synthetic publication failure'); END;",
        )
        .unwrap();
    assert!(store
        .replace_source(
            "a",
            "one",
            &[
                document("valid", "amethyst"),
                document("rejected", "amethyst")
            ]
        )
        .is_err());
    assert_eq!(
        store
            .search("a", &["one".into()], "cobalt", 8)
            .unwrap()
            .len(),
        1
    );
    assert!(store
        .search("a", &["one".into()], "amethyst", 8)
        .unwrap()
        .is_empty());
}

#[test]
fn source_and_document_text_and_search_hit_limits_are_enforced() {
    let store = DocumentStore::open(":memory:").unwrap();
    let mut value = document("doc", "cobalt");
    value.chunks = (0..257)
        .map(|index| ChunkInput {
            locator: format!("line={index}"),
            text: "x".repeat(4096),
        })
        .collect();
    assert!(store.replace_source("a", "one", &[value]).is_err());
    let documents = (0..33)
        .map(|index| {
            let mut value = document(&format!("doc-{index}"), "cobalt");
            value.chunks = (0..256)
                .map(|chunk| ChunkInput {
                    locator: format!("line={chunk}"),
                    text: "x".repeat(4096),
                })
                .collect();
            value
        })
        .collect::<Vec<_>>();
    assert!(store.replace_source("a", "one", &documents).is_err());
    let documents = (0..30)
        .map(|index| document(&format!("doc-{index}"), "cobalt"))
        .collect::<Vec<_>>();
    store.replace_source("a", "one", &documents).unwrap();
    assert_eq!(
        store
            .search("a", &["one".into()], "cobalt", 999)
            .unwrap()
            .len(),
        MAX_SEARCH_HITS
    );
}

#[test]
fn aggregate_quota_rejection_preserves_replaced_source_and_other_corpora() {
    let store = DocumentStore::open_with_limits(":memory:", 24, 512 * 1024).unwrap();
    store
        .replace_source("a", "one", &[document("old", "cobalt")])
        .unwrap();
    store
        .replace_source("b", "two", &[document("other", "harbour")])
        .unwrap();
    assert!(store
        .replace_source(
            "a",
            "one",
            &[document("new", "amethyst".repeat(4).as_str())]
        )
        .is_err());
    assert_eq!(
        store
            .search("a", &["one".into()], "cobalt", 8)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store
            .search("b", &["two".into()], "harbour", 8)
            .unwrap()
            .len(),
        1
    );
    store
        .replace_source("a", "one", &[document("new", "amethyst")])
        .unwrap();
    assert!(store
        .search("a", &["one".into()], "cobalt", 8)
        .unwrap()
        .is_empty());
}

#[test]
fn database_page_quota_is_applied_and_committed_wal_is_truncated() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("bounded.sqlite");
    let store = DocumentStore::open_with_limits(&path, MAX_CACHE_TEXT_BYTES, 128 * 1024).unwrap();
    let connection = store.connection().unwrap();
    let pages: u64 = connection
        .pragma_query_value(None, "max_page_count", |row| row.get(0))
        .unwrap();
    let page_size: u64 = connection
        .pragma_query_value(None, "page_size", |row| row.get(0))
        .unwrap();
    assert!(pages * page_size <= 128 * 1024);
    drop(connection);
    store
        .replace_source("a", "one", &[document("doc", "cobalt")])
        .unwrap();
    let wal = path.with_extension("sqlite-wal");
    assert_eq!(std::fs::metadata(&wal).unwrap().len(), 0);
    let documents = (0..40)
        .map(|index| document(&format!("doc-{index}"), &"unique bulky text ".repeat(200)))
        .collect::<Vec<_>>();
    assert!(store.replace_source("a", "one", &documents).is_err());
    assert_eq!(
        store
            .search("a", &["one".into()], "cobalt", 8)
            .unwrap()
            .len(),
        1
    );
    store.remove_source("a", "one").unwrap();
    assert_eq!(std::fs::metadata(&wal).unwrap().len(), 0);
}
