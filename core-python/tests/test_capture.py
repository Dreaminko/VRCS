import asyncio
import threading

import pytest

from app.audio.capture import AudioCapture


class BlockingStream:
    def __init__(self) -> None:
        self.reading = threading.Event()
        self.release = threading.Event()
        self.read_finished = False
        self.closed_after_read = False

    def read(self, frames: int, exception_on_overflow: bool) -> bytes:
        self.reading.set()
        self.release.wait(timeout=2)
        self.read_finished = True
        return bytes(frames * 2)

    def stop_stream(self) -> None:
        self.release.set()

    def close(self) -> None:
        self.closed_after_read = self.read_finished


class FakeAudio:
    def terminate(self) -> None:
        pass


class ProcessStream:
    def __init__(self) -> None:
        self.stopped = False
        self.closed = False

    def stop_stream(self) -> None:
        self.stopped = True

    def close(self) -> None:
        self.closed = True


async def test_cancelled_read_interrupts_stream_before_it_is_closed() -> None:
    capture = AudioCapture()
    stream = BlockingStream()
    capture._stream = stream
    capture._audio = FakeAudio()

    task = asyncio.create_task(capture.read())
    await asyncio.to_thread(stream.reading.wait, 1)
    task.cancel()
    with pytest.raises(asyncio.CancelledError):
        await asyncio.wait_for(task, timeout=1)

    capture.stop()
    assert stream.read_finished
    assert stream.closed_after_read


def test_process_capture_exposes_vrchat_as_audio_device(monkeypatch) -> None:
    stream = ProcessStream()
    monkeypatch.setattr(
        "app.audio.capture.ProcessLoopbackStream.start",
        lambda process_name: stream,
    )
    capture = AudioCapture()

    device = capture.start(36, process_name="VRChat.exe")

    assert device.id == -1
    assert device.name == "VRChat（仅应用音频）"
    assert device.sample_rate == 16_000
    capture.stop()
    assert stream.stopped
    assert stream.closed
