use super::ocr_geometry::Homography;
use super::{ocr_capture::EyeCapture, renderer::Texture};
use vrcs_core::ocr::TranslatedBlock;

pub fn render_eye(
    eye: &EyeCapture,
    blocks: &[TranslatedBlock],
    opacity: f32,
    source_view: bool,
) -> Result<(Texture, bool), String> {
    let (width, height) = (eye.image.width, eye.image.height);
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return Err("Invalid OCR render dimensions".into());
    }
    if blocks.len() > 256 {
        return Err("OCR render exceeds the block limit".into());
    }
    let mut pixels = vec![0; (width * height * 4) as usize];
    let mut occupied: Vec<Bounds> = blocks
        .iter()
        .map(|block| Bounds::from_quad(block.source.polygon, width, height))
        .collect();
    let mut layout_limited = false;
    let mut cards_used = 0;
    for block in blocks {
        let text = if source_view {
            block.source.text.clone()
        } else {
            block
                .translations
                .iter()
                .filter_map(|translation| translation.text.as_deref())
                .filter(|text| !text.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n")
        };
        if text.is_empty() {
            continue;
        }
        let polygon = &block.source.polygon;
        if polygon.iter().any(|[x, y]| {
            !x.is_finite()
                || !y.is_finite()
                || *x < 0.
                || *x > width as f32
                || *y < 0.
                || *y > height as f32
        }) {
            return Err("Invalid OCR text coordinates".into());
        }
        let bounds = Bounds::from_quad(*polygon, width, height);
        let inverse = Homography::from_quad(*polygon)
            .and_then(Homography::inverse)
            .ok_or("Invalid OCR text quadrilateral")?;
        let edge = |a: usize, b: usize| {
            ((polygon[a][0] - polygon[b][0]).powi(2) + (polygon[a][1] - polygon[b][1]).powi(2))
                .sqrt()
        };
        let tile_width = ((edge(0, 1) + edge(2, 3)) * 0.5).round().clamp(1., 2048.) as u32;
        let tile_height = ((edge(1, 2) + edge(3, 0)) * 0.5).round().clamp(1., 2048.) as u32;
        let Some(tile) =
            super::wrist_renderer::render_text_box(&text, tile_width, tile_height, opacity)?
        else {
            if cards_used < 8 {
                if let Some((card, tile)) =
                    place_card(&text, bounds, &occupied, width, height, opacity)?
                {
                    for y in card.top..card.bottom {
                        for x in card.left..card.right {
                            let offset =
                                (((y - card.top) * card.width() + x - card.left) * 4) as usize;
                            blend(
                                &mut pixels[((y * width + x) * 4) as usize..][..4],
                                tile[offset..offset + 4].try_into().unwrap(),
                            );
                        }
                    }
                    let center = [
                        (bounds.left + bounds.right) as f32 * 0.5,
                        (bounds.top + bounds.bottom) as f32 * 0.5,
                    ];
                    let nearest = [
                        center[0].clamp(card.left as f32, card.right as f32),
                        center[1].clamp(card.top as f32, card.bottom as f32),
                    ];
                    line(&mut pixels, width, height, center, nearest);
                    for i in 0..4 {
                        line(&mut pixels, width, height, polygon[i], polygon[(i + 1) % 4]);
                    }
                    occupied.push(card);
                    cards_used += 1;
                    continue;
                }
            }
            layout_limited = true;
            continue;
        };
        for y in bounds.top..bounds.bottom {
            for x in bounds.left..bounds.right {
                let Some([u, v]) = inverse.map([x as f32 + 0.5, y as f32 + 0.5]) else {
                    continue;
                };
                if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                    continue;
                }
                let color = sample(&tile, tile_width, tile_height, u, v);
                blend(&mut pixels[((y * width + x) * 4) as usize..][..4], color);
            }
        }
    }
    Ok((
        Texture {
            pixels,
            width,
            height,
        },
        layout_limited,
    ))
}

#[derive(Clone, Copy)]
struct Bounds {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

impl Bounds {
    fn width(self) -> u32 {
        self.right.saturating_sub(self.left)
    }
    fn height(self) -> u32 {
        self.bottom.saturating_sub(self.top)
    }
    fn intersects(self, other: Self) -> bool {
        self.left < other.right
            && other.left < self.right
            && self.top < other.bottom
            && other.top < self.bottom
    }
    fn from_quad(quad: [[f32; 2]; 4], width: u32, height: u32) -> Self {
        Self {
            left: quad
                .iter()
                .map(|p| p[0])
                .fold(f32::INFINITY, f32::min)
                .floor()
                .clamp(0., width as f32) as u32,
            right: quad
                .iter()
                .map(|p| p[0])
                .fold(f32::NEG_INFINITY, f32::max)
                .ceil()
                .clamp(0., width as f32) as u32,
            top: quad
                .iter()
                .map(|p| p[1])
                .fold(f32::INFINITY, f32::min)
                .floor()
                .clamp(0., height as f32) as u32,
            bottom: quad
                .iter()
                .map(|p| p[1])
                .fold(f32::NEG_INFINITY, f32::max)
                .ceil()
                .clamp(0., height as f32) as u32,
        }
    }
}

fn place_card(
    text: &str,
    source: Bounds,
    occupied: &[Bounds],
    width: u32,
    height: u32,
    opacity: f32,
) -> Result<Option<(Bounds, Vec<u8>)>, String> {
    let margin = 8;
    if width <= margin * 2 || height <= margin * 2 {
        return Ok(None);
    }
    let (right_edge, bottom_edge) = (width - margin, height - margin);
    let max_width = (width - margin * 2).min(960);
    let max_height = (height - margin * 2).min(720);
    for candidate_width in [160, 320, 480, 960] {
        let Some((card_width, card_height)) = super::wrist_renderer::compact_text_box_size(
            text,
            candidate_width.min(max_width),
            max_height,
        )?
        else {
            continue;
        };
        let x = ((source.left + source.right) / 2)
            .saturating_sub(card_width / 2)
            .clamp(margin, right_edge - card_width);
        let y = ((source.top + source.bottom) / 2)
            .saturating_sub(card_height / 2)
            .clamp(margin, bottom_edge - card_height);
        let candidates = [
            Bounds {
                left: source.right + margin,
                top: y,
                right: source.right + margin + card_width,
                bottom: y + card_height,
            },
            Bounds {
                left: source.left.saturating_sub(margin + card_width),
                top: y,
                right: source.left.saturating_sub(margin).min(right_edge),
                bottom: y + card_height,
            },
            Bounds {
                left: x,
                top: source.bottom + margin,
                right: x + card_width,
                bottom: source.bottom + margin + card_height,
            },
            Bounds {
                left: x,
                top: source.top.saturating_sub(margin + card_height),
                right: x + card_width,
                bottom: source.top.saturating_sub(margin).min(bottom_edge),
            },
        ];
        for card in candidates {
            if card.width() < 12
                || card.height() < 12
                || card.width() != card_width
                || card.height() != card_height
                || card.left < margin
                || card.top < margin
                || card.right > right_edge
                || card.bottom > bottom_edge
                || occupied.iter().any(|other| card.intersects(*other))
            {
                continue;
            }
            if let Some(tile) =
                super::wrist_renderer::render_text_box(text, card.width(), card.height(), opacity)?
            {
                return Ok(Some((card, tile)));
            }
        }
    }
    Ok(None)
}

fn line(pixels: &mut [u8], width: u32, height: u32, from: [f32; 2], to: [f32; 2]) {
    let steps = (to[0] - from[0]).abs().max((to[1] - from[1]).abs()).ceil() as u32;
    for step in 0..=steps {
        let t = step as f32 / steps.max(1) as f32;
        let x = (from[0] + (to[0] - from[0]) * t).round() as i32;
        let y = (from[1] + (to[1] - from[1]) * t).round() as i32;
        for (x, y) in [(x, y), (x + 1, y)] {
            if x >= 0 && y >= 0 && x < width as i32 && y < height as i32 {
                blend(
                    &mut pixels[((y as u32 * width + x as u32) * 4) as usize..][..4],
                    [230; 4],
                );
            }
        }
    }
}

fn sample(pixels: &[u8], width: u32, height: u32, u: f32, v: f32) -> [u8; 4] {
    let x = (u * width as f32 - 0.5).clamp(0., (width - 1) as f32);
    let y = (v * height as f32 - 0.5).clamp(0., (height - 1) as f32);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(width - 1), (y0 + 1).min(height - 1));
    let (tx, ty) = (x - x0 as f32, y - y0 as f32);
    std::array::from_fn(|channel| {
        let value = |x, y| pixels[((y * width + x) * 4) as usize + channel] as f32;
        ((value(x0, y0) * (1. - tx) + value(x1, y0) * tx) * (1. - ty)
            + (value(x0, y1) * (1. - tx) + value(x1, y1) * tx) * ty)
            .round() as u8
    })
}

fn blend(destination: &mut [u8], source: [u8; 4]) {
    for (destination, source_channel) in destination.iter_mut().zip(source) {
        *destination = source_channel
            .saturating_add(((*destination as u16 * (255 - source[3]) as u16) / 255) as u8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vrcs_core::ocr::{BlockTranslation, TextBlock};

    #[test]
    fn ocr_eye_texture_covers_only_translated_regions() {
        let eye = EyeCapture {
            image: Texture {
                width: 100,
                height: 80,
                pixels: vec![0; 100 * 80 * 4],
            },
            projection: [-1., 1., -1., 1.],
            eye_to_head: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
            head_pose: super::super::transform::matrix(0., 0., 0., [0.; 3]),
        };
        let block = TranslatedBlock {
            source: TextBlock {
                id: 0,
                text: "source".into(),
                confidence: 0.95,
                polygon: [[10., 10.], [90., 10.], [90., 60.], [10., 60.]],
            },
            translations: vec![BlockTranslation {
                target_language: "en".into(),
                text: Some("VR".into()),
                error_code: None,
            }],
        };
        let texture = render_eye(&eye, &[block.clone()], 0.6, false).unwrap().0;
        assert_eq!((texture.width, texture.height), (100, 80));
        assert_eq!(&texture.pixels[..4], &[0, 0, 0, 0]);
        assert!(texture
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[0] > 0));
        assert!(texture.pixels[(20 * 100 + 20) * 4 + 3] >= 153);
        let untranslated = TranslatedBlock {
            translations: vec![],
            ..block
        };
        assert!(render_eye(&eye, &[untranslated], 0.6, false)
            .unwrap()
            .0
            .pixels
            .iter()
            .all(|channel| *channel == 0));
    }

    #[test]
    fn tilted_ocr_text_leaves_pixels_outside_its_quadrilateral_transparent() {
        let eye = EyeCapture {
            image: Texture {
                width: 100,
                height: 80,
                pixels: vec![0; 100 * 80 * 4],
            },
            projection: [-1., 1., -1., 1.],
            eye_to_head: super::super::transform::matrix(0., 0., 0., [0.; 3]),
            head_pose: super::super::transform::matrix(0., 0., 0., [0.; 3]),
        };
        let block = TranslatedBlock {
            source: TextBlock {
                id: 0,
                text: "source".into(),
                confidence: 0.95,
                polygon: [[30., 10.], [90., 20.], [70., 70.], [10., 60.]],
            },
            translations: vec![BlockTranslation {
                target_language: "en".into(),
                text: Some("VR".into()),
                error_code: None,
            }],
        };
        let texture = render_eye(&eye, &[block], 0.6, false).unwrap().0;
        assert_eq!(texture.pixels[(12 * 100 + 12) * 4 + 3], 0);
        assert!(texture.pixels[(40 * 100 + 50) * 4 + 3] >= 153);
    }

    #[test]
    fn long_translation_expands_beside_its_source_and_unplaceable_text_preserves_the_original() {
        let mut eye = EyeCapture {
            image: Texture {
                width: 512,
                height: 320,
                pixels: vec![0; 512 * 320 * 4],
            },
            projection: [-1., 1., -1., 1.],
            eye_to_head: super::super::transform::matrix(0., 0., 0., [0.; 3]),
            head_pose: super::super::transform::matrix(0., 0., 0., [0.; 3]),
        };
        let mut block = TranslatedBlock { source: TextBlock { id: 0, text: "source".into(), confidence: 0.95,
            polygon: [[40., 50.], [160., 50.], [160., 74.], [40., 74.]] },
            translations: vec![BlockTranslation { target_language: "en".into(),
                text: Some("A long translation needs more room than its original single line provides, so it should appear in a connected card.".into()), error_code: None }] };
        let (texture, limited) = render_eye(&eye, &[block.clone()], 0.6, false).unwrap();
        assert!(!limited);
        assert_eq!(texture.pixels[(55 * 512 + 50) * 4 + 3], 0);
        assert!(texture.pixels[(100 * 512 + 200) * 4 + 3] >= 153);
        eye.image.width = 100;
        eye.image.height = 80;
        block.source.polygon = [[10., 10.], [90., 10.], [90., 30.], [10., 30.]];
        block.translations[0].text = Some("More text ".repeat(2000));
        let (texture, limited) = render_eye(&eye, &[block], 0.6, false).unwrap();
        assert!(limited);
        assert!(texture.pixels.iter().all(|channel| *channel == 0));
    }

    #[test]
    fn compact_card_avoids_nearby_source_regions_after_the_maximum_card_would_collide() {
        let source = Bounds {
            left: 200,
            top: 140,
            right: 220,
            bottom: 160,
        };
        let occupied = [
            source,
            Bounds {
                left: 100,
                top: 100,
                right: 110,
                bottom: 110,
            },
            Bounds {
                left: 300,
                top: 200,
                right: 310,
                bottom: 210,
            },
        ];

        let card = place_card("VR", source, &occupied, 512, 320, 0.6).unwrap();

        let (bounds, pixels) = card.expect("Short text must fit a compact card");
        assert!(bounds.width() < 160 && bounds.height() < 100);
        assert!(occupied.iter().all(|other| !bounds.intersects(*other)));
        assert_eq!(
            pixels.len(),
            (bounds.width() * bounds.height() * 4) as usize
        );
    }

    #[test]
    fn source_view_renders_recognized_text_without_translations() {
        let eye = EyeCapture {
            image: Texture {
                width: 100,
                height: 80,
                pixels: vec![0; 100 * 80 * 4],
            },
            projection: [-1., 1., -1., 1.],
            eye_to_head: super::super::transform::matrix(0., 0., 0., [0.; 3]),
            head_pose: super::super::transform::matrix(0., 0., 0., [0.; 3]),
        };
        let block = TranslatedBlock {
            source: TextBlock {
                id: 0,
                text: "source text".into(),
                confidence: 0.95,
                polygon: [[10., 10.], [90., 10.], [90., 60.], [10., 60.]],
            },
            translations: vec![],
        };

        let texture = render_eye(&eye, &[block], 0.6, true).unwrap().0;

        assert!(texture
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] > 0));
    }
}
