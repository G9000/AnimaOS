//! Literal PDF objects and an independently drawn standard-font scan raster.

pub fn scan_and_chart() -> Vec<u8> {
    let (width, height, pixels) = scan_pixels();
    let scan = b"q 240 0 0 120 0 120 cm /Scan Do Q";
    let chart = b"0.8 g 30 30 50 80 re f 0.4 g 100 30 50 140 re f 0 g 25 25 m 170 25 l 170 190 l S 20 220 160 60 re S 20 250 m 180 250 l S 100 220 m 100 280 l S";
    let mut image = format!("<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n", pixels.len()).into_bytes();
    image.extend_from_slice(&pixels);
    image.extend_from_slice(b"\nendstream");
    pdf(vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 2 /Kids [3 0 R 6 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 240 320] /Resources << /XObject << /Scan 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        stream(scan),
        image,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 240 320] /Resources << >> /Contents 7 0 R >>".to_vec(),
        stream(chart),
    ])
}

pub fn blank_page(media_box: &str, rotation: u32) -> Vec<u8> {
    pdf(vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox {media_box} /Rotate {rotation} /Resources << >> /Contents 4 0 R >>").into_bytes(),
        stream(b""),
    ])
}

pub fn encrypted_scan(password: &str) -> Vec<u8> {
    use pdf_extract::{Document, EncryptionState, EncryptionVersion, Object, Permissions};
    let mut document = Document::load_mem(&scan_and_chart()).unwrap();
    document.trailer.set(
        "ID",
        Object::Array(vec![
            Object::string_literal(b"independent-visual-test"),
            Object::string_literal(b"independent-visual-test"),
        ]),
    );
    let encryption = EncryptionState::try_from(EncryptionVersion::V1 {
        document: &document,
        owner_password: "owner",
        user_password: password,
        permissions: Permissions::all(),
    })
    .unwrap();
    document.encrypt(&encryption).unwrap();
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).unwrap();
    bytes
}

pub fn one_pixel_png() -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[255])
            .unwrap();
    }
    bytes
}

fn scan_pixels() -> (usize, usize, Vec<u8>) {
    let decoder = png::Decoder::new(std::io::Cursor::new(include_bytes!(
        "scan-standard-font.png"
    )));
    let mut reader = decoder.read_info().unwrap();
    let mut decoded = vec![0; reader.output_buffer_size().unwrap()];
    let frame = reader.next_frame(&mut decoded).unwrap();
    assert_eq!(frame.color_type, png::ColorType::Rgb);
    assert_eq!(frame.bit_depth, png::BitDepth::Eight);
    let pixels = decoded[..frame.buffer_size()]
        .chunks_exact(3)
        .map(|rgb| {
            ((u32::from(rgb[0]) * 77 + u32::from(rgb[1]) * 150 + u32::from(rgb[2]) * 29 + 128) >> 8)
                as u8
        })
        .collect();
    (frame.width as usize, frame.height as usize, pixels)
}

fn stream(bytes: &[u8]) -> Vec<u8> {
    let mut object = format!("<< /Length {} >>\nstream\n", bytes.len()).into_bytes();
    object.extend_from_slice(bytes);
    object.extend_from_slice(b"\nendstream");
    object
}

fn pdf(objects: Vec<Vec<u8>>) -> Vec<u8> {
    let mut output = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(output.len());
        output.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        output.extend_from_slice(object);
        output.extend_from_slice(b"\nendobj\n");
    }
    let xref = output.len();
    output
        .extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
    for offset in offsets.iter().skip(1) {
        output.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    output.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len()
        )
        .as_bytes(),
    );
    output
}
