#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
}

impl Region {
    pub fn from_drag(start: (i32, i32), end: (i32, i32), width: u32, height: u32) -> Option<Self> {
        let clamp = |point: (i32, i32)| (point.0.max(0) as u32, point.1.max(0) as u32);
        let (sx, sy) = clamp(start);
        let (ex, ey) = clamp(end);
        let left = sx.min(ex).min(width);
        let top = sy.min(ey).min(height);
        let right = sx.max(ex).min(width);
        let bottom = sy.max(ey).min(height);
        (right - left >= 4 && bottom - top >= 4).then_some(Self {
            left,
            top,
            width: right - left,
            height: bottom - top,
        })
    }

    pub fn crop(self, width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, String> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4));
        if self.width == 0
            || self.height == 0
            || self
                .left
                .checked_add(self.width)
                .is_none_or(|right| right > width)
            || self
                .top
                .checked_add(self.height)
                .is_none_or(|bottom| bottom > height)
            || expected != Some(pixels.len())
        {
            return Err("desktop_ocr.capture_failed".into());
        }
        let stride = width as usize * 4;
        let row_bytes = self.width as usize * 4;
        let mut cropped = Vec::with_capacity(row_bytes * self.height as usize);
        for row in self.top..self.top + self.height {
            let start = row as usize * stride + self.left as usize * 4;
            cropped.extend_from_slice(&pixels[start..start + row_bytes]);
        }
        Ok(cropped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverse_drag_selects_the_same_region() {
        let expected = Some(Region {
            left: 2,
            top: 3,
            width: 6,
            height: 5,
        });
        assert_eq!(Region::from_drag((2, 3), (8, 8), 10, 10), expected);
        assert_eq!(Region::from_drag((8, 8), (2, 3), 10, 10), expected);
    }

    #[test]
    fn dragging_outside_the_frame_clamps_to_its_edges() {
        assert_eq!(
            Region::from_drag((-100, 2), (200, 200), 20, 10),
            Some(Region {
                left: 0,
                top: 2,
                width: 20,
                height: 8
            })
        );
    }

    #[test]
    fn clicks_and_tiny_drags_do_not_start_recognition() {
        assert_eq!(Region::from_drag((2, 2), (2, 2), 20, 20), None);
        assert_eq!(Region::from_drag((2, 2), (5, 10), 20, 20), None);
        assert_eq!(Region::from_drag((2, 2), (10, 5), 20, 20), None);
    }

    #[test]
    fn crop_contains_only_selected_rgba_rows() {
        let pixels: Vec<u8> = (0..4 * 3 * 4).collect();
        let region = Region {
            left: 1,
            top: 1,
            width: 2,
            height: 2,
        };
        let mut expected = pixels[20..28].to_vec();
        expected.extend_from_slice(&pixels[36..44]);
        assert_eq!(region.crop(4, 3, &pixels).unwrap(), expected);
    }

    #[test]
    fn crop_rejects_invalid_bounds_and_pixel_buffers() {
        assert!(Region {
            left: 3,
            top: 0,
            width: 2,
            height: 1
        }
        .crop(4, 3, &[0; 48])
        .is_err());
        assert!(Region {
            left: 0,
            top: 2,
            width: 1,
            height: 2
        }
        .crop(4, 3, &[0; 48])
        .is_err());
        assert!(Region {
            left: u32::MAX,
            top: 0,
            width: 2,
            height: 1
        }
        .crop(4, 3, &[0; 48])
        .is_err());
        assert!(Region {
            left: 0,
            top: 0,
            width: 1,
            height: 1
        }
        .crop(4, 3, &[0; 4])
        .is_err());
        assert!(Region {
            left: 0,
            top: 0,
            width: 0,
            height: 1
        }
        .crop(4, 3, &[0; 48])
        .is_err());
    }
}
