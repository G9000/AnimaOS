use super::*;

#[test]
fn selected_text_is_exact_and_bounded_in_scalars_and_utf8_bytes() {
    let source = "  Hello 🌍\n".to_owned();
    let selected = SpeechText::new(source.clone()).unwrap();
    assert_eq!(selected.as_str(), source);
    assert!(SpeechText::new("x".repeat(MAX_SPEECH_INPUT_CHARS)).is_ok());
    assert_eq!(
        SpeechText::new("x".repeat(MAX_SPEECH_INPUT_CHARS + 1)).unwrap_err(),
        SpeechError::InvalidInput
    );
    assert!(SpeechText::new("🦊".repeat(MAX_SPEECH_INPUT_CHARS)).is_ok());
    assert_eq!(
        SpeechText::new(format!("{}x", "🦊".repeat(MAX_SPEECH_INPUT_CHARS))).unwrap_err(),
        SpeechError::InvalidInput
    );
    for invalid in ["", " \n\t", "hello\0world"] {
        assert_eq!(
            SpeechText::new(invalid.into()).unwrap_err(),
            SpeechError::InvalidInput
        );
    }
}

#[test]
fn pcm_wraps_as_canonical_mono_24k_pcm16_wav() {
    let fixture =
        include_bytes!("../../../../../hosts/rust-daemon/tests/fixtures/audio/m58-speech.wav");
    assert_eq!(fixture.len(), 12_044);
    let raw = fixture[44..].to_vec();
    let output = SpeechAudio::from_pcm24k(raw).unwrap();
    assert_eq!(output.wav_bytes(), fixture);
    assert_eq!(output.duration_ms(), 250);
    assert_eq!(output.into_bytes().as_slice(), fixture);
}

#[test]
fn pcm_rejects_empty_odd_and_oversized_and_accepts_maximum() {
    assert_eq!(
        SpeechAudio::from_pcm24k(vec![]).unwrap_err(),
        SpeechError::InvalidResponse
    );
    assert_eq!(
        SpeechAudio::from_pcm24k(vec![1]).unwrap_err(),
        SpeechError::InvalidResponse
    );
    assert_eq!(
        SpeechAudio::from_pcm24k(vec![0; MAX_SPEECH_PCM_BYTES + 1]).unwrap_err(),
        SpeechError::InvalidResponse
    );
    let max = SpeechAudio::from_pcm24k(vec![0; MAX_SPEECH_PCM_BYTES]).unwrap();
    assert_eq!(max.wav_bytes().len(), MAX_SPEECH_WAV_BYTES);
    assert_eq!(max.duration_ms(), MAX_SPEECH_DURATION_MS);
    assert_eq!(
        &max.wav_bytes()[4..8],
        &((MAX_SPEECH_WAV_BYTES - 8) as u32).to_le_bytes()
    );
    assert_eq!(
        &max.wav_bytes()[40..44],
        &(MAX_SPEECH_PCM_BYTES as u32).to_le_bytes()
    );
}

#[test]
fn speech_debug_omits_selected_source_and_audio() {
    let text = SpeechText::new("private selected reply".into()).unwrap();
    let audio = SpeechAudio::from_pcm24k(vec![80, 82, 73, 86]).unwrap();
    assert!(!format!("{text:?}").contains("private"));
    assert!(!format!("{audio:?}").contains("80, 82"));
}
