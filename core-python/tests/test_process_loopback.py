from __future__ import annotations

import threading
from io import BytesIO
from pathlib import Path

import pytest

from app.audio import process_loopback
from app.audio.process_loopback import ProcessLoopbackStream


class BlockingOutput:
    def __init__(self) -> None:
        self.release = threading.Event()

    def read(self, size: int) -> bytes:
        del size
        self.release.wait(timeout=1)
        return b""

    def close(self) -> None:
        self.release.set()


class FakeProcess:
    def __init__(self) -> None:
        self.stdout = BlockingOutput()
        self.stderr = BytesIO()
        self.running = True
        self.terminated = False

    def poll(self) -> int | None:
        return None if self.running else 1

    def terminate(self) -> None:
        self.terminated = True
        self.running = False
        self.stdout.release.set()

    def wait(self, timeout: float) -> int:
        del timeout
        return 1

    def kill(self) -> None:
        self.running = False
        self.stdout.release.set()


def test_missing_vrchat_fails_before_helper_is_started(monkeypatch) -> None:
    monkeypatch.setattr(process_loopback.sys, "platform", "win32")
    monkeypatch.setattr(process_loopback, "find_process_id", lambda name: None)
    started = False

    def start_process(*args, **kwargs):
        nonlocal started
        started = True
        raise AssertionError("helper should not be started")

    monkeypatch.setattr(process_loopback.subprocess, "Popen", start_process)

    with pytest.raises(RuntimeError, match="未发现正在运行的 VRChat"):
        ProcessLoopbackStream.start("VRChat.exe")
    assert not started


def test_helper_startup_timeout_terminates_process(monkeypatch) -> None:
    process = FakeProcess()
    monkeypatch.setattr(process_loopback.sys, "platform", "win32")
    monkeypatch.setattr(process_loopback, "find_process_id", lambda name: 42)
    monkeypatch.setattr(process_loopback, "find_helper", lambda: Path("helper.exe"))
    monkeypatch.setattr(process_loopback, "PROCESS_START_TIMEOUT_SECONDS", 0.01)
    monkeypatch.setattr(process_loopback.subprocess, "Popen", lambda *args, **kwargs: process)

    with pytest.raises(RuntimeError, match="启动 VRChat 音频采集超时"):
        ProcessLoopbackStream.start("VRChat.exe")
    assert process.terminated
