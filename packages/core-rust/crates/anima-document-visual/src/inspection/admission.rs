//! Narrower actual-engine admission layered on the existing inert package gate.
use std::{collections::BTreeMap, io::Read};

use anima_document_content::{
    check_office_document_text, inspect_office_document, OfficeEditError, OfficeFormat,
    OfficeTargetValue,
};
use quick_xml::{events::Event, name::ResolveResult, NsReader, XmlVersion};
use rawzip::{CompressionMethod, ZipArchive};

use super::{
    cell_key, hash, serialized_digest, validate_inventory, FormulaIdentity, FormulaInventory,
    InspectionError, OfficeAdmission, MAX_OFFICE_INPUT_BYTES,
};

const S: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const MAX_EXPANDED_BYTES: usize = 16 * 1024 * 1024;
const MAX_XML_EVENTS: usize = 250_000;
const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const C: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
const CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const XDR: &str = "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing";
const PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
const CORE: &str = "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";
const EXT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties";
const CUSTOM: &str = "http://schemas.openxmlformats.org/officeDocument/2006/custom-properties";
const VT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes";
const DC: &str = "http://purl.org/dc/elements/1.1/";
const DCTERMS: &str = "http://purl.org/dc/terms/";

/// Admits actual imported Office bytes before any external engine sees them.
///
/// Reuses the complete existing package and text-policy admission. Inspection
/// additionally rejects all external relationships, even inert hyperlinks, and
/// complex/implicit spreadsheet formulas whose complete identity cannot be
/// mapped to one original cell. No generated-document metadata is trusted.
/// The host must run this parser under its finite worker custody.
///
/// # Errors
/// Rejects unsupported, active, external, malformed or over-budget packages and
/// formulas outside the existing closed formula-function admission set.
pub fn admit_office(
    format: OfficeFormat,
    source: &[u8],
) -> Result<OfficeAdmission, InspectionError> {
    if format == OfficeFormat::Html {
        return Err(InspectionError::Unsupported);
    }
    if source.is_empty() || source.len() > MAX_OFFICE_INPUT_BYTES {
        return Err(InspectionError::ResourceLimit);
    }
    let inventory = inspect_office_document(format, source).map_err(admission_error)?;
    if !inventory.retained_hyperlinks.is_empty() {
        return Err(InspectionError::Unsupported);
    }
    // This visitor admits all logical text rather than treating generation
    // metadata or a truncated extraction as proof of the original package.
    if !check_office_document_text(format, source, |_| true).map_err(admission_error)? {
        return Err(InspectionError::InvalidSource);
    }
    let parts = admitted_xml_parts(source)?;
    let mut main = None;
    for (name, body) in &parts {
        visit_elements(body, |_, _, _| Ok(()))?;
        if name.ends_with(".rels") {
            visit_elements(body, |namespace, local, attrs| {
                if namespace == REL && local == "Relationship" {
                    if attribute(attrs, "", "TargetMode") == Some("External") {
                        return Err(InspectionError::Unsupported);
                    }
                    if name == "_rels/.rels"
                        && attribute(attrs, "", "Type")
                            == Some(format!("{R}/officeDocument").as_str())
                    {
                        let target =
                            attribute(attrs, "", "Target").ok_or(InspectionError::InvalidSource)?;
                        if main.replace(resolve_root_target(target)?).is_some() {
                            return Err(InspectionError::InvalidSource);
                        }
                    }
                }
                Ok(())
            })?;
        }
        if format == OfficeFormat::Xlsx {
            visit_elements(body, |namespace, local, attrs| {
                if namespace == S {
                    match local {
                        // Shared/array/spill formulas and implicit table formulas
                        // can change cell coverage when loaded by an engine.
                        "f" if attribute(attrs, "", "t").is_some_and(|kind| kind != "normal") => {
                            return Err(InspectionError::Unsupported);
                        }
                        "calculatedColumnFormula" | "totalsRowFormula" => {
                            return Err(InspectionError::Unsupported);
                        }
                        _ => {}
                    }
                }
                Ok(())
            })?;
        }
    }
    let mut formulas = Vec::new();
    if format == OfficeFormat::Xlsx {
        let main = main.ok_or(InspectionError::InvalidSource)?;
        let body = parts.get(&main).ok_or(InspectionError::InvalidSource)?;
        let mut sheet_ordinals = BTreeMap::new();
        let mut ordinal = 0u32;
        visit_elements(body, |namespace, local, attrs| {
            if namespace == S && local == "sheet" {
                let id = attribute(attrs, R, "id").ok_or(InspectionError::InvalidSource)?;
                if ordinal >= 32
                    || sheet_ordinals
                        .insert(format!("sheet:{id}"), ordinal)
                        .is_some()
                {
                    return Err(InspectionError::InvalidSource);
                }
                ordinal += 1;
            }
            Ok(())
        })?;
        if ordinal == 0 {
            return Err(InspectionError::InvalidSource);
        }
        for target in inventory.targets {
            if let OfficeTargetValue::Cell {
                sheet_id,
                address,
                formula: Some(formula),
                ..
            } = target.value
            {
                if formula.is_empty() {
                    return Err(InspectionError::Unsupported);
                }
                let sheet_ordinal = *sheet_ordinals
                    .get(&sheet_id)
                    .ok_or(InspectionError::InvalidSource)?;
                formulas.push(FormulaIdentity {
                    sheet_id,
                    sheet_ordinal,
                    address,
                    formula_sha256: hash(formula.as_bytes()),
                });
            }
        }
    }
    formulas.sort_by(|a, b| cell_key(a).cmp(&cell_key(b)));
    let inventory = FormulaInventory {
        sha256: serialized_digest(&formulas)?,
        formulas,
    };
    validate_inventory(&inventory)?;
    Ok(OfficeAdmission {
        format,
        source_sha256: hash(source),
        source_byte_length: source.len() as u64,
        inventory,
    })
}

fn admission_error(error: OfficeEditError) -> InspectionError {
    match error {
        OfficeEditError::UnsupportedFeature(_) => InspectionError::Unsupported,
        OfficeEditError::ResourceLimit => InspectionError::ResourceLimit,
        _ => InspectionError::InvalidSource,
    }
}

// Only after complete package admission do we revisit the original ZIP for its
// workbook order. The second reader independently enforces expansion/checksum
// bounds and never extracts a path or opens an external target.
fn admitted_xml_parts(source: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, InspectionError> {
    let archive = ZipArchive::from_slice(source).map_err(|_| InspectionError::InvalidSource)?;
    let mut entries = archive.entries();
    let mut parts = BTreeMap::new();
    let mut total = 0usize;
    while let Some(entry) = entries
        .next_entry()
        .map_err(|_| InspectionError::InvalidSource)?
    {
        let name = std::str::from_utf8(entry.file_path().as_bytes())
            .map_err(|_| InspectionError::InvalidSource)?;
        if !name.ends_with(".xml") && !name.ends_with(".rels") {
            continue;
        }
        if parts.len() >= 1024 {
            return Err(InspectionError::ResourceLimit);
        }
        let local = archive
            .get_entry(entry.wayfinder())
            .map_err(|_| InspectionError::InvalidSource)?;
        let data = local.data();
        let remaining = MAX_EXPANDED_BYTES
            .checked_sub(total)
            .ok_or(InspectionError::ResourceLimit)?;
        let cap = remaining
            .min(MAX_OFFICE_INPUT_BYTES)
            .min(data.len().saturating_mul(100));
        let bytes = match entry.compression_method() {
            CompressionMethod::STORE => read_bounded(local.verifying_reader(data), cap)?,
            CompressionMethod::DEFLATE => {
                let mut decoder = flate2::bufread::DeflateDecoder::new(data);
                let bytes = read_bounded(local.verifying_reader(&mut decoder), cap)?;
                if decoder.total_in() != data.len() as u64 {
                    return Err(InspectionError::InvalidSource);
                }
                bytes
            }
            _ => return Err(InspectionError::Unsupported),
        };
        total = total
            .checked_add(bytes.len())
            .ok_or(InspectionError::ResourceLimit)?;
        if parts.insert(name.to_owned(), bytes).is_some() {
            return Err(InspectionError::InvalidSource);
        }
    }
    Ok(parts)
}

fn read_bounded(mut reader: impl Read, cap: usize) -> Result<Vec<u8>, InspectionError> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| InspectionError::InvalidSource)?;
        if read == 0 {
            return Ok(bytes);
        }
        if bytes.len().saturating_add(read) > cap {
            return Err(InspectionError::ResourceLimit);
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
}

struct Attribute {
    namespace: String,
    local: String,
    value: String,
}

fn attribute<'a>(attrs: &'a [Attribute], namespace: &str, local: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|a| a.namespace == namespace && a.local == local)
        .map(|a| a.value.as_str())
}

fn namespace(value: ResolveResult<'_>) -> Result<String, InspectionError> {
    match value {
        ResolveResult::Unbound => Ok(String::new()),
        ResolveResult::Bound(value) => std::str::from_utf8(value.as_ref())
            .map(str::to_owned)
            .map_err(|_| InspectionError::InvalidSource),
        ResolveResult::Unknown(_) => Err(InspectionError::InvalidSource),
    }
}

fn visit_elements(
    bytes: &[u8],
    mut visit: impl FnMut(&str, &str, &[Attribute]) -> Result<(), InspectionError>,
) -> Result<(), InspectionError> {
    let mut reader = NsReader::from_reader(bytes);
    for _ in 0..MAX_XML_EVENTS {
        match reader
            .read_event()
            .map_err(|_| InspectionError::InvalidSource)?
        {
            Event::Start(tag) | Event::Empty(tag) => {
                let (ns, local) = reader.resolver().resolve_element(tag.name());
                let ns = namespace(ns)?;
                let local = std::str::from_utf8(local.as_ref())
                    .map_err(|_| InspectionError::InvalidSource)?
                    .to_owned();
                let mut attrs = Vec::new();
                for attr in tag.attributes() {
                    let attr = attr.map_err(|_| InspectionError::InvalidSource)?;
                    if attr.key.as_ref() == b"xmlns" || attr.key.as_ref().starts_with(b"xmlns:") {
                        continue;
                    }
                    let (ns, local) = reader.resolver().resolve_attribute(attr.key);
                    let ns = namespace(ns)?;
                    let local = std::str::from_utf8(local.as_ref())
                        .map_err(|_| InspectionError::InvalidSource)?
                        .to_owned();
                    let value = attr
                        .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                        .map_err(|_| InspectionError::InvalidSource)?
                        .into_owned();
                    attrs.push(Attribute {
                        namespace: ns,
                        local,
                        value,
                    });
                }
                if !known_element(&ns, &local)
                    || attrs.iter().any(|attr| !known_attribute(&ns, &local, attr))
                {
                    return Err(InspectionError::Unsupported);
                }
                visit(&ns, &local, &attrs)?;
            }
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
    Err(InspectionError::ResourceLimit)
}

fn resolve_root_target(target: &str) -> Result<String, InspectionError> {
    let mut parts = Vec::new();
    for part in target.split('/') {
        match part {
            "." => {}
            ".." => {
                parts.pop().ok_or(InspectionError::InvalidSource)?;
            }
            "" => return Err(InspectionError::InvalidSource),
            _ => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

// This is a deliberately closed rendering subset, not an OOXML validator.
// Unknown constructs in otherwise known namespaces must not reach an engine
// merely because the structural editor can safely preserve their source bytes.
fn contains(names: &str, name: &str) -> bool {
    names.split_ascii_whitespace().any(|known| known == name)
}

fn known_element(namespace: &str, local: &str) -> bool {
    let names = match namespace {
        CT => "Types Default Override",
        REL => "Relationships Relationship",
        W => "document body p pPr r rPr t br tab cr b bCs i iCs u color sz szCs rFonts lang highlight strike dstrike caps smallCaps vertAlign vanish rtl noProof spacing ind jc keepNext keepLines pageBreakBefore widowControl contextualSpacing outlineLvl pStyle rStyle numPr ilvl numId tabs sectPr pgSz pgMar cols docGrid titlePg headerReference footerReference tbl tblPr tblStyle tblW tblLayout tblBorders top left bottom right insideH insideV tblGrid gridCol tr trPr tblHeader trHeight cantSplit tc tcPr tcW gridSpan vMerge shd vAlign tcMar styles style name basedOn next link docDefaults rPrDefault pPrDefault numbering abstractNum multiLevelType lvl start numFmt lvlText lvlJc num abstractNumId settings zoom defaultTabStop displayBackgroundShape doNotTrackMoves doNotTrackFormatting evenAndOddHeaders compatibility compat fonts font family pitch charset panose1 sig fontTable hdr ftr footnotes footnote endnotes endnote comments comment commentRangeStart commentRangeEnd commentReference bookmarkStart bookmarkEnd proofErr drawing inline fldSimple instrText fldChar lastRenderedPageBreak",
        S => "workbook workbookPr bookViews workbookView sheets sheet calcPr definedNames definedName worksheet sheetPr pageSetUpPr outlinePr dimension sheetViews sheetView pane selection sheetFormatPr cols col sheetData row c f v is t r rPr b i u sz name color family charset scheme sst si autoFilter filterColumn filters filter customFilters customFilter sortState sortCondition mergeCells mergeCell hyperlinks hyperlink printOptions pageMargins pageSetup headerFooter oddHeader oddFooter evenHeader evenFooter firstHeader firstFooter rowBreaks colBreaks brk drawing legacyDrawing sheetProtection protectedRanges protectedRange styleSheet numFmts numFmt fonts font fills fill patternFill fgColor bgColor borders border left right top bottom diagonal start end cellStyleXfs cellXfs xf alignment protection cellStyles cellStyle dxfs dxf tableStyles tableStyle colors indexedColors rgbColor mruColors conditionalFormatting cfRule formula dataValidations dataValidation formula1 formula2 tableParts tablePart table autoFilter tableColumns tableColumn tableStyleInfo calcChain comments authors author commentList comment text phoneticPr rPh",
        P => "presentation sldMasterIdLst sldMasterId sldIdLst sldId sldSz notesSz defaultTextStyle sld cSld spTree nvGrpSpPr cNvPr cNvGrpSpPr nvPr grpSpPr sp nvSpPr cNvSpPr spPr txBody clrMapOvr clrMap graphicFrame nvGraphicFramePr cNvGraphicFramePr xfrm sldMaster bg bgPr sldLayoutIdLst sldLayoutId txStyles titleStyle bodyStyle otherStyle sldLayout notes notesMaster notesMasterIdLst notesMasterId hf handoutMaster handoutMasterIdLst handoutMasterId pic nvPicPr cNvPicPr blipFill grpSp custShowLst custShow sldLst presProps showPr printPr viewPr normalViewPr slideViewPr outlineViewPr notesTextViewPr sorterViewPr restoredLeft restoredTop cViewPr scale origin gridSpacing tableStyleLst",
        A => "theme themeElements clrScheme dk1 lt1 dk2 lt2 accent1 accent2 accent3 accent4 accent5 accent6 hlink folHlink fontScheme majorFont minorFont latin ea cs font fmtScheme fillStyleLst lnStyleLst effectStyleLst effectStyle bgFillStyleLst objectDefaults extraClrSchemeLst xfrm off ext chOff chExt prstGeom avLst gdLst gd noFill solidFill srgbClr schemeClr sysClr alpha tint shade lumMod lumOff satMod satOff ln prstDash miter round bevel headEnd tailEnd effectLst outerShdw innerShdw glow softEdge reflection bodyPr normAutofit noAutofit spAutoFit lstStyle p pPr r rPr t endParaRPr br fld lnSpc spcPct spcPts spcBef spcAft buChar buNone buFont buSzPct buSzPts buClr buClrTx defRPr lvl1pPr lvl2pPr lvl3pPr lvl4pPr lvl5pPr lvl6pPr lvl7pPr lvl8pPr lvl9pPr graphic graphicData tbl tblPr tblGrid gridCol tr tc txBody tcPr lnL lnR lnT lnB blipFill blip srcRect stretch fillRect masterClrMapping overrideClrMapping scene3d sp3d camera lightRig backdrop",
        C => "chartSpace date1904 lang roundedCorners style chart title tx rich layout manualLayout layoutTarget xMode yMode wMode hMode x y w h overlay autoTitleDeleted plotArea barChart bar3DChart lineChart line3DChart pieChart pie3DChart doughnutChart areaChart area3DChart scatterChart radarChart bubbleChart ofPieChart stockChart grouping barDir varyColors ser idx order tx cat val xVal yVal bubbleSize strRef numRef f strCache numCache ptCount pt v numLit strLit formatCode dPt spPr marker symbol size smooth invertIfNegative dLbls dLbl showLegendKey showVal showCatName showSerName showPercent showBubbleSize showLeaderLines leaderLines dLblPos gapWidth overlap holeSize firstSliceAng axId catAx valAx dateAx serAx scaling orientation min max delete axPos majorGridlines minorGridlines title numFmt majorTickMark minorTickMark tickLblPos crossAx crosses crossesAt auto lblAlgn lblOffset tickLblSkip tickMarkSkip noMultiLvlLbl crossBetween majorUnit minorUnit dispUnits builtInUnit legend legendPos legendEntry txPr printSettings headerFooter pageMargins pageSetup dispBlanksAs showDLblsOverMax plotVisOnly protection",
        WP => "inline anchor extent effectExtent docPr cNvGraphicFramePr positionH positionV posOffset align simplePos wrapNone wrapSquare wrapTight wrapThrough wrapTopAndBottom wrapPolygon start lineTo",
        XDR => "wsDr twoCellAnchor oneCellAnchor absoluteAnchor from to col colOff row rowOff pos ext graphicFrame nvGraphicFramePr cNvPr cNvGraphicFramePr xfrm clientData pic nvPicPr cNvPicPr blipFill spPr sp nvSpPr cNvSpPr txBody grpSp nvGrpSpPr cNvGrpSpPr grpSpPr",
        PIC => "pic nvPicPr cNvPr cNvPicPr blipFill spPr",
        CORE => "coreProperties category contentStatus keywords lastModifiedBy lastPrinted revision version",
        DC => "title subject creator description identifier language",
        DCTERMS => "created modified",
        EXT => "Properties Application AppVersion Company Manager Template TotalTime Pages Words Characters CharactersWithSpaces Lines Paragraphs Slides Notes HiddenSlides MMClips ScaleCrop HeadingPairs TitlesOfParts LinksUpToDate SharedDoc HyperlinksChanged DocSecurity",
        CUSTOM => "Properties property",
        VT => "lpstr lpwstr bstr i1 i2 i4 i8 int ui1 ui2 ui4 ui8 uint r4 r8 decimal bool date filetime vector variant empty null",
        _ => return false,
    };
    contains(names, local)
}

fn known_attribute(element_namespace: &str, element: &str, attr: &Attribute) -> bool {
    match attr.namespace.as_str() {
        "http://www.w3.org/XML/1998/namespace" => {
            return (attr.local == "space"
                && matches!(attr.value.as_str(), "preserve" | "default"))
                || attr.local == "lang";
        }
        "http://www.w3.org/2001/XMLSchema-instance" => {
            return element_namespace == DCTERMS
                && attr.local == "type"
                && attr.value == "dcterms:W3CDTF";
        }
        R => return contains("id embed link", &attr.local),
        W if element_namespace == W => {
            if attr.local == "instr" {
                return element == "fldSimple" && matches!(attr.value.trim(), "PAGE" | "NUMPAGES");
            }
            return contains("val type styleId default customStyle ascii hAnsi eastAsia cs hint before after line lineRule left right top bottom header footer gutter w h fill color sz space ilvl numId abstractNumId pos hanging firstLine afterAutospacing beforeAutospacing themeColor themeFill themeTint themeShade asciiTheme hAnsiTheme eastAsiaTheme cstheme bidi orient code author date id name clear tblStyleRowBandSize tblStyleColBandSize fldCharType dirty lock", &attr.local);
        }
        "" => {}
        _ => return false,
    }
    // The basic XLSX generator emits precisely these inert fill patterns.
    if element_namespace == S && element == "patternFill" && attr.local == "patternType" {
        return matches!(attr.value.as_str(), "none" | "gray125" | "solid");
    }
    let names = match element_namespace {
        CT => "Extension ContentType PartName",
        REL => "Id Type Target TargetMode",
        S => "name sheetId state r s t ref count uniqueCount calcId calcMode fullCalcOnLoad forceFullCalc calcOnSave iterate iterateCount iterateDelta workbookViewId showGridLines showRowColHeaders showZeros tabSelected topLeftCell activePane xSplit ySplit pane activeCell sqref defaultColWidth defaultRowHeight baseColWidth customHeight ht hidden min max width bestFit customWidth outlineLevel collapsed style thickTop thickBot spans dyDescent dyDescent automaticPageBreaks left right top bottom header footer horizontalDpi verticalDpi paperSize orientation scale fitToWidth fitToHeight useFirstPageNumber firstPageNumber copies blackAndWhite draft cellComments errors usePrinterDefaults relationshipId showFormulas rightToLeft view zoomScale zoomScaleNormal tabColor codePage date1904 filterMode summaryBelow summaryRight fitToPage autoPageBreaks horizontalCentered verticalCentered differentOddEven differentFirst alignWithMargins scaleWithDoc manualBreakCount id man pt val rgb indexed theme tint auto numFmtId formatCode fontId fillId borderId xfId applyNumberFormat applyFont applyFill applyBorder applyAlignment applyProtection pivotButton quotePrefix horizontal vertical textRotation wrapText shrinkToFit indent relativeIndent justifyLastLine readingOrder locked builtinId customBuiltin dxfId displayName totalsRowCount totalsRowShown headerRowCount tableType tableBorderDxfId headerRowBorderDxfId totalsRowBorderDxfId showFirstColumn showLastColumn showRowStripes showColumnStripes operator type priority stopIfTrue aboveAverage percent bottom equalAverage rank stdDev allowBlank showInputMessage showErrorMessage errorStyle imeMode promptTitle prompt errorTitle error errorCode localSheetId functionGroupId authorId shapeId phoneticId fontId i l a ca bx dtr dt2D si aca delete ascending descending filterVal and blank showButton secondButton textId differentialFormatId extLst",
        P => "id name descr title hidden show showMasterSp showMasterPhAnim type preserve userDrawn txBox cx cy x y rot flipH flipV bwMode firstSlideNum rtl embedTrueTypeFonts saveSubsetFonts autoCompressPictures bookmarkIdSeed bg1 tx1 bg2 tx2 accent1 accent2 accent3 accent4 accent5 accent6 hlink folHlink idx hasCustomPrompt sz orient sldNum dt hdr ftr action noChangeAspect noChangeArrowheads noChangeShapeType noEditPoints noGrp noMove noResize noRot noSelect noTextEdit chartSpace showComments loop showAnimation useTimings useCurrentPrinter scaleSlidesToFitPaper printWhat clrMode horizontalBarState verticalBarState preferSingleView varScale size autoAdjust",
        A => "id name descr title x y cx cy rot flipH flipV prst val lastClr w h cap cmpd algn lim type len headEnd tailEnd blurRad dist dir sx sy kx ky anchor anchorCtr wrap lIns rIns tIns bIns numCol spcCol rtlCol vert horzOverflow vertOverflow upright compatLnSpc forceAA fontScale lnSpcReduction lang altLang sz b i u strike kern baseline dirty err smtClean normalizeH kumimoji noProof spc typeface pitchFamily charset panose char marL marR indent lvl defTabSz rtl eaLnBrk latinLnBrk hangingPunct fontAlgn uri firstRow firstCol lastRow lastCol bandRow bandCol marT marB l r t b embed link cstate cropTop cropBottom cropLeft cropRight amt pos ang scaled growRot form fmla extrustionH extrusionH contourW prstMaterial z fov zoom rig lat lon rev",
        C => "val idx formatCode sourceLinked name id ref order type",
        WP => "distT distB distL distR simplePos relativeHeight behindDoc locked layoutInCell allowOverlap x y cx cy l t r b id name descr title hidden relativeFrom wrapText edited",
        XDR => "editAs x y cx cy id name descr title hidden fLocksWithSheet fPrintsWithSheet published",
        PIC => "id name descr title hidden",
        CORE | DC | DCTERMS | EXT => "",
        CUSTOM => "fmtid pid name",
        VT => "size baseType",
        _ => return false,
    };
    // graphicData's URI selects a known inert drawing vocabulary, not a loader.
    if element_namespace == A && element == "graphicData" && attr.local == "uri" {
        return matches!(
            attr.value.as_str(),
            "http://schemas.openxmlformats.org/drawingml/2006/table"
                | "http://schemas.openxmlformats.org/drawingml/2006/chart"
                | "http://schemas.openxmlformats.org/drawingml/2006/picture"
        );
    }
    contains(names, &attr.local)
}
