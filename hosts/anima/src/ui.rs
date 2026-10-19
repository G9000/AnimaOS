//! The embedded web console, served as the daemon router's fallback.

use axum::Router;

/// How the console answers a request path that no daemon route claimed.
#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(not(feature = "bundled-ui"), allow(dead_code))] // only the bundled console consults it
enum Fallback {
    /// API and health paths that matched no route stay 404 instead of returning the app shell.
    NotFound,
    /// Anything else: the exact file if it exists, otherwise `index.html` (client-side routing).
    Page,
}

#[cfg_attr(not(feature = "bundled-ui"), allow(dead_code))]
fn classify(path: &str) -> Fallback {
    let is_api = path == "/api" || path.starts_with("/api/");
    let is_health = path == "/health" || path.starts_with("/health/");
    if is_api || is_health {
        Fallback::NotFound
    } else {
        Fallback::Page
    }
}

/// The console router, or `None` when this build doesn't bundle the web console.
pub fn ui_router() -> Option<Router> {
    #[cfg(feature = "bundled-ui")]
    {
        Some(bundled::router())
    }
    #[cfg(not(feature = "bundled-ui"))]
    {
        None
    }
}

#[cfg(feature = "bundled-ui")]
mod bundled {
    use axum::body::Body;
    use axum::http::{header, HeaderValue, StatusCode, Uri};
    use axum::response::{IntoResponse, Response};
    use axum::Router;
    use rust_embed::RustEmbed;

    use super::{classify, Fallback};

    #[derive(RustEmbed)]
    #[folder = "../../apps/web/dist"]
    struct Assets;

    pub(super) fn router() -> Router {
        Router::new().fallback(serve)
    }

    async fn serve(uri: Uri) -> Response {
        let path = uri.path();
        if classify(path) == Fallback::NotFound {
            return StatusCode::NOT_FOUND.into_response();
        }
        let wanted = path.trim_start_matches('/');
        if !wanted.is_empty() {
            if let Some(file) = Assets::get(wanted) {
                let cache = if wanted.starts_with("assets/") {
                    "public, max-age=31536000, immutable"
                } else {
                    "no-cache"
                };
                return file_response(file, cache);
            }
        }
        match Assets::get("index.html") {
            Some(index) => file_response(index, "no-cache"),
            None => StatusCode::NOT_FOUND.into_response(),
        }
    }

    fn file_response(file: rust_embed::EmbeddedFile, cache: &'static str) -> Response {
        let mime = file.metadata.mimetype().to_string();
        let mut response = Response::new(Body::from(file.data.into_owned()));
        let headers = response.headers_mut();
        if let Ok(value) = HeaderValue::from_str(&mime) {
            headers.insert(header::CONTENT_TYPE, value);
        }
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
        response
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use axum::http::Request;
        use tower::ServiceExt;

        async fn get(path: &str) -> Response {
            router()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap()
        }

        async fn body_text(response: Response) -> String {
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            String::from_utf8_lossy(&bytes).into_owned()
        }

        #[tokio::test]
        async fn ui_router_falls_back_to_index() {
            let response = get("/some/deep/path").await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
            assert!(body_text(response).await.contains("<div id=\"root\""));
        }

        #[tokio::test]
        async fn ui_router_404s_api_paths() {
            for path in ["/api/nope", "/api", "/health/nope", "/health"] {
                let response = get(path).await;
                assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
                assert!(!body_text(response).await.contains("<div id=\"root\""));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(feature = "bundled-ui"))]
    #[test]
    fn ui_router_none_without_feature() {
        assert!(ui_router().is_none());
    }

    #[test]
    fn api_and_health_paths_never_fall_back_to_the_app_shell() {
        for path in ["/api", "/api/x", "/api/x/y", "/health", "/health/ready"] {
            assert_eq!(classify(path), Fallback::NotFound, "{path}");
        }
        for path in [
            "/",
            "/agents",
            "/apis",
            "/healthy",
            "/assets/a.js",
            "/x/api/y",
        ] {
            assert_eq!(classify(path), Fallback::Page, "{path}");
        }
    }
}
