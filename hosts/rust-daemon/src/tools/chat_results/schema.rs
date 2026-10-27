//! Provider-visible closed result shapes share the daemon's typed wire contract.
use anima_core::DataValue;
use serde_json::{json, Value};

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

fn array(items: Value, min: usize, max: usize) -> Value {
    json!({"type":"array","items":items,"minItems":min,"maxItems":max})
}

fn text(max: usize) -> Value {
    json!({"type":"string","maxLength":max})
}

pub(super) fn payload() -> DataValue {
    let key = json!({"type":"string","minLength":1,"maxLength":40,"pattern":"^[A-Za-z0-9_]+$"});
    let number = json!({"type":"number","minimum":-9_007_199_254_740_991_i64,"maximum":9_007_199_254_740_991_i64});
    let mut kinds = vec![
        object(
            json!({
                "kind":{"type":"string","enum":["chart"]},
                "chartType":{"type":"string","enum":["bar","line"]},
                "labels":array(text(160),1,100),
                "series":array(object(json!({"label":text(160),"values":array(number.clone(),1,100)}),&["label","values"]),1,8)
            }),
            &["kind", "chartType", "labels", "series"],
        ),
        object(
            json!({
                "kind":{"type":"string","enum":["comparison"]},
                "columns":array(text(160),1,12),
                "rows":array(array(text(2048),1,12),0,100)
            }),
            &["kind", "columns", "rows"],
        ),
        object(
            json!({
                "kind":{"type":"string","enum":["document"]},
                "format":{"type":"string","enum":["text","markdown"]},
                "text":text(32*1024)
            }),
            &["kind", "format", "text"],
        ),
        object(
            json!({
                "kind":{"type":"string","enum":["calculator"]},
                "inputs":array(object(json!({"key":key.clone(),"label":text(160),"value":number.clone(),"min":number.clone(),"max":number.clone()}),&["key","label","value"]),1,12),
                "expression":{"type":"object","description":"Closed arithmetic tree: {op:input,key}, {op:constant,value}, or {op:add|subtract|multiply|divide,left:<tree>,right:<tree>}. At most 64 nodes and depth 8; no scripts or function calls."},
                "outputLabel":text(160)
            }),
            &["kind", "inputs", "expression", "outputLabel"],
        ),
        object(
            json!({
                "kind":{"type":"string","enum":["tracker"]},
                "columns":array(object(json!({"key":key.clone(),"label":text(160),"kind":{"type":"string","enum":["text","number","checkbox"]}}),&["key","label","kind"]),1,12),
                "rows":array(object(json!({"id":key.clone(),"cells":{"type":"object","additionalProperties":{"anyOf":[text(2048),number,{"type":"boolean"}]},"maxProperties":12,"description":"Exactly every declared column key, with a value matching its column kind"}}),&["id","cells"]),0,100)
            }),
            &["kind", "columns", "rows"],
        ),
    ];
    let widget_payload = json!({"oneOf":kinds});
    kinds.push(object(json!({
        "kind":{"type":"string","enum":["dashboard"]},
        "widgets":array(object(json!({"id":key,"title":text(160),"payload":widget_payload}),&["id","title","payload"]),1,8)
    }),&["kind","widgets"]));
    crate::tools::generated_documents::schema_data(json!({"oneOf":kinds}))
}
