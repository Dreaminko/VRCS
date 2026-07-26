from __future__ import annotations

import ctypes
import os
import queue
import subprocess
import sys
import threading
from ctypes import wintypes
from pathlib import Path


READY_HEADER = b"VRCS"
PROCESS_START_TIMEOUT_SECONDS = 8
TH32CS_SNAPPROCESS = 0x00000002
MAX_PATH = 260


class _ProcessEntry32W(ctypes.Structure):
    _fields_ = [
        ("dwSize", wintypes.DWORD),
        ("cntUsage", wintypes.DWORD),
        ("th32ProcessID", wintypes.DWORD),
        ("th32DefaultHeapID", ctypes.c_void_p),
        ("th32ModuleID", wintypes.DWORD),
        ("cntThreads", wintypes.DWORD),
        ("th32ParentProcessID", wintypes.DWORD),
        ("pcPriClassBase", wintypes.LONG),
        ("dwFlags", wintypes.DWORD),
        ("szExeFile", wintypes.WCHAR * MAX_PATH),
    ]


def find_process_id(process_name: str) -> int | None:
    if sys.platform != "win32":
        return None

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.CreateToolhelp32Snapshot.argtypes = [wintypes.DWORD, wintypes.DWORD]
    kernel32.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    kernel32.Process32FirstW.argtypes = [wintypes.HANDLE, ctypes.POINTER(_ProcessEntry32W)]
    kernel32.Process32FirstW.restype = wintypes.BOOL
    kernel32.Process32NextW.argtypes = [wintypes.HANDLE, ctypes.POINTER(_ProcessEntry32W)]
    kernel32.Process32NextW.restype = wintypes.BOOL
    kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel32.CloseHandle.restype = wintypes.BOOL

    snapshot = kernel32.CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
    invalid_handle = ctypes.c_void_p(-1).value
    if snapshot == invalid_handle:
        raise OSError(ctypes.get_last_error(), "无法枚举 Windows 进程")

    target = process_name.casefold()
    entry = _ProcessEntry32W()
    entry.dwSize = ctypes.sizeof(entry)
    try:
        if not kernel32.Process32FirstW(snapshot, ctypes.byref(entry)):
            raise OSError(ctypes.get_last_error(), "无法读取 Windows 进程列表")
        while True:
            if entry.szExeFile.casefold() == target:
                return int(entry.th32ProcessID)
            if not kernel32.Process32NextW(snapshot, ctypes.byref(entry)):
                return None
    finally:
        kernel32.CloseHandle(snapshot)


def find_helper() -> Path | None:
    configured = os.environ.get("VRCS_PROCESS_AUDIO_HELPER")
    candidates: list[Path] = []
    if configured:
        candidates.append(Path(configured))

    candidates.append(Path(sys.executable).resolve().with_name("vrcs-process-audio.exe"))
    repo_root = Path(__file__).resolve().parents[3]
    target_root = repo_root / "apps" / "desktop" / "src-tauri" / "target"
    candidates.extend(
        [
            target_root / "debug" / "vrcs-process-audio.exe",
            target_root / "release" / "vrcs-process-audio.exe",
            target_root
            / "x86_64-pc-windows-msvc"
            / "release"
            / "vrcs-process-audio.exe",
        ]
    )
    return next((candidate for candidate in candidates if candidate.is_file()), None)


class ProcessLoopbackStream:
    def __init__(self, process: subprocess.Popen[bytes]) -> None:
        self._process = process

    @classmethod
    def start(cls, process_name: str) -> ProcessLoopbackStream:
        if sys.platform != "win32":
            raise RuntimeError("仅采集 VRChat 音频只支持 Windows")

        pid = find_process_id(process_name)
        if pid is None:
            raise RuntimeError("未发现正在运行的 VRChat，请先启动 VRChat")

        helper = find_helper()
        if helper is None:
            raise RuntimeError("缺少 VRChat 进程音频采集组件，请重新安装或构建 VRCS")

        process = subprocess.Popen(
            [str(helper), str(pid)],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
        if process.stdout is None:
            process.kill()
            raise RuntimeError("无法连接 VRChat 音频采集组件")

        result: queue.Queue[bytes] = queue.Queue(maxsize=1)
        reader = threading.Thread(
            target=lambda: result.put(process.stdout.read(len(READY_HEADER))),
            name="vrcs-process-audio-startup",
            daemon=True,
        )
        reader.start()
        try:
            header = result.get(timeout=PROCESS_START_TIMEOUT_SECONDS)
        except queue.Empty as exc:
            cls._terminate(process)
            raise RuntimeError("启动 VRChat 音频采集超时") from exc

        if header != READY_HEADER:
            error = cls._read_error(process)
            cls._terminate(process)
            raise RuntimeError(error or "VRChat 音频采集组件启动失败")
        return cls(process)

    def read(self, frames: int, exception_on_overflow: bool = False) -> bytes:
        del exception_on_overflow
        stdout = self._process.stdout
        if stdout is None:
            raise RuntimeError("VRChat 音频采集流已关闭")

        expected = frames * 2
        chunks: list[bytes] = []
        remaining = expected
        while remaining:
            chunk = stdout.read(remaining)
            if not chunk:
                error = self._read_error(self._process)
                raise RuntimeError(error or "VRChat 音频采集意外停止")
            chunks.append(chunk)
            remaining -= len(chunk)
        return b"".join(chunks)

    def stop_stream(self) -> None:
        self._terminate(self._process)

    def close(self) -> None:
        if self._process.stdout is not None:
            self._process.stdout.close()
        if self._process.stderr is not None:
            self._process.stderr.close()

    @staticmethod
    def _terminate(process: subprocess.Popen[bytes]) -> None:
        if process.poll() is not None:
            return
        process.terminate()
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=2)

    @staticmethod
    def _read_error(process: subprocess.Popen[bytes]) -> str:
        if process.poll() is None or process.stderr is None:
            return ""
        return process.stderr.read().decode("utf-8", errors="replace").strip()
