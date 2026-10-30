//! Windows Runtime rendering with in-memory streams and optional installed OCR.

use sha2::{Digest, Sha256};
use windows::{
    Data::Pdf::{PdfDocument, PdfPage, PdfPageRenderOptions},
    Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapEncoder, BitmapPixelFormat},
    Media::Ocr::OcrEngine,
    Storage::Streams::{DataReader, DataWriter, InMemoryRandomAccessStream},
};

use crate::{
    pixel_size, PdfEvidence, PdfPageEvidence, PdfPageSelection, PdfVisualError, RecognizedText,
    MAX_OCR_LANGUAGE_BYTES, MAX_OCR_TEXT_BYTES, MAX_PAGE_PNG_BYTES, PDF_EVIDENCE_VERSION,
};

pub(super) fn render(
    bytes: &[u8],
    selection: &PdfPageSelection,
    page_count: u32,
) -> Result<PdfEvidence, PdfVisualError> {
    let input = input_stream(bytes).map_err(unavailable)?;
    let document = PdfDocument::LoadFromStreamAsync(&input)
        .and_then(|operation| operation.join())
        .map_err(unavailable)?;
    if document.IsPasswordProtected().map_err(unavailable)? {
        return Err(PdfVisualError::Encrypted);
    }
    if document.PageCount().map_err(unavailable)? != page_count {
        return Err(PdfVisualError::InvalidOutput);
    }
    let engine = OcrEngine::TryCreateFromUserProfileLanguages().ok();
    let mut pages = Vec::with_capacity(selection.pages().len());
    for &number in selection.pages() {
        let page = document.GetPage(number - 1).map_err(unavailable)?;
        let rendered = render_page(&page, number, engine.as_ref());
        let closed = page.Close();
        let evidence = rendered?;
        closed.map_err(unavailable)?;
        pages.push(evidence);
    }
    Ok(PdfEvidence {
        version: PDF_EVIDENCE_VERSION,
        source_sha256: Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        page_count,
        pages,
    })
}

fn input_stream(bytes: &[u8]) -> windows::core::Result<InMemoryRandomAccessStream> {
    let stream = InMemoryRandomAccessStream::new()?;
    let output = stream.GetOutputStreamAt(0)?;
    let writer = DataWriter::CreateDataWriter(&output)?;
    writer.WriteBytes(bytes)?;
    let stored = writer.StoreAsync()?.join()?;
    if stored as usize != bytes.len() {
        return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
            0x8007000D_u32 as i32,
        )));
    }
    writer.DetachStream()?;
    writer.Close()?;
    stream.Seek(0)?;
    Ok(stream)
}

fn render_page(
    page: &PdfPage,
    number: u32,
    engine: Option<&OcrEngine>,
) -> Result<PdfPageEvidence, PdfVisualError> {
    let size = page.Size().map_err(unavailable)?;
    let (width, height) = pixel_size(f64::from(size.Width), f64::from(size.Height))?;
    let output = InMemoryRandomAccessStream::new().map_err(unavailable)?;
    let options = PdfPageRenderOptions::new().map_err(unavailable)?;
    options.SetDestinationWidth(width).map_err(unavailable)?;
    options.SetDestinationHeight(height).map_err(unavailable)?;
    options
        .SetBitmapEncoderId(BitmapEncoder::PngEncoderId().map_err(unavailable)?)
        .map_err(unavailable)?;
    page.RenderWithOptionsToStreamAsync(&output, &options)
        .and_then(|operation| operation.join())
        .map_err(unavailable)?;
    let length = output.Size().map_err(unavailable)?;
    if length == 0 || length > MAX_PAGE_PNG_BYTES as u64 {
        return Err(PdfVisualError::TooLarge);
    }
    let input = output.GetInputStreamAt(0).map_err(unavailable)?;
    let reader = DataReader::CreateDataReader(&input).map_err(unavailable)?;
    let loaded = reader
        .LoadAsync(length as u32)
        .and_then(|operation| operation.join())
        .map_err(unavailable)?;
    if u64::from(loaded) != length {
        return Err(PdfVisualError::InvalidOutput);
    }
    let mut png = vec![0; length as usize];
    reader.ReadBytes(&mut png).map_err(unavailable)?;
    reader.DetachStream().map_err(unavailable)?;
    reader.Close().map_err(unavailable)?;
    output.Seek(0).map_err(unavailable)?;
    let recognized = engine
        .and_then(|engine| recognize(engine, &output, width, height).ok())
        .unwrap_or(RecognizedText::Unavailable);
    Ok(PdfPageEvidence {
        page: number,
        width,
        height,
        png,
        recognized,
    })
}

fn recognize(
    engine: &OcrEngine,
    stream: &InMemoryRandomAccessStream,
    width: u32,
    height: u32,
) -> windows::core::Result<RecognizedText> {
    if width.max(height) > OcrEngine::MaxImageDimension()? {
        return Ok(RecognizedText::Unavailable);
    }
    let decoder = BitmapDecoder::CreateAsync(stream)?.join()?;
    let bitmap = decoder
        .GetSoftwareBitmapConvertedAsync(BitmapPixelFormat::Bgra8, BitmapAlphaMode::Ignore)?
        .join()?;
    if bitmap.PixelWidth()? != width as i32 || bitmap.PixelHeight()? != height as i32 {
        return Ok(RecognizedText::Unavailable);
    }
    let result = engine.RecognizeAsync(&bitmap)?.join()?;
    let text = result.Text()?.to_string();
    if text.trim().is_empty() {
        return Ok(RecognizedText::Empty);
    }
    let language = engine.RecognizerLanguage()?.LanguageTag()?.to_string();
    if text.len() > MAX_OCR_TEXT_BYTES
        || text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        || language.is_empty()
        || language.len() > MAX_OCR_LANGUAGE_BYTES
        || !language
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Ok(RecognizedText::Unavailable);
    }
    Ok(RecognizedText::Text { text, language })
}

fn unavailable(_: windows::core::Error) -> PdfVisualError {
    PdfVisualError::Unavailable
}
