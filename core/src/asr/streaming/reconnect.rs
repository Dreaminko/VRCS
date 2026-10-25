use std::collections::VecDeque;

use tokio::sync::{mpsc, oneshot};

use super::StreamingInput;

const MAX_BUFFERED_SAMPLES: usize = 16_000 * 30;
const REALTIME_BUFFER_SECONDS: u64 = 3;

/// Pipeline input held while reconnecting, so capture never blocks on a dead socket.
#[derive(Default)]
pub(super) struct ReconnectBuffer {
    pub inputs: VecDeque<StreamingInput>,
    samples: usize,
    pub closed: bool,
    realtime: bool,
    last_audio: Option<tokio::time::Instant>,
    last_speech: Option<tokio::time::Instant>,
}

impl ReconnectBuffer {
    pub(super) async fn wait_for_speech(
        &mut self,
        audio: &mut mpsc::Receiver<StreamingInput>,
    ) -> bool {
        while self
            .last_speech
            .is_none_or(|last| last.elapsed().as_secs() >= REALTIME_BUFFER_SECONDS)
        {
            if self.closed {
                return false;
            }
            match audio.recv().await {
                Some(input) => self.push(input),
                None => {
                    self.closed = true;
                    return false;
                }
            }
        }
        true
    }

    pub(super) fn realtime() -> Self {
        Self {
            realtime: true,
            ..Self::default()
        }
    }

    pub(super) fn into_inputs(mut self) -> VecDeque<StreamingInput> {
        if self.realtime
            && self
                .last_audio
                .is_none_or(|last| last.elapsed().as_secs() >= REALTIME_BUFFER_SECONDS)
        {
            self.inputs.clear();
        }
        self.inputs
    }

    fn push(&mut self, input: StreamingInput) {
        match input {
            StreamingInput::Commit(result) => {
                // Acknowledge now and keep the utterance boundary for replay.
                let _ = result.send(Ok(()));
                self.inputs
                    .push_back(StreamingInput::Commit(oneshot::channel().0));
            }
            StreamingInput::FinishQuestion(result) => {
                let _ = result.send(Err("Question recognition session reconnected".into()));
            }
            input @ (StreamingInput::Audio(_) | StreamingInput::AudioActivity(_, _)) => {
                self.last_audio = Some(tokio::time::Instant::now());
                if matches!(
                    &input,
                    StreamingInput::Audio(_) | StreamingInput::AudioActivity(_, true)
                ) {
                    self.last_speech = self.last_audio;
                }
                let maximum = if self.realtime {
                    16_000 * REALTIME_BUFFER_SECONDS as usize
                } else {
                    MAX_BUFFERED_SAMPLES
                };
                self.samples += audio_len(&input);
                self.inputs.push_back(input);
                if self.samples > maximum && !self.realtime {
                    tracing::warn!(
                        "Cloud recognition reconnect buffer is full; dropping oldest audio"
                    );
                }
                while self.samples > maximum {
                    let Some(dropped) = self.inputs.pop_front() else {
                        break;
                    };
                    self.samples -= audio_len(&dropped);
                }
            }
        }
    }
}

fn audio_len(input: &StreamingInput) -> usize {
    match input {
        StreamingInput::Audio(samples) | StreamingInput::AudioActivity(samples, _) => samples.len(),
        _ => 0,
    }
}

/// Runs `future` while draining pipeline input into `buffer`.
pub(super) async fn buffer_until<F: std::future::Future>(
    future: F,
    audio: &mut mpsc::Receiver<StreamingInput>,
    buffer: &mut ReconnectBuffer,
) -> F::Output {
    tokio::pin!(future);
    loop {
        tokio::select! {
            result = &mut future => return result,
            input = audio.recv(), if !buffer.closed => match input {
                Some(input) => buffer.push(input),
                None => buffer.closed = true,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn silent_tracks_do_not_reconnect_but_new_speech_wakes_them_without_losing_onset() {
        let mut buffer = ReconnectBuffer::realtime();
        let (tx, mut rx) = mpsc::channel(4);
        tx.send(StreamingInput::AudioActivity(
            Arc::new(vec![0.0; 512]),
            false,
        ))
        .await
        .unwrap();
        assert!(tokio::time::timeout(
            std::time::Duration::from_millis(20),
            buffer.wait_for_speech(&mut rx)
        )
        .await
        .is_err());
        tx.send(StreamingInput::AudioActivity(
            Arc::new(vec![0.1; 512]),
            true,
        ))
        .await
        .unwrap();
        assert!(buffer.wait_for_speech(&mut rx).await);
        assert_eq!(buffer.into_inputs().len(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn realtime_recovery_keeps_recent_speech_without_replaying_a_long_backlog() {
        let mut buffer = ReconnectBuffer::realtime();
        for _ in 0..600 {
            buffer.push(StreamingInput::Audio(Arc::new(vec![0.1; 1600])));
        }
        assert_eq!(buffer.samples, 48_000);
        assert_eq!(buffer.into_inputs().len(), 30);
        let mut buffer = ReconnectBuffer::realtime();
        buffer.push(StreamingInput::Audio(Arc::new(vec![0.1; 1600])));
        tokio::time::advance(std::time::Duration::from_secs(4)).await;
        assert!(buffer.into_inputs().is_empty());
    }

    #[tokio::test]
    async fn commits_are_acknowledged_and_oldest_audio_is_dropped_when_full() {
        let mut buffer = ReconnectBuffer::default();
        let (commit_tx, commit_rx) = oneshot::channel();
        buffer.push(StreamingInput::Audio(Arc::new(vec![
            0.0;
            MAX_BUFFERED_SAMPLES
        ])));
        buffer.push(StreamingInput::Commit(commit_tx));
        buffer.push(StreamingInput::Audio(Arc::new(vec![0.0; 512])));

        assert_eq!(commit_rx.await.unwrap(), Ok(()));
        assert_eq!(buffer.samples, 512);
        assert!(matches!(
            buffer.inputs.iter().collect::<Vec<_>>().as_slice(),
            [StreamingInput::Commit(_), StreamingInput::Audio(_)]
        ));
    }
}
