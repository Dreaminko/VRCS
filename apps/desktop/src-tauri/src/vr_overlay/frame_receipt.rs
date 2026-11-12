//! Metadata shared by CPU snapshots and GPU captures, independent of texture ownership.
pub(crate) fn serial_at_least(value: u32, previous: u32) -> bool {
    value.wrapping_sub(previous) < 0x8000_0000
}

#[cfg(any(windows, linux_vr))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameReceipt {
    pub generation: u32,
    pub revision: u32,
    pub paint_serial: u32,
    pub requested_serial: u32,
    pub bounds: [u32; 4],
}

/// React paints 128 bits in two 64-cell rows. Validate the serial complement
/// before accepting either browser pixels or a corresponding hit map.
pub(crate) fn decode_marker_pixels(
    mut pixel: impl FnMut(usize) -> Option<[u8; 4]>,
) -> Option<(u32, u32, u32)> {
    let mut words = [0u32; 4];
    for bit in 0..128 {
        let pixel = pixel(bit)?;
        let one = pixel[..3].iter().all(|&channel| channel >= 224);
        if pixel[3] < 224 || (!one && !pixel[..3].iter().all(|&channel| channel <= 31)) {
            return None;
        }
        words[bit / 32] = (words[bit / 32] << 1) | u32::from(one);
    }
    (words[0] != 0 && words[1] != 0 && words[2] != 0 && words[3] == !words[2])
        .then_some((words[0], words[1], words[2]))
}

#[cfg(any(windows, linux_vr))]
pub(crate) fn decode_marker(rows: &[u8], row_pitch: usize) -> Option<(u32, u32, u32)> {
    decode_marker_pixels(|bit| {
        let offset = (bit / 64) * row_pitch + ((bit % 64) * 4 + 2) * 4;
        rows.get(offset..offset + 4)?.try_into().ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_mismatching_or_translucent_same_frame_marker() {
        let words = [1u32, 7, 42, !42];
        let mut pixels: Vec<[u8; 4]> = (0..128)
            .map(|bit| {
                let value = if (words[bit / 32] >> (31 - bit % 32)) & 1 != 0 {
                    255
                } else {
                    0
                };
                [value, value, value, 255]
            })
            .collect();
        assert_eq!(
            decode_marker_pixels(|bit| Some(pixels[bit])),
            Some((1, 7, 42))
        );
        pixels[127][0] = 128;
        assert_eq!(decode_marker_pixels(|bit| Some(pixels[bit])), None);
        pixels[127] = [255, 255, 255, 0];
        assert_eq!(decode_marker_pixels(|bit| Some(pixels[bit])), None);
    }
}
