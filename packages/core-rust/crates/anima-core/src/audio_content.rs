//! Bounded audio values and transcription contracts, independent of any host.
//! A validated WAV carries no attachment, filesystem, or provider authority.

use std::fmt;
use std::time::Instant;

use async_trait::async_trait;

use crate::runtime::CancelSignal;

pub const MAX_AUDIO_INPUT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_AUDIO_DURATION_MS: u64 = 120_000;
pub const MAX_TRANSCRIPT_BYTES: usize = 16 * 1024;
const MAX_WAV_CHUNKS: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WavInfo {
    pub sample_rate: u32,
    pub channels: u16,
    /// Duration rounded upward to the next millisecond.
    pub duration_ms: u64,
}

/// An owned, validated PCM16 little-endian RIFF/WAVE payload.
/// Ownership is moved into a transcription operation rather than cloned.
pub struct WavAudio {
    bytes: Vec<u8>,
    info: WavInfo,
}

impl fmt::Debug for WavAudio {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WavAudio")
            .field("byte_len", &self.bytes.len())
            .field("info", &self.info)
            .finish()
    }
}

impl WavAudio {
    pub fn parse(bytes: Vec<u8>) -> Result<Self, AudioValidationError> {
        let info = parse_wav_info(&bytes)?;
        Ok(Self { bytes, info })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn info(&self) -> WavInfo {
        self.info
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioValidationError {
    Malformed,
    Unsupported,
    TooLarge,
    TooLong,
}

impl fmt::Display for AudioValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Malformed => "Malformed WAV audio",
            Self::Unsupported => "Unsupported WAV audio",
            Self::TooLarge => "Audio input exceeds the size limit",
            Self::TooLong => "Audio input exceeds the duration limit",
        })
    }
}

impl std::error::Error for AudioValidationError {}

struct PcmFormat {
    sample_rate: u32,
    channels: u16,
    block_align: u16,
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, AudioValidationError> {
    let end = offset
        .checked_add(2)
        .ok_or(AudioValidationError::Malformed)?;
    let value = bytes
        .get(offset..end)
        .ok_or(AudioValidationError::Malformed)?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, AudioValidationError> {
    let end = offset
        .checked_add(4)
        .ok_or(AudioValidationError::Malformed)?;
    let value = bytes
        .get(offset..end)
        .ok_or(AudioValidationError::Malformed)?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn parse_pcm_format(bytes: &[u8]) -> Result<PcmFormat, AudioValidationError> {
    if bytes.len() != 16 {
        return Err(AudioValidationError::Unsupported);
    }
    let format = read_u16(bytes, 0)?;
    let channels = read_u16(bytes, 2)?;
    let sample_rate = read_u32(bytes, 4)?;
    let byte_rate = read_u32(bytes, 8)?;
    let block_align = read_u16(bytes, 12)?;
    let bits_per_sample = read_u16(bytes, 14)?;
    if format != 1
        || bits_per_sample != 16
        || !(1..=2).contains(&channels)
        || !(8_000..=48_000).contains(&sample_rate)
    {
        return Err(AudioValidationError::Unsupported);
    }
    let expected_align = channels
        .checked_mul(2)
        .ok_or(AudioValidationError::Malformed)?;
    let expected_rate = sample_rate
        .checked_mul(u32::from(expected_align))
        .ok_or(AudioValidationError::Malformed)?;
    if block_align != expected_align || byte_rate != expected_rate {
        return Err(AudioValidationError::Malformed);
    }
    Ok(PcmFormat {
        sample_rate,
        channels,
        block_align,
    })
}

fn parse_wav_info(bytes: &[u8]) -> Result<WavInfo, AudioValidationError> {
    if bytes.len() > MAX_AUDIO_INPUT_BYTES {
        return Err(AudioValidationError::TooLarge);
    }
    if bytes.len() < 12 {
        return Err(AudioValidationError::Malformed);
    }
    if matches!(&bytes[..4], b"RF64" | b"RIFX") {
        return Err(AudioValidationError::Unsupported);
    }
    if &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(AudioValidationError::Malformed);
    }
    let declared_size =
        usize::try_from(read_u32(bytes, 4)?).map_err(|_| AudioValidationError::Malformed)?;
    if declared_size.checked_add(8) != Some(bytes.len()) {
        return Err(AudioValidationError::Malformed);
    }

    let mut offset = 12usize;
    let mut chunks = 0usize;
    let mut format = None;
    let mut data_len = None;
    while offset < bytes.len() {
        chunks = chunks
            .checked_add(1)
            .ok_or(AudioValidationError::Malformed)?;
        if chunks > MAX_WAV_CHUNKS {
            return Err(AudioValidationError::Malformed);
        }
        let header_end = offset
            .checked_add(8)
            .ok_or(AudioValidationError::Malformed)?;
        let header = bytes
            .get(offset..header_end)
            .ok_or(AudioValidationError::Malformed)?;
        let chunk_len =
            usize::try_from(read_u32(header, 4)?).map_err(|_| AudioValidationError::Malformed)?;
        let payload_end = header_end
            .checked_add(chunk_len)
            .ok_or(AudioValidationError::Malformed)?;
        let padded_end = payload_end
            .checked_add(chunk_len % 2)
            .ok_or(AudioValidationError::Malformed)?;
        let payload = bytes
            .get(header_end..payload_end)
            .ok_or(AudioValidationError::Malformed)?;
        if padded_end > bytes.len() {
            return Err(AudioValidationError::Malformed);
        }
        match &header[..4] {
            b"fmt " => {
                if format.is_some() || data_len.is_some() {
                    return Err(AudioValidationError::Malformed);
                }
                format = Some(parse_pcm_format(payload)?);
            }
            b"data" => {
                let pcm = format.as_ref().ok_or(AudioValidationError::Malformed)?;
                if data_len.is_some()
                    || chunk_len == 0
                    || chunk_len % usize::from(pcm.block_align) != 0
                {
                    return Err(AudioValidationError::Malformed);
                }
                data_len = Some(chunk_len);
            }
            _ => {}
        }
        offset = padded_end;
    }
    let pcm = format.ok_or(AudioValidationError::Malformed)?;
    let data_len = data_len.ok_or(AudioValidationError::Malformed)?;
    let frames = u64::try_from(data_len / usize::from(pcm.block_align))
        .map_err(|_| AudioValidationError::Malformed)?;
    let rate = u64::from(pcm.sample_rate);
    let duration_ms = frames
        .checked_mul(1_000)
        .and_then(|value| value.checked_add(rate - 1))
        .ok_or(AudioValidationError::Malformed)?
        / rate;
    if duration_ms > MAX_AUDIO_DURATION_MS {
        return Err(AudioValidationError::TooLong);
    }
    Ok(WavInfo {
        sample_rate: pcm.sample_rate,
        channels: pcm.channels,
        duration_ms,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TranscriptionAvailability {
    Available,
    Disabled,
    MissingCredentials,
    UnsupportedConfiguration,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TranscriptionError {
    Busy,
    Unavailable,
    Cancelled,
    TimedOut,
    InvalidResponse,
    Failed,
}

impl fmt::Display for TranscriptionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Busy => "Transcription is busy",
            Self::Unavailable => "Transcription is unavailable",
            Self::Cancelled => "Transcription was cancelled",
            Self::TimedOut => "Transcription timed out",
            Self::InvalidResponse => "Invalid transcription response",
            Self::Failed => "Transcription failed",
        })
    }
}

impl std::error::Error for TranscriptionError {}

/// Check the raw UTF-8 byte limit before trimming; errors carry no response data.
pub fn validate_transcript_text(text: &str) -> Result<String, TranscriptionError> {
    if text.len() > MAX_TRANSCRIPT_BYTES || text.contains('\0') {
        return Err(TranscriptionError::InvalidResponse);
    }
    let text = text.trim();
    if text.is_empty() {
        return Err(TranscriptionError::InvalidResponse);
    }
    Ok(text.to_owned())
}

#[async_trait]
pub trait TranscriptionPort: Send + Sync {
    fn availability(&self) -> TranscriptionAvailability;

    /// Implementations enforce the same absolute deadline throughout the work.
    /// Local cancellation does not establish that remote processing has stopped.
    async fn transcribe(
        &self,
        audio: WavAudio,
        cancel: CancelSignal,
        deadline: Instant,
    ) -> Result<String, TranscriptionError>;
}

#[cfg(test)]
#[path = "audio_content_tests.rs"]
mod tests;
