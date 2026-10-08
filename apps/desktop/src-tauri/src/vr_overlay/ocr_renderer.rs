use super::ocr_geometry::Homography;
use super::{ocr_capture::EyeCapture, renderer::Texture};
use vrcs_core::ocr::TranslatedBlock;

fn display_text(block: &TranslatedBlock, source_view: bool) -> String {
    if !source_view {
        let translated = block
            .translations
            .iter()
            .filter_map(|t| t.text.as_deref())
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !translated.is_empty() {
            return translated;
        }
    }
    block.source.text.clone()
}

fn tile_size(polygon: [[f32; 2]; 4]) -> (u32, u32) {
    let edge = |a: usize, b: usize| {
        ((polygon[a][0] - polygon[b][0]).powi(2) + (polygon[a][1] - polygon[b][1]).powi(2)).sqrt()
    };
    (
        ((edge(0, 1) + edge(2, 3)) * 0.5).round().clamp(1., 2048.) as u32,
        ((edge(1, 2) + edge(3, 0)) * 0.5).round().clamp(1., 2048.) as u32,
    )
}

pub fn render_eye(
    eye: &EyeCapture,
    blocks: &[TranslatedBlock],
    excluded: &[TranslatedBlock],
    opacity: f32,
    source_view: bool,
) -> Result<(Texture, Vec<usize>), String> {
    let (width, height) = (eye.image.width, eye.image.height);
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return Err("Invalid OCR render dimensions".into());
    }
    if blocks.len() + excluded.len() > 256 {
        return Err("OCR render exceeds the block limit".into());
    }
    let mut pixels = vec![0; (width * height * 4) as usize];
    let mut occupied: Vec<Bounds> = blocks
        .iter()
        .chain(excluded)
        .flat_map(|block| block.fragments())
        .map(|fragment| Bounds::from_quad(fragment.polygon, width, height))
        .collect();
    let mut unplaced = Vec::new();
    let mut cards_used = 0;
    let patches: Vec<_> = blocks
        .iter()
        .flat_map(|block| {
            let translated = !source_view
                && block.translations.iter().any(|translation| {
                    translation
                        .text
                        .as_ref()
                        .is_some_and(|text| !text.trim().is_empty())
                });
            let sources = if translated {
                std::slice::from_ref(&block.source)
            } else {
                block.fragments()
            };
            sources
                .iter()
                .map(move |source| (block, source, translated))
        })
        .collect();
    for (block, source, translated) in patches {
        let text = if translated {
            display_text(block, false)
        } else {
            source.text.clone()
        };
        if text.is_empty() {
            continue;
        }
        let polygon = &source.polygon;
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
        let (tile_width, tile_height) = tile_size(*polygon);
        let (background, foreground) = text_colors(&eye.image, *polygon);
        let grouped = translated && block.fragments().len() > 1;
        let masks: Vec<_> = if grouped {
            block
                .fragments()
                .iter()
                .map(|fragment| {
                    let inverse = Homography::from_quad(fragment.polygon)
                        .and_then(Homography::inverse)
                        .ok_or("Invalid OCR text quadrilateral")?;
                    let (background, _) = text_colors(&eye.image, fragment.polygon);
                    Ok((inverse, background))
                })
                .collect::<Result<_, &str>>()?
        } else {
            Vec::new()
        };
        let member_area: u64 = block
            .fragments()
            .iter()
            .map(|fragment| {
                let (w, h) = tile_size(fragment.polygon);
                w as u64 * h as u64
            })
            .sum();
        let compact = !grouped || member_area * 10 >= tile_width as u64 * tile_height as u64 * 6;
        let obstructed = grouped
            && blocks
                .iter()
                .chain(excluded)
                .filter(|other| other.source.id != block.source.id)
                .flat_map(|other| other.fragments())
                .any(|fragment| {
                    bounds.intersects(Bounds::from_quad(fragment.polygon, width, height))
                });
        let tile = if compact && !obstructed {
            super::wrist_renderer::render_text_box_with_colors(
                &text,
                tile_width,
                tile_height,
                if grouped { 0. } else { opacity },
                background,
                foreground,
            )?
        } else {
            None
        };
        let Some(tile) = tile else {
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
            unplaced.push(block.source.id);
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
                let destination = &mut pixels[((y * width + x) * 4) as usize..][..4];
                if grouped {
                    if let Some((_, background)) = masks.iter().find(|(inverse, _)| {
                        inverse
                            .map([x as f32 + 0.5, y as f32 + 0.5])
                            .is_some_and(|[u, v]| {
                                (0.0..1.0).contains(&u) && (0.0..1.0).contains(&v)
                            })
                    }) {
                        let alpha = (opacity.clamp(0., 1.) * 255.).round() as u8;
                        let background =
                            background.map(|channel| (channel as u16 * alpha as u16 / 255) as u8);
                        blend(
                            destination,
                            [background[0], background[1], background[2], alpha],
                        );
                    }
                }
                blend(destination, color);
            }
        }
    }
    Ok((
        Texture {
            pixels,
            width,
            height,
        },
        unplaced,
    ))
}

fn text_colors(image: &Texture, polygon: [[f32; 2]; 4]) -> ([u8; 3], [u8; 3]) {
    let fallback = ([0; 3], [255; 3]);
    if image.pixels.len() != image.width as usize * image.height as usize * 4 {
        return fallback;
    }
    let Some(mapping) = Homography::from_quad(polygon) else {
        return fallback;
    };
    let mut samples = Vec::with_capacity(36);
    // Sample inside the perimeter so neighboring surfaces do not enter the patch color.
    // The median rejects isolated strokes that intersect the text region's edges.
    for step in 0..9 {
        let t = 0.04 + step as f32 * 0.92 / 8.;
        for uv in [[t, 0.04], [t, 0.96], [0.04, t], [0.96, t]] {
            let Some([x, y]) = mapping.map(uv) else {
                continue;
            };
            let x = x.floor().clamp(0., (image.width - 1) as f32) as u32;
            let y = y.floor().clamp(0., (image.height - 1) as f32) as u32;
            let pixel = &image.pixels[((y * image.width + x) * 4) as usize..][..4];
            if pixel[3] != 0 {
                samples.push([pixel[0], pixel[1], pixel[2]]);
            }
        }
    }
    if samples.is_empty() {
        return fallback;
    }
    let background = std::array::from_fn(|channel| {
        samples.sort_unstable_by_key(|color| color[channel]);
        samples[samples.len() / 2][channel]
    });
    let linear = background.map(|channel| {
        let value = channel as f32 / 255.;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    });
    let luminance = linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722;
    // Select the greater contrast ratio between black and white against this background.
    let foreground = if luminance > 0.179 { [0; 3] } else { [255; 3] };
    (background, foreground)
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

pub(super) fn sample(pixels: &[u8], width: u32, height: u32, u: f32, v: f32) -> [u8; 4] {
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

    fn background_fixture(background: [u8; 4]) -> (EyeCapture, TranslatedBlock) {
        (
            EyeCapture {
                image: Texture {
                    width: 160,
                    height: 96,
                    pixels: background.repeat(160 * 96),
                },
                projection: [-1., 1., -1., 1.],
                eye_to_head: super::super::transform::matrix(0., 0., 0., [0.; 3]),
                head_pose: super::super::transform::matrix(0., 0., 0., [0.; 3]),
            },
            TranslatedBlock {
                fragments: vec![],
                source: TextBlock {
                    id: 0,
                    text: "source".into(),
                    confidence: 0.95,
                    polygon: [[10., 10.], [150., 10.], [150., 80.], [10., 80.]],
                },
                translations: vec![BlockTranslation {
                    target_language: "en".into(),
                    text: Some("VR".into()),
                    error_code: None,
                }],
            },
        )
    }

    #[test]
    fn translated_patches_match_light_and_dark_source_backgrounds_with_contrasting_text() {
        for (background, foreground) in [
            ([240, 230, 220, 255], [0, 0, 0, 255]),
            ([20, 30, 40, 255], [255, 255, 255, 255]),
            ([0, 240, 0, 255], [0, 0, 0, 255]),
            ([0, 0, 200, 255], [255, 255, 255, 255]),
        ] {
            let (eye, block) = background_fixture(background);
            let texture = render_eye(&eye, &[block], &[], 1.0, false).unwrap().0;
            assert_eq!(&texture.pixels[(11 * 160 + 11) * 4..][..4], &background);
            assert!(texture.pixels.as_chunks::<4>().0.contains(&foreground));
            assert_eq!(&texture.pixels[..4], &[0, 0, 0, 0]);
            assert!(texture
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[3] != 0)
                .all(|pixel| pixel[3] == 255));
        }
    }

    #[test]
    fn adaptive_patch_uses_premultiplied_opacity_without_fading_text() {
        let (eye, block) = background_fixture([200, 180, 160, 255]);
        let texture = render_eye(&eye, &[block], &[], 0.6, false).unwrap().0;
        assert_eq!(
            &texture.pixels[(11 * 160 + 11) * 4..][..4],
            &[120, 108, 96, 153]
        );
        assert!(texture.pixels.as_chunks::<4>().0.contains(&[0, 0, 0, 255]));
        assert!(texture
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[..3].iter().all(|channel| *channel <= pixel[3])));
    }

    #[test]
    fn adaptive_background_ignores_sparse_source_ink_near_text_edges() {
        let (mut eye, block) = background_fixture([240, 230, 220, 255]);
        for y in 10..80 {
            for x in 75..85 {
                eye.image.pixels[(y * 160 + x) * 4..][..4].copy_from_slice(&[0, 0, 0, 255]);
            }
        }
        let texture = render_eye(&eye, &[block], &[], 1.0, false).unwrap().0;
        assert_eq!(
            &texture.pixels[(11 * 160 + 11) * 4..][..4],
            &[240, 230, 220, 255]
        );
    }

    #[test]
    fn unavailable_source_pixels_keep_the_black_patch_and_white_text_fallback() {
        for pixels in [vec![], vec![1, 2, 3, 255], vec![0; 160 * 96 * 4]] {
            let (mut eye, block) = background_fixture([240, 230, 220, 255]);
            eye.image.pixels = pixels;
            let texture = render_eye(&eye, &[block], &[], 0.6, false).unwrap().0;
            assert_eq!(&texture.pixels[(11 * 160 + 11) * 4..][..4], &[0, 0, 0, 153]);
            assert!(texture.pixels.as_chunks::<4>().0.contains(&[255; 4]));
        }
    }

    #[test]
    fn untranslated_regions_keep_source_visible_while_translations_arrive() {
        let eye = EyeCapture {
            image: Texture {
                width: 160,
                height: 96,
                pixels: vec![],
            },
            projection: [-1., 1., -1., 1.],
            eye_to_head: super::super::transform::matrix(0., 0., 0., [0.; 3]),
            head_pose: super::super::transform::matrix(0., 0., 0., [0.; 3]),
        };
        let block = TranslatedBlock {
            fragments: vec![],
            source: TextBlock {
                id: 0,
                text: "source".into(),
                confidence: 0.95,
                polygon: [[10., 10.], [150., 10.], [150., 80.], [10., 80.]],
            },
            translations: vec![],
        };
        let texture = render_eye(&eye, &[block], &[], 0.4, false).unwrap().0;
        assert!(texture
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] > 0));
        assert_eq!(&texture.pixels[..4], &[0, 0, 0, 0]);
    }

    #[test]
    fn ocr_eye_texture_covers_only_text_regions() {
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
            fragments: vec![],
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
        let texture = render_eye(&eye, std::slice::from_ref(&block), &[], 0.6, false)
            .unwrap()
            .0;
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
            fragments: vec![],
            source: TextBlock {
                text: String::new(),
                ..block.source
            },
            translations: vec![],
        };
        assert!(render_eye(&eye, &[untranslated], &[], 0.6, false)
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
            fragments: vec![],
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
        let texture = render_eye(&eye, &[block], &[], 0.6, false).unwrap().0;
        assert_eq!(texture.pixels[(12 * 100 + 12) * 4 + 3], 0);
        assert!(texture.pixels[(40 * 100 + 50) * 4 + 3] >= 153);
    }

    #[test]
    fn grouped_translation_masks_only_original_lines_and_keeps_gap_transparent() {
        let eye = EyeCapture {
            image: Texture {
                width: 320,
                height: 160,
                pixels: vec![0; 320 * 160 * 4],
            },
            projection: [-1., 1., -1., 1.],
            eye_to_head: super::super::transform::matrix(0., 0., 0., [0.; 3]),
            head_pose: super::super::transform::matrix(0., 0., 0., [0.; 3]),
        };
        let fragments: Vec<_> = [10., 54.]
            .into_iter()
            .enumerate()
            .map(|(id, y)| TextBlock {
                id,
                text: "original line".into(),
                confidence: 0.9,
                polygon: [[10., y], [300., y], [300., y + 30.], [10., y + 30.]],
            })
            .collect();
        let group = TranslatedBlock {
            source: TextBlock {
                id: 0,
                text: "original line original line".into(),
                confidence: 0.9,
                polygon: [[10., 10.], [300., 10.], [300., 84.], [10., 84.]],
            },
            fragments,
            translations: vec![BlockTranslation {
                target_language: "en".into(),
                text: Some("VR".into()),
                error_code: None,
            }],
        };
        let (texture, unplaced) = render_eye(&eye, &[group], &[], 0.6, false).unwrap();
        assert!(unplaced.is_empty());
        assert_eq!(
            texture.pixels[(46 * 320 + 280) * 4 + 3],
            0,
            "blank space is not a source-covering patch"
        );
        assert!(texture.pixels[(20 * 320 + 280) * 4 + 3] >= 153);
        assert!(texture.pixels[(64 * 320 + 280) * 4 + 3] >= 153);
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
        let mut block = TranslatedBlock { fragments: vec![], source: TextBlock { id: 0, text: "source".into(), confidence: 0.95,
            polygon: [[40., 50.], [160., 50.], [160., 74.], [40., 74.]] },
            translations: vec![BlockTranslation { target_language: "en".into(),
                text: Some("A long translation needs more room than its original single line provides, so it should appear in a connected card.".into()), error_code: None }] };
        let (texture, limited) = render_eye(&eye, &[block.clone()], &[], 0.6, false).unwrap();
        assert!(limited.is_empty());
        assert_eq!(texture.pixels[(55 * 512 + 50) * 4 + 3], 0);
        assert!(texture.pixels[(100 * 512 + 200) * 4 + 3] >= 153);
        let mut excluded = block.clone();
        excluded.source.id = 1;
        excluded.source.polygon = [[0., 0.], [512., 0.], [512., 320.], [0., 320.]];
        let (texture, limited) =
            render_eye(&eye, &[block.clone()], &[excluded], 0.6, false).unwrap();
        assert_eq!(
            limited,
            vec![0],
            "unplaced source regions still block translation cards"
        );
        assert!(texture.pixels.iter().all(|channel| *channel == 0));
        eye.image.width = 100;
        eye.image.height = 80;
        block.source.polygon = [[10., 10.], [90., 10.], [90., 30.], [10., 30.]];
        block.translations[0].text = Some("More text ".repeat(2000));
        let (texture, limited) = render_eye(&eye, &[block], &[], 0.6, false).unwrap();
        assert_eq!(limited, vec![0]);
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
            fragments: vec![],
            source: TextBlock {
                id: 0,
                text: "source text".into(),
                confidence: 0.95,
                polygon: [[10., 10.], [90., 10.], [90., 60.], [10., 60.]],
            },
            translations: vec![],
        };

        let texture = render_eye(&eye, &[block], &[], 0.6, true).unwrap().0;

        assert!(texture
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] > 0));
    }
}
