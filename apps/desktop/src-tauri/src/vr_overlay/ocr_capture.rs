use super::renderer::Texture;
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::time::Instant;

#[repr(C)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}
#[repr(C)]
struct StartupInput {
    version: u32,
    debug_callback: *const c_void,
    suppress_thread: i32,
    suppress_codecs: i32,
}

#[link(name = "gdiplus")]
unsafe extern "system" {
    fn GdiplusStartup(token: *mut usize, input: *const StartupInput, output: *mut c_void) -> u32;
    fn GdiplusShutdown(token: usize);
    fn GdipCreateBitmapFromScan0(
        width: i32,
        height: i32,
        stride: i32,
        format: i32,
        pixels: *mut u8,
        bitmap: *mut *mut c_void,
    ) -> u32;
    fn GdipSaveImageToStream(
        image: *mut c_void,
        stream: *mut c_void,
        encoder: *const Guid,
        parameters: *const c_void,
    ) -> u32;
    fn GdipDisposeImage(image: *mut c_void) -> u32;
}
#[link(name = "ole32")]
unsafe extern "system" {
    fn CreateStreamOnHGlobal(
        memory: *mut c_void,
        delete_on_release: i32,
        stream: *mut *mut c_void,
    ) -> i32;
    fn GetHGlobalFromStream(stream: *mut c_void, memory: *mut *mut c_void) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GlobalLock(memory: *mut c_void) -> *mut c_void;
    fn GlobalUnlock(memory: *mut c_void) -> i32;
    fn GlobalSize(memory: *mut c_void) -> usize;
}

unsafe fn release_stream(stream: *mut c_void) {
    if !stream.is_null() {
        let table = unsafe { *(stream as *const *const *const c_void) };
        let release: unsafe extern "system" fn(*mut c_void) -> u32 =
            unsafe { std::mem::transmute(*table.add(2)) };
        unsafe {
            release(stream);
        }
    }
}

#[derive(Clone)]
pub struct EyeCapture {
    pub image: Texture,
    pub projection: [f32; 4],
    pub eye_to_head: [[f32; 4]; 3],
    /// Sampled near mirror readback; OpenVR does not guarantee a synchronized frame.
    pub head_pose: [[f32; 4]; 3],
}

pub const OCR_MOVEMENT_LIMIT_M: f32 = 0.10;

pub fn pose_within_translation_limit_m(
    before: [[f32; 4]; 3],
    after: [[f32; 4]; 3],
    limit_m: f32,
) -> bool {
    if !limit_m.is_finite() || limit_m < 0. {
        return false;
    }
    before
        .iter()
        .flatten()
        .chain(after.iter().flatten())
        .all(|v| v.is_finite())
        && (0..3)
            .map(|r| (before[r][3] - after[r][3]).powi(2))
            .sum::<f32>()
            <= limit_m.powi(2)
}

#[derive(Clone)]
pub struct StereoCapture {
    pub eyes: [EyeCapture; 2],
    pub pose: [[f32; 4]; 3],
    pub scene_pid: u32,
    pub captured_at: Instant,
    pub origin: i32,
}

pub struct CaptureCrop {
    pub image: Texture,
    pub offset: [f32; 2],
    pub scale: [f32; 2],
}
impl CaptureCrop {
    pub fn restore(&self, polygon: &mut [[f32; 2]; 4]) {
        for [x, y] in polygon {
            *x = *x * self.scale[0] + self.offset[0];
            *y = *y * self.scale[1] + self.offset[1];
        }
    }
}

pub fn center_crop(image: &Texture, fraction: f32) -> Result<CaptureCrop, String> {
    if !fraction.is_finite()
        || !(0.1..=1.0).contains(&fraction)
        || image.width == 0
        || image.height == 0
        || image.width > 4096
        || image.height > 4096
        || image.pixels.len() != image.width as usize * image.height as usize * 4
    {
        return Err("Invalid OCR crop".into());
    }
    let width = ((image.width as f32 * fraction).round() as u32).clamp(1, image.width);
    let height = ((image.height as f32 * fraction).round() as u32).clamp(1, image.height);
    let x = (image.width - width) / 2;
    let y = (image.height - height) / 2;
    let factor = (1536.0 / width.max(height) as f32).min(1.0);
    let output_width = ((width as f32 * factor).round() as u32).max(1);
    let output_height = ((height as f32 * factor).round() as u32).max(1);
    let mut pixels = Vec::with_capacity(output_width as usize * output_height as usize * 4);
    let scale_x = width as f64 / output_width as f64;
    let scale_y = height as f64 / output_height as f64;
    for row in 0..output_height {
        if output_width == width && output_height == height {
            let offset = ((y + row) as usize * image.width as usize + x as usize) * 4;
            pixels.extend_from_slice(&image.pixels[offset..offset + width as usize * 4]);
            continue;
        }
        let top = y as f64 + row as f64 * scale_y;
        let bottom = y as f64 + (row + 1) as f64 * scale_y;
        for column in 0..output_width {
            let left = x as f64 + column as f64 * scale_x;
            let right = x as f64 + (column + 1) as f64 * scale_x;
            let mut sum = [0.0; 4];
            // Average the source area so downsampling retains thin text strokes.
            for source_y in top.floor() as u32..(bottom.ceil() as u32).min(y + height) {
                let weight_y = bottom.min(source_y as f64 + 1.0) - top.max(source_y as f64);
                for source_x in left.floor() as u32..(right.ceil() as u32).min(x + width) {
                    let weight_x = right.min(source_x as f64 + 1.0) - left.max(source_x as f64);
                    let offset = (source_y as usize * image.width as usize + source_x as usize) * 4;
                    for (channel, total) in sum.iter_mut().enumerate() {
                        *total += image.pixels[offset + channel] as f64 * weight_x * weight_y;
                    }
                }
            }
            pixels.extend(sum.map(|total| (total / (scale_x * scale_y)).round() as u8));
        }
    }
    Ok(CaptureCrop {
        image: Texture {
            width: output_width,
            height: output_height,
            pixels,
        },
        offset: [x as f32, y as f32],
        scale: [
            width as f32 / output_width as f32,
            height as f32 / output_height as f32,
        ],
    })
}

pub fn encode_png(image: &Texture) -> Result<Vec<u8>, String> {
    let expected = image
        .width
        .checked_mul(image.height)
        .and_then(|size| size.checked_mul(4));
    if image.width == 0
        || image.height == 0
        || image.width > 4096
        || image.height > 4096
        || expected.map(|size| size as usize) != Some(image.pixels.len())
    {
        return Err("Invalid OCR image".into());
    }
    let mut pixels = image.pixels.clone();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let input = StartupInput {
        version: 1,
        debug_callback: null(),
        suppress_thread: 0,
        suppress_codecs: 0,
    };
    let png_encoder = Guid {
        data1: 0x557cf406,
        data2: 0x1a04,
        data3: 0x11d3,
        data4: [0x9a, 0x73, 0, 0, 0xf8, 0x1e, 0xf3, 0x2e],
    };
    unsafe {
        let mut token = 0;
        if GdiplusStartup(&mut token, &input, null_mut()) != 0 {
            return Err("Could not start image encoder".into());
        }
        let mut bitmap = null_mut();
        let mut stream = null_mut();
        let result = (|| {
            if GdipCreateBitmapFromScan0(
                image.width as i32,
                image.height as i32,
                (image.width * 4) as i32,
                0x0026200a,
                pixels.as_mut_ptr(),
                &mut bitmap,
            ) != 0
            {
                return Err("Could not create OCR bitmap".into());
            }
            if CreateStreamOnHGlobal(null_mut(), 1, &mut stream) < 0 {
                return Err("Could not create image stream".into());
            }
            if GdipSaveImageToStream(bitmap, stream, &png_encoder, null()) != 0 {
                return Err("Could not encode OCR PNG".into());
            }
            type Seek = unsafe extern "system" fn(*mut c_void, i64, u32, *mut u64) -> i32;
            let table = *(stream as *const *const *const c_void);
            let seek: Seek = std::mem::transmute(*table.add(5));
            let mut length = 0;
            if seek(stream, 0, 2, &mut length) < 0 || length == 0 || length > 8 * 1024 * 1024 {
                return Err("Encoded OCR image exceeds size limit".into());
            }
            let mut memory = null_mut();
            if GetHGlobalFromStream(stream, &mut memory) < 0 || GlobalSize(memory) < length as usize
            {
                return Err("Invalid encoded image buffer".into());
            }
            let data = GlobalLock(memory);
            if data.is_null() {
                return Err("Could not read encoded image buffer".into());
            }
            let bytes = std::slice::from_raw_parts(data.cast::<u8>(), length as usize).to_vec();
            GlobalUnlock(memory);
            Ok(bytes)
        })();
        if !bitmap.is_null() {
            GdipDisposeImage(bitmap);
        }
        release_stream(stream);
        GdiplusShutdown(token);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_head_rotation_and_small_translation_are_allowed() {
        let before = super::super::transform::matrix(0., 0., 0., [0.; 3]);
        let rotated = super::super::transform::matrix(0., 20., 0., [0.01, 0., 0.]);
        assert!(pose_within_translation_limit_m(before, rotated, 0.03));
        assert!(pose_within_translation_limit_m(
            before,
            super::super::transform::matrix(0., 0., 0., [0.02, 0., 0.]),
            0.03,
        ));
        assert!(!pose_within_translation_limit_m(
            before,
            super::super::transform::matrix(0., 0., 0., [0.04, 0., 0.]),
            0.03,
        ));
        let mut invalid = before;
        invalid[0][0] = f32::NAN;
        assert!(!pose_within_translation_limit_m(before, invalid, 0.03));
    }

    #[test]
    fn ocr_center_crop_and_coordinate_restore_keep_original_eye_positions() {
        let image = Texture {
            width: 4,
            height: 4,
            pixels: (0..16).flat_map(|value| [value, 0, 0, 255]).collect(),
        };
        let crop = center_crop(&image, 0.5).unwrap();
        assert_eq!((crop.image.width, crop.image.height), (2, 2));
        assert_eq!(
            crop.image.pixels,
            vec![5, 0, 0, 255, 6, 0, 0, 255, 9, 0, 0, 255, 10, 0, 0, 255]
        );
        let mut polygon = [[0., 0.], [2., 0.], [2., 2.], [0., 2.]];
        crop.restore(&mut polygon);
        assert_eq!(polygon, [[1., 1.], [3., 1.], [3., 3.], [1., 3.]]);
        let image = Texture {
            width: 3072,
            height: 2,
            pixels: vec![255; 3072 * 2 * 4],
        };
        let crop = center_crop(&image, 1.0).unwrap();
        assert_eq!((crop.image.width, crop.image.height), (1536, 1));
        let mut polygon = [[0., 0.], [10., 0.], [10., 1.], [0., 1.]];
        crop.restore(&mut polygon);
        assert_eq!(polygon, [[0., 0.], [20., 0.], [20., 2.], [0., 2.]]);
    }

    #[test]
    fn ocr_crop_area_sampling_preserves_a_thin_stroke_at_two_to_one_scale() {
        let mut pixels = [255_u8, 255, 255, 255].repeat(3072 * 2);
        for row in 0..2 {
            pixels[(row * 3072 + 1) * 4..(row * 3072 + 2) * 4].copy_from_slice(&[0, 0, 0, 255]);
        }
        let crop = center_crop(
            &Texture {
                width: 3072,
                height: 2,
                pixels,
            },
            1.0,
        )
        .unwrap();

        assert_eq!((crop.image.width, crop.image.height), (1536, 1));
        assert_eq!(&crop.image.pixels[..4], &[128, 128, 128, 255]);
    }

    #[test]
    fn ocr_crop_area_sampling_preserves_a_thin_stroke_at_noninteger_scale() {
        let mut pixels = [255_u8, 255, 255, 255].repeat(3073);
        pixels[4..8].copy_from_slice(&[0, 0, 0, 255]);
        let crop = center_crop(
            &Texture {
                width: 3073,
                height: 1,
                pixels,
            },
            1.0,
        )
        .unwrap();

        assert_eq!((crop.image.width, crop.image.height), (1536, 1));
        assert_eq!(&crop.image.pixels[..4], &[128, 128, 128, 255]);
        assert_eq!(&crop.image.pixels[(1535 * 4)..], &[255, 255, 255, 255]);
    }

    #[test]
    fn ocr_crop_restore_maps_output_edges_to_fractional_crop_bounds() {
        let image = Texture {
            width: 4095,
            height: 3073,
            pixels: vec![255; 4095 * 3073 * 4],
        };
        let crop = center_crop(&image, 0.75).unwrap();
        assert_eq!((crop.image.width, crop.image.height), (1536, 1153));
        let mut polygon = [
            [0., 0.],
            [crop.image.width as f32, 0.],
            [crop.image.width as f32, crop.image.height as f32],
            [0., crop.image.height as f32],
        ];
        crop.restore(&mut polygon);
        assert_eq!(
            polygon,
            [[512., 384.], [3583., 384.], [3583., 2689.], [512., 2689.]]
        );
    }
    #[link(name = "shlwapi")]
    unsafe extern "system" {
        fn SHCreateMemStream(bytes: *const u8, length: u32) -> *mut c_void;
    }
    #[link(name = "gdiplus")]
    unsafe extern "system" {
        fn GdipLoadImageFromStream(stream: *mut c_void, image: *mut *mut c_void) -> u32;
        fn GdipBitmapGetPixel(image: *mut c_void, x: i32, y: i32, color: *mut u32) -> u32;
    }

    #[test]
    fn ocr_png_encoding_produces_a_standard_image_with_the_capture_dimensions() {
        let image = Texture {
            width: 3,
            height: 2,
            pixels: [10, 20, 30, 255].repeat(6),
        };
        let png = encode_png(&image).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 3);
        assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 2);
        unsafe {
            let input = StartupInput {
                version: 1,
                debug_callback: null(),
                suppress_thread: 0,
                suppress_codecs: 0,
            };
            let mut token = 0;
            assert_eq!(GdiplusStartup(&mut token, &input, null_mut()), 0);
            let stream = SHCreateMemStream(png.as_ptr(), png.len() as u32);
            assert!(!stream.is_null());
            let mut bitmap = null_mut();
            assert_eq!(GdipLoadImageFromStream(stream, &mut bitmap), 0);
            let mut color = 0;
            assert_eq!(GdipBitmapGetPixel(bitmap, 1, 1, &mut color), 0);
            assert_eq!(color, 0xff0a141e);
            GdipDisposeImage(bitmap);
            release_stream(stream);
            GdiplusShutdown(token);
        }
    }
}
