from __future__ import annotations

import logging
from collections.abc import Callable

import numpy as np

from ..asr.runtime import prepare_faster_whisper_runtime


logger = logging.getLogger(__name__)


class _StreamingSileroOnnx:
    """Minimal streaming wrapper around faster-whisper's bundled Silero model."""

    def __init__(self) -> None:
        prepare_faster_whisper_runtime()
        from faster_whisper.vad import get_vad_model

        self._session = get_vad_model().session
        self._h = np.zeros((1, 1, 128), dtype=np.float32)
        self._c = np.zeros((1, 1, 128), dtype=np.float32)
        self._context = np.zeros((1, 64), dtype=np.float32)

    def predict(self, samples: np.ndarray) -> float:
        frame = np.zeros(512, dtype=np.float32)
        sample_count = min(len(samples), len(frame))
        frame[:sample_count] = samples[:sample_count]
        model_input = np.concatenate((self._context, frame.reshape(1, -1)), axis=1)
        probabilities, self._h, self._c = self._session.run(
            None,
            {"input": model_input, "h": self._h, "c": self._c},
        )
        self._context = frame[-64:].reshape(1, -1)
        return float(probabilities.reshape(-1)[0])


class VoiceDetector:
    """Uses Silero VAD when installed, with an energy fallback for development."""

    def __init__(self, threshold: float = 0.5) -> None:
        self.threshold = threshold
        self.backend = "energy"
        self._predict: Callable[[np.ndarray], float] = self._energy
        try:
            model = _StreamingSileroOnnx()
            self._predict = model.predict
            self.backend = "silero-onnx"
        except (ImportError, OSError, RuntimeError) as exc:
            logger.warning("Silero ONNX VAD is unavailable; using energy fallback: %s", exc)

    @staticmethod
    def _energy(samples: np.ndarray) -> float:
        rms = float(np.sqrt(np.mean(np.square(samples)))) if len(samples) else 0.0
        return min(1.0, rms / 0.04)

    def is_speech(self, samples: np.ndarray) -> bool:
        return self._predict(samples) >= self.threshold


class SpeechSegmenter:
    def __init__(
        self,
        sample_rate: int = 16_000,
        silence_seconds: float = 0.4,
        min_speech_seconds: float = 0.25,
        max_speech_seconds: float = 6.0,
    ) -> None:
        self.sample_rate = sample_rate
        self.silence_samples = int(silence_seconds * sample_rate)
        self.min_speech_samples = int(min_speech_seconds * sample_rate)
        self.max_speech_samples = int(max_speech_seconds * sample_rate)
        self._chunks: list[np.ndarray] = []
        self._speech_samples = 0
        self._silence_samples = 0

    def push(self, chunk: np.ndarray, speech: bool) -> np.ndarray | None:
        if speech:
            self._chunks.append(chunk)
            self._speech_samples += len(chunk)
            self._silence_samples = 0
        elif self._chunks:
            self._chunks.append(chunk)
            self._silence_samples += len(chunk)

        total = self._speech_samples + self._silence_samples
        finished = self._silence_samples >= self.silence_samples or total >= self.max_speech_samples
        if not finished:
            return None
        segment = np.concatenate(self._chunks)
        valid = self._speech_samples >= self.min_speech_samples
        self.reset()
        return segment if valid else None

    def flush(self) -> np.ndarray | None:
        if self._speech_samples < self.min_speech_samples:
            self.reset()
            return None
        segment = np.concatenate(self._chunks)
        self.reset()
        return segment

    def reset(self) -> None:
        self._chunks.clear()
        self._speech_samples = 0
        self._silence_samples = 0
