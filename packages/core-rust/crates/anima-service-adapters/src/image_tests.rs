use super::*;

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&vec![0; width as usize * height as usize * 4])
            .unwrap();
    }
    bytes
}

#[test]
fn image_prompt_preserves_exact_text_and_rejects_invalid_envelopes() {
    let exact = "  Draw 🌍 exactly.\n";
    assert_eq!(ImagePrompt::new(exact.into()).unwrap().as_str(), exact);
    for text in [
        String::new(),
        " \n\t".into(),
        "bad\0prompt".into(),
        "x".repeat(MAX_IMAGE_PROMPT_BYTES + 1),
    ] {
        assert_eq!(ImagePrompt::new(text).unwrap_err(), ImageError::NotSent);
    }
}

#[test]
fn image_input_validates_png_size_dimensions_and_integrity() {
    let bytes = png(2, 3);
    let image = ImageInput::new(bytes.clone()).unwrap();
    assert_eq!(image.bytes(), bytes);
    assert_eq!(image.media_type(), "image/png");
    assert_eq!(image.dimensions(), (2, 3));
    let mut corrupt = bytes.clone();
    corrupt[29] ^= 1;
    for invalid in [
        Vec::new(),
        b"not an image".to_vec(),
        bytes[..bytes.len() - 1].to_vec(),
        corrupt,
        vec![0; MAX_IMAGE_INPUT_BYTES + 1],
    ] {
        assert_eq!(ImageInput::new(invalid).unwrap_err(), ImageError::NotSent);
    }
    let mut oversized_dimensions = png(1, 1);
    oversized_dimensions[16..20].copy_from_slice(&(MAX_IMAGE_DIMENSION + 1).to_be_bytes());
    assert_eq!(
        ImageInput::new(oversized_dimensions).unwrap_err(),
        ImageError::NotSent
    );
}

#[test]
fn image_input_recognizes_jpeg_and_webp_envelopes() {
    // A minimal SOF/SOS/EOI envelope; no decompression occurs for input selection.
    let jpeg = vec![
        0xff, 0xd8, 0xff, 0xc0, 0, 11, 8, 0, 3, 0, 2, 1, 1, 0x11, 0, 0xff, 0xda, 0, 8, 1, 1, 0, 0,
        63, 0, 1, 0xff, 0xd9,
    ];
    let image = ImageInput::new(jpeg.clone()).unwrap();
    assert_eq!(image.media_type(), "image/jpeg");
    assert_eq!(image.dimensions(), (2, 3));
    assert!(ImageInput::new(jpeg[..jpeg.len() - 1].to_vec()).is_err());
    let mut webp = b"RIFF\x12\0\0\0WEBPVP8L\x05\0\0\0\x2f\x01\x80\0\0\0".to_vec();
    let image = ImageInput::new(webp.clone()).unwrap();
    assert_eq!(image.media_type(), "image/webp");
    assert_eq!(image.dimensions(), (2, 3));
    webp[4] = 0;
    assert!(ImageInput::new(webp).is_err());
}

#[test]
fn generated_image_accepts_only_one_fixed_size_valid_png() {
    let bytes = png(IMAGE_OUTPUT_DIMENSION, IMAGE_OUTPUT_DIMENSION);
    let usage = ImageUsage {
        input_tokens: Some(3),
        input_text_tokens: Some(3),
        ..ImageUsage::default()
    };
    let image = GeneratedImage::new(bytes.clone(), Some(usage.clone())).unwrap();
    assert_eq!(image.png_bytes(), bytes);
    assert_eq!(image.usage(), Some(&usage));
    assert_eq!(image.into_bytes(), bytes);
    for invalid in [
        png(1, 1),
        b"not png".to_vec(),
        vec![0; MAX_IMAGE_OUTPUT_BYTES + 1],
    ] {
        assert_eq!(
            GeneratedImage::new(invalid, None).unwrap_err(),
            ImageError::OutcomeUnknown
        );
    }
}

#[test]
fn image_debug_and_errors_do_not_disclose_prompt_or_bytes() {
    let prompt = ImagePrompt::new("private prompt marker".into()).unwrap();
    let image = ImageInput::new(png(1, 1)).unwrap();
    assert!(!format!("{prompt:?}").contains("private prompt marker"));
    assert!(!format!("{image:?}").contains("137, 80, 78, 71"));
    for error in [
        ImageError::NotSent,
        ImageError::Rejected,
        ImageError::OutcomeUnknown,
    ] {
        assert!(!error.to_string().contains("private"));
    }
}
