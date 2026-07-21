from types import SimpleNamespace

import numpy as np

from app.asr import WhisperTranscriber
from app.config import AsrConfig


class FakeModel:
    def __init__(self) -> None:
        self.options = None

    def transcribe(self, samples, **options):
        self.options = options
        return [SimpleNamespace(text=" こんにちは ")], SimpleNamespace(language="ja")


def test_whisper_preserves_the_source_language() -> None:
    transcriber = WhisperTranscriber(AsrConfig(language="auto"))
    model = FakeModel()
    transcriber._model = model

    result = transcriber._transcribe(np.zeros(512, dtype=np.float32))

    assert model.options["task"] == "transcribe"
    assert model.options["language"] is None
    assert result.text == "こんにちは"
    assert result.language == "ja"
