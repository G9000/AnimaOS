//! Pure validation of the supported temporal graph wire contract.
use super::ReadFailure;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    Entity,
    Assertion,
    Evidence,
}

impl Kind {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Entity => "entity",
            Self::Assertion => "assertion",
            Self::Evidence => "evidence",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Reference {
    pub(super) kind: Kind,
    pub(super) id: String,
}

impl Reference {
    pub(super) fn from_record(record: &Value) -> Result<Self, ReadFailure> {
        let reference = Self {
            kind: serde_json::from_value(record["draft"]["kind"].clone())
                .map_err(|_| ReadFailure::Unavailable)?,
            id: record["metadata"]["id"]
                .as_str()
                .ok_or(ReadFailure::Unavailable)?
                .into(),
        };
        reference.validate()?;
        Ok(reference)
    }
    fn validate(&self) -> Result<(), ReadFailure> {
        uuid_id(&self.id)
    }
}

pub(super) fn integer(value: &Value) -> Result<u64, ReadFailure> {
    // Portable MCP numbers are f64. Refuse identities we cannot represent exactly.
    value
        .as_f64()
        .filter(|v| v.is_finite() && *v >= 0.0 && *v <= 9_007_199_254_740_991.0 && v.fract() == 0.0)
        .map(|v| v as u64)
        .ok_or(ReadFailure::Unavailable)
}

pub(super) fn nonempty(value: &Value) -> Result<&str, ReadFailure> {
    value
        .as_str()
        .filter(|v| !v.trim().is_empty())
        .ok_or(ReadFailure::Unavailable)
}

fn timestamp(value: &Value) -> Result<(), ReadFailure> {
    let text = nonempty(value)?;
    if text.len() > 40 || !text.ends_with('Z') {
        return Err(ReadFailure::Unavailable);
    }
    chrono::DateTime::parse_from_rfc3339(text).map_err(|_| ReadFailure::Unavailable)?;
    Ok(())
}

fn optional_time(value: &Value) -> Result<(), ReadFailure> {
    if !value.is_null() {
        timestamp(value)?;
    }
    Ok(())
}

fn existing(value: &Value, kind: Kind) -> Result<Reference, ReadFailure> {
    shape(value, &["refType", "kind", "id"], &[])?;
    if value["refType"] != "existing" {
        return Err(ReadFailure::Unavailable);
    }
    let reference: Reference =
        serde_json::from_value(json!({"kind":value["kind"],"id":value["id"]}))
            .map_err(|_| ReadFailure::Unavailable)?;
    reference.validate()?;
    if reference.kind != kind {
        return Err(ReadFailure::Unavailable);
    }
    Ok(reference)
}

pub(super) fn entity_references(record: &Value) -> Result<Vec<Reference>, ReadFailure> {
    if record["draft"]["kind"] != "assertion" {
        return Ok(vec![]);
    }
    let draft = &record["draft"];
    let mut references = vec![existing(&draft["subject"], Kind::Entity)?];
    if draft["object"]["type"] == "entityRef" {
        references.push(existing(&draft["object"]["entityRef"], Kind::Entity)?);
    }
    Ok(references)
}

fn shape(value: &Value, required: &[&str], optional: &[&str]) -> Result<(), ReadFailure> {
    let fields = value.as_object().ok_or(ReadFailure::Unavailable)?;
    if required.iter().any(|key| !fields.contains_key(*key))
        || fields
            .keys()
            .any(|key| !required.contains(&key.as_str()) && !optional.contains(&key.as_str()))
    {
        return Err(ReadFailure::Unavailable);
    }
    Ok(())
}

fn text(value: &Value, limit: usize) -> Result<&str, ReadFailure> {
    let value = nonempty(value)?;
    if value.chars().count() > limit {
        return Err(ReadFailure::Unavailable);
    }
    Ok(value)
}

pub(super) fn uuid_id(value: &str) -> Result<(), ReadFailure> {
    let parsed = uuid::Uuid::parse_str(value).map_err(|_| ReadFailure::Unavailable)?;
    if value.len() != 36
        || (parsed.as_u128() != 0
            && value != "ffffffff-ffff-ffff-ffff-ffffffffffff"
            && (parsed.get_variant() != uuid::Variant::RFC4122
                || !(1..=8).contains(&parsed.get_version_num())))
    {
        return Err(ReadFailure::Unavailable);
    }
    Ok(())
}

fn namespace(value: &str) -> Result<(), ReadFailure> {
    let (prefix, name) = value.split_once(':').ok_or(ReadFailure::Unavailable)?;
    if prefix.is_empty()
        || name.is_empty()
        || value.chars().count() > 200
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._:/-".contains(&c))
    {
        return Err(ReadFailure::Unavailable);
    }
    Ok(())
}

fn bounded_json(value: &Value) -> Result<(), ReadFailure> {
    fn depth(value: &Value, level: usize) -> bool {
        if level > 24 {
            return false;
        }
        match value {
            Value::Array(values) => values.iter().all(|v| depth(v, level + 1)),
            Value::Object(values) => values.values().all(|v| depth(v, level + 1)),
            _ => true,
        }
    }
    if !depth(value, 0)
        || serde_json::to_vec(value)
            .map_err(|_| ReadFailure::Unavailable)?
            .len()
            > 262_144
    {
        return Err(ReadFailure::Unavailable);
    }
    Ok(())
}

fn extensions(value: &Value) -> Result<(), ReadFailure> {
    let entries = value.as_object().ok_or(ReadFailure::Unavailable)?;
    if entries.len() > 32 {
        return Err(ReadFailure::Unavailable);
    }
    for (name, entry) in entries {
        namespace(name)?;
        shape(entry, &["schemaVersion", "payload"], &[])?;
        if !(1..=u32::MAX as u64).contains(&integer(&entry["schemaVersion"])?) {
            return Err(ReadFailure::Unavailable);
        }
        bounded_json(&entry["payload"])?;
    }
    Ok(())
}

fn temporal(value: &Value, validity: bool) -> Result<(), ReadFailure> {
    if validity {
        shape(
            value,
            &["precision"],
            &["validFrom", "validUntil", "sourceTimezone"],
        )?;
    } else {
        shape(value, &["precision"], &["at", "sourceTimezone"])?;
    }
    let precision = value["precision"]
        .as_str()
        .ok_or(ReadFailure::Unavailable)?;
    if !matches!(
        precision,
        "unknown" | "year" | "month" | "day" | "minute" | "second" | "instant"
    ) {
        return Err(ReadFailure::Unavailable);
    }
    if !value["sourceTimezone"].is_null()
        && text(&value["sourceTimezone"], 100)?
            .chars()
            .any(char::is_control)
    {
        return Err(ReadFailure::Unavailable);
    }
    let unknown = if validity {
        let from = &value["validFrom"];
        let until = &value["validUntil"];
        optional_time(from)?;
        optional_time(until)?;
        if let (Some(from), Some(until)) = (from.as_str(), until.as_str()) {
            let from =
                chrono::DateTime::parse_from_rfc3339(from).map_err(|_| ReadFailure::Unavailable)?;
            let until = chrono::DateTime::parse_from_rfc3339(until)
                .map_err(|_| ReadFailure::Unavailable)?;
            if from >= until {
                return Err(ReadFailure::Unavailable);
            }
        }
        from.is_null() && until.is_null()
    } else {
        optional_time(&value["at"])?;
        value["at"].is_null()
    };
    if unknown != (precision == "unknown") {
        return Err(ReadFailure::Unavailable);
    }
    Ok(())
}

fn typed_value(value: &Value) -> Result<(), ReadFailure> {
    match value["type"].as_str() {
        Some("string") => {
            shape(value, &["type", "value"], &[])?;
            text(&value["value"], 16_384)?;
        }
        Some("number") => {
            shape(value, &["type", "value"], &["unit"])?;
            if !value["value"].as_f64().is_some_and(f64::is_finite) {
                return Err(ReadFailure::Unavailable);
            }
            if !value["unit"].is_null() {
                text(&value["unit"], 100)?;
            }
        }
        Some("boolean") => {
            shape(value, &["type", "value"], &[])?;
            if !value["value"].is_boolean() {
                return Err(ReadFailure::Unavailable);
            }
        }
        Some("instant") => {
            shape(value, &["type", "value"], &[])?;
            timestamp(&value["value"])?;
        }
        Some("json") => {
            shape(value, &["type", "value"], &[])?;
            bounded_json(&value["value"])?;
        }
        _ => return Err(ReadFailure::Unavailable),
    }
    Ok(())
}

fn source(value: &Value) -> Result<(), ReadFailure> {
    match value["type"].as_str() {
        Some("uri") => {
            shape(value, &["type", "uri"], &[])?;
            let uri = reqwest::Url::parse(text(&value["uri"], 2048)?)
                .map_err(|_| ReadFailure::Unavailable)?;
            if !matches!(uri.scheme(), "https" | "http" | "urn")
                || !uri.username().is_empty()
                || uri.password().is_some()
            {
                return Err(ReadFailure::Unavailable);
            }
        }
        Some("artifact") => {
            shape(value, &["type", "sha256", "sizeBytes", "mediaType"], &[])?;
            let hash = nonempty(&value["sha256"])?;
            if hash.len() != 64
                || !hash
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                || integer(&value["sizeBytes"])? == 0
            {
                return Err(ReadFailure::Unavailable);
            }
            text(&value["mediaType"], 200)?;
        }
        _ => return Err(ReadFailure::Unavailable),
    }
    Ok(())
}

pub(super) fn validate_record(record: &Value) -> Result<(Reference, u64, bool), ReadFailure> {
    shape(record, &["metadata", "draft"], &[])?;
    let reference = Reference::from_record(record)?;
    let meta = &record["metadata"];
    shape(
        meta,
        &[
            "id",
            "revision",
            "commitSequence",
            "transactionTime",
            "actor",
            "contributor",
            "lifecycle",
        ],
        &["previousRevision", "approver"],
    )?;
    let revision = integer(&meta["revision"])?;
    if revision == 0 || integer(&meta["commitSequence"])? == 0 {
        return Err(ReadFailure::Unavailable);
    }
    if (revision == 1 && !meta["previousRevision"].is_null())
        || (revision > 1 && integer(&meta["previousRevision"])? != revision - 1)
    {
        return Err(ReadFailure::Unavailable);
    }
    timestamp(&meta["transactionTime"])?;
    text(&meta["actor"], 80)?;
    text(&meta["contributor"], 80)?;
    if !meta["approver"].is_null() {
        text(&meta["approver"], 80)?;
    }
    let active = match meta["lifecycle"].as_str() {
        Some("active") => true,
        Some("retracted" | "superseded") => false,
        _ => return Err(ReadFailure::Unavailable),
    };
    let draft = &record["draft"];
    extensions(&draft["extensions"])?;
    match reference.kind {
        Kind::Entity => {
            shape(
                draft,
                &["kind", "entityType", "displayName", "aliases", "extensions"],
                &[],
            )?;
            namespace(nonempty(&draft["entityType"])?)?;
            text(&draft["displayName"], 512)?;
            let aliases = draft["aliases"]
                .as_array()
                .ok_or(ReadFailure::Unavailable)?;
            if aliases.len() > 32 || aliases.iter().any(|v| text(v, 512).is_err()) {
                return Err(ReadFailure::Unavailable);
            }
        }
        Kind::Assertion => {
            shape(
                draft,
                &[
                    "kind",
                    "subject",
                    "predicate",
                    "object",
                    "validTime",
                    "observedTime",
                    "evidence",
                    "extensions",
                ],
                &["confidence"],
            )?;
            entity_references(record)?;
            namespace(nonempty(&draft["predicate"])?)?;
            match draft["object"]["type"].as_str() {
                Some("entityRef") => {
                    shape(&draft["object"], &["type", "entityRef"], &[])?;
                }
                Some("value") => {
                    shape(&draft["object"], &["type", "value"], &[])?;
                    typed_value(&draft["object"]["value"])?;
                }
                _ => return Err(ReadFailure::Unavailable),
            }
            temporal(&draft["validTime"], true)?;
            temporal(&draft["observedTime"], false)?;
            let evidence = draft["evidence"]
                .as_array()
                .ok_or(ReadFailure::Unavailable)?;
            if evidence.len() > 64 {
                return Err(ReadFailure::Unavailable);
            }
            for reference in evidence {
                existing(reference, Kind::Evidence)?;
            }
            if !draft["confidence"].is_null() {
                let confidence = &draft["confidence"];
                shape(confidence, &["score", "basis"], &[])?;
                if !confidence["score"]
                    .as_f64()
                    .is_some_and(|score| score.is_finite() && (0.0..=1.0).contains(&score))
                    || !matches!(
                        confidence["basis"].as_str(),
                        Some("owner_statement" | "source" | "model_inference")
                    )
                {
                    return Err(ReadFailure::Unavailable);
                }
            }
        }
        Kind::Evidence => {
            shape(
                draft,
                &["kind", "source", "observedTime", "extensions"],
                &["excerpt"],
            )?;
            source(&draft["source"])?;
            if !draft["excerpt"].is_null() {
                text(&draft["excerpt"], 16_384)?;
            }
            temporal(&draft["observedTime"], false)?;
        }
    }
    Ok((reference, revision, active))
}

#[derive(Default)]
pub(super) struct Snapshot {
    pub(super) epoch: Option<String>,
    pub(super) watermark: Option<u64>,
    pub(super) queries: Vec<Value>,
}

fn coverage(value: &Value) -> Result<u64, ReadFailure> {
    shape(
        value,
        &[
            "complete",
            "truncated",
            "canonicalWatermark",
            "indexWatermark",
            "searchMode",
        ],
        &["reason", "model", "profile"],
    )?;
    let complete = value["complete"]
        .as_bool()
        .ok_or(ReadFailure::Unavailable)?;
    let truncated = value["truncated"]
        .as_bool()
        .ok_or(ReadFailure::Unavailable)?;
    let watermark = integer(&value["canonicalWatermark"])?;
    if integer(&value["indexWatermark"])? > watermark
        || (complete && truncated)
        || (!complete && value["reason"].is_null())
    {
        return Err(ReadFailure::Unavailable);
    }
    if !value["reason"].is_null() {
        text(&value["reason"], 1000)?;
    }
    let mode = value["searchMode"]
        .as_str()
        .ok_or(ReadFailure::Unavailable)?;
    if !matches!(
        mode,
        "exact" | "literal" | "vector" | "hybrid" | "literal_fallback"
    ) {
        return Err(ReadFailure::Unavailable);
    }
    if matches!(mode, "vector" | "hybrid")
        && (value["model"].is_null() || value["profile"].is_null())
    {
        return Err(ReadFailure::Unavailable);
    }
    for field in ["model", "profile"] {
        if !value[field].is_null() {
            text(&value[field], 200)?;
        }
    }
    Ok(watermark)
}

fn record_within(record: &Value, watermark: u64) -> Result<(), ReadFailure> {
    validate_record(record)?;
    if integer(&record["metadata"]["commitSequence"])? > watermark {
        return Err(ReadFailure::Unavailable);
    }
    Ok(())
}

impl Snapshot {
    pub(super) fn observe(&mut self, response: &Value, query: bool) -> Result<(), ReadFailure> {
        if query {
            shape(
                response,
                &[
                    "version",
                    "workspaceId",
                    "generation",
                    "hits",
                    "coverage",
                    "page",
                ],
                &[],
            )?;
        } else {
            shape(
                response,
                &[
                    "version",
                    "workspaceId",
                    "generation",
                    "record",
                    "coverage",
                    "visibilityEpoch",
                ],
                &[],
            )?;
        }
        if integer(&response["version"])? != 2 {
            return Err(ReadFailure::Unavailable);
        }
        uuid_id(nonempty(&response["workspaceId"])?)?;
        uuid_id(nonempty(&response["generation"])?)?;
        let coverage = &response["coverage"];
        let watermark = self::coverage(coverage)?;
        let epoch = text(
            if query {
                &response["page"]["visibilityEpoch"]
            } else {
                &response["visibilityEpoch"]
            },
            200,
        )?;
        if self
            .epoch
            .as_deref()
            .is_some_and(|previous| previous != epoch)
            || self.watermark.is_some_and(|previous| previous != watermark)
        {
            return Err(ReadFailure::Unavailable);
        }
        self.epoch = Some(epoch.to_owned());
        self.watermark = Some(watermark);
        if query {
            let page = &response["page"];
            shape(
                page,
                &["visibilityEpoch", "snapshotId", "snapshotExpiresAt"],
                &["nextCursor"],
            )?;
            text(&page["snapshotId"], 200)?;
            timestamp(&page["snapshotExpiresAt"])?;
            let cursor = &response["page"]["nextCursor"];
            if !cursor.is_null() {
                text(cursor, 4096)?;
            }
            let hits = response["hits"]
                .as_array()
                .ok_or(ReadFailure::Unavailable)?;
            if hits.len() > 4 {
                return Err(ReadFailure::Unavailable);
            }
            for hit in hits {
                shape(hit, &["record", "sources", "scores"], &[])?;
                record_within(&hit["record"], watermark)?;
                let sources = hit["sources"].as_array().ok_or(ReadFailure::Unavailable)?;
                if sources.len() > 64 {
                    return Err(ReadFailure::Unavailable);
                }
                for source in sources {
                    let reference: Reference = serde_json::from_value(source.clone())
                        .map_err(|_| ReadFailure::Unavailable)?;
                    reference.validate()?;
                    if reference.kind != Kind::Evidence {
                        return Err(ReadFailure::Unavailable);
                    }
                }
                shape(&hit["scores"], &[], &["lexical", "vector"])?;
                for key in ["lexical", "vector"] {
                    if !hit["scores"][key].is_null()
                        && !hit["scores"][key].as_f64().is_some_and(f64::is_finite)
                    {
                        return Err(ReadFailure::Unavailable);
                    }
                }
            }
            self.queries
                .push(json!({"coverage":coverage,"hasMore":cursor.is_string()}));
        } else {
            record_within(&response["record"], watermark)?;
            if coverage["complete"] != true || coverage["truncated"] != false {
                return Err(ReadFailure::Unavailable);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(kind: &str) -> Value {
        let draft = match kind {
            "entity" => {
                json!({"kind":"entity","entityType":"synthetic:project","displayName":"AsterOS","aliases":[],"extensions":{}})
            }
            "assertion" => {
                json!({"kind":"assertion","subject":{"refType":"existing","kind":"entity","id":"b8bf1254-511a-454b-83e8-e81f3f271086"},
                "predicate":"synthetic:status","object":{"type":"value","value":{"type":"string","value":"ready"}},
                "validTime":{"validFrom":null,"validUntil":null,"sourceTimezone":null,"precision":"unknown"},
                "observedTime":{"at":null,"sourceTimezone":null,"precision":"unknown"},"evidence":[],"confidence":null,"extensions":{}})
            }
            _ => {
                json!({"kind":"evidence","source":{"type":"uri","uri":"https://example.invalid/source"},"excerpt":"Synthetic source",
                "observedTime":{"at":null,"sourceTimezone":null,"precision":"unknown"},"extensions":{}})
            }
        };
        json!({"metadata":{"id":"58fb1c6d-aa99-413e-84fc-12e8e15afc48","revision":1,"previousRevision":null,
            "commitSequence":2,"transactionTime":"2026-11-14T00:00:00.000Z","actor":"synthetic-actor",
            "contributor":"synthetic-contributor","approver":null,"lifecycle":"active"},"draft":draft})
    }

    fn response(record: Value, query: bool) -> Value {
        let mut value = json!({"version":2,"workspaceId":"fdb02e89-a206-4ea0-b7ab-2ebda2627175",
            "generation":"55ba8bbb-f30d-401c-b75c-73594d27da8b","coverage":{"complete":true,"truncated":false,
            "reason":null,"canonicalWatermark":8,"indexWatermark":8,"searchMode":"exact","model":null,"profile":null}});
        if query {
            value["hits"] =
                json!([{"record":record,"sources":[],"scores":{"lexical":-5,"vector":10}}]);
            value["page"] = json!({"nextCursor":null,"visibilityEpoch":"opaque","snapshotId":"opaque-snapshot","snapshotExpiresAt":"2027-01-01T00:00:00Z"});
        } else {
            value["record"] = record;
            value["visibilityEpoch"] = json!("opaque");
        }
        value
    }

    #[test]
    fn memory_provider_graph_wire_valid_records_and_unknown_extensions_round_trip() {
        for kind in ["entity", "assertion", "evidence"] {
            let mut record = record(kind);
            record["draft"]["extensions"] = json!({"future:unknown":{"schemaVersion":99,"payload":{"$ref":"ordinary-data","nested":[null,true,3]}}});
            let original = record.clone();
            assert!(validate_record(&record).is_ok());
            assert_eq!(record, original);
            assert!(Snapshot::default()
                .observe(&response(record, true), true)
                .is_ok());
        }
    }

    #[test]
    fn memory_provider_graph_wire_rejects_reversed_or_incoherent_temporal_fields() {
        for (from, until, precision) in [
            (
                json!("2026-11-15T00:00:00Z"),
                json!("2026-11-14T00:00:00Z"),
                json!("instant"),
            ),
            (Value::Null, Value::Null, json!("instant")),
            (Value::Null, Value::Null, json!("not-a-precision")),
            (
                json!("2026-11-14T00:00:00+00:00"),
                Value::Null,
                json!("instant"),
            ),
        ] {
            let mut record = record("assertion");
            record["draft"]["validTime"] = json!({"validFrom":from,"validUntil":until,"sourceTimezone":null,"precision":precision});
            assert!(
                validate_record(&record).is_err(),
                "accepted invalid validity: {}",
                record["draft"]["validTime"]
            );
        }
        let mut record = record("evidence");
        record["draft"]["observedTime"] = json!({"at":null,"sourceTimezone":null});
        assert!(validate_record(&record).is_err());
    }

    #[test]
    fn memory_provider_graph_wire_rejects_unresolved_or_wrong_kind_evidence() {
        for reference in [
            json!({"refType":"item","kind":"evidence","itemKey":"pending"}),
            json!({"refType":"existing","kind":"entity","id":"b8bf1254-511a-454b-83e8-e81f3f271086"}),
            json!({"refType":"existing","kind":"evidence","id":"malformed"}),
        ] {
            let mut record = record("assertion");
            record["draft"]["evidence"] = json!([reference]);
            assert!(validate_record(&record).is_err());
        }
    }

    #[test]
    fn memory_provider_graph_wire_rejects_invalid_sources_without_fetching_them() {
        for source in [
            json!({"type":"uri","uri":"file:///private/source"}),
            json!({"type":"uri","uri":"https://user:password@example.invalid/source"}),
            json!({"type":"artifact","sha256":"x","sizeBytes":1,"mediaType":"text/plain"}),
            json!({"type":"artifact","sha256":"a".repeat(64),"sizeBytes":0,"mediaType":"text/plain"}),
            json!({"type":"unknown","uri":"https://example.invalid"}),
        ] {
            let mut record = record("evidence");
            record["draft"]["source"] = source;
            assert!(validate_record(&record).is_err());
        }
    }

    #[test]
    fn memory_provider_graph_wire_requires_exact_previous_revision() {
        let mut record = record("entity");
        record["metadata"]["revision"] = json!(3);
        record["metadata"]["previousRevision"] = json!(1);
        assert!(validate_record(&record).is_err());
    }

    #[test]
    fn memory_provider_graph_wire_rejects_records_after_the_canonical_watermark() {
        for query in [false, true] {
            let mut record = record("entity");
            record["metadata"]["commitSequence"] = json!(9);
            assert!(Snapshot::default()
                .observe(&response(record, query), query)
                .is_err());
        }
    }

    #[test]
    fn memory_provider_graph_wire_preserves_contract_uuid_boundary_values() {
        for id in [
            "00000000-0000-0000-0000-000000000000",
            "ffffffff-ffff-ffff-ffff-ffffffffffff",
        ] {
            let mut record = record("entity");
            record["metadata"]["id"] = json!(id);
            assert!(validate_record(&record).is_ok());
        }
    }

    #[test]
    fn memory_provider_graph_wire_rejects_invalid_namespaces_confidence_and_core_fields() {
        let mut invalid = Vec::new();
        let mut entity = record("entity");
        entity["draft"]["entityType"] = json!("unnamespaced");
        invalid.push(entity);
        let mut fact = record("assertion");
        fact["draft"]["confidence"] = json!({"score":1.1,"basis":"source"});
        invalid.push(fact);
        let mut fact = record("assertion");
        fact["draft"]["object"]["value"]["value"] = json!("  ");
        invalid.push(fact);
        let mut entity = record("entity");
        entity["extraCoreField"] = json!("not-an-extension");
        invalid.push(entity);
        let mut entity = record("entity");
        entity["draft"]["extensions"] =
            json!({"missing_namespace":{"schemaVersion":1,"payload":null}});
        invalid.push(entity);
        for record in invalid {
            assert!(validate_record(&record).is_err());
        }
    }

    #[test]
    fn memory_provider_graph_wire_rejects_incoherent_coverage_and_query_sources() {
        let mut cases = Vec::new();
        let base = response(record("entity"), true);
        let mut value = base.clone();
        value["coverage"]["truncated"] = json!(true);
        cases.push(value);
        let mut value = base.clone();
        value["coverage"]["complete"] = json!(false);
        cases.push(value);
        let mut value = base.clone();
        value["coverage"]["searchMode"] = json!("hybrid");
        cases.push(value);
        let mut value = base.clone();
        value["hits"][0]["sources"] =
            json!([{"kind":"entity","id":"b8bf1254-511a-454b-83e8-e81f3f271086"}]);
        cases.push(value);
        let mut value = base;
        value["page"]["snapshotExpiresAt"] = json!("not-an-instant");
        cases.push(value);
        for value in cases {
            assert!(Snapshot::default().observe(&value, true).is_err());
        }
    }
}
