//! Packed RGBA images shared by desktop capture and image encoding, independent of VR.

#[derive(Debug, Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[cfg(any(target_os = "linux", test))]
const MAX_IMAGE_BYTES: usize = 512 * 1024 * 1024;

#[cfg(any(target_os = "linux", test))]
pub fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(
        png::Transformations::EXPAND | png::Transformations::STRIP_16 | png::Transformations::ALPHA,
    );
    let mut reader = decoder
        .read_info()
        .map_err(|_| "capture.invalid_frame".to_string())?;
    let info = reader.info();
    let width = info.width;
    let height = info.height;
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .filter(|n| *n <= MAX_IMAGE_BYTES);
    let (Some(expected), true, true) = (expected, width > 0, height > 0) else {
        return Err("capture.invalid_frame".into());
    };
    let mut buffer = vec![0u8; reader.output_buffer_size()];
    let frame = reader
        .next_frame(&mut buffer)
        .map_err(|_| "capture.invalid_frame".to_string())?;
    let raw = &buffer[..frame.buffer_size()];
    let pixels = match frame.color_type {
        png::ColorType::Rgba => {
            if raw.len() != expected {
                return Err("capture.invalid_frame".into());
            }
            raw.to_vec()
        }
        png::ColorType::GrayscaleAlpha => {
            let count = (width as usize) * (height as usize);
            if raw.len() != count * 2 {
                return Err("capture.invalid_frame".into());
            }
            let mut out = Vec::with_capacity(expected);
            for &[l, a] in raw.as_chunks::<2>().0 {
                out.extend_from_slice(&[l, l, l, a]);
            }
            out
        }
        png::ColorType::Rgb => {
            let count = (width as usize) * (height as usize);
            if raw.len() != count * 3 {
                return Err("capture.invalid_frame".into());
            }
            let mut out = Vec::with_capacity(expected);
            for &[r, g, b] in raw.as_chunks::<3>().0 {
                out.extend_from_slice(&[r, g, b, 255]);
            }
            out
        }
        png::ColorType::Grayscale => {
            let count = (width as usize) * (height as usize);
            if raw.len() != count {
                return Err("capture.invalid_frame".into());
            }
            let mut out = Vec::with_capacity(expected);
            for &l in raw {
                out.extend_from_slice(&[l, l, l, 255]);
            }
            out
        }
        _ => return Err("capture.invalid_frame".into()),
    };
    Ok(Image {
        width: frame.width,
        height: frame.height,
        pixels,
    })
}

pub fn encode_png(image: &Image) -> Result<Vec<u8>, String> {
    let expected = (image.width as usize)
        .checked_mul(image.height as usize)
        .and_then(|n| n.checked_mul(4));
    if image.width == 0 || image.height == 0 || expected != Some(image.pixels.len()) {
        return Err("capture.invalid_frame".into());
    }
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, image.width, image.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer
            .write_image_data(&image.pixels)
            .map_err(|e| e.to_string())?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_png_and_decode_png_round_trip() {
        let image = Image {
            width: 2,
            height: 2,
            pixels: vec![
                10, 20, 30, 255, 40, 50, 60, 128, //
                70, 80, 90, 255, 100, 110, 120, 0,
            ],
        };
        let encoded = encode_png(&image).unwrap();
        let decoded = decode_png(&encoded).unwrap();
        assert_eq!((decoded.width, decoded.height), (2, 2));
        assert_eq!(decoded.pixels, image.pixels);
        assert!(decode_png(b"not a png").is_err());
    }
}
