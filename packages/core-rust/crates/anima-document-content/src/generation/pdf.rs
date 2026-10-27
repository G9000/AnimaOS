use super::{error, CellValue, DocumentBlock, DocumentSpec, GenerationError, Output};
use lopdf::{dictionary, Document, Object, Stream, StringFormat};
use std::collections::{BTreeMap, BTreeSet};
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

const FONT: &[u8] = include_bytes!("../../assets/DejaVuSans.ttf");
const LEFT: f64 = 54.0;
const TOP: f64 = 738.0;
const BOTTOM: f64 = 54.0;
const WIDTH: f64 = 504.0;

struct Font<'a> {
    face: ttf_parser::Face<'a>,
    codes: BTreeMap<char, u16>,
}
impl Font<'_> {
    fn advance(&self, ch: char, size: f64) -> f64 {
        let glyph = self
            .face
            .glyph_index(ch)
            .expect("coverage checked before layout");
        f64::from(self.face.glyph_hor_advance(glyph).unwrap_or(0)) * size
            / f64::from(self.face.units_per_em())
    }
    fn encoded(&self, value: &str) -> String {
        value
            .chars()
            .map(|ch| format!("{:04X}", self.codes[&ch]))
            .collect()
    }
    fn lines(&self, value: &str, size: f64, width: f64) -> Result<Vec<String>, GenerationError> {
        let normalized = normalize(value);
        let mut lines = Vec::new();
        for paragraph in normalized.split('\n') {
            let chars = paragraph.chars().collect::<Vec<_>>();
            if chars.is_empty() {
                lines.push(String::new());
                continue;
            }
            let mut start = 0;
            while start < chars.len() {
                let mut end = start;
                let mut measured = 0.0;
                let mut whitespace = None;
                while end < chars.len() {
                    let next = measured + self.advance(chars[end], size);
                    if next > width && end > start {
                        break;
                    }
                    if next > width {
                        return Err(error("PDF column is narrower than a glyph"));
                    }
                    measured = next;
                    if chars[end].is_whitespace() {
                        whitespace = Some(end + 1);
                    }
                    end += 1;
                }
                if end < chars.len() {
                    if let Some(boundary) = whitespace {
                        end = boundary;
                    }
                }
                lines.push(chars[start..end].iter().collect());
                start = end;
                if lines.len() > 20_000 {
                    return Err(error("PDF exceeds its text layout limit"));
                }
            }
        }
        Ok(lines)
    }
}
fn normalize(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\t', "    ")
        .nfc()
        .collect()
}

// Version 1 deliberately supports simple left-to-right Latin, Greek, Cyrillic
// and symbol blocks. It does not shape glyphs, reorder bidi text, attach
// combining marks, or assemble emoji sequences. NFC composes supported accents;
// remaining marks/joiners and every other script fail explicitly. Office files
// retain full Unicode text and delegate typography to the native reader.
fn supported_character(ch: char) -> bool {
    if is_combining_mark(ch) {
        return false;
    }
    matches!(ch,
        '\u{20}'..='\u{2ff}' | '\u{370}'..='\u{52f}' |
        '\u{1e00}'..='\u{1fff}' | '\u{2000}'..='\u{200a}' |
        '\u{2010}'..='\u{2027}' | '\u{202f}'..='\u{205f}' |
        '\u{2070}'..='\u{20cf}' | '\u{2100}'..='\u{2bff}' |
        '\u{2c60}'..='\u{2c7f}' | '\u{a640}'..='\u{a69f}' |
        '\u{a720}'..='\u{a7ff}' | '\u{ab30}'..='\u{ab6f}' |
        '\u{fb00}'..='\u{fb06}' | '\u{1d400}'..='\u{1d7ff}' |
        '\u{1f300}'..='\u{1faff}')
}

struct Layout<'a> {
    font: Font<'a>,
    pages: Vec<String>,
    y: f64,
}
impl Layout<'_> {
    fn page(&mut self) -> Result<(), GenerationError> {
        if self.pages.len() >= 200 {
            return Err(error("PDF exceeds the 200 page limit"));
        }
        self.pages.push(String::new());
        self.y = TOP;
        Ok(())
    }
    fn command(&mut self, command: &str) {
        self.pages
            .last_mut()
            .expect("initial page")
            .push_str(command);
    }
    fn line(&mut self, value: &str, x: f64, y: f64, size: f64) {
        self.command(&format!(
            "BT /F1 {size:.3} Tf 0.12 0.16 0.22 rg 1 0 0 1 {x:.3} {y:.3} Tm <{}> Tj ET\n",
            self.font.encoded(value)
        ));
    }
    fn paragraph(&mut self, value: &str, size: f64, indent: f64) -> Result<(), GenerationError> {
        let lines = self.font.lines(value, size, WIDTH - indent)?;
        let leading = size * 1.4;
        for line in lines {
            if self.y - leading < BOTTOM {
                self.page()?;
            }
            self.line(&line, LEFT + indent, self.y - size, size);
            self.y -= leading;
        }
        self.y -= size * 0.5;
        Ok(())
    }
    fn row_lines(
        &self,
        row: &[String],
        columns: usize,
    ) -> Result<(Vec<Vec<String>>, f64), GenerationError> {
        let width = WIDTH / columns as f64 - 12.0;
        if width <= 0.0 {
            return Err(error("PDF table columns are too narrow"));
        }
        let lines = row
            .iter()
            .map(|value| self.font.lines(value, 10.0, width))
            .collect::<Result<Vec<_>, _>>()?;
        let height = lines.iter().map(Vec::len).max().unwrap_or(1) as f64 * 14.0 + 12.0;
        if height > TOP - BOTTOM {
            return Err(error("PDF table row exceeds a page; shorten the cell text"));
        }
        Ok((lines, height))
    }
    fn draw_row(&mut self, lines: &[Vec<String>], height: f64, header: bool) {
        let width = WIDTH / lines.len() as f64;
        let bottom = self.y - height;
        if header {
            self.command(&format!(
                "q 0.91 0.94 0.97 rg {LEFT:.3} {bottom:.3} {WIDTH:.3} {height:.3} re f Q\n"
            ));
        }
        for (index, cell) in lines.iter().enumerate() {
            let x = LEFT + index as f64 * width;
            self.command(&format!(
                "q 0.75 0.79 0.84 RG 0.5 w {x:.3} {bottom:.3} {width:.3} {height:.3} re S Q\n"
            ));
            for (index, line) in cell.iter().enumerate() {
                self.line(line, x + 6.0, self.y - 16.0 - index as f64 * 14.0, 10.0);
            }
        }
        self.y -= height;
    }
    fn table(
        &mut self,
        headers: &[String],
        rows: &[Vec<CellValue>],
    ) -> Result<(), GenerationError> {
        let (head, height) = self.row_lines(headers, headers.len())?;
        if self.y - height < BOTTOM {
            self.page()?;
        }
        self.draw_row(&head, height, true);
        for row in rows {
            let values = row.iter().map(CellValue::text).collect::<Vec<_>>();
            let (lines, row_height) = self.row_lines(&values, headers.len())?;
            if height + row_height > TOP - BOTTOM {
                return Err(error(
                    "PDF table header and row exceed a page; shorten the cell text",
                ));
            }
            if self.y - row_height < BOTTOM {
                self.page()?;
                self.draw_row(&head, height, true);
            }
            self.draw_row(&lines, row_height, false);
        }
        self.y -= 12.0;
        Ok(())
    }
}

pub(super) fn render(spec: &DocumentSpec) -> Result<Vec<u8>, GenerationError> {
    let face =
        ttf_parser::Face::parse(FONT, 0).map_err(|_| error("Bundled PDF font is invalid"))?;
    let mut used = BTreeSet::new();
    let mut add = |value: &str| {
        for ch in normalize(value).chars().filter(|ch| *ch != '\n') {
            used.insert(ch);
        }
    };
    add(&spec.title);
    for block in &spec.blocks {
        match block {
            DocumentBlock::Paragraph { text } | DocumentBlock::Heading { text, .. } => add(text),
            DocumentBlock::Bullets { items } => {
                for item in items {
                    add(&format!("• {item}"));
                }
            }
            DocumentBlock::Table { headers, rows } => {
                for header in headers {
                    add(header);
                }
                for row in rows {
                    for cell in row {
                        add(&cell.text());
                    }
                }
            }
        }
    }
    let mut codes = BTreeMap::new();
    for (index, ch) in used.into_iter().enumerate() {
        if !supported_character(ch) {
            return Err(error(format!("PDF v1 cannot render script/glyph U+{:04X} without complex shaping or bidirectional layout; use left-to-right Latin, Greek, Cyrillic and simple symbols",ch as u32)));
        }
        if face.glyph_index(ch).is_none() {
            return Err(error(format!(
                "PDF font has no glyph for U+{:04X}",
                ch as u32
            )));
        }
        let code = u16::try_from(index + 1).map_err(|_| error("PDF exceeds its glyph limit"))?;
        codes.insert(ch, code);
    }
    let mut layout = Layout {
        font: Font { face, codes },
        pages: Vec::new(),
        y: TOP,
    };
    layout.page()?;
    layout.paragraph(&spec.title, 22.0, 0.0)?;
    for block in &spec.blocks {
        match block {
            DocumentBlock::Paragraph { text } => layout.paragraph(text, 11.0, 0.0)?,
            DocumentBlock::Heading { text, level } => {
                if layout.y - 60.0 < BOTTOM {
                    layout.page()?;
                }
                layout.y -= 7.0;
                layout.paragraph(text, 19.0 - f64::from(*level), 0.0)?;
            }
            DocumentBlock::Bullets { items } => {
                for item in items {
                    layout.paragraph(&format!("• {item}"), 11.0, 10.0)?;
                }
            }
            DocumentBlock::Table { headers, rows } => layout.table(headers, rows)?,
        }
    }
    serialize(spec, &layout)
}

fn serialize(spec: &DocumentSpec, layout: &Layout<'_>) -> Result<Vec<u8>, GenerationError> {
    let mut doc = Document::with_version("1.4");
    let pages_id = doc.new_object_id();
    let units = f64::from(layout.font.face.units_per_em());
    let scale = |value: i16| Object::Integer((f64::from(value) * 1000.0 / units).round() as i64);
    let bbox = layout.font.face.global_bounding_box();
    let font_file = doc.add_object(Stream::new(
        dictionary! {"Length1"=>FONT.len() as i64},
        FONT.to_vec(),
    ));
    let descriptor=doc.add_object(dictionary! {
        "Type"=>"FontDescriptor","FontName"=>"DejaVuSans","Flags"=>32,
        "FontBBox"=>vec![scale(bbox.x_min),scale(bbox.y_min),scale(bbox.x_max),scale(bbox.y_max)],
        "ItalicAngle"=>0,"Ascent"=>scale(layout.font.face.ascender()),"Descent"=>scale(layout.font.face.descender()),
        "CapHeight"=>scale(layout.font.face.capital_height().unwrap_or(layout.font.face.ascender())),"StemV"=>80,"FontFile2"=>font_file,
    });
    let mut mapping = vec![0; 2 * (layout.font.codes.len() + 1)];
    let mut widths = Vec::new();
    let mut unicode = Vec::new();
    for (&ch, &code) in &layout.font.codes {
        let glyph = layout.font.face.glyph_index(ch).expect("coverage checked");
        mapping[usize::from(code) * 2..usize::from(code) * 2 + 2]
            .copy_from_slice(&glyph.0.to_be_bytes());
        widths.push(Object::Integer(
            (f64::from(layout.font.face.glyph_hor_advance(glyph).unwrap_or(0)) * 1000.0 / units)
                .round() as i64,
        ));
        let utf16 = ch
            .encode_utf16(&mut [0; 2])
            .iter()
            .map(|unit| format!("{unit:04X}"))
            .collect::<String>();
        unicode.push(format!("<{code:04X}> <{utf16}>\n"));
    }
    let cid_map = doc.add_object(Stream::new(dictionary! {}, mapping));
    let descendant=doc.add_object(dictionary! {
        "Type"=>"Font","Subtype"=>"CIDFontType2","BaseFont"=>"DejaVuSans",
        "CIDSystemInfo"=>dictionary! {"Registry"=>Object::string_literal("Adobe"),"Ordering"=>Object::string_literal("Identity"),"Supplement"=>0},
        "FontDescriptor"=>descriptor,"DW"=>1000,"W"=>vec![Object::Integer(1),Object::Array(widths)],"CIDToGIDMap"=>cid_map,
    });
    let mut cmap=String::from("/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /AnimaUnicode def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n");
    for group in unicode.chunks(100) {
        cmap.push_str(&format!("{} beginbfchar\n", group.len()));
        for entry in group {
            cmap.push_str(entry);
        }
        cmap.push_str("endbfchar\n");
    }
    cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    let unicode_map = doc.add_object(Stream::new(dictionary! {}, cmap.into_bytes()));
    let font=doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type0","BaseFont"=>"DejaVuSans","Encoding"=>"Identity-H","DescendantFonts"=>vec![Object::Reference(descendant)],"ToUnicode"=>unicode_map});
    let resources = doc.add_object(dictionary! {"Font"=>dictionary! {"F1"=>font}});
    let mut kids = Vec::new();
    for page in &layout.pages {
        let content = doc.add_object(Stream::new(dictionary! {}, page.as_bytes().to_vec()));
        let id=doc.add_object(dictionary! {"Type"=>"Page","Parent"=>pages_id,"MediaBox"=>vec![0.into(),0.into(),612.into(),792.into()],"Resources"=>resources,"Contents"=>content});
        kids.push(Object::Reference(id));
    }
    doc.objects.insert(
        pages_id,
        dictionary! {"Type"=>"Pages","Count"=>kids.len() as i64,"Kids"=>kids}.into(),
    );
    let catalog = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages_id});
    let mut title = vec![0xfe, 0xff];
    for unit in spec.title.encode_utf16() {
        title.extend_from_slice(&unit.to_be_bytes());
    }
    let info=doc.add_object(dictionary! {"Title"=>Object::String(title,StringFormat::Hexadecimal),"Producer"=>Object::string_literal("AnimaOS document renderer v1")});
    doc.trailer.set("Root", catalog);
    doc.trailer.set("Info", info);
    let mut output = Output::new();
    doc.save_to(&mut output)?;
    Ok(output.finish())
}
