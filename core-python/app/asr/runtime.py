from __future__ import annotations

import ctypes
import os
import sys
from collections.abc import Iterable, Mapping
from dataclasses import dataclass
from pathlib import Path
from types import ModuleType


CUDA_RUNTIME_DLLS = (
    "cudart64_12.dll",
    "cublasLt64_12.dll",
    "cublas64_12.dll",
)
_cuda_directory_handles: list[object] = []
_cuda_library_handles: list[object] = []
_loaded_cuda_directories: set[Path] = set()


@dataclass(frozen=True, slots=True)
class CudaRuntimeStatus:
    available: bool
    directory: Path | None
    error: str | None = None


def _candidate_cuda_directories(
    env: Mapping[str, str] | None = None,
) -> list[Path]:
    values = os.environ if env is None else env
    candidates: list[Path] = []

    def add(path: str | Path | None) -> None:
        if not path:
            return
        candidate = Path(path).expanduser()
        for item in (candidate, candidate / "bin", candidate / "bin" / "x64"):
            if item not in candidates:
                candidates.append(item)

    add(values.get("VRCS_CUDA_RUNTIME_DIR"))
    for key, value in values.items():
        normalized = key.upper()
        if normalized == "CUDA_PATH" or normalized.startswith("CUDA_PATH_V12_"):
            add(value)

    program_files = Path(values.get("ProgramFiles", r"C:\Program Files"))
    toolkit_root = program_files / "NVIDIA GPU Computing Toolkit" / "CUDA"
    if toolkit_root.is_dir():
        for version_root in sorted(toolkit_root.glob("v12*"), reverse=True):
            add(version_root)
    add(program_files / "NVIDIA Corporation" / "NVIDIA Video Effects")
    for entry in values.get("PATH", "").split(os.pathsep):
        add(entry.strip('"'))
    return candidates


def find_cuda_runtime_directory(
    candidates: Iterable[Path] | None = None,
) -> Path | None:
    for candidate in candidates or _candidate_cuda_directories():
        try:
            if all((candidate / library).is_file() for library in CUDA_RUNTIME_DLLS):
                return candidate.resolve()
        except OSError:
            continue
    return None


def cuda_runtime_status(
    *,
    load: bool = False,
    candidates: Iterable[Path] | None = None,
) -> CudaRuntimeStatus:
    if os.name != "nt":
        return CudaRuntimeStatus(available=True, directory=None)

    directory = find_cuda_runtime_directory(candidates)
    if directory is None:
        required = "、".join(CUDA_RUNTIME_DLLS)
        return CudaRuntimeStatus(
            available=False,
            directory=None,
            error=f"未找到 CUDA 12 运行库：{required}",
        )
    if not load or directory in _loaded_cuda_directories:
        return CudaRuntimeStatus(available=True, directory=directory)

    try:
        directory_handle = os.add_dll_directory(str(directory))
        loaded = [
            ctypes.WinDLL(str(directory / library))
            for library in CUDA_RUNTIME_DLLS
        ]
    except (OSError, AttributeError) as exc:
        return CudaRuntimeStatus(
            available=False,
            directory=directory,
            error=f"CUDA 12 运行库无法加载：{exc}",
        )

    _cuda_directory_handles.append(directory_handle)
    _cuda_library_handles.extend(loaded)
    _loaded_cuda_directories.add(directory)
    return CudaRuntimeStatus(available=True, directory=directory)


def prepare_faster_whisper_runtime(
    device: str = "cpu",
) -> CudaRuntimeStatus | None:
    """Allow faster-whisper to run without PyAV for in-memory NumPy audio.

    VRCS never asks faster-whisper to decode a file. The upstream audio module
    imports PyAV eagerly even though NumPy inputs bypass its decode_audio path.
    Release builds exclude PyAV's full FFmpeg distribution, so a module stub is
    sufficient for importing the NumPy-only transcription path.
    """

    if "av" not in sys.modules:
        try:
            __import__("av")
        except ImportError:
            sys.modules["av"] = ModuleType("av")

    if device in {"auto", "cuda"}:
        return cuda_runtime_status(load=True)
    return None
