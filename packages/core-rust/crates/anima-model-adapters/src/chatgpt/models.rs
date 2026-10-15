//! Account-catalog metadata only; presence is not generation acceptance.
use futures::StreamExt;
use serde_json::Value;
use std::{collections::BTreeSet, time::Duration};
use zeroize::Zeroizing;

pub(super) const MODELS_ENDPOINT: &str =
    "https://chatgpt.com/backend-api/codex/models?client_version=99.0.0";
const MAX_BODY: usize = 512 * 1024;
const MAX_ROWS: usize = 512;
const JS_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
const UNAVAILABLE: &str = "Model metadata is unavailable.";
const TIMED_OUT: &str = "Model metadata request timed out.";
const INVALID: &str = "Model metadata response is invalid.";

/// A host-supplied current subscription credential. No fallback or inference.
pub struct ChatGptModelsAdapter {
    client: reqwest::Client,
    token: Zeroizing<String>,
    account_id: String,
    #[cfg(test)]
    test_origin: Option<String>,
}

fn client(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(timeout)
        .build()
        .map_err(|_| UNAVAILABLE.to_owned())
}

fn transport_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        TIMED_OUT
    } else {
        UNAVAILABLE
    }
    .to_owned()
}

impl ChatGptModelsAdapter {
    pub fn new(access_token: String, account_id: String) -> Result<Self, String> {
        let token = Zeroizing::new(access_token);
        if token.is_empty() || account_id.is_empty() {
            return Err(UNAVAILABLE.to_owned());
        }
        Ok(Self {
            client: client(Duration::from_secs(10))?,
            token,
            account_id,
            #[cfg(test)]
            test_origin: None,
        })
    }

    pub async fn fetch(&self) -> Result<Vec<String>, String> {
        #[cfg(test)]
        let test_endpoint = self.test_origin.as_ref().map(|origin| {
            format!(
                "{origin}{}",
                MODELS_ENDPOINT.strip_prefix("https://chatgpt.com").unwrap()
            )
        });
        let endpoint = MODELS_ENDPOINT;
        #[cfg(test)]
        let endpoint = test_endpoint.as_deref().unwrap_or(endpoint);
        let response = self
            .client
            .get(endpoint)
            .bearer_auth(self.token.as_str())
            .header("chatgpt-account-id", &self.account_id)
            .header("originator", "anima")
            .header("User-Agent", "anima/0.1.0")
            .send()
            .await
            .map_err(transport_error)?;
        // Error bodies have no catalog authority and are never read or echoed.
        if !response.status().is_success() {
            return Err(UNAVAILABLE.to_owned());
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_BODY as u64)
        {
            return Err(UNAVAILABLE.to_owned());
        }
        let mut body = Zeroizing::new(Vec::new());
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(transport_error)?;
            if body.len().saturating_add(chunk.len()) > MAX_BODY {
                return Err(UNAVAILABLE.to_owned());
            }
            body.extend_from_slice(&chunk);
        }
        decode_models(&body)
    }

    #[cfg(test)]
    pub(super) fn at_test_origin(
        mut self,
        origin: String,
        timeout: Duration,
    ) -> Result<Self, String> {
        self.client = client(timeout)?;
        self.test_origin = Some(origin);
        Ok(self)
    }
}

pub(super) fn decode_models(body: &[u8]) -> Result<Vec<String>, String> {
    let invalid = || INVALID.to_owned();
    if body.len() > MAX_BODY {
        return Err(invalid());
    }
    let payload: Value = serde_json::from_slice(body).map_err(|_| invalid())?;
    let rows = payload
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    if rows.len() > MAX_ROWS {
        return Err(invalid());
    }
    let mut ranked = Vec::with_capacity(rows.len());
    for row in rows {
        let row = row.as_object().ok_or_else(invalid)?;
        let id = row
            .get("slug")
            .and_then(Value::as_str)
            .ok_or_else(invalid)?;
        if id.is_empty()
            || id.len() > 128
            || !id.as_bytes()[0].is_ascii_alphanumeric()
            || !id.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'/' | b'-')
            })
        {
            return Err(invalid());
        }
        let priority = match row.get("priority") {
            None | Some(Value::Null) => 10_000,
            Some(value) => value
                .as_i64()
                .filter(|value| (-JS_SAFE_INTEGER..=JS_SAFE_INTEGER).contains(value))
                .ok_or_else(invalid)?,
        };
        let visible = match row.get("visibility") {
            None | Some(Value::Null) => true,
            Some(Value::String(value)) if value == "list" => true,
            Some(Value::String(value)) if value == "hide" || value == "hidden" => false,
            _ => return Err(invalid()),
        };
        // Validate consumed fields even on hidden rows. supported_in_api and
        // all other unused metadata are deliberately ignored.
        if visible {
            ranked.push((priority, id.to_owned()));
        }
    }
    ranked.sort_unstable();
    let mut seen = BTreeSet::new();
    Ok(ranked
        .into_iter()
        .filter_map(|(_, id)| seen.insert(id.clone()).then_some(id))
        .collect())
}
