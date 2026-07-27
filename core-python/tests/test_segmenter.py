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


def test_default_segmenter_emits_after_short_silence():
    segmenter = SpeechSegmenter(sample_rate=100)
    speech = np.ones(25, dtype=np.float32)
    silence = np.zeros(10, dtype=np.float32)

    assert segmenter.push(speech, True) is None
    for _ in range(3):
        assert segmenter.push(silence, False) is None
    result = segmenter.push(silence, False)

    assert result is not None
    assert len(result) == 65


def test_default_segmenter_caps_long_utterances():
    segmenter = SpeechSegmenter(sample_rate=100)
    speech = np.ones(100, dtype=np.float32)

    for _ in range(5):
        assert segmenter.push(speech, True) is None
    result = segmenter.push(speech, True)

    assert result is not None
    assert len(result) == 600
