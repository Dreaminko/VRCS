from __future__ import annotations

from collections.abc import Callable

import numpy as np


class VoiceDetector:
    """Uses Silero VAD when installed, with an energy fallback for development."""

    def __init__(self, threshold: float = 0.5) -> None:
        self.threshold = threshold
        self.backend = "energy"
        self._predict: Callable[[np.ndarray], float] = self._energy
        try:
            import torch
            from silero_vad import load_silero_vad

            model = load_silero_vad(onnx=False)

            def silero(samples: np.ndarray) -> float:
                frame = np.zeros(512, dtype=np.float32)
                frame[: min(len(samples), 512)] = samples[:512]
                with torch.no_grad():
                    return float(model(torch.from_numpy(frame), 16_000).item())

            self._predict = silero
            self.backend = "silero"
        except (ImportError, RuntimeError):
            pass

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
        silence_seconds: float = 0.7,
        min_speech_seconds: float = 0.25,
        max_speech_seconds: float = 20.0,
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

