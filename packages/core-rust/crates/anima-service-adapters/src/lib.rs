//! Reusable contracts and validation for service/tool adapters.
//! This crate depends on the engine; the engine never depends on this crate.

pub mod audio_content;
pub mod browser;
pub mod community_bundles;
pub mod connectors;
pub mod context;
pub mod document_content;
pub mod email;
pub mod speech;
pub use audio_content::{
    validate_transcript_text, AudioValidationError, TranscriptionAvailability, TranscriptionError,
    TranscriptionPort, WavAudio, WavInfo, MAX_AUDIO_DURATION_MS, MAX_AUDIO_INPUT_BYTES,
    MAX_TRANSCRIPT_BYTES,
};
pub use browser::{
    decode_observation, decode_operation, decode_scope, operation_digest, scope_digest,
    validate_observation, validate_operation, validate_scope, BrowserArtifact, BrowserArtifactKind,
    BrowserClose, BrowserControl, BrowserEffect, BrowserError, BrowserErrorCode, BrowserLimits,
    BrowserMethod, BrowserNode, BrowserNodeRef, BrowserObservation, BrowserOperation, BrowserPort,
    BrowserRoute, BrowserScope, BrowserSession,
};
pub use context::ServiceTokenEstimator;
pub use speech::{
    SpeechAudio, SpeechAvailability, SpeechError, SpeechPort, SpeechText, MAX_SPEECH_DURATION_MS,
    MAX_SPEECH_INPUT_BYTES, MAX_SPEECH_INPUT_CHARS, MAX_SPEECH_PCM_BYTES, MAX_SPEECH_WAV_BYTES,
};
