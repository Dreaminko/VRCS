use std::collections::VecDeque;

use ::wasapi::{SampleType, WaveFormat};

use super::super::AudioError;
use super::devices::err;

#[derive(Clone, Copy, Debug)]
pub(super) enum SampleEncoding {
    SignedInt {
        container_bytes: usize,
        valid_bits: u16,
    },
    Float32,
    Float64,
}

impl SampleEncoding {
    fn sample_bytes(self) -> usize {
        match self {
            Self::SignedInt {
                container_bytes, ..
            } => container_bytes,
            Self::Float32 => 4,
            Self::Float64 => 8,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct NativeFormat {
    pub(super) sample_rate: u32,
    pub(super) channels: u32,
    pub(super) encoding: SampleEncoding,
}

impl NativeFormat {
    pub(super) fn from_wave_format(format: &WaveFormat) -> Result<Self, AudioError> {
        let bits = format.get_bitspersample();
        let encoding = match format.get_subformat().map_err(err)? {
            SampleType::Int if matches!(bits, 16 | 24 | 32) => {
                let reported_valid_bits = format.get_validbitspersample();
                let valid_bits = if reported_valid_bits == 0 {
                    bits
                } else {
                    reported_valid_bits
                };
                if valid_bits == 0 || valid_bits > bits {
                    return Err(AudioError::new(format!(
                        "无效的 PCM 位深：容器 {bits} bit，有效 {valid_bits} bit"
                    )));
                }
                SampleEncoding::SignedInt {
                    container_bytes: usize::from(bits / 8),
                    valid_bits,
                }
            }
            SampleType::Float if bits == 32 => SampleEncoding::Float32,
            SampleType::Float if bits == 64 => SampleEncoding::Float64,
            sample_type => {
                return Err(AudioError::new(format!(
                    "暂不支持该设备格式（{sample_type:?} {bits} bit），请选择其他输出设备"
                )));
            }
        };
        Ok(Self {
            sample_rate: format.get_samplespersec(),
            channels: u32::from(format.get_nchannels()),
            encoding,
        })
    }
}

fn decode_sample(bytes: &[u8], encoding: SampleEncoding) -> f32 {
    match encoding {
        SampleEncoding::SignedInt {
            container_bytes,
            valid_bits,
        } => {
            let value = match container_bytes {
                2 => i64::from(i16::from_le_bytes([bytes[0], bytes[1]])),
                3 => {
                    let value = i32::from(bytes[0])
                        | (i32::from(bytes[1]) << 8)
                        | (i32::from(bytes[2]) << 16);
                    i64::from(if value & 0x80_0000 != 0 {
                        value | !0xFF_FFFF
                    } else {
                        value
                    })
                }
                4 => i64::from(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
                _ => unreachable!("validated integer container size"),
            };
            let container_bits = (container_bytes * 8) as u16;
            let value = value >> (container_bits - valid_bits);
            let scale = (1u64 << (valid_bits - 1)) as f64;
            (value as f64 / scale) as f32
        }
        SampleEncoding::Float32 => f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        SampleEncoding::Float64 => f64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]) as f32,
    }
}

pub(super) fn append_mono_f32(
    packet: &mut VecDeque<u8>,
    format: NativeFormat,
    pending: &mut VecDeque<f32>,
) -> Result<(), AudioError> {
    let channels = format.channels.max(1) as usize;
    let sample_bytes = format.encoding.sample_bytes();
    let frame_bytes = channels * sample_bytes;
    if !packet.len().is_multiple_of(frame_bytes) {
        return Err(AudioError::new("WASAPI 返回了不完整的音频帧"));
    }
    for frame in packet.make_contiguous().chunks_exact(frame_bytes) {
        let sum = frame
            .chunks_exact(sample_bytes)
            .map(|sample| f64::from(decode_sample(sample, format.encoding)))
            .sum::<f64>();
        pending.push_back((sum / channels as f64) as f32);
    }
    Ok(())
}

pub(super) fn resample_linear(input: &[f32], output_rate: u32, input_rate: u32) -> Vec<f32> {
    if input_rate == output_rate || input.len() <= 1 {
        return input.to_vec();
    }
    let size = ((input.len() as u64 * output_rate as u64 + input_rate as u64 / 2)
        / input_rate as u64) as usize;
    if size == 0 {
        return Vec::new();
    }
    let input_len = input.len();
    let mut output = Vec::with_capacity(size);
    for index in 0..size {
        let position = (index as f64 / size as f64) * input_len as f64;
        let left = (position.floor() as usize).min(input_len - 1);
        let fraction = (position - left as f64) as f32;
        let value = if left + 1 < input_len {
            input[left] + fraction * (input[left + 1] - input[left])
        } else {
            input[left]
        };
        output.push(value);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_packet(samples: &[u8], channels: u32, encoding: SampleEncoding) -> Vec<f32> {
        let mut packet = VecDeque::from(samples.to_vec());
        let mut output = VecDeque::new();
        append_mono_f32(
            &mut packet,
            NativeFormat {
                sample_rate: 48_000,
                channels,
                encoding,
            },
            &mut output,
        )
        .unwrap();
        output.into()
    }

    #[test]
    fn decodes_signed_32_bit_pcm() {
        let mut samples = Vec::new();
        samples.extend(i32::MAX.to_le_bytes());
        samples.extend(i32::MIN.to_le_bytes());
        let decoded = decode_packet(
            &samples,
            1,
            SampleEncoding::SignedInt {
                container_bytes: 4,
                valid_bits: 32,
            },
        );
        assert!((decoded[0] - 1.0).abs() < 1e-6);
        assert_eq!(decoded[1], -1.0);
    }

    #[test]
    fn decodes_left_aligned_24_bit_pcm_in_32_bit_container() {
        let sample = (0x40_0000_i32 << 8).to_le_bytes();
        let decoded = decode_packet(
            &sample,
            1,
            SampleEncoding::SignedInt {
                container_bytes: 4,
                valid_bits: 24,
            },
        );
        assert!((decoded[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn averages_interleaved_float32_channels() {
        let mut samples = Vec::new();
        samples.extend(0.25_f32.to_le_bytes());
        samples.extend(0.75_f32.to_le_bytes());
        let decoded = decode_packet(&samples, 2, SampleEncoding::Float32);
        assert!((decoded[0] - 0.5).abs() < 1e-6);
    }
}
