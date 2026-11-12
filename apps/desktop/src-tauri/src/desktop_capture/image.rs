//! Packed RGBA images shared by desktop capture and image encoding, independent of VR.
#[derive(Debug, Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[derive(Clone, Copy)]
pub enum PixelFormat {
    Rgba,
    Bgra,
    Rgbx,
    Bgrx,
}

impl Image {
    pub fn from_rows(
        width: u32,
        height: u32,
        stride: i32,
        bytes: &[u8],
        format: PixelFormat,
    ) -> Result<Self, String> {
        let row = (width as usize)
            .checked_mul(4)
            .ok_or("capture.invalid_frame")?;
        let pitch = stride.unsigned_abs() as usize;
        let required = pitch
            .checked_mul(height.saturating_sub(1) as usize)
            .and_then(|n| n.checked_add(row));
        if width == 0 || height == 0 || pitch < row || required.is_none_or(|n| n > bytes.len()) {
            return Err("capture.invalid_frame".into());
        }
        let length = row
            .checked_mul(height as usize)
            .filter(|n| *n <= 512 * 1024 * 1024)
            .ok_or("capture.invalid_frame")?;
        let mut pixels = Vec::with_capacity(length);
        for y in 0..height as usize {
            let source_y = if stride < 0 {
                height as usize - 1 - y
            } else {
                y
            };
            for pixel in bytes[source_y * pitch..source_y * pitch + row]
                .as_chunks::<4>()
                .0
            {
                let (red, blue) = match format {
                    PixelFormat::Rgba | PixelFormat::Rgbx => (pixel[0], pixel[2]),
                    _ => (pixel[2], pixel[0]),
                };
                let alpha = match format {
                    PixelFormat::Rgba | PixelFormat::Bgra => pixel[3],
                    _ => 255,
                };
                pixels.extend_from_slice(&[red, pixel[1], blue, alpha]);
            }
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    pub fn crop(&self, region: super::selection::Region) -> Result<Self, String> {
        Ok(Self {
            width: region.width,
            height: region.height,
            pixels: region
                .crop(self.width, self.height, &self.pixels)
                .map_err(|_| "capture.invalid_frame".to_string())?,
        })
    }

    /// SPA video transform: counter-clockwise rotation, optionally horizontally flipped first.
    pub fn transform(self, transform: u32) -> Result<Self, String> {
        if transform == 0 {
            return Ok(self);
        }
        if transform > 7 {
            return Err("capture.unsupported_transform".into());
        }
        let quarter = transform % 4;
        let (width, height) = if quarter % 2 == 1 {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        };
        let mut pixels = vec![0; self.pixels.len()];
        for y in 0..self.height {
            for x in 0..self.width {
                let flipped_x = if transform >= 4 {
                    self.width - 1 - x
                } else {
                    x
                };
                let (dx, dy) = match quarter {
                    1 => (y, self.width - 1 - flipped_x),
                    2 => (self.width - 1 - flipped_x, self.height - 1 - y),
                    3 => (self.height - 1 - y, flipped_x),
                    _ => (flipped_x, y),
                };
                let source = (y as usize * self.width as usize + x as usize) * 4;
                let target = (dy as usize * width as usize + dx as usize) * 4;
                pixels[target..target + 4].copy_from_slice(&self.pixels[source..source + 4]);
            }
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }
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
    fn padded_bgra_and_negative_stride_are_normalized() {
        let bytes = [3, 2, 1, 255, 0, 0, 0, 0, 6, 5, 4, 255];
        let image = Image::from_rows(1, 2, -8, &bytes, PixelFormat::Bgra).unwrap();
        assert_eq!(image.pixels, [4, 5, 6, 255, 1, 2, 3, 255]);
        assert!(Image::from_rows(2, 2, 4, &bytes, PixelFormat::Rgba).is_err());
        assert!(Image::from_rows(1, 2, 8, &bytes[..11], PixelFormat::Rgba).is_err());
    }
    #[test]
    fn rotation_keeps_selection_in_displayed_pixel_coordinates() {
        let image = Image {
            width: 2,
            height: 1,
            pixels: vec![1, 0, 0, 255, 2, 0, 0, 255],
        }
        .transform(1)
        .unwrap();
        assert_eq!((image.width, image.height), (1, 2));
        assert_eq!(image.pixels, [2, 0, 0, 255, 1, 0, 0, 255]);
    }
}
