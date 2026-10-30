//! Bounded owned image requests. This port carries no host attachment authority.

use std::fmt;
use std::io::Cursor;
use std::time::Instant;

use anima_core::CancelSignal;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const IMAGE_MODEL: &str = "gpt-image-2.5-sunburst";
pub const IMAGE_SIZE: &str = "1024x1024";
pub const IMAGE_QUALITY: &str = "medium";
pub const IMAGE_OUTPUT_FORMAT: &str = "png";
pub const IMAGE_COUNT: u32 = 1;
pub const IMAGE_BACKGROUND: &str = "auto";
pub const IMAGE_OUTPUT_DIMENSION: u32 = 1024;
pub const MAX_IMAGE_PROMPT_BYTES: usize = 16 * 1024;
pub const MAX_IMAGE_INPUT_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_IMAGE_OUTPUT_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_IMAGE_DIMENSION: u32 = 8192;
pub const MAX_IMAGE_PIXELS: u64 = 16 * 1024 * 1024;

/// Safe classifications of a paid operation. No variant retains provider text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageError {
    /// No physical provider request was started.
    NotSent,
    /// The provider definitively rejected the request.
    Rejected,
    /// A request may have run; this operation must never be automatically resent.
    OutcomeUnknown,
}

impl fmt::Display for ImageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotSent => "image request was not sent",
            Self::Rejected => "image request was rejected",
            Self::OutcomeUnknown => "image request outcome is unknown",
        })
    }
}

impl std::error::Error for ImageError {}

#[derive(Clone, PartialEq, Eq)]
pub struct ImagePrompt(String);

impl ImagePrompt {
    /// # Errors
    /// Returns `NotSent` for empty, oversized, blank or NUL-containing text.
    pub fn new(text: String) -> Result<Self, ImageError> {
        if text.is_empty()
            || text.len() > MAX_IMAGE_PROMPT_BYTES
            || text.trim().is_empty()
            || text.contains('\0')
        {
            return Err(ImageError::NotSent);
        }
        Ok(Self(text))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ImagePrompt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImagePrompt")
            .field("byte_len", &self.0.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ImageInput {
    bytes: Vec<u8>,
    media_type: &'static str,
    dimensions: (u32, u32),
}

impl ImageInput {
    /// Validates the encoded envelope and dimensions without trusting a file
    /// name or caller-supplied media type. PNG is decoded row by row; JPEG and
    /// WebP are inspected structurally without decompressing their pixels.
    ///
    /// # Errors
    /// Returns `NotSent` for unsupported, malformed or oversized input.
    pub fn new(bytes: Vec<u8>) -> Result<Self, ImageError> {
        if bytes.is_empty() || bytes.len() > MAX_IMAGE_INPUT_BYTES {
            return Err(ImageError::NotSent);
        }
        let (media_type, dimensions) = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            ("image/png", validate_png(&bytes, None))
        } else if bytes.starts_with(b"\xff\xd8") {
            ("image/jpeg", jpeg_dimensions(&bytes))
        } else if bytes.starts_with(b"RIFF") {
            ("image/webp", webp_dimensions(&bytes))
        } else {
            return Err(ImageError::NotSent);
        };
        let dimensions = dimensions.ok_or(ImageError::NotSent)?;
        Ok(Self {
            bytes,
            media_type,
            dimensions,
        })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn media_type(&self) -> &'static str {
        self.media_type
    }

    pub fn dimensions(&self) -> (u32, u32) {
        self.dimensions
    }
}

impl fmt::Debug for ImageInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImageInput")
            .field("byte_len", &self.bytes.len())
            .field("media_type", &self.media_type)
            .field("dimensions", &self.dimensions)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageRequest {
    Generate {
        prompt: ImagePrompt,
    },
    Edit {
        prompt: ImagePrompt,
        image: ImageInput,
    },
}

/// Provider-reported token counts only. Missing fields and cost remain unknown.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImageUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub input_text_tokens: Option<u64>,
    pub input_image_tokens: Option<u64>,
    pub output_text_tokens: Option<u64>,
    pub output_image_tokens: Option<u64>,
}

pub struct GeneratedImage {
    bytes: Vec<u8>,
    usage: Option<ImageUsage>,
}

impl GeneratedImage {
    /// # Errors
    /// Returns `OutcomeUnknown` unless the bytes are one valid fixed-size PNG
    /// within the encoded byte limit. Invalid output cannot prove no charge.
    pub fn new(bytes: Vec<u8>, usage: Option<ImageUsage>) -> Result<Self, ImageError> {
        if bytes.is_empty()
            || bytes.len() > MAX_IMAGE_OUTPUT_BYTES
            || validate_png(
                &bytes,
                Some((IMAGE_OUTPUT_DIMENSION, IMAGE_OUTPUT_DIMENSION)),
            )
            .is_none()
        {
            return Err(ImageError::OutcomeUnknown);
        }
        Ok(Self { bytes, usage })
    }

    pub fn png_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub fn usage(&self) -> Option<&ImageUsage> {
        self.usage.as_ref()
    }
}

impl fmt::Debug for GeneratedImage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GeneratedImage")
            .field("byte_len", &self.bytes.len())
            .field("usage", &self.usage)
            .finish()
    }
}

fn bounded_dimensions(width: u32, height: u32) -> Option<(u32, u32)> {
    (width > 0
        && height > 0
        && width <= MAX_IMAGE_DIMENSION
        && height <= MAX_IMAGE_DIMENSION
        && u64::from(width) * u64::from(height) <= MAX_IMAGE_PIXELS)
        .then_some((width, height))
}

fn validate_png(bytes: &[u8], required: Option<(u32, u32)>) -> Option<(u32, u32)> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    // The decoder may stop at IEND. Walk the outer chunks too, so appended
    // images, trailing bytes and animation chunks never become one image.
    let mut offset = 8_usize;
    let mut saw_data = false;
    let mut saw_end = false;
    while offset < bytes.len() {
        let header = bytes.get(offset..offset.checked_add(8)?)?;
        let length = usize::try_from(u32::from_be_bytes(header[..4].try_into().ok()?)).ok()?;
        let end = offset.checked_add(length)?.checked_add(12)?;
        if end > bytes.len() {
            return None;
        }
        let kind = &header[4..8];
        if (offset == 8 && (kind != b"IHDR" || length != 13))
            || (offset != 8 && kind == b"IHDR")
            || matches!(kind, b"acTL" | b"fcTL" | b"fdAT")
        {
            return None;
        }
        if kind == b"IDAT" {
            saw_data = true;
        }
        if kind == b"IEND" {
            if length != 0 || !saw_data || end != bytes.len() {
                return None;
            }
            saw_end = true;
        }
        offset = end;
    }
    if !saw_end {
        return None;
    }
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: 16 * 1024 * 1024,
        },
    );
    decoder.ignore_checksums(false);
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    let mut reader = decoder.read_info().ok()?;
    let dimensions = bounded_dimensions(reader.info().width, reader.info().height)?;
    if required.is_some_and(|required| dimensions != required)
        || reader.info().animation_control.is_some()
    {
        return None;
    }
    // Rows keep validation memory bounded even for a large permitted source.
    while reader.next_row().ok()?.is_some() {}
    reader.finish().ok()?;
    Some(dimensions)
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if !bytes.starts_with(b"\xff\xd8") || !bytes.ends_with(b"\xff\xd9") {
        return None;
    }
    let mut offset = 2_usize;
    let mut dimensions = None;
    let mut entropy = false;
    let mut saw_scan = false;
    let mut saw_entropy = false;
    while offset < bytes.len() {
        if entropy {
            loop {
                let byte = *bytes.get(offset)?;
                if byte != 0xff {
                    saw_entropy = true;
                    offset += 1;
                    continue;
                }
                let mut marker_offset = offset + 1;
                while bytes.get(marker_offset) == Some(&0xff) {
                    marker_offset += 1;
                }
                let marker = *bytes.get(marker_offset)?;
                if marker == 0 || (0xd0..=0xd7).contains(&marker) {
                    saw_entropy = true;
                    offset = marker_offset + 1;
                    continue;
                }
                break;
            }
        }
        if bytes.get(offset) != Some(&0xff) {
            return None;
        }
        while bytes.get(offset) == Some(&0xff) {
            offset += 1;
        }
        let marker = *bytes.get(offset)?;
        offset += 1;
        match marker {
            0xd9 => {
                return (offset == bytes.len() && saw_scan && saw_entropy)
                    .then_some(dimensions)
                    .flatten()
            }
            0 | 0x01 | 0xd0..=0xd8 => return None,
            _ => {}
        }
        let length = usize::from(u16::from_be_bytes(
            bytes.get(offset..offset + 2)?.try_into().ok()?,
        ));
        if length < 2 {
            return None;
        }
        let end = offset.checked_add(length)?;
        let payload = bytes.get(offset + 2..end)?;
        if matches!(marker, 0xc0..=0xc2) {
            if dimensions.is_some() || payload.len() < 6 || payload[0] != 8 {
                return None;
            }
            let components = usize::from(payload[5]);
            if !(1..=4).contains(&components) || payload.len() != 6 + components * 3 {
                return None;
            }
            dimensions = Some(bounded_dimensions(
                u32::from(u16::from_be_bytes(payload[3..5].try_into().ok()?)),
                u32::from(u16::from_be_bytes(payload[1..3].try_into().ok()?)),
            )?);
        } else if marker == 0xda {
            dimensions?;
            let components = usize::from(*payload.first()?);
            if !(1..=4).contains(&components) || payload.len() != 4 + components * 2 {
                return None;
            }
            entropy = true;
            saw_scan = true;
        } else {
            entropy = false;
        }
        offset = end;
    }
    None
}

fn webp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WEBP" {
        return None;
    }
    let length = usize::try_from(u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?)).ok()?;
    if length.checked_add(8)? != bytes.len() {
        return None;
    }
    let mut offset = 12_usize;
    let mut canvas = None;
    let mut image = None;
    while offset < bytes.len() {
        let header = bytes.get(offset..offset.checked_add(8)?)?;
        let length = usize::try_from(u32::from_le_bytes(header[4..8].try_into().ok()?)).ok()?;
        let end = offset.checked_add(8)?.checked_add(length)?;
        let payload = bytes.get(offset + 8..end)?;
        match &header[..4] {
            b"VP8X" => {
                if offset != 12 || length != 10 || payload[0] & 0xc3 != 0 || payload[1..4] != [0; 3]
                {
                    return None;
                }
                canvas = Some(bounded_dimensions(
                    1 + u32::from_le_bytes([payload[4], payload[5], payload[6], 0]),
                    1 + u32::from_le_bytes([payload[7], payload[8], payload[9], 0]),
                )?);
            }
            b"VP8L" => {
                if image.is_some() || payload.len() < 5 || payload[0] != 0x2f {
                    return None;
                }
                let dimensions = u32::from_le_bytes(payload[1..5].try_into().ok()?);
                if dimensions >> 29 != 0 {
                    return None;
                }
                image = Some(bounded_dimensions(
                    1 + (dimensions & 0x3fff),
                    1 + ((dimensions >> 14) & 0x3fff),
                )?);
            }
            b"VP8 " => {
                if image.is_some()
                    || payload.len() < 10
                    || payload[0] & 1 != 0
                    || payload[3..6] != [0x9d, 0x01, 0x2a]
                {
                    return None;
                }
                image = Some(bounded_dimensions(
                    u32::from(u16::from_le_bytes(payload[6..8].try_into().ok()?) & 0x3fff),
                    u32::from(u16::from_le_bytes(payload[8..10].try_into().ok()?) & 0x3fff),
                )?);
            }
            b"ANIM" | b"ANMF" => return None,
            b"ALPH" if canvas.is_none() || image.is_some() || payload.is_empty() => return None,
            _ => {}
        }
        offset = end.checked_add(length % 2)?;
        if length % 2 != 0 && bytes.get(end) != Some(&0) {
            return None;
        }
    }
    let dimensions = image?;
    if canvas.is_some_and(|canvas| canvas != dimensions) {
        return None;
    }
    Some(dimensions)
}

#[async_trait]
pub trait ImagePort: Send + Sync {
    /// Honors one absolute deadline and never retries. Cancellation after send
    /// cannot establish that the provider stopped processing or charging.
    async fn generate(
        &self,
        request: ImageRequest,
        cancel: CancelSignal,
        deadline: Instant,
    ) -> Result<GeneratedImage, ImageError>;
}

#[cfg(test)]
#[path = "image_tests.rs"]
mod tests;
