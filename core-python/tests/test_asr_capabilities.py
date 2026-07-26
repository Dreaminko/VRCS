import sys
from pathlib import Path
from types import SimpleNamespace

import pytest

import app.asr.capabilities as capabilities_module
import app.asr.runtime as runtime_module
from app.asr.capabilities import (
    asr_capabilities,
    model_is_downloaded,
    validate_asr_config,
)
from app.asr.runtime import (
    CUDA_RUNTIME_DLLS,
    CudaRuntimeStatus,
    cuda_runtime_status,
    find_cuda_runtime_directory,
)
from app.config import AsrConfig


def test_model_download_status_reads_hugging_face_snapshot(tmp_path, monkeypatch):
    cache = tmp_path / "hub"
    snapshot = (
        cache
        / "models--Systran--faster-whisper-small"
        / "snapshots"
        / "revision"
    )
    snapshot.mkdir(parents=True)
    (snapshot / "model.bin").write_bytes(b"model")
    (snapshot / "config.json").write_text("{}", encoding="utf-8")
    monkeypatch.setenv("HUGGINGFACE_HUB_CACHE", str(cache))

    assert model_is_downloaded("small") is True
    assert model_is_downloaded("medium") is False


def test_cuda_preflight_and_compute_type_filtering(monkeypatch):
    runtime = SimpleNamespace(
        get_cuda_device_count=lambda: 1,
        get_supported_compute_types=lambda device: (
            {"int8", "float16", "int8_float16"}
            if device == "cuda"
            else {"int8"}
        ),
    )
    monkeypatch.setitem(sys.modules, "ctranslate2", runtime)
    monkeypatch.setattr(
        capabilities_module,
        "cuda_runtime_status",
        lambda **_: CudaRuntimeStatus(
            available=True,
            directory=Path(r"C:\CUDA\v12\bin"),
        ),
    )

    capabilities = asr_capabilities()

    assert capabilities["cuda"]["available"] is True
    assert capabilities["compute_types"]["cpu"] == ["int8"]
    assert capabilities["compute_types"]["cuda"] == [
        "int8",
        "float16",
        "int8_float16",
    ]


def test_invalid_cuda_selection_is_rejected(monkeypatch):
    runtime = SimpleNamespace(
        get_cuda_device_count=lambda: 0,
        get_supported_compute_types=lambda device: {"int8"} if device == "cpu" else set(),
    )
    monkeypatch.setitem(sys.modules, "ctranslate2", runtime)

    with pytest.raises(ValueError, match="CUDA 预检失败"):
        validate_asr_config(
            AsrConfig(device="cuda", compute_type="float16")
        )


def test_cuda_selection_is_rejected_when_runtime_dlls_are_missing(monkeypatch):
    runtime = SimpleNamespace(
        get_cuda_device_count=lambda: 1,
        get_supported_compute_types=lambda device: (
            {"int8", "float16"} if device == "cuda" else {"int8"}
        ),
    )
    monkeypatch.setitem(sys.modules, "ctranslate2", runtime)
    monkeypatch.setattr(
        capabilities_module,
        "cuda_runtime_status",
        lambda **_: CudaRuntimeStatus(
            available=False,
            directory=None,
            error="未找到 CUDA 12 运行库",
        ),
    )

    capabilities = asr_capabilities()

    assert capabilities["cuda"] == {
        "available": False,
        "device_count": 1,
        "error": "未找到 CUDA 12 运行库",
    }
    assert capabilities["compute_types"]["cuda"] == []
    assert capabilities["compute_types"]["auto"] == ["int8"]
    with pytest.raises(ValueError, match="未找到 CUDA 12 运行库"):
        validate_asr_config(
            AsrConfig(device="cuda", compute_type="float16")
        )


def test_cuda_runtime_directory_requires_the_complete_dll_set(tmp_path):
    incomplete = tmp_path / "incomplete"
    complete = tmp_path / "complete"
    incomplete.mkdir()
    complete.mkdir()
    for library in CUDA_RUNTIME_DLLS:
        (complete / library).write_bytes(b"dll")
    (incomplete / CUDA_RUNTIME_DLLS[0]).write_bytes(b"dll")

    assert find_cuda_runtime_directory([incomplete]) is None
    assert find_cuda_runtime_directory([incomplete, complete]) == complete.resolve()


def test_cuda_runtime_preloads_dlls_from_the_detected_directory(
    tmp_path,
    monkeypatch,
):
    runtime_dir = tmp_path / "cuda"
    runtime_dir.mkdir()
    for library in CUDA_RUNTIME_DLLS:
        (runtime_dir / library).write_bytes(b"dll")
    loaded: list[str] = []
    directory_handle = object()
    monkeypatch.setattr(
        runtime_module.os,
        "add_dll_directory",
        lambda directory: directory_handle,
    )
    monkeypatch.setattr(
        runtime_module.ctypes,
        "WinDLL",
        lambda library: loaded.append(library) or object(),
    )

    status = cuda_runtime_status(load=True, candidates=[runtime_dir])

    assert status.available is True
    assert status.directory == runtime_dir.resolve()
    assert loaded == [
        str(runtime_dir.resolve() / library)
        for library in CUDA_RUNTIME_DLLS
    ]
