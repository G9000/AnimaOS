//! Owner-only Microsoft file consent, selection, and metadata browsing.
use super::{contracts::*, http::json_response, AppState};
use crate::connectors::microsoft_files::MicrosoftFileError;
use axum::{
    extract::{Path, Request, State},
    http::{header, HeaderValue, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
};

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
fn error(error: MicrosoftFileError) -> Response {
    no_store(json_response(
        match error {
            MicrosoftFileError::Invalid => StatusCode::BAD_REQUEST,
            MicrosoftFileError::NotFound => StatusCode::NOT_FOUND,
            MicrosoftFileError::PermissionDenied => StatusCode::FORBIDDEN,
            MicrosoftFileError::Conflict | MicrosoftFileError::Reauthorization => {
                StatusCode::CONFLICT
            }
            _ => StatusCode::SERVICE_UNAVAILABLE,
        },
        &serde_json::json!({"error":error.to_string()}),
    ))
}
fn guard(state: &AppState, request: &Request, read: bool) -> Result<(), Response> {
    let result = if read {
        state.local_owner.authorize_read(request.headers())
    } else {
        state.local_owner.authorize(request.headers())
    };
    result.map_err(|_| {
        no_store(json_response(
            StatusCode::FORBIDDEN,
            &serde_json::json!({"error":"Local owner authorization required"}),
        ))
    })
}
async fn body<T: serde::de::DeserializeOwned>(
    state: &AppState,
    request: Request,
) -> Result<T, Response> {
    let bytes = super::http::read_limited_body(request, state.config.max_request_bytes.min(16384))
        .await
        .map_err(no_store)?;
    serde_json::from_slice(&bytes).map_err(|_| error(MicrosoftFileError::Invalid))
}

#[utoipa::path(get,path="/api/agents/{agent_id}/connectors/microsoft-files",tag="connectors",params(("agent_id"=String,Path)),responses((status=200,body=MicrosoftFilesStatusResponse),(status=403),(status=404)))]
pub(super) async fn status(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = guard(&state, &request, true) {
        return response;
    }
    match state.microsoft_files.status(&agent).await {
        Ok((configured, records)) => no_store(json_response(
            StatusCode::OK,
            &MicrosoftFilesStatusResponse {
                configured,
                connectors: records
                    .iter()
                    .map(MicrosoftFileConnectorResponse::from)
                    .collect(),
            },
        )),
        Err(reason) => error(reason),
    }
}

#[utoipa::path(post,path="/api/agents/{agent_id}/connectors/microsoft-files/connect",tag="connectors",params(("agent_id"=String,Path)),request_body=MicrosoftFilesConnectInput,responses((status=201),(status=400),(status=403),(status=409)))]
pub(super) async fn connect(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(response) = guard(&state, &request, false) {
        return response;
    }
    let input: MicrosoftFilesConnectInput = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    match state
        .microsoft_files
        .begin_connect(agent, input.mode, input.approved_site_url)
        .await
    {
        Ok(result) => no_store(json_response(StatusCode::CREATED, &result)),
        Err(reason) => error(reason),
    }
}

#[utoipa::path(get,path="/api/agents/{agent_id}/connectors/microsoft-files/{connector_id}/files",tag="connectors",params(("agent_id"=String,Path),("connector_id"=String,Path)),responses((status=200),(status=403),(status=404)))]
pub(super) async fn files(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = guard(&state, &request, true) {
        return response;
    }
    let params = match super::http::request_query(request.uri()) {
        Ok(value) => value,
        Err(_) => return error(MicrosoftFileError::Invalid),
    };
    if params
        .keys()
        .any(|key| key != "folderKey" && key != "cursor")
    {
        return error(MicrosoftFileError::Invalid);
    }
    match state
        .microsoft_files
        .browse(
            agent,
            id,
            params.get("folderKey").cloned(),
            params.get("cursor").cloned(),
        )
        .await
    {
        Ok(result) => no_store(json_response(StatusCode::OK, &result)),
        Err(reason) => error(reason),
    }
}

#[utoipa::path(put,path="/api/agents/{agent_id}/connectors/microsoft-files/{connector_id}/selection",tag="connectors",params(("agent_id"=String,Path),("connector_id"=String,Path)),request_body=MicrosoftFilesSelectionInput,responses((status=200,body=MicrosoftFilesSelectionResponse),(status=400),(status=403),(status=409)))]
pub(super) async fn selection(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = guard(&state, &request, false) {
        return response;
    }
    let input: MicrosoftFilesSelectionInput = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    match state
        .microsoft_files
        .select(
            agent,
            id,
            input.expected_generation,
            input.expected_selection_revision,
            input.file_keys,
        )
        .await
    {
        Ok(record) => no_store(json_response(
            StatusCode::OK,
            &MicrosoftFilesSelectionResponse {
                connector: MicrosoftFileConnectorResponse::from(&record),
            },
        )),
        Err(reason) => error(reason),
    }
}

#[utoipa::path(delete,path="/api/agents/{agent_id}/connectors/microsoft-files/{connector_id}",tag="connectors",params(("agent_id"=String,Path),("connector_id"=String,Path)),request_body=MicrosoftFilesDisconnectInput,responses((status=200),(status=400),(status=403),(status=409)))]
pub(super) async fn disconnect(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(response) = guard(&state, &request, false) {
        return response;
    }
    let input: MicrosoftFilesDisconnectInput = match body(&state, request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    match state
        .microsoft_files
        .disconnect(agent, id, input.expected_generation)
        .await
    {
        Ok(()) => no_store(json_response(
            StatusCode::OK,
            &serde_json::json!({"disconnected":true}),
        )),
        Err(reason) => error(reason),
    }
}

fn callback_query(uri: &Uri) -> Result<(String, Option<String>), MicrosoftFileError> {
    let query = uri.query().unwrap_or_default();
    if query.len() > 16_384 {
        return Err(MicrosoftFileError::Invalid);
    }
    let mut keys = std::collections::HashSet::new();
    for part in query.split('&').filter(|part| !part.is_empty()) {
        let single: Uri = format!("/?{part}")
            .parse()
            .map_err(|_| MicrosoftFileError::Invalid)?;
        for key in super::http::request_query(&single)
            .map_err(|_| MicrosoftFileError::Invalid)?
            .keys()
        {
            if !keys.insert(key.clone()) {
                return Err(MicrosoftFileError::Invalid);
            }
        }
    }
    let params = super::http::request_query(uri).map_err(|_| MicrosoftFileError::Invalid)?;
    let nonce = params
        .get("state")
        .filter(|value| !value.is_empty() && value.len() <= 128)
        .cloned()
        .ok_or(MicrosoftFileError::Invalid)?;
    let code = if params.contains_key("error") {
        None
    } else {
        params.get("code").cloned()
    };
    Ok((nonce, code))
}
fn callback_page(success: bool) -> Response {
    let text = if success {
        "Microsoft files connected. You can close this page and return to Connections."
    } else {
        "Microsoft files could not be connected. Return to Connections to try again or check your organization's site approval."
    };
    let mut response = no_store((if success { StatusCode::OK } else { StatusCode::BAD_REQUEST }, Html(format!("<!doctype html><meta charset=\"utf-8\"><title>Microsoft files</title><p>{text}</p>"))).into_response());
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'; base-uri 'none'"),
    );
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}
#[utoipa::path(get,path="/api/connectors/microsoft-files/callback",tag="connectors",params(("state"=String,Query),("code"=Option<String>,Query),("error"=Option<String>,Query)),responses((status=200),(status=400)))]
pub(super) async fn callback(State(state): State<AppState>, uri: Uri) -> Response {
    match callback_query(&uri) {
        Ok((nonce, code)) => callback_page(
            state
                .microsoft_files
                .complete_connect(nonce, code)
                .await
                .is_ok(),
        ),
        Err(_) => callback_page(false),
    }
}
pub(super) async fn protect_cache(request: Request, next: axum::middleware::Next) -> Response {
    let private = request.uri().path().contains("/connectors/microsoft-files");
    let response = next.run(request).await;
    if private {
        no_store(response)
    } else {
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_rejects_duplicate_state_without_reflecting_provider_data() {
        assert!(
            callback_query(&"/callback?state=a&%73tate=b&code=private".parse().unwrap()).is_err()
        );
        assert!(
            callback_query(&"/callback?state=a&error=private".parse().unwrap())
                .unwrap()
                .1
                .is_none()
        );
    }
}
