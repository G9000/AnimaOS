//! Selected native thread reads with closed identity handling and honest bounded coverage.

use super::{search, MailClient, MailError, Provider};
use crate::connected_search::IndexedDocument;
use crate::connectors::mail::search::ProjectMailThread;

pub(in crate::connectors::mail) fn project_thread(
    documents: Vec<IndexedDocument>,
    mut partial: bool,
) -> Result<ProjectMailThread, MailError> {
    let title = documents
        .first()
        .map(|d| d.title.clone())
        .ok_or(MailError::NotFound)?;
    let mut text = String::new();
    let mut versions = Vec::new();
    partial |= documents.len() > 25;
    let mut seen = std::collections::BTreeSet::new();
    for document in documents.into_iter().take(25) {
        if !seen.insert(document.external_id.clone()) {
            return Err(MailError::Upstream);
        }
        partial |= document.truncated;
        let part = format!(
            "Message: {}\nFrom: {}\nDate: {}\nSubject: {}\n\n{}\n\n",
            document.external_id,
            document.sender.as_deref().unwrap_or(""),
            document.source_modified_at.as_deref().unwrap_or(""),
            document.title,
            document.text
        );
        if text.len() + part.len() > 1024 * 1024 {
            partial = true;
            break;
        }
        versions.push((document.external_id, document.revision));
        text.push_str(&part);
    }
    if text.trim().is_empty() {
        return Err(MailError::Upstream);
    }
    let revision = anima_document_store_sqlite::source_sha256(
        &serde_json::to_vec(&(versions, &text, partial)).map_err(|_| MailError::Upstream)?,
    );
    Ok(ProjectMailThread {
        title,
        text,
        revision,
        partial,
    })
}

fn opaque(value: &str) -> bool {
    !value.is_empty() && value.len() <= 1024 && !value.chars().any(char::is_control)
}

impl MailClient {
    pub(super) async fn fetch_project_thread(
        &self,
        provider: Provider,
        access: &str,
        selected: &str,
    ) -> Result<ProjectMailThread, MailError> {
        if !opaque(selected) {
            return Err(MailError::Invalid);
        }
        match provider {
            Provider::Email => Err(MailError::Invalid),
            Provider::Gmail => {
                if !selected.bytes().all(|b| b.is_ascii_alphanumeric()) {
                    return Err(MailError::Invalid);
                }
                let anchor = self
                    .json(
                        self.http
                            .get(format!("{}/users/me/messages/{selected}", self.gmail_api))
                            .query(&[("format", "metadata")])
                            .bearer_auth(access),
                        false,
                    )
                    .await?;
                if anchor["id"].as_str() != Some(selected) {
                    return Err(MailError::Upstream);
                }
                let thread = anchor["threadId"]
                    .as_str()
                    .filter(|id| {
                        !id.is_empty()
                            && id.len() <= 256
                            && id.bytes().all(|b| b.is_ascii_alphanumeric())
                    })
                    .ok_or(MailError::Upstream)?;
                let value = self
                    .json(
                        self.http
                            .get(format!("{}/users/me/threads/{thread}", self.gmail_api))
                            .query(&[("format", "full")])
                            .bearer_auth(access),
                        false,
                    )
                    .await?;
                if value["id"].as_str() != Some(thread) {
                    return Err(MailError::Upstream);
                }
                let messages = value["messages"].as_array().ok_or(MailError::Upstream)?;
                if !messages.iter().any(|m| m["id"].as_str() == Some(selected)) {
                    return Err(MailError::Upstream);
                }
                let mut documents = Vec::new();
                for message in messages.iter().take(25) {
                    if message["threadId"].as_str() != Some(thread) {
                        return Err(MailError::Upstream);
                    }
                    let mut document = search::gmail_document(message)?;
                    document.revision = value["historyId"].as_str().unwrap_or_default().into();
                    documents.push(document);
                }
                project_thread(documents, messages.len() > 25)
            }
            Provider::Outlook => {
                let mut url = reqwest::Url::parse(&format!("{}/me/messages", self.graph_api))
                    .map_err(|_| MailError::Upstream)?;
                url.path_segments_mut()
                    .map_err(|_| MailError::Upstream)?
                    .push(selected);
                let anchor = self
                    .json(
                        self.http
                            .get(url)
                            .query(&[("$select", "id,conversationId")])
                            .header("Prefer", "outlook.body-content-type=\"text\"")
                            .bearer_auth(access),
                        false,
                    )
                    .await?;
                if anchor["id"].as_str() != Some(selected) {
                    return Err(MailError::Upstream);
                }
                let thread = anchor["conversationId"]
                    .as_str()
                    .filter(|id| opaque(id))
                    .ok_or(MailError::Upstream)?;
                let filter = format!("conversationId eq '{}'", thread.replace('\'', "''"));
                let page = self.json(self.http.get(format!("{}/me/messages", self.graph_api)).query(&[("$filter", filter.as_str()), ("$top", "25"), ("$select", "id,conversationId,changeKey,from,subject,body,receivedDateTime,webLink")]).header("Prefer", "outlook.body-content-type=\"text\"").bearer_auth(access), false).await?;
                let messages = page["value"].as_array().ok_or(MailError::Upstream)?;
                let mut documents = Vec::new();
                for message in messages.iter().take(25) {
                    if message["conversationId"].as_str() != Some(thread) {
                        return Err(MailError::Upstream);
                    }
                    let mut document = search::graph_document(message)?;
                    document.revision = message["changeKey"].as_str().unwrap_or_default().into();
                    documents.push(document);
                }
                // The anchor can fall outside this bounded page. Coverage must never imply a full thread.
                project_thread(
                    documents,
                    !page["@odata.nextLink"].is_null() || messages.len() > 25,
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{project_thread, MailClient, Provider};
    use crate::connected_search::{providers::document, DocumentKind};

    #[test]
    fn bounds_selected_thread_and_binds_message_identity_to_revision() {
        let messages = (0..30)
            .map(|id| {
                document(
                    DocumentKind::Mail,
                    id.to_string(),
                    "selected".into(),
                    "Budget".into(),
                    format!("Message {id}"),
                )
            })
            .collect();
        let snapshot = project_thread(messages, false).unwrap();
        assert!(snapshot.partial);
        assert!(snapshot.text.contains("Message 24"));
        assert!(!snapshot.text.contains("Message 25"));
        let one = document(
            DocumentKind::Mail,
            "one".into(),
            "selected".into(),
            "Budget".into(),
            "same content".into(),
        );
        let mut other = one.clone();
        other.external_id = "other".into();
        assert_ne!(
            project_thread(vec![one], false).unwrap().revision,
            project_thread(vec![other], false).unwrap().revision
        );
    }

    #[tokio::test]
    async fn selected_native_threads_bind_versions_reject_foreign_messages_and_report_unread_pages()
    {
        use axum::{
            extract::{Path, State},
            http::Uri,
            routing::get,
            Json, Router,
        };
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        use serde_json::{json, Value};
        use std::sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        };
        #[derive(Clone)]
        struct Fixture {
            wrong_thread: Arc<AtomicBool>,
            revision: Arc<AtomicUsize>,
            graph_calls: Arc<AtomicUsize>,
        }
        async fn gmail_anchor(Path(id): Path<String>) -> Json<Value> {
            Json(json!({"id":id,"threadId":"thread1"}))
        }
        async fn gmail_thread(State(f): State<Fixture>) -> Json<Value> {
            Json(
                json!({"id":"thread1","historyId":f.revision.load(Ordering::SeqCst).to_string(),"messages":[{"id":"message1","threadId":if f.wrong_thread.load(Ordering::SeqCst) { "anotherthread" } else { "thread1" },"payload":{"mimeType":"text/plain","headers":[{"name":"Subject","value":"Selected report"}],"body":{"data":URL_SAFE_NO_PAD.encode("full selected message body")}}}]}),
            )
        }
        async fn graph_anchor(State(f): State<Fixture>, Path(id): Path<String>) -> Json<Value> {
            f.graph_calls.fetch_add(1, Ordering::SeqCst);
            Json(json!({"id":id,"conversationId":"conversation'one"}))
        }
        async fn graph_page(State(f): State<Fixture>, uri: Uri) -> Json<Value> {
            f.graph_calls.fetch_add(1, Ordering::SeqCst);
            let parsed = reqwest::Url::parse(&format!("http://localhost{uri}")).unwrap();
            assert!(parsed
                .query_pairs()
                .any(|(key, value)| key == "$filter"
                    && value == "conversationId eq 'conversation''one'"));
            assert!(parsed
                .query_pairs()
                .any(|(key, value)| key == "$top" && value == "25"));
            Json(
                json!({"value":[{"id":"selected/message","conversationId":"conversation'one","changeKey":"revision1","subject":"Selected report","body":{"content":"full selected graph message"}}],"@odata.nextLink":"https://untrusted.invalid/never-requested"}),
            )
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let fixture = Fixture {
            wrong_thread: Arc::new(AtomicBool::new(false)),
            revision: Arc::new(AtomicUsize::new(1)),
            graph_calls: Arc::new(AtomicUsize::new(0)),
        };
        let router = Router::new()
            .route("/gmail/users/me/messages/{id}", get(gmail_anchor))
            .route("/gmail/users/me/threads/thread1", get(gmail_thread))
            .route("/graph/me/messages/{id}", get(graph_anchor))
            .route("/graph/me/messages", get(graph_page))
            .with_state(fixture.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let client = MailClient {
            http: reqwest::Client::new(),
            gmail_api: format!("{base}/gmail"),
            graph_api: format!("{base}/graph"),
        };
        let first = client
            .fetch_project_thread(Provider::Gmail, "synthetic-token", "message1")
            .await
            .unwrap();
        assert!(!first.partial);
        assert!(first.text.contains("full selected message body"));
        fixture.revision.store(2, Ordering::SeqCst);
        let changed = client
            .fetch_project_thread(Provider::Gmail, "synthetic-token", "message1")
            .await
            .unwrap();
        assert_ne!(
            first.revision, changed.revision,
            "native thread revision changes invalidate a citation even with unchanged body text"
        );
        fixture.wrong_thread.store(true, Ordering::SeqCst);
        assert!(
            client
                .fetch_project_thread(Provider::Gmail, "synthetic-token", "message1")
                .await
                .is_err(),
            "provider returned messages outside the selected native thread"
        );
        assert!(client
            .fetch_project_thread(Provider::Gmail, "synthetic-token", "../other")
            .await
            .is_err());
        let graph = client
            .fetch_project_thread(Provider::Outlook, "synthetic-token", "selected/message")
            .await
            .unwrap();
        assert!(
            graph.partial,
            "a remaining page cannot be presented as a complete conversation"
        );
        assert!(graph.text.contains("full selected graph message"));
        assert_eq!(
            fixture.graph_calls.load(Ordering::SeqCst),
            2,
            "bounded thread reads never follow a provider-supplied arbitrary continuation URL"
        );
        server.abort();
    }
}
