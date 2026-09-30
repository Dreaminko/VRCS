use std::collections::VecDeque;

use super::{SharedAudio, StreamingInput};

const SAMPLE_RATE: usize = 16_000;
const PRE_ROLL_SAMPLES: usize = SAMPLE_RATE / 5;
const MAX_RESUME_SAMPLES: usize = SAMPLE_RATE * 30;
pub(super) const IDLE_SAMPLES: usize = SAMPLE_RATE * 30;

#[derive(Default)]
pub(super) struct Activity {
    seen_speech: bool,
    silent_samples: usize,
}

impl Activity {
    pub fn push(&mut self, samples: usize, speech: bool) -> bool {
        if speech {
            self.seen_speech = true;
            self.silent_samples = 0;
        } else if self.seen_speech {
            self.silent_samples = self.silent_samples.saturating_add(samples);
        }
        self.silent_samples >= IDLE_SAMPLES
    }
}

#[derive(Default)]
pub(super) struct ResumeBuffer {
    audio: VecDeque<(SharedAudio, bool)>,
    samples: usize,
    triggers: usize,
    pub ready: bool,
    pub closed: bool,
}

impl ResumeBuffer {
    pub fn has_capacity(&self) -> bool {
        self.samples < MAX_RESUME_SAMPLES
    }

    pub fn push(&mut self, input: StreamingInput) {
        let (samples, speech) = match input {
            StreamingInput::Audio(samples) => (samples, true),
            StreamingInput::AudioActivity(samples, speech) => (samples, speech),
            StreamingInput::Commit(result) => {
                let _ = result.send(Ok(()));
                return;
            }
        };
        self.triggers = if speech { self.triggers + 1 } else { 0 };
        self.ready |= self.triggers >= 2;
        self.samples += samples.len();
        self.audio.push_back((samples, speech));
        if !self.ready {
            // Retain enough onset audio for two consecutive speech chunks plus pre-roll.
            while self.audio.len() > 1
                && self.samples - self.audio.front().unwrap().0.len() >= PRE_ROLL_SAMPLES
            {
                self.samples -= self.audio.pop_front().unwrap().0.len();
            }
        }
    }

    pub fn take(&mut self) -> impl Iterator<Item = StreamingInput> + '_ {
        self.samples = 0;
        self.audio
            .drain(..)
            .map(|(samples, speech)| StreamingInput::AudioActivity(samples, speech))
    }
}

// Keep capturing onset while a close handshake or new connection is in flight.
pub(super) async fn collect_until<F: std::future::Future>(
    future: F,
    audio: &mut tokio::sync::mpsc::Receiver<StreamingInput>,
    buffer: &mut ResumeBuffer,
) -> F::Output {
    tokio::pin!(future);
    loop {
        tokio::select! {
            result = &mut future => return result,
            input = audio.recv(), if buffer.has_capacity() && !buffer.closed => match input {
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
    async fn close_and_connect_keep_capturing_and_handle_closed_channels() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(2);
        let mut buffer = ResumeBuffer::default();
        let producer = tokio::spawn(async move {
            for value in [0.1, 0.2, 0.3] {
                tx.send(StreamingInput::AudioActivity(
                    Arc::new(vec![value; 512]),
                    true,
                ))
                .await
                .unwrap();
            }
        });
        collect_until(
            tokio::time::sleep(std::time::Duration::from_millis(20)),
            &mut rx,
            &mut buffer,
        )
        .await;
        producer.await.unwrap();
        assert!(buffer.ready && buffer.closed);
        assert_eq!(buffer.samples, 1536);
        // A closed receiver must not spin or terminate connection work early.
        let result = collect_until(async { 42 }, &mut rx, &mut buffer).await;
        assert_eq!(result, 42);
        assert_eq!(buffer.take().count(), 3);
    }

    #[test]
    fn only_long_silence_after_speech_suspends_an_active_session() {
        let mut activity = Activity::default();
        assert!(!activity.push(IDLE_SAMPLES * 2, false));
        assert!(!activity.push(512, true));
        assert!(!activity.push(IDLE_SAMPLES - 1, false));
        assert!(activity.push(1, false));
        assert!(!activity.push(512, true));
    }

    #[test]
    fn resume_discards_idle_audio_but_preserves_onset_and_connecting_audio() {
        let mut buffer = ResumeBuffer::default();
        for _ in 0..1000 {
            buffer.push(StreamingInput::AudioActivity(
                Arc::new(vec![0.0; 512]),
                false,
            ));
        }
        assert!(buffer.samples < PRE_ROLL_SAMPLES + 512);
        buffer.push(StreamingInput::AudioActivity(
            Arc::new(vec![0.1; 512]),
            true,
        ));
        assert!(!buffer.ready);
        buffer.push(StreamingInput::AudioActivity(
            Arc::new(vec![0.2; 512]),
            true,
        ));
        assert!(buffer.ready);
        for _ in 0..20 {
            buffer.push(StreamingInput::AudioActivity(
                Arc::new(vec![0.3; 512]),
                false,
            ));
        }
        let audio: Vec<_> = buffer
            .take()
            .flat_map(|input| match input {
                StreamingInput::AudioActivity(samples, _) => samples.as_ref().clone(),
                _ => unreachable!(),
            })
            .collect();
        assert!(audio
            .windows(1024)
            .any(|w| w[..512] == [0.1; 512] && w[512..] == [0.2; 512]));
        assert_eq!(&audio[audio.len() - 512..], &[0.3; 512]);
    }
}
