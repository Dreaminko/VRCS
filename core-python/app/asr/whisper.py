from __future__ import annotations

import asyncio
from dataclasses import dataclass
from typing import Any

import numpy as np

from ..config import AsrConfig
from .runtime import prepare_faster_whisper_runtime


class AsrUnavailableError(RuntimeError):
    pass


@dataclass(slots=True)
class Transcription:
    text: str
    language: str | None


class WhisperTranscriber:
    def __init__(self, config: AsrConfig) -> None:
        self.config = config
        self._model: Any = None
        self._lock = asyncio.Lock()
        self.status = "not_loaded"

    def update(self, config: AsrConfig) -> None:
        if config != self.config:
            self.config = config
            self._model = None
            self.status = "not_loaded"

    def _load(self) -> Any:
        if self._model is not None:
            return self._model
        runtime_device = self.config.device
        runtime_compute_type = self.config.compute_type
        try:
            cuda_runtime = prepare_faster_whisper_runtime(self.config.device)
            if (
                self.config.device == "cuda"
                and cuda_runtime is not None
                and not cuda_runtime.available
            ):
                raise AsrUnavailableError(cuda_runtime.error or "CUDA 12 运行库不可用")
            from faster_whisper import WhisperModel
            if self.config.device == "auto":
                import ctranslate2

                if (
                    cuda_runtime is not None
                    and not cuda_runtime.available
                ) or ctranslate2.get_cuda_device_count() == 0:
                    runtime_device = "cpu"
                    runtime_compute_type = "int8"
        except ImportError as exc:
            raise AsrUnavailableError(
                "ASR support is not installed. Run: pip install -e '.[asr]'"
            ) from exc
        self.status = "loading"
        try:
            self._model = WhisperModel(
                self.config.model,
                device=runtime_device,
                compute_type=runtime_compute_type,
            )
        except Exception:
            self.status = "error"
            raise
        self.status = "ready"
        return self._model

    def _transcribe(self, samples: np.ndarray) -> Transcription:
        model = self._load()
        segments, info = model.transcribe(
            samples,
            task="transcribe",
            language=None if self.config.language == "auto" else self.config.language,
            vad_filter=False,
            beam_size=1,
        )
        text = " ".join(segment.text.strip() for segment in segments).strip()
        return Transcription(text=text, language=getattr(info, "language", None))

    async def transcribe(self, samples: np.ndarray) -> Transcription:
        async with self._lock:
            return await asyncio.to_thread(self._transcribe, samples)
