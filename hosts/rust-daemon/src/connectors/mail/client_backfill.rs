//! Bounded inbox history pages; these reads never advance mailbox polling state.
use super::*;
use crate::connected_search::{HistoryPage, HistoryPageRequest, ImportCursor};
use std::collections::HashSet;

impl MailClient {
    async fn optional_message(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<Option<Value>, MailError> {
        let mut response = request.send().await.map_err(|_| MailError::Upstream)?;
        match response.status().as_u16() {
            404 => return Ok(None),
            401 | 403 => return Err(MailError::Unauthorized),
            200..=299 => {}
            _ => return Err(MailError::Upstream),
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| MailError::Upstream)? {
            if bytes.len().saturating_add(chunk.len()) > 128 * 1024 {
                return Err(MailError::Upstream);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| MailError::Upstream)
    }

    pub(super) async fn check_inbox_membership(
        &self,
        provider: Provider,
        access: &str,
        ids: &[String],
    ) -> Result<Vec<bool>, MailError> {
        if ids.len() > 20
            || ids
                .iter()
                .any(|id| id.is_empty() || id.len() > 1024 || id.chars().any(char::is_control))
        {
            return Err(MailError::Invalid);
        }
        let inbox_id = if provider == Provider::Outlook {
            let folder = self
                .json(
                    self.http
                        .get(format!("{}/me/mailFolders/inbox", self.graph_api))
                        .query(&[("$select", "id")])
                        .bearer_auth(access),
                    false,
                )
                .await?;
            let id = text(&folder, "id", 1024);
            if id.is_empty() {
                return Err(MailError::Upstream);
            }
            Some(id)
        } else if provider == Provider::Gmail {
            None
        } else {
            return Err(MailError::Invalid);
        };
        let mut result = Vec::with_capacity(ids.len());
        for id in ids {
            let base = match provider {
                Provider::Gmail => format!("{}/users/me/messages", self.gmail_api),
                Provider::Outlook => format!("{}/me/messages", self.graph_api),
                Provider::Email => return Err(MailError::Invalid),
            };
            let mut url = reqwest::Url::parse(&base).map_err(|_| MailError::Invalid)?;
            url.path_segments_mut()
                .map_err(|_| MailError::Invalid)?
                .push(id);
            let request = if provider == Provider::Gmail {
                self.http
                    .get(url)
                    .query(&[("format", "minimal")])
                    .bearer_auth(access)
            } else {
                self.http
                    .get(url)
                    .query(&[("$select", "id,parentFolderId")])
                    .bearer_auth(access)
            };
            let present = match self.optional_message(request).await? {
                None => false,
                Some(message) => {
                    if message["id"].as_str() != Some(id.as_str()) {
                        return Err(MailError::Upstream);
                    }
                    if provider == Provider::Gmail {
                        message["labelIds"]
                            .as_array()
                            .ok_or(MailError::Upstream)?
                            .iter()
                            .any(|label| label.as_str() == Some("INBOX"))
                    } else {
                        let parent = message["parentFolderId"]
                            .as_str()
                            .ok_or(MailError::Upstream)?;
                        inbox_id.as_deref() == Some(parent)
                    }
                }
            };
            result.push(present);
        }
        Ok(result)
    }

    pub(super) async fn search_history_page(
        &self,
        provider: Provider,
        access: &str,
        request: &HistoryPageRequest,
    ) -> Result<HistoryPage, MailError> {
        if request.from_ms >= request.before_ms {
            return Err(MailError::Invalid);
        }
        match provider {
            Provider::Gmail => self.gmail_history_page(access, request).await,
            Provider::Outlook => self.outlook_history_page(access, request).await,
            Provider::Email => Err(MailError::Invalid),
        }
    }

    async fn gmail_history_page(
        &self,
        access: &str,
        request: &HistoryPageRequest,
    ) -> Result<HistoryPage, MailError> {
        let token = match &request.cursor {
            Some(ImportCursor::Gmail { page_token })
                if !page_token.is_empty() && page_token.len() <= 4096 =>
            {
                Some(page_token)
            }
            None => None,
            _ => return Err(MailError::Invalid),
        };
        let after = request.from_ms / 1000;
        let before = request.before_ms.saturating_add(999) / 1000;
        let query = format!("after:{} before:{}", after.saturating_sub(1), before);
        let mut list = self
            .http
            .get(format!("{}/users/me/messages", self.gmail_api))
            .query(&[
                ("labelIds", "INBOX"),
                ("maxResults", "20"),
                ("q", query.as_str()),
            ])
            .bearer_auth(access);
        if let Some(token) = token {
            list = list.query(&[("pageToken", token)]);
        }
        let page = self.json(list, false).await?;
        let mut documents = Vec::new();
        let mut ids = HashSet::new();
        if let Some(messages) = page.get("messages") {
            let messages = messages.as_array().ok_or(MailError::Upstream)?;
            if messages.len() > 20 {
                return Err(MailError::Upstream);
            }
            for message in messages {
                let id = text(message, "id", 256);
                if id.is_empty()
                    || !id.bytes().all(|byte| byte.is_ascii_alphanumeric())
                    || !ids.insert(id.clone())
                {
                    return Err(MailError::Upstream);
                }
                let detail = self
                    .json(
                        self.http
                            .get(format!("{}/users/me/messages/{id}", self.gmail_api))
                            .query(&[("format", "full")])
                            .bearer_auth(access),
                        false,
                    )
                    .await?;
                let document = search::gmail_document(&detail)?;
                if document.external_id != id {
                    return Err(MailError::Upstream);
                }
                let received = detail["internalDate"]
                    .as_str()
                    .and_then(|v| v.parse::<u64>().ok())
                    .ok_or(MailError::Upstream)?;
                if received >= request.from_ms && received < request.before_ms {
                    let labels = detail["labelIds"].as_array().ok_or(MailError::Upstream)?;
                    if labels.iter().any(|label| label.as_str() == Some("INBOX")) {
                        documents.push(document);
                    }
                }
            }
        }
        let next = search::page_token(&page, "nextPageToken")?;
        if token.is_some() && next.as_ref() == token {
            return Err(MailError::Upstream);
        }
        Ok(HistoryPage {
            documents,
            exhausted: next.is_none(),
            next_cursor: next.map(|page_token| ImportCursor::Gmail { page_token }),
        })
    }

    async fn outlook_history_page(
        &self,
        access: &str,
        request: &HistoryPageRequest,
    ) -> Result<HistoryPage, MailError> {
        let url = match &request.cursor {
            Some(ImportCursor::Outlook { next_link }) => {
                search::next_graph(&self.graph_api, next_link)?
            }
            None => {
                let from = i64::try_from(request.from_ms)
                    .ok()
                    .and_then(chrono::DateTime::from_timestamp_millis)
                    .ok_or(MailError::Invalid)?
                    .to_rfc3339();
                let before = i64::try_from(request.before_ms)
                    .ok()
                    .and_then(chrono::DateTime::from_timestamp_millis)
                    .ok_or(MailError::Invalid)?
                    .to_rfc3339();
                let mut url = reqwest::Url::parse(&format!(
                    "{}/me/mailFolders/inbox/messages",
                    self.graph_api
                ))
                .map_err(|_| MailError::Invalid)?;
                url.query_pairs_mut()
                    .append_pair("$top", "20")
                    .append_pair("$select", "id,from,subject,body,receivedDateTime,webLink")
                    .append_pair(
                        "$filter",
                        &format!("receivedDateTime ge {from} and receivedDateTime lt {before}"),
                    )
                    .append_pair("$orderby", "receivedDateTime desc");
                url
            }
            _ => return Err(MailError::Invalid),
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
        let values = page["value"].as_array().ok_or(MailError::Upstream)?;
        if values.len() > 20 {
            return Err(MailError::Upstream);
        }
        let mut documents = Vec::new();
        let mut ids = HashSet::new();
        for value in values {
            let document = search::graph_document(value)?;
            if !ids.insert(document.external_id.clone()) {
                return Err(MailError::Upstream);
            }
            let received = document
                .source_modified_at
                .as_deref()
                .and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok())
                .and_then(|v| u64::try_from(v.timestamp_millis()).ok())
                .ok_or(MailError::Upstream)?;
            if received >= request.from_ms && received < request.before_ms {
                documents.push(document);
            }
        }
        let next = match &page["@odata.nextLink"] {
            Value::Null => None,
            Value::String(next) => {
                search::next_graph(&self.graph_api, next)?;
                Some(next.clone())
            }
            _ => return Err(MailError::Upstream),
        };
        if let (Some(ImportCursor::Outlook { next_link }), Some(next)) = (&request.cursor, &next) {
            if next_link == next {
                return Err(MailError::Upstream);
            }
        }
        Ok(HistoryPage {
            documents,
            exhausted: next.is_none(),
            next_cursor: next.map(|next_link| ImportCursor::Outlook { next_link }),
        })
    }
}

#[cfg(test)]
#[path = "client_backfill_tests.rs"]
mod tests;
