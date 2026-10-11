use super::*;
use std::collections::BTreeSet;
use utoipa::OpenApi;

const README: &str = include_str!("../../../README.md");
const START: &str = "<!-- openapi-route-inventory:start -->";
const END: &str = "<!-- openapi-route-inventory:end -->";

fn inventory() -> BTreeSet<(String, String)> {
    let document = serde_json::to_value(crate::routes::ApiDoc::openapi()).unwrap();
    let mut entries = BTreeSet::new();
    for (path, item) in document["paths"].as_object().unwrap() {
        for method in ["get", "post", "put", "patch", "delete", "head", "options"] {
            if item.get(method).is_some() {
                entries.insert((method.to_uppercase(), path.clone()));
            }
        }
    }
    entries
}

#[test]
fn docs_contracts_readme_inventory_matches_actual_openapi() {
    let table = README
        .split_once(START)
        .expect("inventory start")
        .1
        .split_once(END)
        .expect("inventory end")
        .0;
    let documented: BTreeSet<_> = table
        .lines()
        .filter_map(|line| {
            let cells: Vec<_> = line.split('|').map(str::trim).collect();
            if cells.len() < 4 || !cells[1].starts_with('`') {
                return None;
            }
            Some((
                cells[1].trim_matches('`').to_string(),
                cells[2].trim_matches('`').to_string(),
            ))
        })
        .collect();
    assert_eq!(
        documented,
        inventory(),
        "Regenerate the marked README table from ApiDoc::openapi(); aliases belong outside it."
    );
}

#[test]
#[ignore = "Print sorted OpenAPI README rows for explicit documentation regeneration"]
fn docs_contracts_print_inventory() {
    for (method, path) in inventory() {
        println!("| `{method}` | `{path}` |");
    }
}

fn owner_console_path(path: &str) -> bool {
    path == "/api/status"
        || path.starts_with("/api/logs")
        || path.starts_with("/api/usage")
        || path.starts_with("/api/pricing")
        || path.starts_with("/api/skills")
        || (path.starts_with("/api/agents/")
            && [
                "/sessions",
                "/events",
                "/runs",
                "/approvals",
                "/approval-policy",
                "/approval-rules",
                "/agent-files",
            ]
            .iter()
            .any(|part| path.contains(part)))
}

#[tokio::test]
async fn docs_contracts_console_operations_are_mounted_behind_owner_boundary() {
    let state = Arc::new(RwLock::new(DaemonState::new()));
    let app = router(state, DaemonConfig::default());
    let mut checked = 0;
    for (method, path) in inventory()
        .into_iter()
        .filter(|(_, path)| owner_console_path(path))
    {
        let mut concrete = String::new();
        let mut placeholder = false;
        for character in path.chars() {
            match character {
                '{' => {
                    placeholder = true;
                    concrete.push_str("missing");
                }
                '}' => placeholder = false,
                _ if !placeholder => concrete.push(character),
                _ => {}
            }
        }
        let request = Request::builder()
            .method(method.as_str())
            .uri(&concrete)
            .header("host", "127.0.0.1:8080")
            .header("origin", "https://foreign.example.invalid")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "{method} {path} must reach a mounted owner guard before reading/mutating"
        );
        checked += 1;
    }
    assert!(checked >= 25, "console inventory unexpectedly shrank");
}
