//! Bounded binary page frames inside the existing physically owned document worker.
use std::io::{self, Write};

use anima_document_visual::{validate_pdf_evidence, PdfEvidence, PdfPageEvidence, RecognizedText};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt};

use super::worker::WorkerFailure;

const MAX_MANIFEST: usize = 256 * 1024;
const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 2 * MAX_PAGE_BYTES;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    source_sha256: String,
    page_count: u32,
    pages: Vec<Page>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    page: u32,
    width: u32,
    height: u32,
    png_bytes: usize,
    png_sha256: String,
    recognized: RecognizedText,
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid visual document frame")
}

pub(super) fn write(writer: &mut impl Write, value: &PdfEvidence) -> io::Result<()> {
    validate_pdf_evidence(value).map_err(|_| invalid())?;
    let manifest = Manifest {
        version: value.version,
        source_sha256: value.source_sha256.clone(),
        page_count: value.page_count,
        pages: value
            .pages
            .iter()
            .map(|page| Page {
                page: page.page,
                width: page.width,
                height: page.height,
                png_bytes: page.png.len(),
                png_sha256: hex::encode(Sha256::digest(&page.png)),
                recognized: page.recognized.clone(),
            })
            .collect(),
    };
    let bytes = serde_json::to_vec(&manifest).map_err(|_| invalid())?;
    if bytes.len() > MAX_MANIFEST {
        return Err(invalid());
    }
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&bytes)?;
    for page in &value.pages {
        writer.write_all(&page.png)?;
    }
    writer.flush()
}

pub(super) async fn read(
    reader: &mut (impl AsyncRead + Unpin),
) -> Result<PdfEvidence, WorkerFailure> {
    let invalid = || WorkerFailure::InvalidOutput;
    let length = reader.read_u32_le().await.map_err(|_| invalid())? as usize;
    if length == 0 || length > MAX_MANIFEST {
        return Err(invalid());
    }
    let mut json = vec![0; length];
    reader.read_exact(&mut json).await.map_err(|_| invalid())?;
    let manifest: Manifest = serde_json::from_slice(&json).map_err(|_| invalid())?;
    if manifest.pages.is_empty() || manifest.pages.len() > 2 {
        return Err(invalid());
    }
    let mut total = 0usize;
    let mut previous = 0;
    for page in &manifest.pages {
        total = total.checked_add(page.png_bytes).ok_or_else(invalid)?;
        if page.png_bytes == 0
            || page.png_bytes > MAX_PAGE_BYTES
            || total > MAX_TOTAL_BYTES
            || page.page <= previous
            || page.page > manifest.page_count
            || page.width == 0
            || page.height == 0
            || page.width > 1600
            || page.height > 1600
            || page.png_sha256.len() != 64
            || !page.png_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(invalid());
        }
        previous = page.page;
    }
    let mut pages = Vec::with_capacity(manifest.pages.len());
    for page in manifest.pages {
        let mut png = vec![0; page.png_bytes];
        reader.read_exact(&mut png).await.map_err(|_| invalid())?;
        if hex::encode(Sha256::digest(&png)) != page.png_sha256 {
            return Err(invalid());
        }
        pages.push(PdfPageEvidence {
            page: page.page,
            width: page.width,
            height: page.height,
            png,
            recognized: page.recognized,
        });
    }
    let mut trailing = [0; 1];
    if reader.read(&mut trailing).await.map_err(|_| invalid())? != 0 {
        return Err(invalid());
    }
    let value = PdfEvidence {
        version: manifest.version,
        source_sha256: manifest.source_sha256,
        page_count: manifest.page_count,
        pages,
    };
    validate_pdf_evidence(&value).map_err(|_| invalid())?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    fn evidence() -> PdfEvidence {
        PdfEvidence {
            version: 1, source_sha256: "a".repeat(64), page_count: 1,
            pages: vec![PdfPageEvidence {
                page: 1, width: 1, height: 1,
                png: base64::engine::general_purpose::STANDARD.decode(
                    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAAAAAA6fptVAAAACklEQVR4nGP4DwABAQEAsTj2FAAAAABJRU5ErkJggg=="
                ).unwrap(),
                recognized: RecognizedText::Empty,
            }],
        }
    }

    #[tokio::test]
    async fn page_frame_round_trip_preserves_original_page_and_pixels() {
        let expected = evidence();
        let mut frame = Vec::new();
        write(&mut frame, &expected).unwrap();
        let actual = read(&mut frame.as_slice()).await.unwrap();
        assert_eq!(actual.pages[0].png, expected.pages[0].png);
        assert_eq!(actual.pages[0].page, 1);
        assert_eq!(actual.source_sha256, expected.source_sha256);
    }

    #[tokio::test]
    async fn oversized_manifest_is_rejected_before_payload_allocation() {
        let frame = ((MAX_MANIFEST + 1) as u32).to_le_bytes();
        assert!(matches!(
            read(&mut frame.as_slice()).await,
            Err(WorkerFailure::InvalidOutput)
        ));
    }

    #[tokio::test]
    async fn payload_mutation_truncation_and_trailing_bytes_are_refused() {
        let mut frame = Vec::new();
        write(&mut frame, &evidence()).unwrap();
        let mut changed = frame.clone();
        *changed.last_mut().unwrap() ^= 1;
        assert!(read(&mut changed.as_slice()).await.is_err());
        assert!(read(&mut &frame[..frame.len() - 1]).await.is_err());
        frame.push(0);
        assert!(read(&mut frame.as_slice()).await.is_err());
    }
}
