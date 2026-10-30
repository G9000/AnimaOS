//! Injected vectors test store mechanics; they make no claims about model quality.

use super::*;
use crate::{source_sha256, ChunkInput, DocumentInput};

fn document(id: &str, text: &str) -> DocumentInput {
    DocumentInput {
        document_id: id.into(),
        relative_path: format!("{id}.txt"),
        title: format!("Document {id}"),
        source_sha256: source_sha256(text.as_bytes()),
        chunks: vec![ChunkInput {
            locator: "line=1".into(),
            text: text.into(),
        }],
    }
}

fn profile() -> EmbeddingProfile {
    EmbeddingProfile::new("test-model/session-a/windows-v1".into(), 2).unwrap()
}

fn publish(store: &DocumentStore, corpus: &str, sources: &[String], profile: &EmbeddingProfile) {
    let pending = store
        .pending_embeddings(corpus, sources, profile, 1000)
        .unwrap();
    let vectors = pending
        .into_iter()
        .map(|passage| PassageEmbedding {
            citation_id: passage.citation_id,
            window: passage.window,
            vector: if passage.text.contains("purchase") {
                vec![1.0, 0.0]
            } else {
                vec![0.0, 1.0]
            },
        })
        .collect::<Vec<_>>();
    assert_eq!(
        store.publish_embeddings(corpus, profile, &vectors).unwrap(),
        vectors.len()
    );
}

#[test]
fn semantic_lane_can_return_a_passage_without_any_query_terms() {
    let store = DocumentStore::open(":memory:").unwrap();
    let sources = vec!["selected".into()];
    store
        .replace_source(
            "a",
            "selected",
            &[document(
                "meaning",
                "purchase authorization requires two signatures",
            )],
        )
        .unwrap();
    assert!(store
        .search("a", &sources, "spending approval", 20)
        .unwrap()
        .is_empty());
    let profile = profile();
    publish(&store, "a", &sources, &profile);
    let result = store
        .hybrid_search(
            "a",
            &sources,
            "spending approval",
            20,
            &profile,
            &[1.0, 0.0],
        )
        .unwrap();
    assert_eq!(result.hits[0].document_id, "meaning");
    assert_eq!(
        result.coverage,
        SemanticCoverage {
            indexed_windows: 1,
            total_windows: 1,
            omitted_windows: 0
        }
    );
}

#[test]
fn exact_phrase_codename_and_numeric_identifiers_keep_keyword_precedence() {
    for query in ["Cobalt", "\"Cobalt harbour\"", "7319", "XR-7319"] {
        let store = DocumentStore::open(":memory:").unwrap();
        let sources = vec!["selected".into()];
        store
            .replace_source(
                "a",
                "selected",
                &[
                    document("precise", "Cobalt harbour reference XR-7319 is approved"),
                    document("semantic", "purchase authorization requires two signatures"),
                ],
            )
            .unwrap();
        let profile = profile();
        publish(&store, "a", &sources, &profile);
        let result = store
            .hybrid_search("a", &sources, query, 20, &profile, &[1.0, 0.0])
            .unwrap();
        assert_eq!(result.hits[0].document_id, "precise", "query {query}");
    }
}

#[test]
fn semantic_ranking_deduplicates_windows_and_preserves_late_clause_citations() {
    let store = DocumentStore::open(":memory:").unwrap();
    let sources = vec!["selected".into()];
    let text = format!(
        "{} purchase authorization requires two signatures",
        "🦀 padding ".repeat(230)
    );
    store
        .replace_source("a", "selected", &[document("late", &text)])
        .unwrap();
    let profile = profile();
    let pending = store
        .pending_embeddings("a", &sources, &profile, 100)
        .unwrap();
    assert!(pending.len() > 2);
    assert!(pending.iter().all(|passage| passage.text.len() <= 384));
    assert!(pending
        .iter()
        .any(|passage| passage.text.contains("purchase authorization")));
    let vectors = pending
        .iter()
        .map(|passage| PassageEmbedding {
            citation_id: passage.citation_id.clone(),
            window: passage.window,
            vector: if passage.text.contains("purchase authorization") {
                vec![1.0, 0.0]
            } else {
                vec![0.0, 1.0]
            },
        })
        .collect::<Vec<_>>();
    store.publish_embeddings("a", &profile, &vectors).unwrap();
    let result = store
        .hybrid_search(
            "a",
            &sources,
            "spending approval",
            20,
            &profile,
            &[1.0, 0.0],
        )
        .unwrap();
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].citation_id, pending[0].citation_id);
    assert_eq!(result.hits[0].text, text);
    assert!(result.hits[0].excerpt.contains("purchase authorization"));
}

#[test]
fn source_allowlist_and_corpus_are_applied_before_scoring() {
    let store = DocumentStore::open(":memory:").unwrap();
    let profile = profile();
    for (corpus, source) in [("a", "selected"), ("a", "denied"), ("b", "selected")] {
        store
            .replace_source(
                corpus,
                source,
                &[document("same", "purchase authorization")],
            )
            .unwrap();
        publish(&store, corpus, &[source.into()], &profile);
    }
    let result = store
        .hybrid_search(
            "a",
            &["selected".into()],
            "spending approval",
            20,
            &profile,
            &[1.0, 0.0],
        )
        .unwrap();
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].source_id, "selected");
    assert_eq!(result.coverage.total_windows, 1);
    let empty = store
        .hybrid_search("a", &[], "spending approval", 20, &profile, &[1.0, 0.0])
        .unwrap();
    assert!(empty.hits.is_empty());
    assert_eq!(empty.coverage.total_windows, 0);
}

#[test]
fn stale_publication_replacement_revocation_and_profile_changes_cannot_reuse_vectors() {
    let store = DocumentStore::open(":memory:").unwrap();
    let sources = vec!["selected".into()];
    let profile = profile();
    store
        .replace_source(
            "a",
            "selected",
            &[document("same", "old purchase authorization")],
        )
        .unwrap();
    let old = store
        .pending_embeddings("a", &sources, &profile, 20)
        .unwrap()
        .remove(0);
    publish(&store, "a", &sources, &profile);
    store
        .replace_source(
            "a",
            "selected",
            &[document("same", "replacement purchase controls")],
        )
        .unwrap();
    assert_eq!(
        store
            .publish_embeddings(
                "a",
                &profile,
                &[PassageEmbedding {
                    citation_id: old.citation_id,
                    window: old.window,
                    vector: vec![1.0, 0.0]
                }]
            )
            .unwrap(),
        0
    );
    assert_eq!(
        store
            .hybrid_search(
                "a",
                &sources,
                "spending approval",
                20,
                &profile,
                &[1.0, 0.0]
            )
            .unwrap()
            .coverage
            .indexed_windows,
        0
    );
    publish(&store, "a", &sources, &profile);
    let other = EmbeddingProfile::new("test-model/session-b/windows-v1".into(), 2).unwrap();
    assert_eq!(
        store
            .hybrid_search("a", &sources, "spending approval", 20, &other, &[1.0, 0.0])
            .unwrap()
            .coverage
            .indexed_windows,
        0
    );
    store.retain_sources("a", &[]).unwrap();
    let remaining: usize = store
        .connection()
        .unwrap()
        .query_row("SELECT count(*) FROM passage_embeddings", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(remaining, 0);
    store
        .replace_source(
            "a",
            "selected",
            &[document("same", "old purchase authorization")],
        )
        .unwrap();
    assert_eq!(
        store
            .hybrid_search(
                "a",
                &sources,
                "spending approval",
                20,
                &profile,
                &[1.0, 0.0]
            )
            .unwrap()
            .coverage
            .indexed_windows,
        0
    );
}

#[test]
fn invalid_vectors_are_rejected_and_corrupt_rows_are_not_coverage_or_candidates() {
    let store = DocumentStore::open(":memory:").unwrap();
    let sources = vec!["selected".into()];
    let profile = profile();
    store
        .replace_source(
            "a",
            "selected",
            &[document("one", "purchase authorization")],
        )
        .unwrap();
    let passage = store
        .pending_embeddings("a", &sources, &profile, 20)
        .unwrap()
        .remove(0);
    for vector in [
        vec![0.0, 0.0],
        vec![f32::NAN, 0.0],
        vec![f32::INFINITY, 0.0],
        vec![1.0],
        vec![1.0, 0.0, 0.0],
    ] {
        assert!(store
            .publish_embeddings(
                "a",
                &profile,
                &[PassageEmbedding {
                    citation_id: passage.citation_id.clone(),
                    window: 0,
                    vector
                }]
            )
            .is_err());
    }
    assert!(EmbeddingProfile::new("invalid".into(), 0).is_err());
    assert!(EmbeddingProfile::new("invalid".into(), 1025).is_err());
    publish(&store, "a", &sources, &profile);
    store
        .connection()
        .unwrap()
        .execute("UPDATE passage_embeddings SET vector=?1", [vec![0u8; 3]])
        .unwrap();
    let result = store
        .hybrid_search(
            "a",
            &sources,
            "spending approval",
            20,
            &profile,
            &[1.0, 0.0],
        )
        .unwrap();
    assert!(result.hits.is_empty());
    assert_eq!(result.coverage.indexed_windows, 0);
    assert_eq!(
        store
            .pending_embeddings("a", &sources, &profile, 20)
            .unwrap()
            .len(),
        1
    );
    assert!(store
        .hybrid_search("a", &sources, "approval", 20, &profile, &[0.0, 0.0])
        .is_err());
}

#[test]
fn scope_cap_reports_omitted_windows_without_limiting_keyword_search() {
    let store = DocumentStore::open(":memory:").unwrap();
    let mut documents = Vec::new();
    for index in 0..3 {
        let mut value = document(&format!("doc-{index}"), "ordinary");
        value.chunks = (0..1500)
            .map(|chunk| ChunkInput {
                locator: format!("line={chunk}"),
                text: if index == 2 && chunk == 1499 {
                    "late exact-marker-9182".into()
                } else {
                    "ordinary".into()
                },
            })
            .collect();
        documents.push(value);
    }
    store.replace_source("a", "selected", &documents).unwrap();
    let sources = vec!["selected".into()];
    let profile = profile();
    let pending = store
        .pending_embeddings("a", &sources, &profile, usize::MAX)
        .unwrap();
    assert_eq!(pending.len(), 4096);
    let result = store
        .hybrid_search(
            "a",
            &sources,
            "exact-marker-9182",
            20,
            &profile,
            &[1.0, 0.0],
        )
        .unwrap();
    assert_eq!(result.hits[0].document_id, "doc-2");
    assert_eq!(result.coverage.total_windows, 4500);
    assert_eq!(result.coverage.omitted_windows, 404);
}

#[test]
fn vector_payload_quota_covers_old_profiles_and_never_blocks_text_replacement() {
    let store =
        DocumentStore::open_with_limits(":memory:", MAX_CACHE_TEXT_BYTES, 256 * 1024).unwrap();
    store
        .replace_source("a", "selected", &[document("old", "cobalt")])
        .unwrap();
    let mut value = document("one", "cobalt");
    value.chunks = (0..180)
        .map(|index| ChunkInput {
            locator: format!("line={index}"),
            text: format!("cobalt passage {index}"),
        })
        .collect();
    store.replace_source("other", "reserved", &[value]).unwrap();
    let profile = EmbeddingProfile::new("large/session-a".into(), 1024).unwrap();
    let pending = store
        .pending_embeddings("other", &["reserved".into()], &profile, 1000)
        .unwrap();
    let vectors = pending
        .into_iter()
        .map(|passage| PassageEmbedding {
            citation_id: passage.citation_id,
            window: passage.window,
            vector: vec![1.0; 1024],
        })
        .collect::<Vec<_>>();
    // SQLite's smaller physical quota may skip derived vectors before the logical quota.
    assert!(
        store
            .publish_embeddings("other", &profile, &vectors)
            .unwrap()
            < vectors.len()
    );
    let replacement = (0..20)
        .map(|index| document(&format!("new-{index}"), &"fresh cobalt ".repeat(100)))
        .collect::<Vec<_>>();
    store.replace_source("a", "selected", &replacement).unwrap();
    assert_eq!(
        store
            .search("a", &["selected".into()], "fresh", 20)
            .unwrap()
            .len(),
        20
    );
    assert_eq!(
        store
            .search("other", &["reserved".into()], "cobalt", 20)
            .unwrap()
            .len(),
        20
    );
    let rows: usize = store
        .connection()
        .unwrap()
        .query_row("SELECT count(*) FROM passage_embeddings", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        rows, 0,
        "text publication should reclaim disposable vectors in other corpora"
    );
}

#[test]
fn weighted_rrf_prefers_semantics_and_results_have_a_deterministic_limit() {
    let store = DocumentStore::open(":memory:").unwrap();
    let sources = vec!["selected".into()];
    let documents = (0..30)
        .map(|index| {
            document(
                &format!("doc-{index}"),
                if index == 0 {
                    "purchase authorization"
                } else {
                    "spending reference"
                },
            )
        })
        .collect::<Vec<_>>();
    store.replace_source("a", "selected", &documents).unwrap();
    let profile = profile();
    publish(&store, "a", &sources, &profile);
    let first = store
        .hybrid_search(
            "a",
            &sources,
            "how do we approve spending",
            999,
            &profile,
            &[1.0, 0.0],
        )
        .unwrap();
    let second = store
        .hybrid_search(
            "a",
            &sources,
            "how do we approve spending",
            999,
            &profile,
            &[1.0, 0.0],
        )
        .unwrap();
    assert_eq!(first.hits[0].document_id, "doc-0");
    assert_eq!(first.hits.len(), MAX_SEARCH_HITS);
    assert_eq!(first.hits, second.hits);
}

#[test]
fn exact_phrase_outside_the_first_twenty_bm25_matches_is_still_protected() {
    let store = DocumentStore::open(":memory:").unwrap();
    let sources = vec!["selected".into()];
    let mut documents = (0..30)
        .map(|index| {
            document(
                &format!("loud-{index}"),
                &"cobalt reference harbour context ".repeat(20),
            )
        })
        .collect::<Vec<_>>();
    let exact = format!(
        "{} cobalt harbour {}",
        "background ".repeat(180),
        "context ".repeat(180)
    );
    documents.push(document("precise", &exact));
    store.replace_source("a", "selected", &documents).unwrap();
    let profile = profile();
    let result = store
        .hybrid_search(
            "a",
            &sources,
            "\"cobalt harbour\"",
            20,
            &profile,
            &[1.0, 0.0],
        )
        .unwrap();
    assert_eq!(result.hits[0].document_id, "precise");
}

#[test]
fn exact_anchor_verification_rejects_identifier_substrings() {
    let store = DocumentStore::open(":memory:").unwrap();
    let sources = vec!["selected".into()];
    store
        .replace_source(
            "a",
            "selected",
            &[
                document("exact", "reference XR-7319"),
                document("substring", "purchase authorization reference XR-73190"),
            ],
        )
        .unwrap();
    let profile = profile();
    publish(&store, "a", &sources, &profile);
    let result = store
        .hybrid_search("a", &sources, "XR-7319", 20, &profile, &[1.0, 0.0])
        .unwrap();
    assert_eq!(result.hits[0].document_id, "exact");
}

#[test]
fn current_profile_retires_old_session_rows_only_in_its_own_corpus() {
    let store = DocumentStore::open(":memory:").unwrap();
    let sources = vec!["selected".into()];
    let first = profile();
    for corpus in ["a", "b"] {
        store
            .replace_source(
                corpus,
                "selected",
                &[document("one", "purchase authorization")],
            )
            .unwrap();
        publish(&store, corpus, &sources, &first);
    }
    let second = EmbeddingProfile::new("test-model/session-b/windows-v1".into(), 2).unwrap();
    assert_eq!(
        store
            .pending_embeddings("a", &sources, &second, 20)
            .unwrap()
            .len(),
        1
    );
    let rows: usize = store
        .connection()
        .unwrap()
        .query_row("SELECT count(*) FROM passage_embeddings", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(rows, 1);
    assert_eq!(
        store
            .hybrid_search("b", &sources, "spending approval", 20, &first, &[1.0, 0.0])
            .unwrap()
            .coverage
            .indexed_windows,
        1
    );
}

#[test]
fn reopening_preserves_profile_vectors_and_page_only_rows_have_no_windows() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("semantic.sqlite");
    let sources = vec!["selected".into()];
    let profile = profile();
    {
        let store = DocumentStore::open(&path).unwrap();
        let mut page = document("image", "unused");
        page.chunks = vec![ChunkInput {
            locator: crate::pdf_page_locator(1, "test-v1").unwrap(),
            text: String::new(),
        }];
        store
            .replace_source(
                "a",
                "selected",
                &[document("text", "purchase authorization"), page],
            )
            .unwrap();
        publish(&store, "a", &sources, &profile);
    }
    let store = DocumentStore::open(&path).unwrap();
    assert!(store
        .pending_embeddings("a", &sources, &profile, 20)
        .unwrap()
        .is_empty());
    let result = store
        .hybrid_search(
            "a",
            &sources,
            "spending approval",
            20,
            &profile,
            &[1.0, 0.0],
        )
        .unwrap();
    assert_eq!(result.coverage.total_windows, 1);
    assert_eq!(result.coverage.indexed_windows, 1);
}

#[test]
fn whitespace_only_windows_never_poison_pending_work_or_hide_late_meaning() {
    let store = DocumentStore::open(":memory:").unwrap();
    let sources = vec!["selected".into()];
    let profile = profile();
    let text = format!(
        "early clause {} purchase authorization needs two signatures",
        " \t\n".repeat(400)
    );
    store
        .replace_source("a", "selected", &[document("separated", &text)])
        .unwrap();
    let pending = store
        .pending_embeddings("a", &sources, &profile, 100)
        .unwrap();
    assert!(pending.len() >= 2);
    assert!(pending
        .iter()
        .all(|passage| !passage.text.trim().is_empty()));
    assert!(pending
        .iter()
        .enumerate()
        .all(|(index, passage)| passage.window == index));
    assert!(pending
        .last()
        .unwrap()
        .text
        .contains("purchase authorization"));
    publish(&store, "a", &sources, &profile);
    assert!(store
        .pending_embeddings("a", &sources, &profile, 100)
        .unwrap()
        .is_empty());
    let result = store
        .hybrid_search(
            "a",
            &sources,
            "spending approval",
            20,
            &profile,
            &[1.0, 0.0],
        )
        .unwrap();
    assert_eq!(result.coverage.total_windows, pending.len());
    assert_eq!(result.coverage.indexed_windows, pending.len());
    assert_eq!(result.coverage.omitted_windows, 0);
    assert!(result.hits[0].excerpt.contains("purchase authorization"));
}

#[test]
fn nonprotected_fused_hits_keep_the_best_semantic_window_in_the_excerpt() {
    let store = DocumentStore::open(":memory:").unwrap();
    let sources = vec!["selected".into()];
    let profile = profile();
    let text = format!(
        "spending reference {} purchase authorization needs two signatures",
        "background ".repeat(250)
    );
    store
        .replace_source("a", "selected", &[document("late", &text)])
        .unwrap();
    publish(&store, "a", &sources, &profile);
    let result = store
        .hybrid_search(
            "a",
            &sources,
            "how do we approve spending",
            20,
            &profile,
            &[1.0, 0.0],
        )
        .unwrap();
    assert_eq!(result.hits.len(), 1);
    assert!(result.hits[0].excerpt.contains("purchase authorization"));
    assert_eq!(result.hits[0].text, text);
}

#[test]
fn prior_or_missing_window_contract_rebuilds_nonzero_counts_and_retires_all_vectors() {
    for prior in [Some("bytes384-overlap64-v1"), None, Some("")] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("window-migration.sqlite");
        let sources = vec!["selected".into()];
        let profile = profile();
        let text = format!(
            "{} Access can be ended after written notice. The stored credentials are removed within thirty days.",
            "Monthly attendance records are filed for administrative review. ".repeat(44)
        );
        let expected_count;
        {
            let store = DocumentStore::open(&path).unwrap();
            let mut value = document("many", &text);
            value.chunks = (0..260)
                .map(|index| ChunkInput {
                    locator: format!("line={index}"),
                    text: text.clone(),
                })
                .collect();
            // Keep the source within its document text envelope while crossing the
            // migration cursor's 256-row boundary.
            store.replace_source("a", "selected", &[value]).unwrap();
            store
                .replace_source(
                    "b",
                    "selected",
                    &[document("other", "purchase authorization")],
                )
                .unwrap();
            expected_count = store
                .pending_embeddings("a", &sources, &profile, 32_768)
                .unwrap()
                .len();
            publish(&store, "a", &sources, &profile);
            publish(&store, "b", &sources, &profile);
            let connection = store.connection().unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE IF NOT EXISTS semantic_index_metadata (key TEXT PRIMARY KEY,value TEXT NOT NULL);
                     DELETE FROM semantic_index_metadata;
                     UPDATE chunks SET embedding_window_count=777;",
                )
                .unwrap();
            if let Some(prior) = prior {
                connection
                    .execute(
                        "INSERT INTO semantic_index_metadata(key,value) VALUES('embedding_window_contract',?1)",
                        [prior],
                    )
                    .unwrap();
            }
        }
        let store = DocumentStore::open(&path).unwrap();
        let row_count: usize = store
            .connection()
            .unwrap()
            .query_row("SELECT count(*) FROM passage_embeddings", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(
            row_count, 0,
            "contract {prior:?} must retire every corpus's derived vectors"
        );
        let result = store
            .hybrid_search("a", &sources, "access", 20, &profile, &[1.0, 0.0])
            .unwrap();
        assert_eq!(result.coverage.total_windows, expected_count);
        assert_eq!(result.coverage.indexed_windows, 0);
        assert_eq!(result.coverage.omitted_windows, 0);
        assert_eq!(
            store
                .pending_embeddings("a", &sources, &profile, 32_768)
                .unwrap()
                .len(),
            expected_count
        );
        let stale: usize = store
            .connection()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM chunks WHERE embedding_window_count=777",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            stale, 0,
            "nonzero counts after the first cursor batch must be refreshed"
        );
    }
}

#[test]
fn current_window_contract_reopen_keeps_vectors_and_zero_count_rows_stable() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("current-windows.sqlite");
    let sources = vec!["selected".into()];
    let profile = profile();
    let text = format!(
        "{} The last sentence is retained.",
        "Record is archived. ".repeat(40)
    );
    let expected;
    {
        let store = DocumentStore::open(&path).unwrap();
        let mut page = document("image", "unused");
        page.chunks = vec![ChunkInput {
            locator: crate::pdf_page_locator(1, "test-v1").unwrap(),
            text: String::new(),
        }];
        store
            .replace_source("a", "selected", &[document("text", &text), page])
            .unwrap();
        publish(&store, "a", &sources, &profile);
        expected = store
            .hybrid_search("a", &sources, "last", 20, &profile, &[1.0, 0.0])
            .unwrap()
            .coverage;
    }
    for _ in 0..2 {
        let store = DocumentStore::open(&path).unwrap();
        assert!(store
            .pending_embeddings("a", &sources, &profile, 100)
            .unwrap()
            .is_empty());
        assert_eq!(
            store
                .hybrid_search("a", &sources, "last", 20, &profile, &[1.0, 0.0])
                .unwrap()
                .coverage,
            expected
        );
        let empty_count: usize = store
            .connection()
            .unwrap()
            .query_row(
                "SELECT embedding_window_count FROM chunks WHERE text=''",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(empty_count, 0);
    }
}

#[test]
fn contract_migration_advances_past_blank_and_corrupt_chunk_rows() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("corrupt-windows.sqlite");
    let sources = vec!["selected".into()];
    let profile = profile();
    {
        let store = DocumentStore::open(&path).unwrap();
        let mut value = document("rows", "ordinary text");
        value.chunks = (0..5)
            .map(|index| ChunkInput {
                locator: format!("line={index}"),
                text: "ordinary text".into(),
            })
            .collect();
        store.replace_source("a", "selected", &[value]).unwrap();
        publish(&store, "a", &sources, &profile);
        store
            .connection()
            .unwrap()
            .execute_batch(
                "UPDATE chunks SET embedding_window_count=777;
                 UPDATE chunks SET text='' WHERE locator='line=1';
                 UPDATE chunks SET text='   ' WHERE locator='line=2';
                 UPDATE chunks SET text=X'80' WHERE locator='line=3';
                 UPDATE chunks SET text=CAST(X'80' AS TEXT) WHERE locator='line=4';
                 UPDATE semantic_index_metadata SET value=X'80' WHERE key='embedding_window_contract';",
            )
            .unwrap();
    }
    for _ in 0..2 {
        let store = DocumentStore::open(&path).unwrap();
        let counts = {
            let connection = store.connection().unwrap();
            let mut statement = connection
                .prepare("SELECT embedding_window_count FROM chunks ORDER BY locator")
                .unwrap();
            statement
                .query_map([], |row| row.get::<_, usize>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(counts, vec![1, 0, 0, 0, 0]);
        assert_eq!(
            store
                .pending_embeddings("a", &sources, &profile, 20)
                .unwrap()
                .len(),
            1
        );
        let coverage = store
            .hybrid_search("a", &sources, "unmatched", 20, &profile, &[1.0, 0.0])
            .unwrap()
            .coverage;
        assert_eq!(coverage.total_windows, 1);
        assert_eq!(coverage.indexed_windows, 0);
        let contract: String = store
            .connection()
            .unwrap()
            .query_row(
                "SELECT value FROM semantic_index_metadata WHERE key='embedding_window_contract'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(contract, EMBEDDING_WINDOW_CONTRACT);
    }
}
