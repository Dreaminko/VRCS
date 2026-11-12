use crate::desktop_capture::image::{encode_png, Image};

pub(crate) trait ImageSource {
    fn dimensions(&self) -> (u32, u32);
    fn pixels(&self) -> &[u8];
}
impl ImageSource for Image {
    fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}
#[cfg(windows)]
impl ImageSource for crate::vr_overlay::renderer::Texture {
    fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}
use vrcs_core::VisionConfig;

pub(crate) fn resize(source: &impl ImageSource, max_edge: u32) -> Image {
    let (width, height) = source.dimensions();
    let image = Image {
        width,
        height,
        pixels: Vec::new(),
    };
    let scale = (max_edge.max(1) as f64 / image.width.max(image.height) as f64).min(1.0);
    let width = (image.width as f64 * scale).floor().max(1.0) as u32;
    let height = (image.height as f64 * scale).floor().max(1.0) as u32;
    let mut pixels = vec![0; width as usize * height as usize * 4];
    for y in 0..height {
        for x in 0..width {
            let source_x = u64::from(x) * u64::from(image.width) / u64::from(width);
            let source_y = u64::from(y) * u64::from(image.height) / u64::from(height);
            let source_offset = (source_y as usize * image.width as usize + source_x as usize) * 4;
            let target = (y as usize * width as usize + x as usize) * 4;
            pixels[target..target + 4]
                .copy_from_slice(&source.pixels()[source_offset..source_offset + 4]);
        }
    }
    Image {
        width,
        height,
        pixels,
    }
}

pub(crate) fn prepare_image(
    image: impl ImageSource,
    config: &VisionConfig,
) -> Result<Vec<u8>, String> {
    let (width, height) = image.dimensions();
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|size| size.checked_mul(4));
    if width == 0 || height == 0 || expected != Some(image.pixels().len()) {
        return Err("Invalid visual image".into());
    }
    let mut image = resize(&image, config.image_max_edge.min(4096));
    loop {
        let bytes = encode_png(&image)?;
        if bytes.len() <= config.image_max_bytes {
            return Ok(bytes);
        }
        let edge = image.width.max(image.height);
        if edge <= 1 {
            return Err("Visual image exceeds the configured byte limit".into());
        }
        image = resize(&image, (edge * 3 / 4).max(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop_capture::image::Image as Texture;

    #[test]
    fn image_preparation_respects_dimensions_and_rejects_invalid_buffers() {
        let image = Texture {
            width: 16,
            height: 8,
            pixels: vec![255; 16 * 8 * 4],
        };
        let config = VisionConfig {
            image_max_edge: 8,
            ..Default::default()
        };
        let png = prepare_image(image, &config).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 8);
        assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 4);
        assert!(png.len() <= config.image_max_bytes);
        assert!(prepare_image(
            Texture {
                width: 16,
                height: 8,
                pixels: vec![0; 4]
            },
            &config
        )
        .is_err());
    }

    #[test]
    fn byte_limit_does_not_accept_an_oversize_png() {
        let config = VisionConfig {
            image_max_bytes: 1,
            ..Default::default()
        };
        assert!(prepare_image(
            Texture {
                width: 4,
                height: 4,
                pixels: vec![255; 64]
            },
            &config
        )
        .is_err());
    }
}
