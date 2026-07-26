from __future__ import annotations

import os
from pathlib import Path
from typing import Any

from ..config import AsrConfig
from .runtime import CudaRuntimeStatus, cuda_runtime_status


MODEL_REPOSITORIES = {
    "tiny": "Systran/faster-whisper-tiny",
    "base": "Systran/faster-whisper-base",
    "small": "Systran/faster-whisper-small",
    "medium": "Systran/faster-whisper-medium",
    "large-v3": "Systran/faster-whisper-large-v3",
}
MODEL_DOWNLOAD_BYTES = {
    "tiny": 75_000_000,
    "base": 142_000_000,
    "small": 466_000_000,
    "medium": 1_530_000_000,
    "large-v3": 3_100_000_000,
}
EXPOSED_COMPUTE_TYPES = ("int8", "float16", "int8_float16")


def _hugging_face_cache() -> Path:
    configured = os.environ.get("HUGGINGFACE_HUB_CACHE")
    if configured:
        return Path(configured)
    hf_home = Path(
        os.environ.get("HF_HOME", Path.home() / ".cache" / "huggingface")
    )
    return hf_home / "hub"


def model_cache_path(model: str) -> Path | None:
    repository = MODEL_REPOSITORIES.get(model)
    if repository is None:
        return None
    return _hugging_face_cache() / f"models--{repository.replace('/', '--')}"


def model_is_downloaded(model: str) -> bool:
    direct = Path(model)
    if direct.exists():
        return True
    repository = MODEL_REPOSITORIES.get(model)
    if repository is None:
        return False
    model_root = model_cache_path(model)
    if model_root is None:
        return False
    snapshots = model_root / "snapshots"
    if not snapshots.exists():
        return False
    try:
        return any(
            (snapshot / "model.bin").is_file()
            and (snapshot / "config.json").is_file()
            for snapshot in snapshots.iterdir()
            if snapshot.is_dir()
        )
    except OSError:
        return False


def _supported_compute_types(device: str) -> set[str]:
    try:
        import ctranslate2

        return {
            str(item)
            for item in ctranslate2.get_supported_compute_types(device)
        }
    except Exception:
        return {"int8"} if device == "cpu" else set()


def asr_capabilities(
    active_config: AsrConfig | None = None,
    runtime_status: str = "not_loaded",
) -> dict[str, Any]:
    cuda_error: str | None = None
    try:
        import ctranslate2

        cuda_device_count = int(ctranslate2.get_cuda_device_count())
        runtime_available = True
    except Exception as exc:
        cuda_device_count = 0
        runtime_available = False
        cuda_error = str(exc)

    cuda_runtime = (
        cuda_runtime_status(load=True)
        if cuda_device_count
        else CudaRuntimeStatus(available=False, directory=None)
    )
    cuda_available = cuda_device_count > 0 and cuda_runtime.available
    if cuda_device_count and not cuda_runtime.available:
        cuda_error = cuda_runtime.error

    cpu_types = _supported_compute_types("cpu")
    cuda_types = _supported_compute_types("cuda") if cuda_available else set()
    resolved_auto_types = cuda_types if cuda_available else cpu_types
    combinations = {
        "auto": [
            item for item in EXPOSED_COMPUTE_TYPES if item in resolved_auto_types
        ],
        "cpu": [item for item in EXPOSED_COMPUTE_TYPES if item in cpu_types],
        "cuda": [item for item in EXPOSED_COMPUTE_TYPES if item in cuda_types],
    }
    if not combinations["auto"]:
        combinations["auto"] = ["int8"]
    if not combinations["cpu"]:
        combinations["cpu"] = ["int8"]

    models = []
    for model, repository in MODEL_REPOSITORIES.items():
        status = "downloaded" if model_is_downloaded(model) else "not_downloaded"
        if active_config and active_config.model == model and runtime_status in {
            "loading",
            "ready",
            "error",
        }:
            status = runtime_status
        models.append(
            {
                "id": model,
                "repository": repository,
                "status": status,
            }
        )

    return {
        "runtime_available": runtime_available,
        "cuda": {
            "available": cuda_available,
            "device_count": cuda_device_count,
            "error": cuda_error,
        },
        "compute_types": combinations,
        "models": models,
    }


def validate_asr_config(config: AsrConfig) -> None:
    if config.model not in MODEL_REPOSITORIES:
        raise ValueError(f"不支持的识别模型：{config.model}")
    capabilities = asr_capabilities()
    if config.device == "cuda" and not capabilities["cuda"]["available"]:
        detail = capabilities["cuda"]["error"]
        if capabilities["cuda"]["device_count"] and detail:
            raise ValueError(f"CUDA 预检失败：{detail}")
        raise ValueError("CUDA 预检失败：未发现可用的 NVIDIA CUDA 设备")
    valid_types = capabilities["compute_types"].get(config.device, [])
    if config.compute_type not in valid_types:
        supported = "、".join(valid_types) or "无"
        raise ValueError(
            f"{config.device} 不支持 {config.compute_type}，可用计算类型：{supported}"
        )
