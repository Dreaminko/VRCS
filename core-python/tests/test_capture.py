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
        pass

    def close(self) -> None:
        self.closed_after_read = self.read_finished


class FakeAudio:
    def terminate(self) -> None:
        pass


@pytest.mark.asyncio
async def test_cancelled_read_finishes_before_stream_is_closed() -> None:
    capture = AudioCapture()
    stream = BlockingStream()
    capture._stream = stream
    capture._audio = FakeAudio()

    task = asyncio.create_task(capture.read())
    await asyncio.to_thread(stream.reading.wait, 1)
    task.cancel()
    await asyncio.sleep(0)

    assert not task.done()
    stream.release.set()
    with pytest.raises(asyncio.CancelledError):
        await task

    capture.stop()
    assert stream.closed_after_read
