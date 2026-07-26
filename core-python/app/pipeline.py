from __future__ import annotations

import asyncio
from datetime import datetime, timezone

from .asr import WhisperTranscriber
from .audio.capture import AudioCapture
from .models import AudioDevice, Subtitle, SubtitleSource
from .subtitles import SubtitleStore
from .vad import SpeechSegmenter, VoiceDetector


class TranscriptionPipeline:
    def __init__(
        self,
        capture: AudioCapture,
        detector: VoiceDetector,
        segmenter: SpeechSegmenter,
        transcriber: WhisperTranscriber,
        subtitles: SubtitleStore,
        source: SubtitleSource = "speaker",
    ) -> None:
        self.capture = capture
        self.detector = detector
        self.segmenter = segmenter
        self.transcriber = transcriber
        self.subtitles = subtitles
        self.source = source
        self.task: asyncio.Task[None] | None = None
        self.last_error: str | None = None

    @property
    def running(self) -> bool:
        return self.task is not None and not self.task.done()

    def start(
        self,
        device_id: int | None,
        process_name: str | None = None,
    ) -> AudioDevice:
        if self.running:
            raise RuntimeError("Transcription is already running")
        device = self.capture.start(device_id, process_name=process_name)
        self.last_error = None
        self.task = asyncio.create_task(self._run(), name="transcription-pipeline")
        return device

    async def stop(self) -> None:
        task, self.task = self.task, None
        if task is not None:
            task.cancel()
            self.capture.interrupt()
            try:
                await task
            except asyncio.CancelledError:
                pass
        self.capture.stop()
        self.segmenter.reset()

    async def _run(self) -> None:
        try:
            while True:
                chunk = await self.capture.read()
                segment = self.segmenter.push(chunk, self.detector.is_speech(chunk))
                if segment is None:
                    continue
                result = await self.transcriber.transcribe(segment)
                if result.text:
                    await self.subtitles.publish(
                        Subtitle(
                            text=result.text,
                            language=result.language,
                            source=self.source,
                            created_at=datetime.now(timezone.utc),
                        )
                    )
        except asyncio.CancelledError:
            raise
        except Exception as exc:
            self.last_error = str(exc)
            self.capture.stop()
