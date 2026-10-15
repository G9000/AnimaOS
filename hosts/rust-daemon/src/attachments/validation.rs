use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ValidatedUpload {
    pub(crate) name: String,
    pub(crate) mime: String,
    pub(crate) kind: AttachmentKind,
}

fn basename(filename: &str) -> &str {
    filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim_end_matches([' ', '.'])
}

pub(crate) fn sanitize_upload_name(filename: &str, extension: &str) -> String {
    let leaf = basename(filename);
    let stem = leaf.rsplit_once('.').map_or(leaf, |(stem, _)| stem);
    let mut stem: String = stem
        .chars()
        .filter(|c| {
            !c.is_control()
                && !crate::skills::is_hidden_in_one_line(*c)
                && (c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'))
        })
        .collect();
    stem = stem.trim_matches([' ', '.']).to_string();
    let device = stem.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        || ["COM", "LPT"].iter().any(|prefix| {
            device.strip_prefix(prefix).is_some_and(|n| {
                matches!(
                    n,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        });
    if stem.is_empty() || reserved {
        stem = "attachment".into();
    }
    // Leave space for the largest collision suffix as well as the extension.
    let max = MAX_UPLOAD_NAME_BYTES.saturating_sub(extension.len() + 1);
    while stem.len() > max {
        stem.pop();
    }
    stem = stem.trim_end_matches([' ', '.']).to_string();
    if stem.is_empty() {
        stem = "attachment".into();
    }
    format!("{stem}.{extension}")
}

pub(crate) fn validate_upload(
    filename: &str,
    bytes: &[u8],
) -> Result<ValidatedUpload, AttachmentError> {
    let leaf = basename(filename);
    let extension = leaf
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .ok_or(AttachmentError::Invalid)?;
    let (kind, mime) = match extension.as_str() {
        "png" => (AttachmentKind::Image, "image/png"),
        "jpg" | "jpeg" => (AttachmentKind::Image, "image/jpeg"),
        "gif" => (AttachmentKind::Image, "image/gif"),
        "webp" => (AttachmentKind::Image, "image/webp"),
        "wav" => (AttachmentKind::Audio, "audio/wav"),
        "txt" => (AttachmentKind::Text, "text/plain"),
        "md" | "markdown" => (AttachmentKind::Text, "text/markdown"),
        "csv" => (AttachmentKind::Text, "text/csv"),
        "json" => (AttachmentKind::Text, "application/json"),
        "yaml" | "yml" => (AttachmentKind::Text, "application/yaml"),
        "xml" => (AttachmentKind::Text, "application/xml"),
        "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "java" | "c" | "h" | "cpp" | "css"
        | "sql" | "toml" => (AttachmentKind::Text, "text/plain"),
        "pdf" => (AttachmentKind::Document, "application/pdf"),
        "doc" => (AttachmentKind::Document, "application/msword"),
        "xls" => (AttachmentKind::Document, "application/vnd.ms-excel"),
        "ppt" => (AttachmentKind::Document, "application/vnd.ms-powerpoint"),
        "docx" => (
            AttachmentKind::Document,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        ),
        "xlsx" => (
            AttachmentKind::Document,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        ),
        "pptx" => (
            AttachmentKind::Document,
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        ),
        _ => return Err(AttachmentError::Invalid),
    };
    if bytes.len() > kind.max_bytes() {
        return Err(AttachmentError::TooLarge);
    }
    let valid = match extension.as_str() {
        "wav" => {
            use anima_core::audio_content::{AudioValidationError, WavAudio};
            WavAudio::parse(bytes.to_vec()).map_err(|error| match error {
                AudioValidationError::Unsupported => AttachmentError::AudioUnsupported,
                AudioValidationError::TooLarge => AttachmentError::TooLarge,
                AudioValidationError::Malformed | AudioValidationError::TooLong => {
                    AttachmentError::AudioInvalid
                }
            })?;
            true
        }
        "png" => valid_png(bytes),
        "jpg" | "jpeg" => valid_jpeg(bytes),
        "gif" => {
            bytes.len() >= 15
                && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"))
                && le16(bytes, 6).is_some_and(|v| v > 0)
                && le16(bytes, 8).is_some_and(|v| v > 0)
                && bytes.last() == Some(&0x3b)
        }
        "webp" => valid_webp(bytes),
        "pdf" => {
            bytes.len() >= 16
                && bytes.starts_with(b"%PDF-")
                && bytes.get(5).is_some_and(u8::is_ascii_digit)
                && bytes.get(6) == Some(&b'.')
                && bytes.get(7).is_some_and(u8::is_ascii_digit)
                && bytes.get(8).is_some_and(u8::is_ascii_whitespace)
        }
        "doc" | "xls" | "ppt" => valid_cfb_envelope(bytes),
        "docx" => valid_office_zip(bytes, b"word/document.xml"),
        "xlsx" => valid_office_zip(bytes, b"xl/workbook.xml"),
        "pptx" => valid_office_zip(bytes, b"ppt/presentation.xml"),
        _ => std::str::from_utf8(bytes).is_ok_and(|text| {
            !text.is_empty()
                && !text
                    .chars()
                    .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        }),
    };
    if !valid {
        return Err(AttachmentError::Invalid);
    }
    Ok(ValidatedUpload {
        name: sanitize_upload_name(filename, &extension),
        mime: mime.into(),
        kind,
    })
}

// Legacy Office is saved, not parsed: validate only a plausible CFB envelope.
// The extension selects its public family MIME; these checks make no family claim.
fn valid_cfb_envelope(bytes: &[u8]) -> bool {
    if bytes.len() < 512
        || !bytes.starts_with(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1")
        || le16(bytes, 28) != Some(0xfffe)
        || le16(bytes, 32) != Some(6)
    {
        return false;
    }
    let sector = match (le16(bytes, 26), le16(bytes, 30)) {
        (Some(3), Some(9)) => 512usize,
        (Some(4), Some(12)) => 4096usize,
        _ => return false,
    };
    if bytes.len() % sector != 0
        || bytes.len() < sector * 3
        || (sector == 512 && le32(bytes, 40) != Some(0))
    {
        return false;
    }
    let count = bytes.len() / sector - 1;
    let Some(fats) = le32(bytes, 44)
        .map(|value| value as usize)
        .filter(|value| *value > 0 && *value < count)
    else {
        return false;
    };
    if le32(bytes, 56) != Some(4096) {
        return false;
    }
    let valid_sector = |id: u32| (id as usize) < count;
    let Some(directory) = le32(bytes, 48).filter(|id| valid_sector(*id)) else {
        return false;
    };
    let Some(first_fat) = le32(bytes, 76).filter(|id| valid_sector(*id) && *id != directory) else {
        return false;
    };
    for index in 0..fats.min(109) {
        if !le32(bytes, 76 + index * 4).is_some_and(valid_sector) {
            return false;
        }
    }
    if fats > 109
        && (!le32(bytes, 68).is_some_and(valid_sector)
            || !le32(bytes, 72).is_some_and(|value| value > 0 && (value as usize) < count))
    {
        return false;
    }
    let dir = (directory as usize + 1) * sector;
    let Some(name_len) = le16(bytes, dir + 64)
        .map(|value| value as usize)
        .filter(|value| (2..=64).contains(value) && *value % 2 == 0)
    else {
        return false;
    };
    if name_len != 22
        || bytes.get(dir + 66) != Some(&5)
        || le16(bytes, dir + name_len - 2) != Some(0)
        || bytes.get(dir..dir + 20) != Some(&b"R\0o\0o\0t\0 \0E\0n\0t\0r\0y\0"[..])
    {
        return false;
    }
    if !le32(bytes, dir + 116).is_some_and(|id| id == 0xfffffffe || valid_sector(id)) {
        return false;
    }
    let fat = (first_fat as usize + 1) * sector;
    (fat..fat + sector).step_by(4).all(|offset| {
        le32(bytes, offset).is_some_and(|id| {
            valid_sector(id) || matches!(id, 0xffffffff | 0xfffffffe | 0xfffffffd | 0xfffffffc)
        })
    })
}

fn le16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}
fn le32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}
fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

// Structural checks only, not decoding or CRC verification.
fn valid_png(bytes: &[u8]) -> bool {
    if bytes.len() < 57 || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return false;
    }
    let mut offset = 8;
    let mut image_data = false;
    let mut chunks = 0;
    while offset + 12 <= bytes.len() && chunks < 4096 {
        let Some(length) = be32(bytes, offset).map(|n| n as usize) else {
            return false;
        };
        let Some(end) = offset
            .checked_add(12)
            .and_then(|n| n.checked_add(length))
            .filter(|n| *n <= bytes.len())
        else {
            return false;
        };
        let kind = &bytes[offset + 4..offset + 8];
        if offset == 8
            && (kind != b"IHDR"
                || length != 13
                || be32(bytes, offset + 8) == Some(0)
                || be32(bytes, offset + 12) == Some(0))
        {
            return false;
        }
        if kind == b"IDAT" && length > 0 {
            image_data = true;
        }
        if kind == b"IEND" {
            return image_data && length == 0 && end == bytes.len();
        }
        offset = end;
        chunks += 1;
    }
    false
}
fn valid_jpeg(bytes: &[u8]) -> bool {
    if bytes.len() < 12 || !bytes.starts_with(&[0xff, 0xd8]) || !bytes.ends_with(&[0xff, 0xd9]) {
        return false;
    }
    let mut offset = 2;
    let mut segments = 0;
    while offset + 4 <= bytes.len() && segments < 4096 {
        if bytes[offset] != 0xff {
            return false;
        }
        while bytes.get(offset) == Some(&0xff) {
            offset += 1;
        }
        let Some(&marker) = bytes.get(offset) else {
            return false;
        };
        offset += 1;
        if marker == 0 || marker == 0xd8 || marker == 0xd9 {
            return false;
        }
        let Some(length) = bytes
            .get(offset..offset + 2)
            .map(|v| u16::from_be_bytes([v[0], v[1]]) as usize)
        else {
            return false;
        };
        if length < 2 || offset + length > bytes.len() - 2 {
            return false;
        }
        if marker == 0xda {
            return length >= 6 && offset + length < bytes.len() - 2;
        }
        offset += length;
        segments += 1;
    }
    false
}
fn valid_webp(bytes: &[u8]) -> bool {
    if bytes.len() < 22
        || !bytes.starts_with(b"RIFF")
        || bytes.get(8..12) != Some(b"WEBP")
        || le32(bytes, 4).map(|n| n as usize) != Some(bytes.len() - 8)
    {
        return false;
    }
    let mut offset = 12;
    let mut chunks = 0;
    let mut image = false;
    while offset + 8 <= bytes.len() && chunks < 4096 {
        let Some(length) = le32(bytes, offset + 4).map(|n| n as usize) else {
            return false;
        };
        let Some(end) = offset
            .checked_add(8)
            .and_then(|n| n.checked_add(length))
            .and_then(|n| n.checked_add(length % 2))
            .filter(|n| *n <= bytes.len())
        else {
            return false;
        };
        if matches!(&bytes[offset..offset + 4], b"VP8 " | b"VP8L" | b"VP8X") {
            if length == 0 {
                return false;
            }
            image = true;
        }
        offset = end;
        chunks += 1;
    }
    image && offset == bytes.len()
}

// Inspect bounded ZIP directory names and local headers, never decompress.
// This refuses arbitrary ZIPs carrying marker strings in their payload.
fn valid_office_zip(bytes: &[u8], required: &[u8]) -> bool {
    if bytes.len() < 22 || !bytes.starts_with(b"PK\x03\x04") {
        return false;
    }
    let start = bytes.len().saturating_sub(22 + 65535);
    let Some(end) = (start..=bytes.len() - 22).rev().find(|&i| {
        bytes.get(i..i + 4) == Some(b"PK\x05\x06")
            && le16(bytes, i + 20).is_some_and(|n| i + 22 + n as usize == bytes.len())
    }) else {
        return false;
    };
    let Some(count) = le16(bytes, end + 10) else {
        return false;
    };
    if count < 2
        || count > 4096
        || le16(bytes, end + 8) != Some(count)
        || le16(bytes, end + 4) != Some(0)
        || le16(bytes, end + 6) != Some(0)
    {
        return false;
    }
    let (Some(offset), Some(size)) = (le32(bytes, end + 16), le32(bytes, end + 12)) else {
        return false;
    };
    let mut cursor = offset as usize;
    if cursor.checked_add(size as usize) != Some(end) {
        return false;
    }
    let mut content_types = false;
    let mut document = false;
    for _ in 0..count {
        if bytes.get(cursor..cursor + 4) != Some(b"PK\x01\x02") {
            return false;
        }
        let (Some(name_len), Some(extra), Some(comment), Some(local)) = (
            le16(bytes, cursor + 28),
            le16(bytes, cursor + 30),
            le16(bytes, cursor + 32),
            le32(bytes, cursor + 42),
        ) else {
            return false;
        };
        let Some(name) = bytes.get(cursor + 46..cursor + 46 + name_len as usize) else {
            return false;
        };
        let local = local as usize;
        if local >= offset as usize
            || bytes.get(local..local + 4) != Some(b"PK\x03\x04")
            || le16(bytes, local + 26) != Some(name_len)
        {
            return false;
        }
        let Some(local_extra) = le16(bytes, local + 28) else {
            return false;
        };
        if bytes.get(local + 30..local + 30 + name_len as usize) != Some(name) {
            return false;
        }
        let Some(compressed) = le32(bytes, cursor + 20) else {
            return false;
        };
        if local
            .checked_add(30 + name_len as usize + local_extra as usize)
            .and_then(|n| n.checked_add(compressed as usize))
            .is_none_or(|n| n > offset as usize)
        {
            return false;
        }
        if name == b"[Content_Types].xml" || name == required {
            if compressed == 0
                || le16(bytes, cursor + 8).is_none_or(|flags| flags & 1 != 0)
                || !matches!(le16(bytes, cursor + 10), Some(0 | 8))
            {
                return false;
            }
            content_types |= name == b"[Content_Types].xml";
            document |= name == required;
        }
        cursor += 46 + name_len as usize + extra as usize + comment as usize;
        if cursor > end {
            return false;
        }
    }
    cursor == end && content_types && document
}

#[cfg(test)]
mod tests {
    use super::*;
    fn png() -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
        b.extend_from_slice(&13u32.to_be_bytes());
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&1u32.to_be_bytes());
        b.extend_from_slice(&1u32.to_be_bytes());
        b.extend_from_slice(&[8, 2, 0, 0, 0, 0, 0, 0, 0]);
        b.extend_from_slice(&1u32.to_be_bytes());
        b.extend_from_slice(b"IDAT");
        b.extend_from_slice(&[0, 0, 0, 0, 0]);
        b.extend_from_slice(&0u32.to_be_bytes());
        b.extend_from_slice(b"IEND");
        b.extend_from_slice(&[0; 4]);
        b
    }
    fn office_zip(marker: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut central = Vec::new();
        for name in ["[Content_Types].xml", marker] {
            let offset = bytes.len() as u32;
            let mut header = vec![0u8; 30];
            header[..4].copy_from_slice(b"PK\x03\x04");
            header[18..22].copy_from_slice(&1u32.to_le_bytes());
            header[22..26].copy_from_slice(&1u32.to_le_bytes());
            header[26..28].copy_from_slice(&(name.len() as u16).to_le_bytes());
            bytes.extend(header);
            bytes.extend(name.as_bytes());
            bytes.push(b'x');
            let mut header = vec![0u8; 46];
            header[..4].copy_from_slice(b"PK\x01\x02");
            header[20..24].copy_from_slice(&1u32.to_le_bytes());
            header[24..28].copy_from_slice(&1u32.to_le_bytes());
            header[28..30].copy_from_slice(&(name.len() as u16).to_le_bytes());
            header[42..46].copy_from_slice(&offset.to_le_bytes());
            central.extend(header);
            central.extend(name.as_bytes());
        }
        let offset = bytes.len() as u32;
        let size = central.len() as u32;
        bytes.extend(central);
        let mut end = vec![0u8; 22];
        end[..4].copy_from_slice(b"PK\x05\x06");
        end[8..10].copy_from_slice(&2u16.to_le_bytes());
        end[10..12].copy_from_slice(&2u16.to_le_bytes());
        end[12..16].copy_from_slice(&size.to_le_bytes());
        end[16..20].copy_from_slice(&offset.to_le_bytes());
        bytes.extend(end);
        bytes
    }
    #[test]
    fn attachment_magic_classification_rejects_mismatches_and_truncation() {
        let png = png();
        let jpeg = b"\xff\xd8\xff\xda\x00\x08\x01\x01\x00\x00\x3f\x00\x01\xff\xd9";
        let gif = b"GIF89a\x01\x00\x01\x00\x00\x00\x00\x2c\x3b";
        let webp = b"RIFF\x0e\x00\x00\x00WEBPVP8L\x01\x00\x00\x00\x2f\x00";
        for (name, bytes, mime) in [
            ("x.png", png.as_slice(), "image/png"),
            ("x.jpg", jpeg.as_slice(), "image/jpeg"),
            ("x.gif", gif.as_slice(), "image/gif"),
            ("x.webp", webp.as_slice(), "image/webp"),
        ] {
            let row = validate_upload(name, bytes).unwrap();
            assert_eq!(row.kind, AttachmentKind::Image);
            assert_eq!(row.mime, mime);
            assert!(validate_upload(name, &bytes[..8]).is_err());
            assert!(validate_upload(name, b"<svg>wrong</svg>").is_err());
        }
        assert!(validate_upload("wrong.jpg", &png).is_err());
        assert!(validate_upload("x.svg", b"<svg/>").is_err());
        assert!(validate_upload("x.html", b"<html/>").is_err());
        assert!(validate_upload("x.exe", b"MZ").is_err());
    }
    #[test]
    fn attachment_text_and_document_validation_is_explicit() {
        for ext in [
            "txt", "md", "csv", "json", "yaml", "yml", "xml", "rs", "ts", "tsx", "js", "py",
        ] {
            assert_eq!(
                validate_upload(&format!("file.{ext}"), b"hello\n")
                    .unwrap()
                    .kind,
                AttachmentKind::Text
            );
            assert!(validate_upload(&format!("file.{ext}"), &[0xff]).is_err());
        }
        assert_eq!(
            validate_upload("file.pdf", b"%PDF-1.7\n1 0 obj\nendobj\n%%EOF")
                .unwrap()
                .mime,
            "application/pdf"
        );
        assert!(validate_upload("file.pdf", b"%PDF-").is_err());
        let cfb = cfb_envelope();
        for ext in ["doc", "xls", "ppt"] {
            assert_eq!(
                validate_upload(&format!("file.{ext}"), &cfb).unwrap().kind,
                AttachmentKind::Document
            );
        }
        for (ext, marker) in [
            ("docx", "word/document.xml"),
            ("xlsx", "xl/workbook.xml"),
            ("pptx", "ppt/presentation.xml"),
        ] {
            let zip = office_zip(marker);
            assert_eq!(
                validate_upload(&format!("file.{ext}"), &zip).unwrap().kind,
                AttachmentKind::Document
            );
            assert!(validate_upload(&format!("file.{ext}"), b"PK\x03\x04arbitrary zip").is_err());
            let renamed = office_zip("arbitrary.txt");
            assert!(validate_upload(&format!("file.{ext}"), &renamed).is_err());
            let mut forged = b"PK\x03\x04[Content_Types].xml ".to_vec();
            forged.extend(marker.as_bytes());
            assert!(validate_upload(&format!("file.{ext}"), &forged).is_err());
            assert!(validate_upload(&format!("file.{ext}"), &zip[..zip.len() - 1]).is_err());
        }
        assert!(validate_upload("file.docm", &cfb).is_err());
        assert!(validate_upload("file.pdf", b"#!/bin/sh\nexit").is_err());
    }
    #[test]
    fn attachment_limits_use_classified_bytes() {
        let mut text = vec![b'a'; MAX_TEXT_BYTES];
        assert!(validate_upload("x.txt", &text).is_ok());
        text.push(b'a');
        assert_eq!(
            validate_upload("x.txt", &text).unwrap_err(),
            AttachmentError::TooLarge
        );
        let mut exact_image = png()[..33].to_vec();
        let data_len = MAX_IMAGE_BYTES - 33 - 12 - 12;
        exact_image.extend_from_slice(&(data_len as u32).to_be_bytes());
        exact_image.extend_from_slice(b"IDAT");
        exact_image.resize(33 + 8 + data_len + 4, 0);
        exact_image.extend_from_slice(&0u32.to_be_bytes());
        exact_image.extend_from_slice(b"IEND");
        exact_image.extend_from_slice(&[0; 4]);
        assert_eq!(exact_image.len(), MAX_IMAGE_BYTES);
        assert!(validate_upload("x.png", &exact_image).is_ok());
        let mut image = png();
        image.resize(MAX_IMAGE_BYTES + 1, 0);
        assert_eq!(
            validate_upload("x.png", &image).unwrap_err(),
            AttachmentError::TooLarge
        );
        let mut doc = b"%PDF-1.7\nobject data".to_vec();
        doc.resize(MAX_DOCUMENT_BYTES, b' ');
        assert!(validate_upload("x.pdf", &doc).is_ok());
        doc.push(b' ');
        assert_eq!(
            validate_upload("x.pdf", &doc).unwrap_err(),
            AttachmentError::TooLarge
        );
        assert!(MAX_UPLOAD_FORM_BYTES > MAX_DOCUMENT_BYTES + MAX_UPLOAD_HEADER_BYTES + 140);
    }
    #[test]
    fn attachment_names_are_bounded_windows_safe_basenames() {
        assert_eq!(
            sanitize_upload_name("C:\\fakepath\\notes.txt", "txt"),
            "notes.txt"
        );
        assert_eq!(sanitize_upload_name("../../notes.txt", "txt"), "notes.txt");
        for input in [
            "CON.txt",
            "con.extra.txt",
            "LPT9.txt",
            ".txt",
            "...txt",
            "\u{202e}\u{e0041}.txt",
        ] {
            assert_eq!(
                sanitize_upload_name(input, "txt"),
                "attachment.txt",
                "{input}"
            );
        }
        let name = sanitize_upload_name("C:odd:<name>\u{200b}.txt", "txt");
        assert!(!name.contains([':', '<', '>', '\u{200b}']));
        let name = sanitize_upload_name(&format!("{}.txt", "界".repeat(200)), "txt");
        assert!(name.len() <= MAX_UPLOAD_NAME_BYTES);
        assert!(name.ends_with(".txt"));
        assert!(!name.starts_with('.'));
        assert_eq!(sanitize_upload_name("notes.txt... ", "txt"), "notes.txt");
    }
    #[test]
    fn attachment_cfb_rejects_header_only_and_accepts_plausible_envelope() {
        let mut header = vec![0u8; 512];
        header[..8].copy_from_slice(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1");
        header[26..28].copy_from_slice(&3u16.to_le_bytes());
        header[28..30].copy_from_slice(&0xfffeu16.to_le_bytes());
        header[30..32].copy_from_slice(&9u16.to_le_bytes());
        header[32..34].copy_from_slice(&6u16.to_le_bytes());
        for extension in ["doc", "xls", "ppt"] {
            assert!(
                validate_upload(&format!("legacy.{extension}"), &header).is_err(),
                "header-only CFB stub"
            );
        }
        let plausible = cfb_envelope();
        for extension in ["doc", "xls", "ppt"] {
            assert_eq!(
                validate_upload(&format!("legacy.{extension}"), &plausible)
                    .unwrap()
                    .kind,
                AttachmentKind::Document
            );
        }
        let mut version4 = vec![0u8; 12288];
        version4[..512].copy_from_slice(&plausible[..512]);
        version4[26..28].copy_from_slice(&4u16.to_le_bytes());
        version4[30..32].copy_from_slice(&12u16.to_le_bytes());
        version4[40..44].copy_from_slice(&1u32.to_le_bytes());
        version4[4096..4608].copy_from_slice(&plausible[512..1024]);
        version4[8192..].fill(0xff);
        version4[8192..8704].copy_from_slice(&plausible[1024..1536]);
        assert!(validate_upload("version4.doc", &version4).is_ok());
        for truncated in [&plausible[..1024], &plausible[..plausible.len() - 1]] {
            assert!(validate_upload("legacy.doc", truncated).is_err());
        }
        let mut bad = plausible.clone();
        bad[48..52].copy_from_slice(&99u32.to_le_bytes());
        assert!(validate_upload("legacy.doc", &bad).is_err());
        let mut bad = plausible.clone();
        bad[512 + 66] = 0;
        assert!(validate_upload("legacy.doc", &bad).is_err());
    }
    fn cfb_envelope() -> Vec<u8> {
        let mut bytes = vec![0u8; 1536];
        bytes[..8].copy_from_slice(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1");
        bytes[26..28].copy_from_slice(&3u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&0xfffeu16.to_le_bytes());
        bytes[30..32].copy_from_slice(&9u16.to_le_bytes());
        bytes[32..34].copy_from_slice(&6u16.to_le_bytes());
        bytes[44..48].copy_from_slice(&1u32.to_le_bytes());
        bytes[48..52].copy_from_slice(&0u32.to_le_bytes());
        bytes[56..60].copy_from_slice(&4096u32.to_le_bytes());
        bytes[60..64].copy_from_slice(&0xfffffffeu32.to_le_bytes());
        bytes[68..72].copy_from_slice(&0xfffffffeu32.to_le_bytes());
        for offset in (76..512).step_by(4) {
            bytes[offset..offset + 4].copy_from_slice(&0xffffffffu32.to_le_bytes());
        }
        bytes[76..80].copy_from_slice(&1u32.to_le_bytes());
        for (index, unit) in "Root Entry\0".encode_utf16().enumerate() {
            bytes[512 + index * 2..514 + index * 2].copy_from_slice(&unit.to_le_bytes());
        }
        bytes[512 + 64..512 + 66].copy_from_slice(&22u16.to_le_bytes());
        bytes[512 + 66] = 5;
        bytes[512 + 67] = 1;
        for offset in [68, 72, 76] {
            bytes[512 + offset..516 + offset].copy_from_slice(&0xffffffffu32.to_le_bytes());
        }
        bytes[512 + 116..512 + 120].copy_from_slice(&0xfffffffeu32.to_le_bytes());
        for offset in (1024..1536).step_by(4) {
            bytes[offset..offset + 4].copy_from_slice(&0xffffffffu32.to_le_bytes());
        }
        bytes[1024..1028].copy_from_slice(&0xfffffffeu32.to_le_bytes());
        bytes[1028..1032].copy_from_slice(&0xfffffffdu32.to_le_bytes());
        bytes
    }
}
