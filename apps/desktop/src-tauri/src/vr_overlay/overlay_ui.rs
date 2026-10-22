//! Native drawing helpers for the desktop preview fixture.
use super::renderer::Texture;
use super::text_raster::{fill_rounded_rect, Rect, TextMask};
use windows_sys::Win32::Graphics::Gdi::{DT_CENTER, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER};

pub struct Canvas {
    pub texture: Texture,
}
impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            texture: Texture {
                width,
                height,
                pixels: vec![0; (width * height * 4) as usize],
            },
        }
    }
    pub fn round(&mut self, r: [i32; 4], radius: i32, color: [u8; 4]) {
        fill_rounded_rect(
            &mut self.texture.pixels,
            self.texture.width,
            self.texture.height,
            Rect::new(r[0], r[1], r[2], r[3]),
            radius,
            color,
        );
    }
    pub fn text(
        &mut self,
        text: &str,
        r: [i32; 4],
        size: i32,
        color: [u8; 4],
    ) -> Result<(), String> {
        let (width, height) = (self.texture.width, self.texture.height);
        let mut mask = TextMask::new(width, height, size)?;
        mask.draw(
            text,
            Rect::new(r[0], r[1], r[2], r[3]),
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            true,
        )?;
        for y in r[1].max(0)..r[3].min(height as i32) {
            for x in r[0].max(0)..r[2].min(width as i32) {
                let offset = ((y as u32 * width + x as u32) * 4) as usize;
                let alpha = mask.pixels()[offset..offset + 3]
                    .iter()
                    .copied()
                    .max()
                    .unwrap_or(0) as u32;
                if alpha == 0 {
                    continue;
                }
                let pixel = &mut self.texture.pixels[offset..offset + 4];
                for i in 0..3 {
                    pixel[i] =
                        ((color[i] as u32 * alpha + pixel[i] as u32 * (255 - alpha)) / 255) as u8;
                }
                pixel[3] = pixel[3].max(alpha as u8);
            }
        }
        Ok(())
    }
}
