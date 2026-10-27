use super::*;
use crate::connected_search::providers::document;
use crate::connected_search::{DocumentKind, IndexedDocument};
use futures::{StreamExt, TryStreamExt};
fn page_token(page: &Value, key: &str) -> Result<Option<String>, MailError> {
    match &page[key] {
        Value::Null => Ok(None),
        Value::String(t) if !t.is_empty() && t.len() <= 4096 => Ok(Some(t.clone())),
        _ => Err(MailError::Upstream),
    }
}

fn payload_text(payload: &Value, depth: usize) -> Result<String, MailError> {
    if depth > 12 {
        return Err(MailError::Upstream);
    }
    let mime = payload["mimeType"].as_str().unwrap_or("");
    if matches!(mime, "text/plain" | "text/html") {
        let Some(data) = payload["body"]["data"].as_str() else {
            return Ok(String::new());
        };
        let bytes = URL_SAFE_NO_PAD
            .decode(data.trim_end_matches('='))
            .map_err(|_| MailError::Upstream)?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        return Ok(text);
    }
    let parts = payload["parts"].as_array();
    let mut fallback = String::new();
    if let Some(parts) = parts {
        if parts.len() > 100 {
            return Err(MailError::Upstream);
        }
        for part in parts {
            let text = payload_text(part, depth + 1)?;
            if part["mimeType"] == "text/plain" && !text.is_empty() {
                return Ok(text);
            }
            if fallback.is_empty() {
                fallback = text;
            }
        }
    }
    Ok(fallback)
}
fn gmail_document(v: &Value) -> Result<IndexedDocument, MailError> {
    let id = text(v, "id", 256);
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(MailError::Upstream);
    }
    let headers = v["payload"]["headers"].as_array();
    let mut d = document(
        DocumentKind::Mail,
        id.clone(),
        "inbox".into(),
        max_header(headers, "Subject"),
        payload_text(&v["payload"], 0)?,
    );
    d.sender = Some(max_header(headers, "From"));
    d.source_modified_at = v["internalDate"]
        .as_str()
        .and_then(|s| s.parse::<i64>().ok())
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|d| d.to_rfc3339());
    d.source_url = Some(format!("https://mail.google.com/mail/u/0/#all/{id}"));
    Ok(d)
}
fn graph_document(v: &Value) -> Result<IndexedDocument, MailError> {
    let id = text(v, "id", 1024);
    if id.is_empty() {
        return Err(MailError::Upstream);
    }
    let mut d = document(
        DocumentKind::Mail,
        id,
        "inbox".into(),
        text(v, "subject", 512),
        text(&v["body"], "content", 64 * 1024),
    );
    d.sender = Some(text(&v["from"]["emailAddress"], "address", 512));
    d.source_modified_at = Some(text(v, "receivedDateTime", 128));
    d.source_url = v["webLink"].as_str().map(str::to_owned);
    d.truncated = v["body"]["content"]
        .as_str()
        .is_some_and(|t| t.chars().count() > 64 * 1024);
    Ok(d)
}
fn next_graph(base: &str, next: &str) -> Result<reqwest::Url, MailError> {
    if next.len() > 8192 {
        return Err(MailError::Upstream);
    }
    let url = reqwest::Url::parse(next).map_err(|_| MailError::Upstream)?;
    let base = reqwest::Url::parse(base).map_err(|_| MailError::Upstream)?;
    if url.origin() != base.origin()
        || url.path()
            != format!(
                "{}/me/mailFolders/inbox/messages",
                base.path().trim_end_matches('/')
            )
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(MailError::Upstream);
    }
    Ok(url)
}
impl MailClient {
    pub(super) async fn search_documents(
        &self,
        p: Provider,
        access: &str,
    ) -> Result<Vec<IndexedDocument>, MailError> {
        match p {
            Provider::Email => Err(MailError::Invalid),
            Provider::Gmail => {
                let mut token = None::<String>;
                let mut seen = std::collections::HashSet::new();
                let mut ids = vec![];
                for _ in 0..20 {
                    let mut request = self
                        .http
                        .get(format!("{}/users/me/messages", self.gmail_api))
                        .query(&[("labelIds", "INBOX"), ("maxResults", "100")])
                        .bearer_auth(access);
                    if let Some(t) = &token {
                        request = request.query(&[("pageToken", t)]);
                    }
                    let page = self.json(request, false).await?;
                    if let Some(messages) = page["messages"].as_array() {
                        for m in messages {
                            let id = text(m, "id", 256);
                            if id.is_empty()
                                || !id.bytes().all(|b| b.is_ascii_alphanumeric())
                                || !seen.insert(id.clone())
                            {
                                return Err(MailError::Upstream);
                            }
                            ids.push(id);
                            if ids.len() == 100 {
                                break;
                            }
                        }
                    }
                    if ids.len() == 100 {
                        break;
                    }
                    token = page_token(&page, "nextPageToken")?;
                    if token.is_none() {
                        break;
                    }
                }
                if ids.len() < 100 && token.is_some() {
                    return Err(MailError::Upstream);
                }
                let docs = futures::stream::iter(ids.into_iter().map(|id| async move {
                    let detail = self
                        .json(
                            self.http
                                .get(format!("{}/users/me/messages/{id}", self.gmail_api))
                                .query(&[("format", "full")])
                                .bearer_auth(access),
                            false,
                        )
                        .await?;
                    let d = gmail_document(&detail)?;
                    if d.external_id != id {
                        return Err(MailError::Upstream);
                    }
                    Ok::<_, MailError>(d)
                }))
                .buffer_unordered(4)
                .try_collect::<Vec<_>>()
                .await?;
                Ok(docs)
            }
            Provider::Outlook => {
                let mut next=Some(reqwest::Url::parse(&format!("{}/me/mailFolders/inbox/messages?$top=25&$select=id,from,subject,body,receivedDateTime,webLink&$orderby=receivedDateTime%20desc",self.graph_api)).map_err(|_|MailError::Upstream)?);
                let mut docs = vec![];
                let mut seen = std::collections::HashSet::new();
                for _ in 0..20 {
                    let Some(url) = next.take() else {
                        break;
                    };
                    let page = self
                        .json(
                            self.http
                                .get(url)
                                .header("Prefer", "outlook.body-content-type=\"text\"")
                                .bearer_auth(access),
                            false,
                        )
                        .await?;
                    for value in page["value"].as_array().ok_or(MailError::Upstream)? {
                        let d = graph_document(value)?;
                        if !seen.insert(d.external_id.clone()) {
                            return Err(MailError::Upstream);
                        }
                        docs.push(d);
                        if docs.len() == 100 {
                            break;
                        }
                    }
                    if docs.len() == 100 {
                        break;
                    }
                    next = match &page["@odata.nextLink"] {
                        Value::Null => None,
                        Value::String(url) => Some(next_graph(&self.graph_api, url)?),
                        _ => return Err(MailError::Upstream),
                    };
                    if next.is_none() {
                        break;
                    }
                }
                if next.is_some() && docs.len() < 100 {
                    return Err(MailError::Upstream);
                }
                Ok(docs)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn native_mail_fetches_complete_retained_pages_and_full_message_content() {
        use axum::{
            extract::{Path, State},
            http::Uri,
            routing::get,
            Json, Router,
        };
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        #[derive(Clone)]
        struct Fixture {
            base: String,
            pages: Arc<AtomicUsize>,
        }
        async fn gmail(State(f): State<Fixture>, uri: Uri) -> Json<Value> {
            f.pages.fetch_add(1, Ordering::SeqCst);
            Json(if uri.query().unwrap_or("").contains("pageToken=second") {
                json!({"messages":[{"id":"m2"}]})
            } else {
                json!({"messages":[{"id":"m1"}],"nextPageToken":"second"})
            })
        }
        async fn detail(Path(id): Path<String>) -> Json<Value> {
            Json(
                json!({"id":id,"snippet":"preview only","payload":{"mimeType":"text/plain","headers":[{"name":"Subject","value":"Invoice"}],"body":{"data":URL_SAFE_NO_PAD.encode("full body amount 983")}}}),
            )
        }
        async fn graph(State(f): State<Fixture>, uri: Uri) -> Json<Value> {
            f.pages.fetch_add(1, Ordering::SeqCst);
            let second = uri.query().unwrap_or("").contains("skiptoken=second");
            let mut page = json!({"value":[{"id":if second{"second"}else{"first"},"subject":"Invoice","bodyPreview":"preview only","body":{"content":"full body amount 194"},"receivedDateTime":"2026-10-27T00:00:00Z"}]});
            if !second {
                page["@odata.nextLink"] = json!(format!(
                    "{}/graph/me/mailFolders/inbox/messages?$skiptoken=second",
                    f.base
                ));
            }
            Json(page)
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let pages = Arc::new(AtomicUsize::new(0));
        let fixture = Fixture {
            base: base.clone(),
            pages: pages.clone(),
        };
        let router = Router::new()
            .route("/gmail/users/me/messages", get(gmail))
            .route("/gmail/users/me/messages/{id}", get(detail))
            .route("/graph/me/mailFolders/inbox/messages", get(graph))
            .with_state(fixture);
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let client = MailClient {
            http: reqwest::Client::new(),
            gmail_api: format!("{base}/gmail"),
            graph_api: format!("{base}/graph"),
        };
        let gmail = client
            .search_documents(Provider::Gmail, "synthetic")
            .await
            .unwrap();
        assert_eq!(gmail.len(), 2);
        assert!(gmail.iter().all(|d| d.text.contains("983")));
        let outlook = client
            .search_documents(Provider::Outlook, "synthetic")
            .await
            .unwrap();
        assert_eq!(outlook.len(), 2);
        assert!(outlook.iter().all(|d| d.text.contains("194")));
        assert_eq!(pages.load(Ordering::SeqCst), 4);
        server.abort();
    }
    #[test]
    fn present_malformed_pagination_is_not_a_complete_snapshot() {
        for value in [
            json!({"nextPageToken":7}),
            json!({"nextPageToken":""}),
            json!({"nextPageToken":"a".repeat(4097)}),
        ] {
            assert!(page_token(&value, "nextPageToken").is_err());
        }
    }
    #[test]
    fn gmail_indexes_full_body_beyond_the_preview() {
        let v = json!({"id":"abc123","snippet":"Preview only","payload":{"mimeType":"multipart/alternative","headers":[{"name":"Subject","value":"Invoice"}],"parts":[{"mimeType":"text/plain","body":{"data":URL_SAFE_NO_PAD.encode("Full invoice amount 983 not in preview")}}]}});
        assert!(gmail_document(&v).unwrap().text.contains("983"));
    }
    #[test]
    fn graph_indexes_full_body_and_rejects_cross_origin_pagination() {
        let v = json!({"id":"opaque/Id==","subject":"Invoice","bodyPreview":"preview","body":{"content":"Full invoice 194"}});
        assert!(graph_document(&v).unwrap().text.contains("194"));
        assert!(next_graph(
            "https://graph.microsoft.com/v1.0",
            "https://evil.test/v1.0/me/mailFolders/inbox/messages?$skip=25"
        )
        .is_err());
        assert!(next_graph(
            "https://graph.microsoft.com/v1.0",
            "https://graph.microsoft.com/v1.0/me/mailFolders/inbox/messages?$skiptoken=opaque"
        )
        .is_ok());
    }
}
