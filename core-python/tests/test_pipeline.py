import asyncio

import numpy as np

from app.asr import Transcription
from app.models import AudioDevice
from app.pipeline import TranscriptionPipeline
from app.vad import SpeechSegmenter


class FakeCapture:
    def __init__(self) -> None:
        self.device = None

    def start(self, device_id, process_name=None):
        del process_name
        self.device = AudioDevice(
            id=device_id,
            name="microphone",
            sample_rate=16_000,
            channels=1,
        )
        return self.device

    async def read(self):
        await asyncio.sleep(0)
        return np.ones(512, dtype=np.float32)

    def interrupt(self):
        pass

    def stop(self):
        self.device = None


class SpeechDetector:
    def is_speech(self, samples):
        return True


class FakeTranscriber:
    async def transcribe(self, samples):
        return Transcription(text="hello", language="en")


class SubtitleSink:
    def __init__(self) -> None:
        self.subtitle = None
        self.published = asyncio.Event()

    async def publish(self, subtitle):
        self.subtitle = subtitle
        self.published.set()


async def test_pipeline_labels_microphone_subtitles():
    sink = SubtitleSink()
    pipeline = TranscriptionPipeline(
        FakeCapture(),
        SpeechDetector(),
        SpeechSegmenter(silence_seconds=0, min_speech_seconds=0),
        FakeTranscriber(),
        sink,
        source="microphone",
    )

    pipeline.start(20)
    await asyncio.wait_for(sink.published.wait(), timeout=1)
    await pipeline.stop()

    assert sink.subtitle.source == "microphone"
