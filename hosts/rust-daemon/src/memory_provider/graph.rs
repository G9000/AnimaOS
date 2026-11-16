//! Optional current graph candidates. No graph write, history or persistent cursor authority.
use super::{MotherProvider, ReadFailure, RecallPlan, SelectedRecord};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[path = "graph_wire.rs"]
mod wire;
use wire::{entity_references, integer, nonempty, validate_record, Kind, Reference, Snapshot};

const MAX_HITS: usize = 4;
const MAX_GETS: usize = 4;
const MAX_STARTS: usize = 2;
const READERS: [&str; 3] = [
    "mother_get_capabilities",
    "mother_garden_query",
    "mother_garden_get",
];

fn identity(provider: &MotherProvider, value: &Value, capability: bool) -> Result<(), ReadFailure> {
    let identity = if capability {
        &value["workspace"]
    } else {
        value
    };
    for field in ["workspaceId", "generation"] {
        wire::uuid_id(nonempty(&identity[field])?).map_err(|_| ReadFailure::Authority)?;
    }
    if identity["workspaceId"] != provider.binding.workspace_id
        || identity["generation"] != provider.binding.generation
    {
        return Err(ReadFailure::Authority);
    }
    Ok(())
}

/// Ranking reads known core text only. Optional extensions stay opaque data.
fn view(record: &Value, related: &[Value]) -> Value {
    let draft = &record["draft"];
    let mut names = Vec::new();
    for entity in std::iter::once(record).chain(related) {
        if entity["draft"]["kind"] == "entity" {
            if let Some(name) = entity["draft"]["displayName"].as_str() {
                names.push(name.to_owned());
            }
            names.extend(
                entity["draft"]["aliases"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned),
            );
        }
    }
    let content = match draft["kind"].as_str() {
        Some("assertion") => format!(
            "{} {}",
            draft["predicate"].as_str().unwrap_or_default(),
            draft["object"]
        ),
        Some("evidence") => format!(
            "{} {}",
            draft["excerpt"].as_str().unwrap_or_default(),
            draft["source"]
        ),
        _ => String::new(),
    };
    json!({"title":names.join(" "),"content":content})
}

fn request(provider: &MotherProvider) -> Value {
    json!({"version":2,"workspaceId":provider.binding.workspace_id,"generation":provider.binding.generation,"requiredExtensions":{}})
}

async fn query(
    provider: &MotherProvider,
    mut args: Value,
    snapshot: &mut Snapshot,
) -> Result<Vec<Value>, ReadFailure> {
    let response = match provider
        .read_complete("mother_garden_query", args.clone())
        .await
    {
        Err(ReadFailure::Partial) => {
            args["limit"] = json!(1);
            provider.read_complete("mother_garden_query", args).await?
        }
        result => result?,
    };
    identity(provider, &response, false)?;
    snapshot.observe(&response, true)?;
    let hits = response["hits"]
        .as_array()
        .ok_or(ReadFailure::Unavailable)?;
    if hits.len() > MAX_HITS {
        return Err(ReadFailure::Unavailable);
    }
    let mut records = Vec::new();
    for hit in hits {
        let (_, _, active) = validate_record(&hit["record"])?;
        if active {
            records.push(hit["record"].clone());
        }
    }
    Ok(records)
}

fn query_request(provider: &MotherProvider) -> Value {
    let mut args = request(provider);
    for (key,value) in json!({"ids":[],"kinds":[],"entityType":null,"predicate":null,"property":null,
        "validAt":null,"knownAt":null,"text":null,"vector":null,"traversal":null,"limit":MAX_HITS,"cursor":null}).as_object().into_iter().flatten() {
        args[key] = value.clone();
    }
    args
}

pub(super) async fn collect(
    provider: &MotherProvider,
    plan: &RecallPlan,
    coverage: &mut Value,
) -> Result<Vec<SelectedRecord>, ReadFailure> {
    *coverage = json!({"status":"not_admitted","mode":"literal"});
    // Discovery/review never grants a tool. Old/context-only connections do no extra I/O.
    for remote in READERS {
        if provider
            .manager
            .workspace_context_tool(&provider.agent.id, &provider.binding.connection_id, remote)
            .await
            .is_none()
        {
            return Ok(vec![]);
        }
    }
    let capabilities = provider
        .read_complete("mother_get_capabilities", json!({}))
        .await?;
    identity(provider, &capabilities, true)?;
    let garden = &capabilities["garden"];
    let available = integer(&capabilities["version"])? == 1
        && integer(&garden["contractVersion"])? == 2
        && garden["available"] == true
        && garden["capabilities"]
            .as_array()
            .is_some_and(|v| v.iter().any(|c| c == "garden.query.v2"))
        && ["mother_garden_get", "mother_garden_query"]
            .iter()
            .all(|name| {
                garden["availableTools"]
                    .as_array()
                    .is_some_and(|v| v.iter().any(|t| t.as_str() == Some(*name)))
            });
    if !available {
        coverage["status"] = json!("unsupported");
        return Ok(vec![]);
    }
    // Only successful queries/gets under the server's garden:read grant can return data.
    let mut snapshot = Snapshot::default();
    let mut candidates = BTreeMap::new();
    let topics = [plan.query.as_str()];
    for topic in topics.into_iter().filter(|t| !t.is_empty()).take(2) {
        let mut args = query_request(provider);
        args["text"] = json!(topic);
        for record in query(provider, args, &mut snapshot).await? {
            candidates.insert(Reference::from_record(&record)?, record);
        }
    }
    let mut seeds: Vec<_> = candidates
        .iter()
        .filter(|(r, v)| r.kind == Kind::Entity && plan.matches_current(&view(v, &[])))
        .map(|(r, v)| (r.clone(), v.clone()))
        .collect();
    seeds.sort_by(|(_, left), (_, right)| {
        plan.rank(&view(right, &[]), false)
            .cmp(&plan.rank(&view(left, &[]), false))
    });
    seeds.truncate(MAX_STARTS);
    if !seeds.is_empty() {
        let mut args = query_request(provider);
        args["traversal"] = json!({"start":seeds.iter().map(|(r,_)| r).collect::<Vec<_>>(),"direction":"both","maxHops":1,"maxEdges":8,"workBudget":32});
        for record in query(provider, args, &mut snapshot).await? {
            candidates.insert(Reference::from_record(&record)?, record);
        }
    }
    let mut order: Vec<_> = candidates
        .iter()
        .map(|(reference, record)| {
            let related: Vec<_> = entity_references(record)?
                .into_iter()
                .filter_map(|r| candidates.get(&r).cloned())
                .collect();
            Ok((reference.clone(), plan.rank(&view(record, &related), false)))
        })
        .collect::<Result<_, ReadFailure>>()?;
    order.sort_by(|(left, lr), (right, rr)| {
        let left_seed = seeds.iter().any(|(r, _)| r == left);
        let right_seed = seeds.iter().any(|(r, _)| r == right);
        right_seed.cmp(&left_seed).then_with(|| rr.cmp(lr))
    });
    let mut current = BTreeMap::new();
    for (reference, _) in order.into_iter().take(MAX_GETS) {
        let mut args = request(provider);
        args["reference"] = json!(reference);
        args["revision"] = Value::Null;
        args["knownAt"] = Value::Null;
        let response = provider.read_complete("mother_garden_get", args).await?;
        identity(provider, &response, false)?;
        snapshot.observe(&response, false)?;
        let (actual, revision, active) = validate_record(&response["record"])?;
        if actual != reference {
            return Err(ReadFailure::Unavailable);
        }
        if !active {
            return Err(ReadFailure::Unavailable);
        }
        current.insert(
            reference,
            (
                response["record"].clone(),
                revision,
                response["coverage"].clone(),
            ),
        );
    }
    let mut selected = Vec::new();
    let mut seen = BTreeSet::new();
    for (reference, (record, revision, coverage)) in &current {
        let related: Vec<_> = entity_references(record)?
            .iter()
            .filter_map(|r| current.get(r).map(|(v, _, _)| v.clone()))
            .collect();
        let evidence_view = view(record, &related);
        if !plan.matches_current(&evidence_view) || !seen.insert(reference.clone()) {
            continue;
        }
        let text = json!({"provider":"mother","format":"garden.v2","temporalSelection":"current","workspaceId":provider.binding.workspace_id,
            "generation":provider.binding.generation,"reference":reference,"record":record,"relatedEntities":related,
            "coverage":coverage,"queryCoverage":snapshot.queries,"visibilityEpoch":snapshot.epoch}).to_string();
        selected.push(SelectedRecord {
            text,
            source_id: format!(
                "mother-garden:{}/{}/{}/{}@{}",
                provider.binding.workspace_id,
                provider.binding.generation,
                reference.kind.name(),
                reference.id,
                revision
            ),
            dedup_key: format!("graph:{}/{}", reference.kind.name(), reference.id),
            owner_order: usize::MAX,
            rank: plan.rank(&evidence_view, false),
        });
    }
    *coverage = json!({"status":"queried","mode":"literal","queries":snapshot.queries});
    Ok(selected)
}
