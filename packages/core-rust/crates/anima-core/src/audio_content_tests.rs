use super::*;

fn pcm_format(sample_rate: u32, channels: u16) -> Vec<u8> {
    let block_align = channels * 2;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * u32::from(block_align)).to_le_bytes());
    bytes.extend_from_slice(&block_align.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes
}

fn riff(chunks: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    let mut bytes = b"RIFF\0\0\0\0WAVE".to_vec();
    for (id, payload) in chunks {
        bytes.extend_from_slice(&id);
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&payload);
        if payload.len() % 2 != 0 {
            bytes.push(0);
        }
    }
    let size = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    bytes
}

fn pcm_wav(sample_rate: u32, channels: u16, frames: usize) -> Vec<u8> {
    riff(vec![
        (*b"fmt ", pcm_format(sample_rate, channels)),
        (*b"data", vec![0; frames * usize::from(channels) * 2]),
    ])
}

#[test]
fn accepted_pcm_preserves_original_bytes_and_reports_ceiling_duration() {
    for (sample_rate, channels, frames, duration_ms) in
        [(8_000, 1, 8_001, 1_001), (48_000, 2, 1, 1)]
    {
        let bytes = pcm_wav(sample_rate, channels, frames);
        let parsed = WavAudio::parse(bytes.clone()).unwrap();
        assert_eq!(parsed.bytes(), bytes);
        assert_eq!(
            parsed.info(),
            WavInfo {
                sample_rate,
                channels,
                duration_ms,
            }
        );
        assert_eq!(parsed.into_bytes(), bytes);
    }
}

#[test]
fn input_and_duration_limits_accept_exact_boundary_and_reject_one_over() {
    let max_duration_frames = 8_000 * 120;
    assert_eq!(
        WavAudio::parse(pcm_wav(8_000, 1, max_duration_frames))
            .unwrap()
            .info()
            .duration_ms,
        MAX_AUDIO_DURATION_MS
    );
    assert_eq!(
        WavAudio::parse(pcm_wav(8_000, 1, max_duration_frames + 1)).unwrap_err(),
        AudioValidationError::TooLong
    );

    let mut at_limit = riff(vec![
        (*b"fmt ", pcm_format(8_000, 1)),
        (*b"JUNK", vec![0; MAX_AUDIO_INPUT_BYTES - 54]),
        (*b"data", vec![0; 2]),
    ]);
    assert_eq!(at_limit.len(), MAX_AUDIO_INPUT_BYTES);
    assert!(WavAudio::parse(at_limit.clone()).is_ok());
    at_limit.push(0);
    assert_eq!(
        WavAudio::parse(at_limit).unwrap_err(),
        AudioValidationError::TooLarge
    );
}

#[test]
fn skips_bounded_unknown_chunks_with_required_odd_byte_padding() {
    let bytes = riff(vec![
        (*b"JUNK", vec![1, 2, 3]),
        (*b"fmt ", pcm_format(16_000, 1)),
        (*b"LIST", vec![4]),
        (*b"data", vec![0; 2]),
        (*b"tail", vec![5, 6]),
    ]);
    assert!(WavAudio::parse(bytes).is_ok());

    // Remove the required padding from the final unknown chunk.
    let mut missing_pad = riff(vec![
        (*b"fmt ", pcm_format(8_000, 1)),
        (*b"data", vec![0; 2]),
        (*b"JUNK", vec![1]),
    ]);
    missing_pad.pop();
    let size = (missing_pad.len() - 8) as u32;
    missing_pad[4..8].copy_from_slice(&size.to_le_bytes());
    assert_eq!(
        WavAudio::parse(missing_pad).unwrap_err(),
        AudioValidationError::Malformed
    );
}

#[test]
fn chunk_limit_counts_format_data_and_empty_unknown_chunks() {
    let chunks = |unknown| {
        let mut chunks = vec![(*b"fmt ", pcm_format(8_000, 1))];
        chunks.extend((0..unknown).map(|_| (*b"JUNK", Vec::new())));
        chunks.push((*b"data", vec![0; 2]));
        chunks
    };
    assert!(WavAudio::parse(riff(chunks(126))).is_ok());
    assert_eq!(
        WavAudio::parse(riff(chunks(127))).unwrap_err(),
        AudioValidationError::Malformed
    );
}

#[test]
fn rejects_every_truncated_prefix_and_inexact_riff_size() {
    let bytes = pcm_wav(8_000, 1, 2);
    for end in 0..bytes.len() {
        assert_eq!(
            WavAudio::parse(bytes[..end].to_vec()).unwrap_err(),
            AudioValidationError::Malformed,
            "prefix {end}"
        );
    }
    for declared in [0, (bytes.len() - 9) as u32, bytes.len() as u32, u32::MAX] {
        let mut bad = bytes.clone();
        bad[4..8].copy_from_slice(&declared.to_le_bytes());
        assert_eq!(
            WavAudio::parse(bad).unwrap_err(),
            AudioValidationError::Malformed
        );
    }
    let mut trailing = bytes;
    trailing.extend_from_slice(&[0]);
    assert_eq!(
        WavAudio::parse(trailing).unwrap_err(),
        AudioValidationError::Malformed
    );
}

#[test]
fn rejects_non_wave_headers_and_unsupported_riff_variants() {
    for (offset, value, expected) in [
        (0, *b"RIFF", None),
        (0, *b"RIFX", Some(AudioValidationError::Unsupported)),
        (0, *b"RF64", Some(AudioValidationError::Unsupported)),
        (0, *b"nope", Some(AudioValidationError::Malformed)),
        (8, *b"AVI ", Some(AudioValidationError::Malformed)),
    ] {
        let mut bytes = pcm_wav(8_000, 1, 1);
        bytes[offset..offset + 4].copy_from_slice(&value);
        match expected {
            Some(error) => assert_eq!(WavAudio::parse(bytes).unwrap_err(), error),
            None => assert!(WavAudio::parse(bytes).is_ok()),
        }
    }
}

#[test]
fn requires_one_format_before_one_nonempty_aligned_data_chunk() {
    let format = (*b"fmt ", pcm_format(8_000, 1));
    let data = (*b"data", vec![0; 2]);
    for chunks in [
        vec![],
        vec![format.clone()],
        vec![data.clone()],
        vec![data.clone(), format.clone()],
        vec![format.clone(), format.clone(), data.clone()],
        vec![format.clone(), data.clone(), data.clone()],
        vec![format.clone(), (*b"data", vec![])],
        vec![format.clone(), (*b"data", vec![0; 3])],
        vec![(*b"fmt ", pcm_format(8_000, 2)), data],
    ] {
        assert_eq!(
            WavAudio::parse(riff(chunks)).unwrap_err(),
            AudioValidationError::Malformed
        );
    }
}

#[test]
fn rejects_truncated_chunk_header_and_extreme_declared_chunk_length() {
    let mut short_header = pcm_wav(8_000, 1, 1);
    short_header.extend_from_slice(b"JUNK\0\0\0");
    let size = (short_header.len() - 8) as u32;
    short_header[4..8].copy_from_slice(&size.to_le_bytes());
    assert_eq!(
        WavAudio::parse(short_header).unwrap_err(),
        AudioValidationError::Malformed
    );

    let mut huge_chunk = pcm_wav(8_000, 1, 1);
    huge_chunk[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        WavAudio::parse(huge_chunk).unwrap_err(),
        AudioValidationError::Malformed
    );
}

#[test]
fn rejects_compressed_extensible_and_unselected_sample_formats() {
    for (offset, value) in [(0, 3u16), (0, 0xfffe), (2, 0), (2, 3), (14, 8), (14, 24)] {
        let mut format = pcm_format(8_000, 1);
        format[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        assert_eq!(
            WavAudio::parse(riff(vec![(*b"fmt ", format), (*b"data", vec![0; 4])])).unwrap_err(),
            AudioValidationError::Unsupported
        );
    }
    for rate in [0, 7_999, 48_001, u32::MAX] {
        let mut format = pcm_format(8_000, 1);
        format[4..8].copy_from_slice(&rate.to_le_bytes());
        assert_eq!(
            WavAudio::parse(riff(vec![(*b"fmt ", format), (*b"data", vec![0; 2])])).unwrap_err(),
            AudioValidationError::Unsupported
        );
    }
    for format in [vec![0; 14], vec![0; 18], vec![0; 40]] {
        assert_eq!(
            WavAudio::parse(riff(vec![(*b"fmt ", format), (*b"data", vec![0; 2])])).unwrap_err(),
            AudioValidationError::Unsupported
        );
    }
}

#[test]
fn rejects_inconsistent_byte_rate_and_block_alignment() {
    for (offset, value) in [(8, 0u32), (8, 15_999), (12, 4)] {
        let mut format = pcm_format(8_000, 1);
        if offset == 12 {
            format[offset..offset + 2].copy_from_slice(&(value as u16).to_le_bytes());
        } else {
            format[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        assert_eq!(
            WavAudio::parse(riff(vec![(*b"fmt ", format), (*b"data", vec![0; 4])])).unwrap_err(),
            AudioValidationError::Malformed
        );
    }
}

#[test]
fn transcript_validation_bounds_raw_utf8_before_trimming_and_rejects_nul() {
    assert_eq!(
        validate_transcript_text(" \nhello 世界\t ").unwrap(),
        "hello 世界"
    );
    let at_limit = "é".repeat(MAX_TRANSCRIPT_BYTES / 2);
    assert_eq!(validate_transcript_text(&at_limit).unwrap(), at_limit);
    for invalid in [
        String::new(),
        " \n\t".into(),
        "\0hello".into(),
        "hello\0world".into(),
        "x".repeat(MAX_TRANSCRIPT_BYTES + 1),
        format!(" {}", "x".repeat(MAX_TRANSCRIPT_BYTES)),
    ] {
        assert_eq!(
            validate_transcript_text(&invalid).unwrap_err(),
            TranscriptionError::InvalidResponse
        );
    }
}

#[test]
fn debug_output_does_not_include_audio_payload() {
    let mut bytes = pcm_wav(8_000, 1, 4);
    bytes[44..52].copy_from_slice(b"private!");
    let debug = format!("{:?}", WavAudio::parse(bytes).unwrap());
    assert!(!debug.contains("private!"));
    assert!(!debug.contains("112, 114, 105"));
}
