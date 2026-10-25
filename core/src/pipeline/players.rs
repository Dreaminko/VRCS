use super::*;
use crate::player_audio::{AudioEvent, Capture, SharedStatus, Status};
use std::collections::HashMap;

impl TranscriptionPipeline {
    pub(crate) async fn start_players(
        &mut self,
        config: &crate::config::PlayerAudioConfig,
        threshold: f32,
        vad: &VadConfig,
        asr: AsrConfig,
        echo: AsrEchoGuard,
        dependencies: PipelineDependencies,
    ) -> Result<AudioDevice, AudioError> {
        if self.running() {
            return Err(AudioError::with_code(
                "capture.already_running",
                "Transcription is already running",
            ));
        }
        self.task.take();
        self.stop.take();
        self.device = None;
        let status = Arc::new(Mutex::new(Status::default()));
        self.player_status = Some(Arc::clone(&status));
        *self.last_error.lock().expect("pipeline error") = None;
        let smart_turn = if vad.endpointing == "smart_turn"
            && asr.backend != crate::providers::SERVICE_FUN_ASR_REALTIME
            && !crate::providers::is_live_translation(&asr.backend)
        {
            Some(self.smart_turn_runtime.clone())
        } else {
            None
        };
        let options = config.clone();
        let settings = dependencies.player_settings();
        let shared = Arc::clone(&status);
        #[cfg(windows)]
        let cache = Arc::clone(&self.player_cache);
        let capture = tokio::task::spawn_blocking(move || {
            Capture::start(
                options,
                shared,
                #[cfg(windows)]
                cache,
                settings,
            )
        })
        .await
        .map_err(|e| AudioError::with_code("audio.player_audio_unavailable", e.to_string()))?
        .map_err(|e| {
            let mut state = status.lock().expect("player status");
            state.phase = "failed";
            state.detail = Some(e.clone());
            AudioError::with_code("audio.player_audio_unavailable", e)
        })?;
        let (stop, stop_rx) = watch::channel(false);
        self.stop = Some(stop);
        self.discard_on_stop.store(false, Ordering::SeqCst);
        let input = PlayerRun {
            vad: vad.clone(),
            asr,
            echo,
            dependencies,
            threshold,
            vad_path: self.vad_model_path.clone(),
            vad_runtime: self.vad_runtime.clone(),
            shutdown: self.shutdown.clone(),
            stop: stop_rx,
            status,
            maximum: config.max_players,
            smart_turn,
            discard: Arc::clone(&self.discard_on_stop),
        };
        let last_error = Arc::clone(&self.last_error);
        self.task = Some(tokio::spawn(async move {
            if let Err(error) = run_players(capture, input).await {
                *last_error.lock().expect("pipeline error") = Some(error);
            }
        }));
        let device = AudioDevice {
            id: -2,
            name: "VRChat player audio (experimental)".into(),
            is_default: false,
            is_loopback: true,
            sample_rate: 16_000,
            channels: 1,
        };
        self.device = Some(device.clone());
        Ok(device)
    }
}

struct PlayerRun {
    vad: VadConfig,
    asr: AsrConfig,
    echo: AsrEchoGuard,
    dependencies: PipelineDependencies,
    threshold: f32,
    vad_path: PathBuf,
    vad_runtime: VadRuntimeState,
    shutdown: watch::Receiver<bool>,
    stop: watch::Receiver<bool>,
    status: SharedStatus,
    maximum: usize,
    smart_turn: Option<crate::smart_turn::SmartTurnRuntime>,
    discard: Arc<AtomicBool>,
}
struct PlayerTask {
    dependencies: PipelineDependencies,
    sender: mpsc::Sender<Vec<f32>>,
    stop: watch::Sender<bool>,
    task: JoinHandle<Result<(), String>>,
    next_position: u64,
    last_frame: Instant,
    discard: Arc<AtomicBool>,
    idle: bool,
    ready: Arc<AtomicBool>,
}

const WARM_PLAYER_SESSION: Duration = Duration::from_secs(90);

fn retire_player(
    id: String,
    task: PlayerTask,
    retired: &mut tokio::task::JoinSet<String>,
    retiring: &mut HashMap<String, usize>,
) {
    *retiring.entry(id.clone()).or_default() += 1;
    retired.spawn(async move {
        let _ = task.stop.send(true);
        let _ = task.task.await;
        id
    });
}

async fn run_players(mut capture: Capture, mut input: PlayerRun) -> Result<(), String> {
    let keep_warm = crate::asr::connection_rate_controlled(&input.asr);
    let mut tasks: HashMap<String, PlayerTask> = HashMap::new();
    let mut retired = tokio::task::JoinSet::new();
    let mut retiring: HashMap<String, usize> = HashMap::new();
    let mut retry_after: HashMap<String, Instant> = HashMap::new();
    let mut sweep = tokio::time::interval(Duration::from_millis(50));
    loop {
        if *input.shutdown.borrow() || *input.stop.borrow() {
            break;
        }
        let event = tokio::select! {
            _ = input.shutdown.changed() => break,
            _ = input.stop.changed() => break,
            _ = sweep.tick() => {
                retry_after.retain(|_, until| *until > Instant::now());
                let expired: Vec<_> = tasks.iter().filter(|(_, t)| !t.dependencies.current_player_selected() || t.last_frame.elapsed() > if keep_warm { WARM_PLAYER_SESSION } else { Duration::from_secs(16) })
                    .map(|(id, _)| id.clone()).collect();
                for id in expired {
                    let task = tasks.remove(&id).unwrap();
                    if !task.dependencies.current_player_selected() { task.discard.store(true, Ordering::SeqCst); }
                    retire_player(id, task, &mut retired, &mut retiring);
                }
                continue;
            }
            result = retired.join_next(), if !retired.is_empty() => {
                if let Some(Ok(id)) = result {
                    if let Some(count) = retiring.get_mut(&id) {
                        *count -= 1;
                        if *count == 0 { retiring.remove(&id); }
                    }
                }
                continue;
            }
            event = capture.events.recv() => match event { Some(event) => event, None => break },
        };
        match event {
            AudioEvent::Speaker(speaker) => {
                if let Some(task) = tasks.get(&speaker.id) {
                    task.dependencies.update_player(&speaker);
                }
                input.dependencies.publish_speaker(speaker).await;
            }
            AudioEvent::Closed(id) => {
                if let Some(task) = tasks.remove(&id) {
                    if !task.dependencies.current_player_selected() {
                        task.discard.store(true, Ordering::SeqCst);
                    }
                    retire_player(id, task, &mut retired, &mut retiring);
                }
            }
            AudioEvent::Idle(id) => {
                if keep_warm
                    && tasks
                        .get(&id)
                        .is_some_and(|task| task.ready.load(Ordering::SeqCst))
                {
                    if let Some(task) = tasks.get_mut(&id) {
                        task.idle = true;
                    }
                } else if let Some(task) = tasks.remove(&id) {
                    retire_player(id, task, &mut retired, &mut retiring);
                }
            }
            AudioEvent::Frame {
                speaker,
                samples,
                position,
            } => {
                let id = speaker.id.clone();
                // Discard frames already queued when the selection changed too.
                if !input.dependencies.player_selected(&speaker) {
                    continue;
                }
                if let Some(task) = tasks.get(&id) {
                    if task.dependencies.update_player(&speaker) {
                        input.dependencies.publish_speaker(speaker.clone()).await;
                    }
                }
                if retry_after
                    .get(&id)
                    .is_some_and(|until| *until > Instant::now())
                {
                    continue;
                }
                if tasks.get(&id).is_some_and(|task| task.task.is_finished()) {
                    let task = tasks.remove(&id).unwrap();
                    let error = match task.task.await {
                        Ok(Err(error)) => error,
                        Ok(Ok(())) => "Player recognition session ended".into(),
                        Err(error) => format!("Player recognition task failed: {error}"),
                    };
                    input.status.lock().expect("player status").detail = Some(error);
                    retry_after.insert(id, Instant::now() + Duration::from_secs(5));
                    continue;
                }
                if !tasks.contains_key(&id) {
                    if retiring.get(&id).copied().unwrap_or(0) >= 2
                        || (input.maximum > 0 && retired.len() >= input.maximum * 2)
                    {
                        input.status.lock().expect("player status").dropped_frames += 1;
                        continue;
                    }
                    // Also bounds live recognition sessions if a close notification
                    // was lost to capture backpressure.
                    if input.maximum > 0 && tasks.len() >= input.maximum {
                        let oldest = tasks
                            .iter()
                            .min_by_key(|(_, t)| t.last_frame)
                            .unwrap()
                            .0
                            .clone();
                        let task = tasks.remove(&oldest).unwrap();
                        retire_player(oldest, task, &mut retired, &mut retiring);
                    }
                    let (sender, receiver) = mpsc::channel(64);
                    let (stop, stop_rx) = watch::channel(false);
                    if speaker.display_name.is_some() || speaker.user_id.is_some() {
                        // Also repair history if capture backpressure dropped
                        // a metadata notification while this player was idle.
                        input.dependencies.publish_speaker(speaker.clone()).await;
                    }
                    let dependencies = input.dependencies.clone().with_player(speaker);
                    let player_dependencies = dependencies.clone();
                    let mut asr = input.asr.clone();
                    asr.single_speaker = true;
                    let vad = input.vad.clone();
                    let echo = input.echo.clone();
                    let vad_path = input.vad_path.clone();
                    let vad_runtime = input.vad_runtime.clone();
                    let shutdown = input.shutdown.clone();
                    let threshold = input.threshold;
                    let smart_turn = input.smart_turn.clone();
                    let discard = Arc::new(AtomicBool::new(false));
                    let task_discard = Arc::clone(&discard);
                    let ready = Arc::new(AtomicBool::new(false));
                    let task_ready = Arc::clone(&ready);
                    let task = tokio::spawn(async move {
                        let result = run_player(
                            receiver,
                            stop_rx,
                            shutdown,
                            dependencies.clone(),
                            asr,
                            vad,
                            echo,
                            vad_path,
                            vad_runtime,
                            threshold,
                            smart_turn,
                            discard,
                            task_ready,
                        )
                        .await;
                        dependencies.reset_recognition("speaker");
                        result
                    });
                    tasks.insert(
                        id.clone(),
                        PlayerTask {
                            dependencies: player_dependencies,
                            sender,
                            stop,
                            task,
                            next_position: position,
                            last_frame: Instant::now(),
                            discard: task_discard,
                            idle: false,
                            ready,
                        },
                    );
                }
                let task = tasks.get_mut(&id).unwrap();
                if position == 0
                    && (task.idle || task.last_frame.elapsed() > Duration::from_secs(15))
                {
                    // Capture intentionally starts a fresh decoder after silence.
                    // Keep the cloud socket, VAD state and authoritative identity.
                    task.next_position = 0;
                }
                task.idle = false;
                // A bounded queue drops a slow player's stream, never other players' audio.
                if position != task.next_position || task.sender.try_send(samples).is_err() {
                    let task = tasks.remove(&id).unwrap();
                    input.status.lock().expect("player status").dropped_frames += 1;
                    retry_after.insert(id.clone(), Instant::now() + Duration::from_millis(500));
                    retire_player(id, task, &mut retired, &mut retiring);
                } else {
                    task.next_position += 512;
                    task.last_frame = Instant::now();
                }
            }
        }
    }
    for (id, task) in tasks {
        if input.discard.load(Ordering::SeqCst) {
            task.discard.store(true, Ordering::SeqCst);
        }
        retire_player(id, task, &mut retired, &mut retiring);
    }
    capture.shutdown().await;
    while retired.join_next().await.is_some() {}
    let status = input.status.lock().expect("player status");
    if status.phase == "failed" {
        Err(status
            .detail
            .clone()
            .unwrap_or_else(|| "Player audio stopped unexpectedly".into()))
    } else {
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_player(
    mut receiver: mpsc::Receiver<Vec<f32>>,
    mut stop: watch::Receiver<bool>,
    mut shutdown: watch::Receiver<bool>,
    dependencies: PipelineDependencies,
    asr: AsrConfig,
    vad: VadConfig,
    echo: AsrEchoGuard,
    vad_path: PathBuf,
    vad_runtime: VadRuntimeState,
    threshold: f32,
    smart_turn: Option<crate::smart_turn::SmartTurnRuntime>,
    discard: Arc<AtomicBool>,
    ready: Arc<AtomicBool>,
) -> Result<(), String> {
    if *stop.borrow() || *shutdown.borrow() {
        return Ok(());
    }
    let managed_qwen = dependencies.managed_qwen();
    let controlled = crate::asr::connection_rate_controlled(&asr);
    let connect_timeout = if asr.backend == crate::config::QWEN_MANAGED_BACKEND {
        Duration::from_secs(120)
    } else {
        Duration::from_secs(15)
    };
    let silence_seconds = vad.silence_seconds;
    let connect = async move {
        // Defer model preparation until there is actual player audio. While it
        // connects, the priming loop below bounds retained speech to a short tail.
        let smart_turn = if let Some(runtime) = smart_turn {
            runtime.prepare().await.ok().map(|()| runtime)
        } else {
            None
        };
        let session = if asr.backend == crate::config::QWEN_MANAGED_BACKEND {
            let (runtime, manager) = managed_qwen.ok_or("Managed Qwen runtime is unavailable")?;
            spawn_managed_qwen_session(asr, runtime, manager).await
        } else {
            spawn_cloud_recognition_session(asr, silence_seconds).await
        }?;
        Ok::<_, String>((session, smart_turn))
    };
    let connect = async {
        if controlled {
            // Queue admission is not a network timeout. The actual handshake
            // has its own deadlines; stop/selection changes cancel this wait.
            connect.await
        } else {
            tokio::time::timeout(connect_timeout, connect)
                .await
                .map_err(|_| "Player recognition connection timed out")?
        }
    };
    tokio::pin!(connect);
    // Preserve up to three seconds of recent speech during quota admission.
    // A slow connection
    // cannot build up minutes of latency or repeatedly overflow and restart.
    let mut priming = VecDeque::new();
    let mut last_priming = Instant::now();
    let (session, smart_turn) = loop {
        tokio::select! {
            result = &mut connect => break result?,
            _ = stop.changed() => return Ok(()),
            _ = shutdown.changed() => return Ok(()),
            frame = receiver.recv() => match frame {
                None => return Ok(()),
                Some(frame) => { last_priming = Instant::now(); priming.push_back(frame); if priming.len() > if controlled { 94 } else { 32 } { priming.pop_front(); } }
            },
        }
    };
    if controlled && last_priming.elapsed() >= Duration::from_secs(3) {
        priming.clear();
    }
    ready.store(true, Ordering::SeqCst);
    let detector = tokio::task::spawn_blocking(move || {
        VoiceDetector::load_with_runtime(&vad_path, vad_runtime)
    })
    .await
    .map_err(|e| e.to_string())?;
    let mut capture = AudioCapture::from_stream(receiver, priming);
    let segmenter = if smart_turn.is_some() {
        SpeechSegmenter::smart(16_000, vad.silence_seconds, vad.max_speech_seconds)
    } else {
        SpeechSegmenter::new(16_000, vad.silence_seconds, vad.max_speech_seconds)
    };
    run(
        &mut capture,
        detector,
        segmenter,
        PipelineRunContext {
            dependencies,
            source: "speaker",
            cloud: Some(session),
            echo_guard: echo,
            sample_rate: 16_000,
            trigger_threshold_dbfs: Some(threshold),
            discard_on_stop: discard,
            smart_turn,
        },
        &mut shutdown,
        stop,
    )
    .await
}
