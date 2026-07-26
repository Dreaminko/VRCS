#[cfg(windows)]
mod windows_capture {
    use std::collections::VecDeque;
    use std::error::Error;
    use std::io::{self, Write};

    use wasapi::{initialize_mta, AudioClient, Direction, SampleType, StreamMode, WaveFormat};

    const SAMPLE_RATE: usize = 16_000;
    const CHANNELS: usize = 1;
    const BUFFER_DURATION_HNS: i64 = 200_000;
    const READY_HEADER: &[u8; 4] = b"VRCS";

    pub fn run() -> Result<(), Box<dyn Error>> {
        let pid = std::env::args()
            .nth(1)
            .ok_or("usage: vrcs-process-audio <process-id>")?
            .parse::<u32>()?;

        initialize_mta().ok()?;
        let format = WaveFormat::new(16, 16, &SampleType::Int, SAMPLE_RATE, CHANNELS, None);
        let mut audio_client = AudioClient::new_application_loopback_client(pid, true)?;
        let stream_mode = StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: BUFFER_DURATION_HNS,
        };
        audio_client.initialize_client(&format, &Direction::Capture, &stream_mode)?;

        let event = audio_client.set_get_eventhandle()?;
        let capture_client = audio_client.get_audiocaptureclient()?;
        audio_client.start_stream()?;

        let stdout = io::stdout();
        let mut output = stdout.lock();
        output.write_all(READY_HEADER)?;
        output.flush()?;

        let capture_result = stream_audio(&event, &capture_client, &mut output);
        let _ = audio_client.stop_stream();
        capture_result
    }

    fn stream_audio(
        event: &wasapi::Handle,
        capture_client: &wasapi::AudioCaptureClient,
        output: &mut impl Write,
    ) -> Result<(), Box<dyn Error>> {
        let mut data = VecDeque::new();
        loop {
            if event.wait_for_event(1_000).is_err() {
                continue;
            }
            while capture_client.get_next_packet_size()?.is_some() {
                data.clear();
                capture_client.read_from_device_to_deque(&mut data)?;
                let (first, second) = data.as_slices();
                output.write_all(first)?;
                output.write_all(second)?;
            }
        }
    }
}

#[cfg(windows)]
fn main() {
    if let Err(error) = windows_capture::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("VRChat process audio capture is only available on Windows");
    std::process::exit(1);
}
