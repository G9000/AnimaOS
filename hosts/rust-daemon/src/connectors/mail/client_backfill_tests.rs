use super::*;
use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Fixture {
    queries: Arc<Mutex<Vec<String>>>,
}

async fn gmail_list(State(fixture): State<Fixture>, uri: axum::http::Uri) -> Json<Value> {
    let query = uri.query().unwrap_or_default().to_owned();
    fixture.queries.lock().unwrap().push(query.clone());
    if query.contains("pageToken=second") {
        Json(json!({"messages":[{"id":"m2"}]}))
    } else {
        Json(json!({"messages":[{"id":"m1"}],"nextPageToken":"second"}))
    }
}

async fn gmail_detail(Path(id): Path<String>) -> Json<Value> {
    Json(
        json!({"id":id,"labelIds":["INBOX"],"internalDate":"1000","payload":{"mimeType":"text/plain","headers":[{"name":"Subject","value":"invoice"}],"body":{"data":URL_SAFE_NO_PAD.encode("old invoice")}}}),
    )
}

#[tokio::test]
async fn gmail_history_pages_stay_inside_the_fixed_inbox_range() {
    let fixture = Fixture {
        queries: Arc::new(Mutex::new(vec![])),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/gmail/users/me/messages", get(gmail_list))
        .route("/gmail/users/me/messages/{id}", get(gmail_detail))
        .with_state(fixture.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = MailClient {
        http: reqwest::Client::new(),
        gmail_api: format!("{base}/gmail"),
        graph_api: format!("{base}/graph"),
    };
    let request = HistoryPageRequest {
        from_ms: 0,
        before_ms: 2_000,
        cursor: None,
    };
    let first = client
        .search_history_page(Provider::Gmail, "synthetic", &request)
        .await
        .unwrap();
    assert_eq!(first.documents.len(), 1);
    assert!(!first.exhausted);
    let second = client
        .search_history_page(
            Provider::Gmail,
            "synthetic",
            &HistoryPageRequest {
                cursor: first.next_cursor,
                ..request
            },
        )
        .await
        .unwrap();
    assert_eq!(second.documents.len(), 1);
    assert!(second.exhausted);
    let queries = fixture.queries.lock().unwrap();
    assert!(queries.iter().all(|query| query.contains("labelIds=INBOX")));
    assert!(queries
        .iter()
        .all(|query| query.contains("before%3A2") || query.contains("before%3a2")));
    server.abort();
}

#[test]
fn outlook_history_rejects_cross_origin_or_wrong_path_continuations() {
    assert!(super::super::search::next_graph(
        "https://graph.microsoft.com/v1.0",
        "https://evil.example/v1.0/me/mailFolders/inbox/messages?$skiptoken=x"
    )
    .is_err());
    assert!(super::super::search::next_graph(
        "https://graph.microsoft.com/v1.0",
        "https://graph.microsoft.com/v1.0/me/messages?$skiptoken=x"
    )
    .is_err());
}
