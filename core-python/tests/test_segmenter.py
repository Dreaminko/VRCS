import numpy as np

from app.vad import SpeechSegmenter


def test_segmenter_emits_after_silence():
    segmenter = SpeechSegmenter(sample_rate=100, silence_seconds=0.2, min_speech_seconds=0.1)
    speech = np.ones(10, dtype=np.float32)
    silence = np.zeros(10, dtype=np.float32)

    assert segmenter.push(speech, True) is None
    assert segmenter.push(silence, False) is None
    result = segmenter.push(silence, False)

    assert result is not None
    assert len(result) == 30

