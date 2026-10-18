//! Bounded text and canonical PCM speech output, independent of any host.

use std::fmt;
use std::time::Instant;

use async_trait::async_trait;

use anima_core::CancelSignal;

pub const MAX_SPEECH_INPUT_CHARS: usize = 2_048;
pub const MAX_SPEECH_INPUT_BYTES: usize = 8_192;
pub const MAX_SPEECH_PCM_BYTES: usize = 5_760_000;
pub const MAX_SPEECH_WAV_BYTES: usize = MAX_SPEECH_PCM_BYTES + 44;
pub const MAX_SPEECH_DURATION_MS: u32 = 120_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpeechAvailability {
    Available,
    Disabled,
    MissingCredentials,
    UnsupportedConfiguration,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpeechError {
    Busy,
    Cancelled,
    TimedOut,
    Unavailable,
    InvalidInput,
    InvalidResponse,
    Failed,
}

impl fmt::Display for SpeechError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Busy => "Speech is busy",
            Self::Cancelled => "Speech was cancelled",
            Self::TimedOut => "Speech timed out",
            Self::Unavailable => "Speech is unavailable",
            Self::InvalidInput => "Invalid speech text",
            Self::InvalidResponse => "Invalid speech response",
            Self::Failed => "Speech failed",
        })
    }
}

impl std::error::Error for SpeechError {}

/// Exact selected reply text. Validation does not trim or normalize it.
pub struct SpeechText(String);

impl fmt::Debug for SpeechText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SpeechText")
            .field("byte_len", &self.0.len())
            .finish()
    }
}

impl SpeechText {
    pub fn new(text: String) -> Result<Self, SpeechError> {
        if text.is_empty()
            || text.len() > MAX_SPEECH_INPUT_BYTES
            || text.chars().count() > MAX_SPEECH_INPUT_CHARS
            || text.contains('\0')
            || text.trim().is_empty()
        {
            return Err(SpeechError::InvalidInput);
        }
        Ok(Self(text))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Owned canonical 44-byte-header RIFF/WAVE, PCM16 mono at 24 kHz.
pub struct SpeechAudio {
    bytes: Vec<u8>,
    duration_ms: u32,
}

impl fmt::Debug for SpeechAudio {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SpeechAudio")
            .field("byte_len", &self.bytes.len())
            .field("duration_ms", &self.duration_ms)
            .finish()
    }
}

impl SpeechAudio {
    pub fn from_pcm24k(pcm: Vec<u8>) -> Result<Self, SpeechError> {
        if pcm.is_empty() || pcm.len() % 2 != 0 || pcm.len() > MAX_SPEECH_PCM_BYTES {
            return Err(SpeechError::InvalidResponse);
        }
        let frames = pcm.len() / 2;
        let duration_ms = u32::try_from((frames * 1_000).div_ceil(24_000))
            .map_err(|_| SpeechError::InvalidResponse)?;
        if duration_ms > MAX_SPEECH_DURATION_MS {
            return Err(SpeechError::InvalidResponse);
        }
        let pcm_len = u32::try_from(pcm.len()).map_err(|_| SpeechError::InvalidResponse)?;
        let riff_len = pcm_len + 36;
        let mut bytes = Vec::with_capacity(pcm.len() + 44);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&riff_len.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&24_000_u32.to_le_bytes());
        bytes.extend_from_slice(&48_000_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&pcm_len.to_le_bytes());
        bytes.extend_from_slice(&pcm);
        Ok(Self { bytes, duration_ms })
    }

    pub fn wav_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub fn duration_ms(&self) -> u32 {
        self.duration_ms
    }
}

#[async_trait]
pub trait SpeechPort: Send + Sync {
    fn availability(&self) -> SpeechAvailability;

    /// Implementations honor the same absolute deadline throughout transport.
    /// Local cancellation does not establish that remote processing has stopped.
    async fn synthesize(
        &self,
        text: SpeechText,
        cancel: CancelSignal,
        deadline: Instant,
    ) -> Result<SpeechAudio, SpeechError>;
}

#[cfg(test)]
#[path = "speech_tests.rs"]
mod tests;
