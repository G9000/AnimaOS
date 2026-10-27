use super::{
    error, xml, CellValue, DocumentBlock, DocumentSpec, GeneratedFormat, GenerationError, Output,
};
use std::io::Write;

const XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>";
const REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const OREL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const S: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
type Parts = Vec<(String, String)>;

pub(super) fn render(spec: &DocumentSpec, source: &[u8]) -> Result<Vec<u8>, GenerationError> {
    let mut parts = Vec::new();
    let mut types = Vec::new();
    let main = match spec.format {
        GeneratedFormat::Docx => {
            docx(spec, &mut parts, &mut types);
            "word/document.xml"
        }
        GeneratedFormat::Xlsx => {
            xlsx(spec, &mut parts, &mut types);
            "xl/workbook.xml"
        }
        GeneratedFormat::Pptx => {
            pptx(spec, &mut parts, &mut types)?;
            "ppt/presentation.xml"
        }
        _ => return Err(error("Unsupported Office format")),
    };
    parts.push(("_rels/.rels".into(), relationships(&[
        ("rId1".into(),format!("{OREL}/officeDocument"),main.into()),
        ("rId2".into(),"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties".into(),"docProps/core.xml".into()),
        ("rId3".into(),format!("{OREL}/extended-properties"),"docProps/app.xml".into()),
    ])));
    parts.push(("docProps/core.xml".into(),format!("{XML}<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>{}</dc:title><dc:creator>AnimaOS</dc:creator></cp:coreProperties>",xml(&spec.title))));
    parts.push(("docProps/app.xml".into(),format!("{XML}<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>AnimaOS</Application></Properties>")));
    types.push((
        "docProps/core.xml".into(),
        "application/vnd.openxmlformats-package.core-properties+xml".into(),
    ));
    types.push((
        "docProps/app.xml".into(),
        "application/vnd.openxmlformats-officedocument.extended-properties+xml".into(),
    ));
    let overrides = types
        .iter()
        .map(|(path, kind)| format!("<Override PartName=\"/{path}\" ContentType=\"{kind}\"/>"))
        .collect::<String>();
    parts.push(("[Content_Types].xml".into(),format!("{XML}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/>{overrides}</Types>")));
    let mut output = Output::new();
    let mut archive = rawzip::ZipArchiveWriter::new(&mut output);
    archive.set_comment(source);
    for (path, body) in parts {
        let (mut entry, config) = archive
            .new_file(&path)
            .compression_method(rawzip::CompressionMethod::STORE)
            .start()
            .map_err(|_| error("Cannot create Office package"))?;
        let mut writer = config.wrap(&mut entry);
        writer.write_all(body.as_bytes())?;
        let (_, descriptor) = writer.finish()?;
        entry.finish(descriptor)?;
    }
    archive.finish()?;
    Ok(output.finish())
}

fn relationships(values: &[(String, String, String)]) -> String {
    format!(
        "{XML}<Relationships xmlns=\"{REL}\">{}</Relationships>",
        values
            .iter()
            .map(|(id, kind, target)| format!(
                "<Relationship Id=\"{id}\" Type=\"{kind}\" Target=\"{target}\"/>"
            ))
            .collect::<String>()
    )
}
fn part(parts: &mut Parts, types: &mut Parts, path: &str, kind: &str, body: String) {
    parts.push((path.into(), body));
    types.push((path.into(), kind.into()));
}

fn word_runs(value: &str) -> String {
    let mut out = String::new();
    // Preserve explicit line breaks and tabs rather than depending on an XML
    // consumer's whitespace normalization inside a text node.
    for (index, line) in value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .enumerate()
    {
        if index > 0 {
            out.push_str("<w:r><w:br/></w:r>");
        }
        for (index, segment) in line.split('\t').enumerate() {
            if index > 0 {
                out.push_str("<w:r><w:tab/></w:r>");
            }
            out.push_str(&format!(
                "<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>",
                xml(segment)
            ));
        }
    }
    out
}
fn word_p(value: &str, properties: &str) -> String {
    format!("<w:p><w:pPr>{properties}</w:pPr>{}</w:p>", word_runs(value))
}
fn word_table(headers: &[String], rows: &[Vec<CellValue>]) -> String {
    let width = 9360 / headers.len();
    let mut out=format!("<w:tbl><w:tblPr><w:tblStyle w:val=\"TableGrid\"/><w:tblW w:w=\"9360\" w:type=\"dxa\"/><w:tblLayout w:type=\"fixed\"/></w:tblPr><w:tblGrid>{}</w:tblGrid>",(0..headers.len()).map(|_|format!("<w:gridCol w:w=\"{width}\"/>")).collect::<String>());
    let head=headers.iter().map(|x|format!("<w:tc><w:tcPr><w:tcW w:w=\"{width}\" w:type=\"dxa\"/><w:shd w:fill=\"E8EEF5\"/></w:tcPr>{}</w:tc>",word_p(x,"<w:pStyle w:val=\"TableHeader\"/>"))).collect::<String>();
    out.push_str(&format!(
        "<w:tr><w:trPr><w:tblHeader/></w:trPr>{head}</w:tr>"
    ));
    for row in rows {
        out.push_str("<w:tr>");
        for cell in row {
            out.push_str(&format!(
                "<w:tc><w:tcPr><w:tcW w:w=\"{width}\" w:type=\"dxa\"/></w:tcPr>{}</w:tc>",
                word_p(&cell.text(), "")
            ));
        }
        out.push_str("</w:tr>");
    }
    out.push_str("</w:tbl>");
    out
}
fn docx(spec: &DocumentSpec, parts: &mut Parts, types: &mut Parts) {
    let mut body = word_p(&spec.title, "<w:pStyle w:val=\"Title\"/>");
    for block in &spec.blocks {
        match block {
            DocumentBlock::Paragraph { text } => body.push_str(&word_p(text, "")),
            DocumentBlock::Heading { text, level } => body.push_str(&word_p(
                text,
                &format!("<w:pStyle w:val=\"Heading{level}\"/>"),
            )),
            DocumentBlock::Bullets { items } => {
                for item in items {
                    body.push_str(&word_p(
                        item,
                        "<w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr>",
                    ));
                }
            }
            DocumentBlock::Table { headers, rows } => body.push_str(&word_table(headers, rows)),
        }
    }
    body.push_str("<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:pgMar w:top=\"1080\" w:right=\"1440\" w:bottom=\"1080\" w:left=\"1440\" w:header=\"360\" w:footer=\"360\" w:gutter=\"0\"/></w:sectPr>");
    part(
        parts,
        types,
        "word/document.xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
        format!("{XML}<w:document xmlns:w=\"{W}\"><w:body>{body}</w:body></w:document>"),
    );
    let headings=(1..=6).map(|level|format!("<w:style w:type=\"paragraph\" w:styleId=\"Heading{level}\"><w:name w:val=\"heading {level}\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:keepNext/><w:spacing w:before=\"240\" w:after=\"120\"/><w:outlineLvl w:val=\"{}\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"{}\"/></w:rPr></w:style>",level-1,36-level*2)).collect::<String>();
    part(parts,types,"word/styles.xml","application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml",format!("{XML}<w:styles xmlns:w=\"{W}\"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Arial\" w:hAnsi=\"Arial\"/><w:sz w:val=\"22\"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"160\" w:line=\"276\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style><w:style w:type=\"paragraph\" w:styleId=\"Title\"><w:name w:val=\"Title\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:keepNext/></w:pPr><w:rPr><w:sz w:val=\"44\"/><w:b/></w:rPr></w:style>{headings}<w:style w:type=\"paragraph\" w:styleId=\"TableHeader\"><w:name w:val=\"Table Header\"/><w:basedOn w:val=\"Normal\"/><w:rPr><w:b/></w:rPr></w:style><w:style w:type=\"table\" w:styleId=\"TableGrid\"><w:name w:val=\"Table Grid\"/><w:tblPr><w:tblBorders><w:top w:val=\"single\" w:sz=\"4\" w:color=\"C7D0DA\"/><w:left w:val=\"single\" w:sz=\"4\" w:color=\"C7D0DA\"/><w:bottom w:val=\"single\" w:sz=\"4\" w:color=\"C7D0DA\"/><w:right w:val=\"single\" w:sz=\"4\" w:color=\"C7D0DA\"/><w:insideH w:val=\"single\" w:sz=\"4\" w:color=\"C7D0DA\"/><w:insideV w:val=\"single\" w:sz=\"4\" w:color=\"C7D0DA\"/></w:tblBorders></w:tblPr></w:style></w:styles>"));
    part(parts,types,"word/numbering.xml","application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml",format!("{XML}<w:numbering xmlns:w=\"{W}\"><w:abstractNum w:abstractNumId=\"0\"><w:multiLevelType w:val=\"singleLevel\"/><w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/><w:lvlText w:val=\"•\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:tabs><w:tab w:val=\"num\" w:pos=\"360\"/></w:tabs><w:ind w:left=\"360\" w:hanging=\"240\"/></w:pPr><w:rPr><w:rFonts w:ascii=\"Arial\" w:hAnsi=\"Arial\"/></w:rPr></w:lvl></w:abstractNum><w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num></w:numbering>"));
    parts.push((
        "word/_rels/document.xml.rels".into(),
        relationships(&[
            ("rId1".into(), format!("{OREL}/styles"), "styles.xml".into()),
            (
                "rId2".into(),
                format!("{OREL}/numbering"),
                "numbering.xml".into(),
            ),
        ]),
    ));
}

fn column(mut index: usize) -> String {
    let mut out = String::new();
    index += 1;
    while index > 0 {
        let digit = (index - 1) % 26;
        out.insert(0, (b'A' + digit as u8) as char);
        index = (index - 1) / 26;
    }
    out
}
fn spreadsheet_cell(address: &str, value: &CellValue, header: bool) -> String {
    let style = if header { " s=\"1\"" } else { "" };
    match value {
        CellValue::String(value)=>format!("<c r=\"{address}\"{style} t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",spreadsheet_string(value)),
        CellValue::Number(value)=>format!("<c r=\"{address}\"{style}><v>{value}</v></c>"),
        CellValue::Bool(value)=>format!("<c r=\"{address}\"{style} t=\"b\"><v>{}</v></c>",u8::from(*value)),
        CellValue::Null=>format!("<c r=\"{address}\"{style}/>"),
    }
}
fn spreadsheet_string(value: &str) -> String {
    // ST_Xstring reserves _xHHHH_ for encoded UTF-16 units. Escape each
    // original leading underscore once, including overlapping-looking tokens.
    let mut out = String::with_capacity(value.len());
    for (offset, ch) in value.char_indices() {
        let tail = &value.as_bytes()[offset..];
        if ch == '_'
            && tail.len() >= 7
            && matches!(tail[1], b'x' | b'X')
            && tail[2..6].iter().all(u8::is_ascii_hexdigit)
            && tail[6] == b'_'
        {
            out.push_str("_x005F_");
        } else {
            out.push(ch);
        }
    }
    xml(&out)
}
fn xlsx(spec: &DocumentSpec, parts: &mut Parts, types: &mut Parts) {
    let mut sheets = String::new();
    let mut rels = Vec::new();
    for (index, sheet) in spec.sheets.iter().enumerate() {
        let id = index + 1;
        let last = format!(
            "{}{}",
            column(sheet.columns.len() - 1),
            sheet.rows.len() + 1
        );
        let mut rows = String::from("<row r=\"1\" ht=\"24\" customHeight=\"1\">");
        for (col, header) in sheet.columns.iter().enumerate() {
            rows.push_str(&spreadsheet_cell(
                &format!("{}1", column(col)),
                &CellValue::String(header.clone()),
                true,
            ));
        }
        rows.push_str("</row>");
        for (r, row) in sheet.rows.iter().enumerate() {
            let n = r + 2;
            rows.push_str(&format!("<row r=\"{n}\">"));
            for (c, value) in row.iter().enumerate() {
                rows.push_str(&spreadsheet_cell(
                    &format!("{}{n}", column(c)),
                    value,
                    false,
                ));
            }
            rows.push_str("</row>");
        }
        part(parts,types,&format!("xl/worksheets/sheet{id}.xml"),"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml",format!("{XML}<worksheet xmlns=\"{S}\"><dimension ref=\"A1:{last}\"/><sheetViews><sheetView workbookViewId=\"0\"><pane ySplit=\"1\" topLeftCell=\"A2\" activePane=\"bottomLeft\" state=\"frozen\"/><selection pane=\"bottomLeft\" activeCell=\"A2\" sqref=\"A2\"/></sheetView></sheetViews><sheetFormatPr defaultColWidth=\"22\" defaultRowHeight=\"18\"/><sheetData>{rows}</sheetData><autoFilter ref=\"A1:{last}\"/><pageMargins left=\"0.5\" right=\"0.5\" top=\"0.75\" bottom=\"0.75\" header=\"0.3\" footer=\"0.3\"/></worksheet>"));
        sheets.push_str(&format!(
            "<sheet name=\"{}\" sheetId=\"{id}\" r:id=\"rId{id}\"/>",
            spreadsheet_string(&sheet.name)
        ));
        rels.push((
            format!("rId{id}"),
            format!("{OREL}/worksheet"),
            format!("worksheets/sheet{id}.xml"),
        ));
    }
    rels.push((
        format!("rId{}", spec.sheets.len() + 1),
        format!("{OREL}/styles"),
        "styles.xml".into(),
    ));
    parts.push(("xl/_rels/workbook.xml.rels".into(), relationships(&rels)));
    part(parts,types,"xl/workbook.xml","application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml",format!("{XML}<workbook xmlns=\"{S}\" xmlns:r=\"{OREL}\"><bookViews><workbookView/></bookViews><sheets>{sheets}</sheets></workbook>"));
    part(parts,types,"xl/styles.xml","application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml",format!("{XML}<styleSheet xmlns=\"{S}\"><fonts count=\"2\"><font><sz val=\"11\"/><name val=\"Arial\"/></font><font><b/><sz val=\"11\"/><name val=\"Arial\"/></font></fonts><fills count=\"3\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill><fill><patternFill patternType=\"solid\"><fgColor rgb=\"FFE8EEF5\"/><bgColor indexed=\"64\"/></patternFill></fill></fills><borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs><cellXfs count=\"2\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyAlignment=\"1\"><alignment vertical=\"top\" wrapText=\"1\"/></xf><xf numFmtId=\"0\" fontId=\"1\" fillId=\"2\" borderId=\"0\" xfId=\"0\" applyFont=\"1\" applyFill=\"1\" applyAlignment=\"1\"><alignment vertical=\"top\" wrapText=\"1\"/></xf></cellXfs><cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles></styleSheet>"));
}

const GROUP:&str="<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>";
const COLORS:&str="<p:clrMap accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" bg1=\"lt1\" bg2=\"lt2\" folHlink=\"folHlink\" hlink=\"hlink\" tx1=\"dk1\" tx2=\"dk2\"/>";

fn drawing_p(value: &str, size: u32, bold: bool, bullet: bool) -> String {
    let mut out = String::new();
    for line in value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\t', "    ")
        .split('\n')
    {
        let properties = if bullet {
            " marL=\"228600\" indent=\"-171450\""
        } else {
            ""
        };
        let bullets = if bullet {
            "<a:buChar char=\"•\"/>"
        } else {
            "<a:buNone/>"
        };
        let weight = if bold { " b=\"1\"" } else { "" };
        out.push_str(&format!("<a:p><a:pPr{properties}><a:lnSpc><a:spcPct val=\"115000\"/></a:lnSpc>{bullets}</a:pPr><a:r><a:rPr lang=\"en-US\" sz=\"{size}\"{weight}><a:solidFill><a:srgbClr val=\"1F2937\"/></a:solidFill><a:latin typeface=\"Arial\"/></a:rPr><a:t>{}</a:t></a:r><a:endParaRPr lang=\"en-US\" sz=\"{size}\"/></a:p>",xml(line)));
    }
    out
}
fn text_shape(id: usize, name: &str, y: i64, height: i64, paragraphs: &str) -> String {
    format!("<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{name}\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"548640\" y=\"{y}\"/><a:ext cx=\"11094720\" cy=\"{height}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/><a:ln><a:noFill/></a:ln></p:spPr><p:txBody><a:bodyPr wrap=\"square\" lIns=\"0\" tIns=\"0\" rIns=\"0\" bIns=\"0\"><a:normAutofit/></a:bodyPr><a:lstStyle/>{paragraphs}</p:txBody></p:sp>")
}
fn estimate_lines(value: &str, size: u32, width: f64) -> usize {
    let face = ttf_parser::Face::parse(include_bytes!("../../assets/DejaVuSans.ttf"), 0)
        .expect("validated bundled font");
    let advance = |ch: char| {
        let units = f64::from(face.units_per_em());
        let advance = face
            .glyph_index(ch)
            .and_then(|glyph| face.glyph_hor_advance(glyph))
            .map(f64::from)
            .unwrap_or(units);
        // The installed Office font can differ. A safety margin plus native
        // normal autofit avoids clipping around metric and word-wrap edges.
        advance / units * f64::from(size) / 100.0 * 1.15
    };
    let mut lines = 0;
    for line in value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\t', "    ")
        .split('\n')
    {
        let chars = line.chars().collect::<Vec<_>>();
        if chars.is_empty() {
            lines += 1;
            continue;
        }
        let mut start = 0;
        while start < chars.len() {
            let mut end = start;
            let mut measured = 0.0;
            let mut boundary = None;
            while end < chars.len() {
                let next = measured + advance(chars[end]);
                if next > width && end > start {
                    break;
                }
                measured = next;
                if chars[end].is_whitespace() {
                    boundary = Some(end + 1);
                }
                end += 1;
            }
            if end < chars.len() {
                if let Some(value) = boundary {
                    end = value;
                }
            }
            lines += 1;
            start = end;
        }
    }
    lines.max(1)
}
fn drawing_table(
    id: usize,
    y: i64,
    headers: &[String],
    rows: &[Vec<CellValue>],
) -> Result<(String, i64), GenerationError> {
    // Fixed, visible table geometry. Fail explicitly if a single slide cannot
    // accommodate it; never silently discard cells or shrink unreadably.
    if headers.len() > 8 {
        return Err(error("Slide tables support at most 8 columns"));
    }
    let width = 11094720 / headers.len() as i64;
    let cell_width = width as f64 / 12700.0 - 12.0;
    let all = std::iter::once(
        headers
            .iter()
            .map(|x| CellValue::String(x.clone()))
            .collect::<Vec<_>>(),
    )
    .chain(rows.iter().cloned());
    let mut body = String::new();
    let mut height = 0;
    for (r, row) in all.enumerate() {
        let lines = row
            .iter()
            .map(|v| estimate_lines(&v.text(), 1400, cell_width))
            .max()
            .unwrap_or(1);
        let row_height = lines as i64 * 280000 + 110000;
        height += row_height;
        if height > 5_000_000 {
            return Err(error(
                "Slide table exceeds the slide content area; split it across slides",
            ));
        }
        body.push_str(&format!("<a:tr h=\"{row_height}\">"));
        for cell in row {
            let fill = if r == 0 { "E8EEF5" } else { "FFFFFF" };
            let borders=["lnL","lnR","lnT","lnB"].iter().map(|side|format!("<a:{side} w=\"6350\"><a:solidFill><a:srgbClr val=\"C7D0DA\"/></a:solidFill></a:{side}>")).collect::<String>();
            body.push_str(&format!("<a:tc><a:txBody><a:bodyPr lIns=\"70000\" tIns=\"35000\" rIns=\"70000\" bIns=\"35000\"/><a:lstStyle/>{}</a:txBody><a:tcPr marL=\"70000\" marR=\"70000\" marT=\"35000\" marB=\"35000\">{borders}<a:solidFill><a:srgbClr val=\"{fill}\"/></a:solidFill></a:tcPr></a:tc>",drawing_p(&cell.text(),1400,r==0,false)));
        }
        body.push_str("</a:tr>");
    }
    let grid = (0..headers.len())
        .map(|_| format!("<a:gridCol w=\"{width}\"/>"))
        .collect::<String>();
    Ok((format!("<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"{id}\" name=\"Table {id}\"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x=\"548640\" y=\"{y}\"/><a:ext cx=\"11094720\" cy=\"{height}\"/></p:xfrm><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/table\"><a:tbl><a:tblPr firstRow=\"1\" bandRow=\"0\"/><a:tblGrid>{grid}</a:tblGrid>{body}</a:tbl></a:graphicData></a:graphic></p:graphicFrame>"),height))
}

fn pptx(spec: &DocumentSpec, parts: &mut Parts, types: &mut Parts) -> Result<(), GenerationError> {
    let mut ids = String::new();
    let mut rels = vec![(
        "rId1".into(),
        format!("{OREL}/slideMaster"),
        "slideMasters/slideMaster1.xml".into(),
    )];
    for (index, slide) in spec.slides.iter().enumerate() {
        let n = index + 1;
        let rid = index + 2;
        if estimate_lines(&slide.title, 2800, 873.6) > 2 {
            return Err(error("Slide title exceeds two lines"));
        }
        let mut shapes = text_shape(
            2,
            "Title",
            320040,
            1016000,
            &drawing_p(&slide.title, 2800, true, false),
        );
        let mut y = 1_500_000;
        for (index, block) in slide.blocks.iter().enumerate() {
            let id = index + 3;
            let (shape, height) = match block {
                DocumentBlock::Table { headers, rows } => drawing_table(id, y, headers, rows)?,
                _ => {
                    let (paragraphs, lines, size) = match block {
                        DocumentBlock::Paragraph { text } => (
                            drawing_p(text, 1800, false, false),
                            estimate_lines(text, 1800, 873.6),
                            1800,
                        ),
                        DocumentBlock::Heading { text, level } => {
                            let size = 2600 - u32::from(*level) * 150;
                            (
                                drawing_p(text, size, true, false),
                                estimate_lines(text, size, 873.6),
                                size,
                            )
                        }
                        DocumentBlock::Bullets { items } => (
                            items
                                .iter()
                                .map(|x| drawing_p(x, 1800, false, true))
                                .collect::<String>(),
                            items.iter().map(|x| estimate_lines(x, 1800, 850.0)).sum(),
                            1800,
                        ),
                        _ => unreachable!(),
                    };
                    let height = lines as i64 * size as i64 * 127 * 140 / 100 + 90000;
                    (
                        text_shape(id, &format!("Content {id}"), y, height, &paragraphs),
                        height,
                    )
                }
            };
            if y + height > 6_500_000 {
                return Err(error(
                    "Slide content exceeds the slide area; split it across slides",
                ));
            }
            shapes.push_str(&shape);
            y += height + 100000;
        }
        part(parts,types,&format!("ppt/slides/slide{n}.xml"),"application/vnd.openxmlformats-officedocument.presentationml.slide+xml",format!("{XML}<p:sld xmlns:p=\"{P}\" xmlns:a=\"{A}\" xmlns:r=\"{OREL}\"><p:cSld name=\"{}\"><p:spTree>{GROUP}{shapes}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>",xml(&slide.title)));
        parts.push((
            format!("ppt/slides/_rels/slide{n}.xml.rels"),
            relationships(&[(
                "rId1".into(),
                format!("{OREL}/slideLayout"),
                "../slideLayouts/slideLayout1.xml".into(),
            )]),
        ));
        ids.push_str(&format!("<p:sldId id=\"{}\" r:id=\"rId{rid}\"/>", 255 + n));
        rels.push((
            format!("rId{rid}"),
            format!("{OREL}/slide"),
            format!("slides/slide{n}.xml"),
        ));
    }
    part(parts,types,"ppt/presentation.xml","application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml",format!("{XML}<p:presentation xmlns:p=\"{P}\" xmlns:a=\"{A}\" xmlns:r=\"{OREL}\"><p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId1\"/></p:sldMasterIdLst><p:sldIdLst>{ids}</p:sldIdLst><p:sldSz cx=\"12192000\" cy=\"6858000\" type=\"screen16x9\"/><p:notesSz cx=\"6858000\" cy=\"9144000\"/><p:defaultTextStyle/></p:presentation>"));
    parts.push((
        "ppt/_rels/presentation.xml.rels".into(),
        relationships(&rels),
    ));
    part(parts,types,"ppt/slideMasters/slideMaster1.xml","application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml",format!("{XML}<p:sldMaster xmlns:p=\"{P}\" xmlns:a=\"{A}\" xmlns:r=\"{OREL}\"><p:cSld><p:bg><p:bgPr><a:solidFill><a:srgbClr val=\"FFFFFF\"/></a:solidFill><a:effectLst/></p:bgPr></p:bg><p:spTree>{GROUP}</p:spTree></p:cSld>{COLORS}<p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rId1\"/></p:sldLayoutIdLst><p:txStyles><p:titleStyle/><p:bodyStyle/><p:otherStyle/></p:txStyles></p:sldMaster>"));
    parts.push((
        "ppt/slideMasters/_rels/slideMaster1.xml.rels".into(),
        relationships(&[
            (
                "rId1".into(),
                format!("{OREL}/slideLayout"),
                "../slideLayouts/slideLayout1.xml".into(),
            ),
            (
                "rId2".into(),
                format!("{OREL}/theme"),
                "../theme/theme1.xml".into(),
            ),
        ]),
    ));
    part(parts,types,"ppt/slideLayouts/slideLayout1.xml","application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml",format!("{XML}<p:sldLayout xmlns:p=\"{P}\" xmlns:a=\"{A}\" xmlns:r=\"{OREL}\" type=\"blank\" preserve=\"1\"><p:cSld name=\"Blank\"><p:spTree>{GROUP}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"));
    parts.push((
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels".into(),
        relationships(&[(
            "rId1".into(),
            format!("{OREL}/slideMaster"),
            "../slideMasters/slideMaster1.xml".into(),
        )]),
    ));
    part(
        parts,
        types,
        "ppt/theme/theme1.xml",
        "application/vnd.openxmlformats-officedocument.theme+xml",
        theme(),
    );
    Ok(())
}

fn theme() -> String {
    let colors = [
        ("dk1", "1F2937"),
        ("lt1", "FFFFFF"),
        ("dk2", "374151"),
        ("lt2", "F3F4F6"),
        ("accent1", "2563EB"),
        ("accent2", "10B981"),
        ("accent3", "F59E0B"),
        ("accent4", "8B5CF6"),
        ("accent5", "EC4899"),
        ("accent6", "06B6D4"),
        ("hlink", "2563EB"),
        ("folHlink", "7C3AED"),
    ]
    .iter()
    .map(|(name, value)| format!("<a:{name}><a:srgbClr val=\"{value}\"/></a:{name}>"))
    .collect::<String>();
    let font = "<a:latin typeface=\"Arial\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/>";
    let fill = "<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>";
    let line=format!("<a:ln w=\"12700\" cap=\"flat\" cmpd=\"sng\" algn=\"ctr\">{fill}<a:prstDash val=\"solid\"/><a:miter lim=\"800000\"/></a:ln>");
    format!("{XML}<a:theme xmlns:a=\"{A}\" name=\"AnimaOS\"><a:themeElements><a:clrScheme name=\"AnimaOS\">{colors}</a:clrScheme><a:fontScheme name=\"Arial\"><a:majorFont>{font}</a:majorFont><a:minorFont>{font}</a:minorFont></a:fontScheme><a:fmtScheme name=\"AnimaOS\"><a:fillStyleLst>{fill}{fill}{fill}</a:fillStyleLst><a:lnStyleLst>{line}{line}{line}</a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst>{fill}{fill}{fill}</a:bgFillStyleLst></a:fmtScheme></a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>")
}
