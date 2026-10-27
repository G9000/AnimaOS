use super::*;
use crate::connected_search::{DocumentKind, IndexedDocument};
use serde_json::Value;

fn event_document(
    value: &Value,
    calendar: &str,
) -> Result<Option<IndexedDocument>, GoogleTransportError> {
    if value["status"] == "cancelled" {
        return Ok(None);
    }
    let id = value["id"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 1024)
        .ok_or(GoogleTransportError::Unavailable)?;
    let text = |key: &str, max: usize| {
        value[key]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(max)
            .collect::<String>()
    };
    let description = text("description", 32 * 1024);
    let mut d = crate::connected_search::providers::document(
        DocumentKind::Event,
        id.into(),
        calendar.into(),
        text("summary", 512),
        format!("{description}\n{}", text("location", 1024)),
    );
    let time = |key: &str| {
        value[key]["dateTime"]
            .as_str()
            .or(value[key]["date"].as_str())
            .map(str::to_owned)
    };
    d.event_start = time("start");
    d.event_end = time("end");
    d.source_modified_at = value["updated"].as_str().map(str::to_owned);
    d.source_url = value["htmlLink"].as_str().map(str::to_owned);
    d.truncated = value["description"]
        .as_str()
        .is_some_and(|s| s.chars().count() > 32 * 1024);
    Ok(Some(d))
}
impl GoogleCalendarClient {
    pub(super) async fn search_documents(
        &self,
        access: &str,
        calendar: &str,
        min: &str,
        max: &str,
    ) -> Result<Vec<IndexedDocument>, GoogleTransportError> {
        let mut token = None::<String>;
        let mut tokens = std::collections::HashSet::new();
        let mut docs = vec![];
        for _ in 0..20 {
            let mut request = self
                .client
                .get(format!(
                    "{}/calendars/{}/events",
                    self.api_base,
                    url_encode(calendar)
                ))
                .query(&[
                    ("timeMin", min),
                    ("timeMax", max),
                    ("singleEvents", "true"),
                    ("orderBy", "startTime"),
                    ("maxResults", "250"),
                ])
                .bearer_auth(access);
            if let Some(t) = &token {
                request = request.query(&[("pageToken", t)]);
            }
            let response = request
                .send()
                .await
                .map_err(|_| GoogleTransportError::Unavailable)?;
            let page: Value = read_bounded_json(response).await?;
            for value in page["items"]
                .as_array()
                .ok_or(GoogleTransportError::Unavailable)?
            {
                if let Some(d) = event_document(value, calendar)? {
                    docs.push(d);
                }
                if docs.len() > 500 {
                    return Err(GoogleTransportError::Unavailable);
                }
            }
            token = match &page["nextPageToken"] {
                Value::Null => None,
                Value::String(t) => Some(t.clone()),
                _ => return Err(GoogleTransportError::Unavailable),
            };
            if let Some(t) = &token {
                if t.is_empty() || t.len() > 4096 || !tokens.insert(t.clone()) {
                    return Err(GoogleTransportError::Unavailable);
                }
            } else {
                break;
            }
        }
        if token.is_some() {
            return Err(GoogleTransportError::Unavailable);
        }
        Ok(docs)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn calendar_pagination_preserves_all_events_and_rejects_partial_results() {
        use axum::{extract::State, http::Uri, routing::get, Json, Router};
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        async fn page(State(fail): State<Arc<AtomicBool>>, uri: Uri) -> Json<Value> {
            let second = uri.query().unwrap_or("").contains("pageToken=second");
            Json(if second && fail.load(Ordering::SeqCst) {
                serde_json::json!({"items":[],"nextPageToken":42})
            } else if second {
                serde_json::json!({"items":[{"id":"two","summary":"Second","description":"invoice 778"}]})
            } else {
                serde_json::json!({"items":[{"id":"one","summary":"First"}],"nextPageToken":"second"})
            })
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let fail = Arc::new(AtomicBool::new(false));
        let router = Router::new()
            .route("/calendars/primary/events", get(page))
            .with_state(fail.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let client = GoogleCalendarClient {
            client: reqwest::Client::new(),
            api_base: base,
        };
        let docs = client
            .search_documents(
                "synthetic",
                "primary",
                "2026-10-01T00:00:00Z",
                "2027-01-01T00:00:00Z",
            )
            .await
            .unwrap();
        assert_eq!(docs.len(), 2);
        assert!(docs[1].text.contains("778"));
        fail.store(true, Ordering::SeqCst);
        assert!(client
            .search_documents(
                "synthetic",
                "primary",
                "2026-10-01T00:00:00Z",
                "2027-01-01T00:00:00Z"
            )
            .await
            .is_err());
        server.abort();
    }
    #[test]
    fn all_day_events_and_body_have_citations() {
        let d=event_document(&serde_json::json!({"id":"event","summary":"Review","description":"invoice amount 778","start":{"date":"2026-10-28"},"end":{"date":"2026-10-29"},"htmlLink":"https://calendar.google.com/calendar/event?eid=test"}),"primary").unwrap().unwrap();
        assert_eq!(d.event_start.as_deref(), Some("2026-10-28"));
        assert!(d.text.contains("778"));
        assert!(d.source_url.is_some());
    }
    #[test]
    fn cancelled_events_are_removed_from_the_retained_snapshot() {
        assert!(event_document(
            &serde_json::json!({"id":"gone","status":"cancelled"}),
            "primary"
        )
        .unwrap()
        .is_none());
    }
}
