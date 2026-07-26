import sys

from app.main import run


def release_self_test() -> None:
    """Validate imports that are only exercised after a user installs VRCS."""

    import ctranslate2
    import httpx
    import pyaudiowpatch
    from huggingface_hub import snapshot_download

    from app.vad import VoiceDetector

    if not callable(snapshot_download):
        raise RuntimeError("Hugging Face model download support is unavailable")
    if not hasattr(httpx, "Client"):
        raise RuntimeError("HTTP model download support is unavailable")
    if not hasattr(ctranslate2, "models"):
        raise RuntimeError("CTranslate2 model support is unavailable")
    if not hasattr(pyaudiowpatch, "PyAudio"):
        raise RuntimeError("WASAPI audio support is unavailable")
    if VoiceDetector().backend != "silero-onnx":
        raise RuntimeError("Silero ONNX VAD is unavailable")


if __name__ == "__main__":
    if "--release-self-test" in sys.argv:
        release_self_test()
    else:
        run()
