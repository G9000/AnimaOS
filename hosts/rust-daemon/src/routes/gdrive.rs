//! Owner selection and private single-use OAuth callback; no public content endpoint.
use super::{contracts::*, http::json_response, AppState};
use crate::connectors::drive::DriveError;
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
fn error(error: DriveError) -> Response {
    no_store(json_response(
        match error {
            DriveError::InvalidInput => StatusCode::BAD_REQUEST,
            DriveError::NotFound => StatusCode::NOT_FOUND,
            DriveError::NotSelected => StatusCode::FORBIDDEN,
            DriveError::Conflict | DriveError::ReauthRequired | DriveError::NotConnected => {
                StatusCode::CONFLICT
            }
            _ => StatusCode::SERVICE_UNAVAILABLE,
        },
        &ErrorBody {
            error: error.to_string(),
        },
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
            &ErrorBody {
                error: "Local owner authorization required".into(),
            },
        ))
    })
}
#[utoipa::path(get,path="/api/agents/{agent_id}/connectors/gdrive",tag="connectors",params(("agent_id"=String,Path)),responses((status=200,body=DriveStatusEnvelope),(status=403,body=ErrorBody),(status=404,body=ErrorBody),(status=503,body=ErrorBody)))]
pub(super) async fn status(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(error) = guard(&state, &request, true) {
        return error;
    }
    match state.drive.status(&agent).await {
        Ok((configured, connector)) => no_store(json_response(
            StatusCode::OK,
            &DriveStatusEnvelope {
                configured,
                connector: connector.as_ref().map(DriveConnectorResponse::from),
            },
        )),
        Err(e) => error(e),
    }
}
#[utoipa::path(post,path="/api/agents/{agent_id}/connectors/gdrive",tag="connectors",params(("agent_id"=String,Path)),request_body=DriveConnectInput,responses((status=201,body=DriveConnectEnvelope),(status=400,body=ErrorBody),(status=403,body=ErrorBody),(status=404,body=ErrorBody),(status=409,body=ErrorBody),(status=503,body=ErrorBody)))]
pub(super) async fn connect(
    State(state): State<AppState>,
    Path(agent): Path<String>,
    request: Request,
) -> Response {
    if let Err(error) = guard(&state, &request, false) {
        return error;
    }
    let bytes =
        match super::http::read_limited_body(request, state.config.max_request_bytes.min(16384))
            .await
        {
            Ok(bytes) => bytes,
            Err(response) => return no_store(response),
        };
    let input: DriveConnectInput = match serde_json::from_slice(&bytes) {
        Ok(input) => input,
        Err(_) => return error(DriveError::InvalidInput),
    };
    match state
        .drive
        .begin_connect(agent, input.selected_file_ids)
        .await
    {
        Ok((connector, consent_url)) => no_store(json_response(
            StatusCode::CREATED,
            &DriveConnectEnvelope {
                connector: DriveConnectorResponse::from(&connector),
                consent_url,
            },
        )),
        Err(e) => error(e),
    }
}
#[utoipa::path(delete,path="/api/agents/{agent_id}/connectors/gdrive/{connector_id}",tag="connectors",params(("agent_id"=String,Path),("connector_id"=String,Path)),responses((status=200,body=DeleteResponse),(status=403,body=ErrorBody),(status=404,body=ErrorBody),(status=503,body=ErrorBody)))]
pub(super) async fn disconnect(
    State(state): State<AppState>,
    Path((agent, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    if let Err(error) = guard(&state, &request, false) {
        return error;
    }
    match state.drive.disconnect(agent, id).await {
        Ok(()) => no_store(json_response(
            StatusCode::OK,
            &DeleteResponse { deleted: true },
        )),
        Err(e) => error(e),
    }
}
fn callback_query(uri: &Uri) -> Result<(String, Option<String>), DriveError> {
    let query = uri.query().unwrap_or_default();
    if query.len() > 16384 {
        return Err(DriveError::InvalidInput);
    }
    let mut keys = std::collections::HashSet::new();
    for part in query.split('&').filter(|part| !part.is_empty()) {
        let single: Uri = format!("/?{part}")
            .parse()
            .map_err(|_| DriveError::InvalidInput)?;
        let decoded = super::http::request_query(&single).map_err(|_| DriveError::InvalidInput)?;
        for key in decoded.keys() {
            if !keys.insert(key.clone()) {
                return Err(DriveError::InvalidInput);
            }
        }
    }
    let params = super::http::request_query(uri).map_err(|_| DriveError::InvalidInput)?;
    let nonce = params
        .get("state")
        .filter(|state| !state.is_empty() && state.len() <= 128)
        .cloned()
        .ok_or(DriveError::InvalidInput)?;
    let code = if params.contains_key("error") {
        None
    } else {
        params.get("code").cloned()
    };
    Ok((nonce, code))
}
fn callback_page(success: bool) -> Response {
    let text = if success {
        "Anima Drive connected. You can close this page and refresh Connectors."
    } else {
        "Anima Drive connection was not completed. Return to Connectors and start a new consent attempt."
    };
    let mut response = no_store(
        (
            if success {
                StatusCode::OK
            } else {
                StatusCode::BAD_REQUEST
            },
            Html(format!(
                "<!doctype html><meta charset=\"utf-8\"><title>Anima Drive</title><p>{text}</p>"
            )),
        )
            .into_response(),
    );
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
#[utoipa::path(get,path="/api/connectors/gdrive/callback",tag="connectors",params(("state"=String,Query),("code"=Option<String>,Query),("error"=Option<String>,Query)),responses((status=200,description="Private pending consent completed"),(status=400,description="Consent was not completed")))]
pub(super) async fn callback(State(state): State<AppState>, uri: Uri) -> Response {
    match callback_query(&uri) {
        Ok((nonce, code)) => callback_page(state.drive.complete_connect(nonce, code).await.is_ok()),
        Err(_) => callback_page(false),
    }
}
pub(super) async fn protect_cache(request: Request, next: axum::middleware::Next) -> Response {
    let drive = request.uri().path().contains("/connectors/gdrive");
    let response = next.run(request).await;
    if drive {
        no_store(response)
    } else {
        response
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_rejects_duplicate_nonce_without_reflecting_provider_fields() {
        assert!(callback_query(
            &"/api/connectors/gdrive/callback?state=a&state=b&code=secret"
                .parse()
                .unwrap()
        )
        .is_err());
        assert!(callback_query(
            &"/api/connectors/gdrive/callback?state=a&%73tate=b&code=secret"
                .parse()
                .unwrap()
        )
        .is_err());
    }
}

#[cfg(test)]
mod owner_boundary_tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use tower::ServiceExt;
    #[tokio::test]
    async fn owner_guards_precede_body_and_state_and_callback_errors_are_static_no_store() {
        let shared =
            std::sync::Arc::new(tokio::sync::RwLock::new(crate::state::DaemonState::new()));
        let app =
            crate::app::app_with_state_for_owner_test(shared, crate::app::DaemonConfig::default());
        for (method, path, origin, status) in [
            (
                "GET",
                "/api/agents/missing/connectors/gdrive",
                "https://unapproved.example",
                StatusCode::FORBIDDEN,
            ),
            (
                "POST",
                "/api/agents/missing/connectors/gdrive",
                "https://unapproved.example",
                StatusCode::FORBIDDEN,
            ),
            (
                "GET",
                "/api/agents/missing/connectors/gdrive",
                "http://localhost:4200",
                StatusCode::NOT_FOUND,
            ),
            (
                "GET",
                "/api/connectors/gdrive/callback?state=private&code=secret&error=provider-secret",
                "",
                StatusCode::BAD_REQUEST,
            ),
        ] {
            let mut request = Request::builder()
                .method(method)
                .uri(path)
                .header("host", "127.0.0.1:8080");
            if !origin.is_empty() {
                request = request.header("origin", origin);
                if origin == "http://localhost:4200" {
                    request = crate::routes::owner_test_support::authenticate(request);
                }
            }
            let response = app
                .clone()
                .oneshot(request.body(Body::from("unparsed-private-body")).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), status);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            let body = to_bytes(response.into_body(), 16384).await.unwrap();
            let text = String::from_utf8_lossy(&body);
            for forbidden in ["secret", "private", "unparsed"] {
                assert!(!text.contains(forbidden));
            }
        }
    }
}
