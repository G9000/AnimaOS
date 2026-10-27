use serde_json::{json, Value};

/// Model-facing shape. The renderer additionally applies format, source-byte,
/// font coverage and layout limits before producing any successful result.
pub fn document_schema() -> Value {
    let scalar = json!({"type":["string","number","boolean","null"]});
    let table = json!({"type":"object","additionalProperties":false,"properties":{
        "type":{"const":"table"},"headers":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"string","maxLength":1024}},
        "rows":{"type":"array","maxItems":1000,"items":{"type":"array","maxItems":32,"items":scalar}}
    },"required":["type","headers","rows"]});
    let block = json!({"oneOf":[
        {"type":"object","additionalProperties":false,"properties":{"type":{"const":"paragraph"},"text":{"type":"string","maxLength":8192}},"required":["type","text"]},
        {"type":"object","additionalProperties":false,"properties":{"type":{"const":"heading"},"text":{"type":"string","minLength":1,"maxLength":8192},"level":{"type":"integer","minimum":1,"maximum":6,"default":2}},"required":["type","text"]},
        {"type":"object","additionalProperties":false,"properties":{"type":{"const":"bullets"},"items":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"string","minLength":1,"maxLength":8192}}},"required":["type","items"]},table
    ]});
    json!({"type":"object","additionalProperties":false,"properties":{
        "format":{"type":"string","enum":["docx","xlsx","pptx","pdf"]},
        "title":{"type":"string","minLength":1,"maxLength":200},
        "blocks":{"type":"array","maxItems":128,"items":block},
        "sheets":{"type":"array","maxItems":16,"items":{"type":"object","additionalProperties":false,"properties":{
            "name":{"type":"string","minLength":1,"maxLength":31},"columns":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"string","minLength":1,"maxLength":1024}},
            "rows":{"type":"array","maxItems":1000,"items":{"type":"array","maxItems":32,"items":scalar}}
        },"required":["name","columns","rows"]}},
        "slides":{"type":"array","maxItems":64,"items":{"type":"object","additionalProperties":false,"properties":{
            "title":{"type":"string","minLength":1,"maxLength":200},"blocks":{"type":"array","maxItems":128,"items":block}
        },"required":["title","blocks"]}}
    },"required":["format","title"],"description":"DOCX/PDF require blocks; XLSX requires sheets; PPTX requires slides. Other content arrays must be empty. Text length limits use UTF-8 bytes. Source is limited to 48 KiB and output to 4 MiB. Strings beginning = are literal, never spreadsheet formulas. PDF supports left-to-right Latin, Greek, Cyrillic and simple symbols with font coverage; it rejects scripts or combining sequences requiring complex shaping. Slides reject overflowing content."})
}
