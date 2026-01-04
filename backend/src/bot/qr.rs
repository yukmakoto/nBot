use base64::Engine as _;
use image::{DynamicImage, ImageFormat, Luma};
use qrcode::QrCode;
use std::io::Cursor;

pub fn generate_qr_png_data_url(text: &str) -> Option<String> {
    let code = QrCode::new(text.as_bytes()).ok()?;
    let img = code
        .render::<Luma<u8>>()
        .min_dimensions(320, 320)
        .quiet_zone(true)
        .build();

    let mut bytes: Vec<u8> = Vec::new();
    DynamicImage::ImageLuma8(img)
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .ok()?;

    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:image/png;base64,{b64}"))
}

