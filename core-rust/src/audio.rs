//! WASAPI 音频采集：系统回环、VRChat 进程回环与麦克风。
//! 替代 Python 版的 PyAudioWPatch 封装与 `vrcs-process-audio` 子进程方案：
//! 设备回环按 WASAPI 原生 PCM/Float 混音格式读取，进程回环请求 16 kHz 单声道 Int16；
//! 统一转换为单声道 f32，并按 512 采样切块后经 channel 交给下游管线。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use tokio::sync::mpsc;

use crate::models::AudioDevice;

/// 每块 512 采样（16 kHz 下 32 ms），与 VAD 的输入窗口一致
#[allow(dead_code)]
pub const CHUNK_FRAMES: usize = 512;
#[allow(dead_code)]
const CHANNEL_CAPACITY: usize = 128;
#[allow(dead_code)]
const START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

#[derive(Debug, Clone)]
pub struct AudioError(pub String);

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AudioError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureSource {
    Speaker,
    Microphone,
}

/// 采集会话：下一阶段由识别管线驱动 start/read/stop。
#[allow(dead_code)]
struct CaptureSession {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    rx: mpsc::Receiver<Vec<f32>>,
}

/// 采集器：下一阶段由识别管线持有并驱动。
#[allow(dead_code)]
pub struct AudioCapture {
    output_rate: u32,
    source: CaptureSource,
    session: Option<CaptureSession>,
    device: Option<AudioDevice>,
}

/// 下一阶段由识别管线调用。
#[allow(dead_code)]
impl AudioCapture {
    pub fn new(output_rate: u32, source: CaptureSource) -> Self {
        Self {
            output_rate,
            source,
            session: None,
            device: None,
        }
    }

    pub fn device(&self) -> Option<&AudioDevice> {
        self.device.as_ref()
    }

    pub fn start(
        &mut self,
        device_id: Option<i64>,
        process_name: Option<&str>,
    ) -> Result<AudioDevice, AudioError> {
        if self.session.is_some() {
            return Err(AudioError("Audio capture is already running".into()));
        }
        if let Some(name) = process_name {
            if self.source != CaptureSource::Speaker {
                return Err(AudioError(
                    "Process loopback is only valid for speaker capture".into(),
                ));
            }
            let pid = platform::find_process_id(name)?
                .ok_or_else(|| AudioError("未发现正在运行的 VRChat，请先启动 VRChat".into()))?;
            return self.start_session(platform::CaptureTarget::Process(pid));
        }
        let direction = match self.source {
            CaptureSource::Speaker => platform::DeviceDirection::Render,
            CaptureSource::Microphone => platform::DeviceDirection::Capture,
        };
        let target = match device_id {
            Some(id) => platform::CaptureTarget::Device {
                wasapi_id: Some(platform::resolve_device_id(id, self.source)?),
                direction,
            },
            None => platform::CaptureTarget::Device {
                wasapi_id: None,
                direction,
            },
        };
        self.start_session(target)
    }

    /// 启动采集线程并等待握手结果（与旧子进程方案的 READY_HEADER 语义一致）。
    fn start_session(
        &mut self,
        target: platform::CaptureTarget,
    ) -> Result<AudioDevice, AudioError> {
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<AudioDevice, String>>();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let output_rate = self.output_rate;
        let join = std::thread::Builder::new()
            .name("vrcs-audio-capture".into())
            .spawn(move || {
                platform::capture_main(target, output_rate, thread_stop, tx, ready_tx);
            })
            .map_err(|e| AudioError(format!("无法启动音频采集线程：{e}")))?;

        let device = match ready_rx.recv_timeout(START_TIMEOUT) {
            Ok(Ok(device)) => device,
            Ok(Err(message)) => {
                stop.store(true, Ordering::Relaxed);
                let _ = join.join();
                return Err(AudioError(message));
            }
            Err(_) => {
                stop.store(true, Ordering::Relaxed);
                let _ = join.join();
                return Err(AudioError("启动音频采集超时".into()));
            }
        };
        self.session = Some(CaptureSession {
            stop,
            join: Some(join),
            rx,
        });
        self.device = Some(device.clone());
        Ok(device)
    }

    /// 读取一块 512 采样的 f32 PCM；采集停止或出错时返回 Err。
    pub async fn read(&mut self) -> Result<Vec<f32>, AudioError> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| AudioError("Audio capture is not running".into()))?;
        session
            .rx
            .recv()
            .await
            .ok_or_else(|| AudioError("音频采集已停止".into()))
    }

    /// 让采集线程退出，使阻塞中的 read 解除（channel 随线程结束关闭）。
    pub fn interrupt(&mut self) {
        if let Some(session) = &self.session {
            session.stop.store(true, Ordering::Relaxed);
        }
    }

    pub fn stop(&mut self) {
        if let Some(mut session) = self.session.take() {
            session.stop.store(true, Ordering::Relaxed);
            if let Some(join) = session.join.take() {
                let _ = join.join();
            }
        }
        self.device = None;
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn list_devices() -> Result<Vec<AudioDevice>, AudioError> {
    platform::list_devices()
}

pub fn validate_device_id(
    device_id: i64,
    source: CaptureSource,
) -> Result<AudioDevice, AudioError> {
    let expected_loopback = source == CaptureSource::Speaker;
    let devices = platform::list_devices()?;
    devices
        .into_iter()
        .find(|item| item.id == device_id && item.is_loopback == expected_loopback)
        .ok_or_else(|| {
            let label = if expected_loopback {
                "系统输出"
            } else {
                "麦克风"
            };
            AudioError(format!("所选{label}设备已失效，请重新选择"))
        })
}

// ---------------------------------------------------------------------------
// Windows 实现
// ---------------------------------------------------------------------------

#[cfg(windows)]
pub(crate) mod platform {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use tokio::sync::mpsc;
    use wasapi::{
        initialize_mta, AudioClient, Device, DeviceEnumerator, Direction, SampleType, StreamMode,
        WaveFormat,
    };

    #[derive(Clone, Copy, Debug)]
    enum SampleEncoding {
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

    /// 线程内实际采集到的格式（设备原生或进程回环的 16 kHz 单声道）。
    #[derive(Clone, Copy, Debug)]
    struct NativeFormat {
        sample_rate: u32,
        channels: u32,
        encoding: SampleEncoding,
    }

    impl NativeFormat {
        fn from_wave_format(format: &WaveFormat) -> Result<Self, AudioError> {
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
                        return Err(AudioError(format!(
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
                    return Err(AudioError(format!(
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

    use super::{AudioDevice, AudioError, CHUNK_FRAMES};

    const BUFFER_DURATION_HNS: i64 = 200_000; // 20 ms
    const EVENT_WAIT_MS: u32 = 200;

    pub(crate) enum CaptureTarget {
        /// 按 PID 的进程回环（VRChat）
        Process(u32),
        /// WASAPI 端点 ID；None 表示该方向的默认设备。
        Device {
            wasapi_id: Option<String>,
            direction: DeviceDirection,
        },
    }

    #[derive(Clone, Copy, PartialEq)]
    pub(crate) enum DeviceDirection {
        Render,
        Capture,
    }

    fn err<E: std::fmt::Display>(e: E) -> AudioError {
        AudioError(e.to_string())
    }

    /// 端点 ID 字符串的稳定散列，限制到 JavaScript 可精确表示的 53 位整数。
    /// （Python 版用 PortAudio 序号，重启后可能变化；散列方案跨重启稳定。）
    fn device_key(wasapi_id: &str) -> i64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in wasapi_id.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        (hash & ((1u64 << 53) - 1)) as i64
    }

    fn endpoint_info(
        device: &Device,
        is_default: bool,
        is_loopback: bool,
    ) -> Result<AudioDevice, AudioError> {
        let wasapi_id = device.get_id().map_err(err)?;
        let format = device.get_device_format().map_err(err)?;
        Ok(AudioDevice {
            id: device_key(&wasapi_id),
            name: device.get_friendlyname().map_err(err)?,
            is_default,
            is_loopback,
            sample_rate: format.get_samplespersec(),
            channels: u32::from(format.get_nchannels()),
        })
    }

    fn init_com() -> Result<(), AudioError> {
        let result = initialize_mta();
        if result.is_ok() {
            Ok(())
        } else {
            Err(AudioError(format!("COM 初始化失败：{result:?}")))
        }
    }

    /// 先列回环设备（对应系统输出），再列麦克风，与 Python 版顺序一致。
    pub(crate) fn list_devices() -> Result<Vec<AudioDevice>, AudioError> {
        init_com()?;
        let enumerator = DeviceEnumerator::new().map_err(err)?;
        let default_render_id = enumerator
            .get_default_device(&Direction::Render)
            .and_then(|d| d.get_id())
            .ok();
        let default_capture_id = enumerator
            .get_default_device(&Direction::Capture)
            .and_then(|d| d.get_id())
            .ok();
        let mut devices = Vec::new();
        for (direction, is_loopback, default_id) in [
            (Direction::Render, true, default_render_id),
            (Direction::Capture, false, default_capture_id),
        ] {
            let collection = enumerator.get_device_collection(&direction).map_err(err)?;
            for index in 0..collection.get_nbr_devices().map_err(err)? {
                let device = collection.get_device_at_index(index).map_err(err)?;
                if device.get_state().map_err(err)? != wasapi::DeviceState::Active {
                    continue;
                }
                let wasapi_id = device.get_id().map_err(err)?;
                let is_default = default_id.as_deref() == Some(wasapi_id.as_str());
                devices.push(endpoint_info(&device, is_default, is_loopback)?);
            }
        }
        Ok(devices)
    }

    /// 按数值 id 找到 WASAPI 端点 ID 字符串；校验设备类型与采集来源一致。
    pub(crate) fn resolve_device_id(
        device_id: i64,
        source: super::CaptureSource,
    ) -> Result<String, AudioError> {
        // validate_device_id 内部已枚举并校验设备类型，这里只为取回端点 ID
        let device = super::validate_device_id(device_id, source)?;
        let direction = match source {
            super::CaptureSource::Speaker => Direction::Render,
            super::CaptureSource::Microphone => Direction::Capture,
        };
        init_com()?;
        let enumerator = DeviceEnumerator::new().map_err(err)?;
        let collection = enumerator.get_device_collection(&direction).map_err(err)?;
        for index in 0..collection.get_nbr_devices().map_err(err)? {
            let device_handle = collection.get_device_at_index(index).map_err(err)?;
            let wasapi_id = device_handle.get_id().map_err(err)?;
            if device_key(&wasapi_id) == device.id {
                return Ok(wasapi_id);
            }
        }
        Err(AudioError("所选音频设备已失效，请重新选择".into()))
    }

    pub(crate) fn find_process_id(process_name: &str) -> Result<Option<u32>, AudioError> {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };

        let target = process_name.to_lowercase();
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
                .map_err(|e| AudioError(format!("无法枚举 Windows 进程：{e}")))?;
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut result = None;
            if Process32FirstW(snapshot, &mut entry).is_ok() {
                loop {
                    let len = entry
                        .szExeFile
                        .iter()
                        .position(|c| *c == 0)
                        .unwrap_or(entry.szExeFile.len());
                    let exe = String::from_utf16_lossy(&entry.szExeFile[..len]);
                    if exe.to_lowercase() == target {
                        result = Some(entry.th32ProcessID);
                        break;
                    }
                    if Process32NextW(snapshot, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snapshot);
            Ok(result)
        }
    }

    fn find_device_by_wasapi_id(
        wasapi_id: &str,
        direction: &Direction,
    ) -> Result<Device, AudioError> {
        init_com()?;
        let enumerator = DeviceEnumerator::new().map_err(err)?;
        // 先按端点 ID 直接取（最可靠），失败时回退到集合遍历
        if let Ok(device) = enumerator.get_device(wasapi_id) {
            return Ok(device);
        }
        let collection = enumerator.get_device_collection(direction).map_err(err)?;
        for index in 0..collection.get_nbr_devices().map_err(err)? {
            let device = collection.get_device_at_index(index).map_err(err)?;
            if device.get_id().map_err(err)? == wasapi_id {
                return Ok(device);
            }
        }
        Err(AudioError("所选音频设备已失效，请重新选择".into()))
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

    /// 设备原生交错采样 → 单声道 f32（多声道取平均）。
    fn append_mono_f32(
        packet: &mut VecDeque<u8>,
        format: NativeFormat,
        pending: &mut VecDeque<f32>,
    ) -> Result<(), AudioError> {
        let channels = format.channels.max(1) as usize;
        let sample_bytes = format.encoding.sample_bytes();
        let frame_bytes = channels * sample_bytes;
        if !packet.len().is_multiple_of(frame_bytes) {
            return Err(AudioError("WASAPI 返回了不完整的音频帧".into()));
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

    /// 线性插值重采样，逐帧移植 Python 版 np.interp 逻辑；同采样率直接透传。
    fn resample_linear(input: &[f32], output_rate: u32, input_rate: u32) -> Vec<f32> {
        if input_rate == output_rate || input.len() <= 1 {
            return input.to_vec();
        }
        let size = ((input.len() as u64 * output_rate as u64 + input_rate as u64 / 2)
            / input_rate as u64) as usize;
        if size == 0 {
            return Vec::new();
        }
        let n = input.len();
        let mut out = Vec::with_capacity(size);
        for i in 0..size {
            // np.interp(x=i/size, xp=j/n, fp=input)
            let pos = (i as f64 / size as f64) * n as f64;
            let j = (pos.floor() as usize).min(n - 1);
            let frac = (pos - j as f64) as f32;
            let y = if j + 1 < n {
                input[j] + frac * (input[j + 1] - input[j])
            } else {
                input[j]
            };
            out.push(y);
        }
        out
    }

    /// 采集线程主体：COM 初始化、客户端创建、握手、事件驱动读取。
    /// 全部 COM 对象都在本线程内创建与销毁，避免跨线程封送。
    pub(crate) fn capture_main(
        target: CaptureTarget,
        output_rate: u32,
        stop: Arc<AtomicBool>,
        tx: mpsc::Sender<Vec<f32>>,
        ready: std::sync::mpsc::Sender<Result<AudioDevice, String>>,
    ) {
        let run = || -> Result<(AudioClient, AudioDevice, NativeFormat), AudioError> {
            init_com()?;
            let (mut client, device, wave_format, native) = match &target {
                CaptureTarget::Process(pid) => {
                    // 进程回环支持 autoconvert（原 vrcs-process-audio 同款方案）：
                    // 直接向引擎请求 16 kHz 单声道 Int16。
                    let client = AudioClient::new_application_loopback_client(*pid, true)
                        .map_err(|e| AudioError(format!("无法连接 VRChat 音频：{e}")))?;
                    let device = AudioDevice {
                        id: -1,
                        name: "VRChat（仅应用音频）".into(),
                        is_default: false,
                        is_loopback: true,
                        sample_rate: output_rate,
                        channels: 1,
                    };
                    let format =
                        WaveFormat::new(16, 16, &SampleType::Int, output_rate as usize, 1, None);
                    (
                        client,
                        device,
                        format,
                        NativeFormat {
                            sample_rate: output_rate,
                            channels: 1,
                            encoding: SampleEncoding::SignedInt {
                                container_bytes: 2,
                                valid_bits: 16,
                            },
                        },
                    )
                }
                CaptureTarget::Device {
                    wasapi_id,
                    direction,
                } => {
                    let wasapi_direction = match direction {
                        DeviceDirection::Render => Direction::Render,
                        DeviceDirection::Capture => Direction::Capture,
                    };
                    let device = match wasapi_id {
                        Some(id) => find_device_by_wasapi_id(id, &wasapi_direction)?,
                        None => {
                            let enumerator = DeviceEnumerator::new().map_err(err)?;
                            enumerator
                                .get_default_device(&wasapi_direction)
                                .map_err(err)?
                        }
                    };
                    // 共享模式回环不支持 autoconvert：按设备混音格式打开，
                    // 采样率/声道转换在采集线程内完成（对齐 Python 版 capture.py）。
                    let wave_format = device.get_device_format().map_err(err)?;
                    let native = NativeFormat::from_wave_format(&wave_format)?;
                    let info = endpoint_info(
                        &device,
                        wasapi_id.is_none(),
                        *direction == DeviceDirection::Render,
                    )?;
                    (
                        device.get_iaudioclient().map_err(err)?,
                        info,
                        wave_format,
                        native,
                    )
                }
            };
            let autoconvert = matches!(target, CaptureTarget::Process(_));
            let mode = StreamMode::EventsShared {
                autoconvert,
                buffer_duration_hns: BUFFER_DURATION_HNS,
            };
            client
                .initialize_client(&wave_format, &Direction::Capture, &mode)
                .map_err(err)?;
            Ok((client, device, native))
        };

        let (client, device, native) = match run() {
            Ok(ok) => ok,
            Err(e) => {
                let _ = ready.send(Err(e.0));
                return;
            }
        };

        let stream = (|| -> Result<(), AudioError> {
            let event = client.set_get_eventhandle().map_err(err)?;
            let capture = client.get_audiocaptureclient().map_err(err)?;
            client.start_stream().map_err(err)?;
            let _ = ready.send(Ok(device));

            // 与 Python 版一致：每帧输入样本数 = round(输入采样率 * 512 / 输出采样率)
            let frames_per_chunk = ((native.sample_rate as u64 * CHUNK_FRAMES as u64
                + output_rate as u64 / 2)
                / output_rate as u64)
                .max(1) as usize;
            let mut pending: VecDeque<f32> = VecDeque::new(); // 单声道、输入采样率
            let mut packet: VecDeque<u8> = VecDeque::new();
            while !stop.load(Ordering::Relaxed) {
                // 等待事件超时也要醒一次，检查停止标志
                let _ = event.wait_for_event(EVENT_WAIT_MS);
                // GetNextPacketSize 返回 0 表示没有排队包（wasapi 映射为 Some(0)）
                while let Some(size) = capture.get_next_packet_size().map_err(err)? {
                    if size == 0 {
                        break;
                    }
                    packet.clear();
                    capture
                        .read_from_device_to_deque(&mut packet)
                        .map_err(err)?;
                    append_mono_f32(&mut packet, native, &mut pending)?;
                    while pending.len() >= frames_per_chunk {
                        let frame: Vec<f32> = pending.drain(..frames_per_chunk).collect();
                        let chunk = resample_linear(&frame, output_rate, native.sample_rate);
                        if chunk.is_empty() {
                            continue;
                        }
                        // 下游消费不过来时丢块，保持实时性
                        let _ = tx.try_send(chunk);
                    }
                }
            }
            Ok(())
        })();
        let _ = client.stop_stream();
        if let Err(e) = stream {
            tracing::warn!("audio capture stopped with error: {}", e.0);
        }
    }

    #[cfg(test)]
    mod sample_tests {
        use super::*;

        #[test]
        fn device_keys_are_javascript_safe() {
            let key = device_key("render-device-that-produces-a-large-fnv-hash");
            assert!(
                (0..=9_007_199_254_740_991).contains(&key),
                "device id must round-trip through a JavaScript number"
            );
        }

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
}

// ---------------------------------------------------------------------------
// 非 Windows 占位实现
// ---------------------------------------------------------------------------

#[cfg(not(windows))]
pub(crate) mod platform {
    use super::{AudioDevice, AudioError};

    pub(crate) enum CaptureTarget {
        Process(u32),
        Device {
            wasapi_id: Option<String>,
            direction: DeviceDirection,
        },
    }

    #[derive(Clone, Copy, PartialEq)]
    pub(crate) enum DeviceDirection {
        Render,
        Capture,
    }

    pub(crate) fn list_devices() -> Result<Vec<AudioDevice>, AudioError> {
        Err(AudioError("音频采集仅支持 Windows".into()))
    }

    pub(crate) fn resolve_device_id(
        _id: i64,
        _source: super::CaptureSource,
    ) -> Result<String, AudioError> {
        Err(AudioError("音频采集仅支持 Windows".into()))
    }

    pub(crate) fn find_process_id(_name: &str) -> Result<Option<u32>, AudioError> {
        Err(AudioError("音频采集仅支持 Windows".into()))
    }

    pub(crate) fn capture_main(
        _target: CaptureTarget,
        _rate: u32,
        _stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        _tx: tokio::sync::mpsc::Sender<Vec<f32>>,
        ready: std::sync::mpsc::Sender<Result<AudioDevice, String>>,
    ) {
        let _ = ready.send(Err("音频采集仅支持 Windows".into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn lists_loopback_and_microphone_devices() {
        let devices = list_devices().unwrap();
        // 开发机上至少应有一个回环设备；CI 无音频设备时列表为空也算通过
        for device in &devices {
            assert!(!device.name.is_empty());
            assert!(device.sample_rate > 0);
        }
        let ids: std::collections::HashSet<i64> = devices.iter().map(|d| d.id).collect();
        assert_eq!(ids.len(), devices.len(), "设备 id 应唯一");
    }

    /// 需要真实音频设备，默认忽略；手动运行：cargo test -- --ignored
    #[cfg(windows)]
    #[tokio::test]
    #[ignore]
    async fn captures_default_loopback_chunks() {
        let mut capture = AudioCapture::new(16_000, CaptureSource::Speaker);
        let device = capture.start(None, None).unwrap();
        assert!(device.is_loopback);
        for _ in 0..3 {
            let chunk = capture.read().await.unwrap();
            assert_eq!(chunk.len(), CHUNK_FRAMES);
        }
        capture.stop();
        assert!(capture.device().is_none());
    }
}
