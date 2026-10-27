//! Manual interoperability fixture. The caller owns and removes the directory.
use anima_document_content::{render_document, DocumentSpec};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::path::PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("Supply an existing temporary output directory")?,
    );
    if !directory.is_dir() {
        return Err("Output directory must already exist".into());
    }
    let blocks = json!([
        {"type":"heading","text":"Results & next steps","level":2},
        {"type":"paragraph","text":"Café — résumé, Ελληνικά, Привет. Literal <markup> & XML punctuation.\nA second line with a tab:\tvalue."},
        {"type":"bullets","items":["Review the quarterly plan","Confirm the next milestone"]},
        {"type":"table","headers":["Item","Amount","Ready"],"rows":[["Café",12.5,true],["=SUM(1,2)",null,false]]}
    ]);
    let mut long_blocks = blocks.as_array().unwrap().clone();
    for start in [0, 70] {
        long_blocks.push(json!({"type":"paragraph","text":(start..start+70).map(|n|format!("Milestone {n}: a complete and readable report with café notes.\n")).collect::<String>()}));
    }
    let documents = [
        json!({"format":"docx","title":"Quarterly report","blocks":blocks}),
        json!({"format":"xlsx","title":"Quarterly budget","sheets":[
            {"name":"Budget & plan","columns":["Item","Amount","Ready"],"rows":[["Café",12.5,true],["=SUM(1,2)",null,false]]},
            {"name":"Notes","columns":["Comment"],"rows":[["Literal = text and Ελληνικά"]]}
        ]}),
        json!({"format":"pptx","title":"Quarterly briefing","slides":[
            {"title":"A practical plan","blocks":[{"type":"paragraph","text":"Café — a clear quarterly briefing."},{"type":"bullets","items":["Review the plan","Approve the next milestone"]}]},
            {"title":"Budget & owners","blocks":[{"type":"table","headers":["Work","Owner","Amount"],"rows":[["Launch","Lee",12.5],["Review","Sam",null]]}]}
        ]}),
        json!({"format":"pdf","title":"Quarterly report","blocks":long_blocks}),
    ];
    for document in documents {
        let spec: DocumentSpec = serde_json::from_value(document)?;
        let generated = render_document(&spec)?;
        let path = directory.join(format!("sample.{}", spec.format.extension()));
        std::fs::write(&path, &generated.bytes)?;
        println!("{} ({} bytes)", path.display(), generated.bytes.len());
    }
    Ok(())
}
