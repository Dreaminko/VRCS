from __future__ import annotations

import logging
import shutil
import threading
from pathlib import Path
from typing import Any, Callable

from .capabilities import (
    MODEL_DOWNLOAD_BYTES,
    MODEL_REPOSITORIES,
    model_cache_path,
    model_is_downloaded,
)


DownloadFunction = Callable[[str], None]
logger = logging.getLogger(__name__)


def _download_snapshot(repository: str) -> None:
    from huggingface_hub import snapshot_download

    snapshot_download(repo_id=repository)


def _directory_size(path: Path | None) -> int:
    if path is None or not path.exists():
        return 0
    scan_root = path / "blobs"
    if not scan_root.exists():
        scan_root = path
    total = 0
    try:
        for item in scan_root.rglob("*"):
            if item.is_file():
                total += item.stat().st_size
    except OSError:
        return total
    return total


class ModelManager:
    def __init__(self, download: DownloadFunction = _download_snapshot) -> None:
        self._download = download
        self._lock = threading.Lock()
        self._jobs: dict[str, dict[str, str | None]] = {}

    def _validate_model(self, model: str) -> str:
        repository = MODEL_REPOSITORIES.get(model)
        if repository is None:
            raise ValueError(f"不支持的识别模型：{model}")
        return repository

    def _job_status(self, model: str) -> tuple[str | None, str | None]:
        with self._lock:
            job = self._jobs.get(model)
            if job is None:
                return None, None
            return job["status"], job["error"]

    def describe(
        self,
        model: str,
        *,
        active_model: str | None = None,
        runtime_status: str = "not_loaded",
    ) -> dict[str, Any]:
        repository = self._validate_model(model)
        expected_bytes = MODEL_DOWNLOAD_BYTES[model]
        cache_bytes = _directory_size(model_cache_path(model))
        downloaded = model_is_downloaded(model)
        job_status, job_error = self._job_status(model)

        if job_status == "downloading":
            status = "downloading"
        elif job_status == "error":
            status = "error"
        elif (
            model == active_model
            and runtime_status in {"loading", "ready"}
        ):
            status = runtime_status
        else:
            status = "downloaded" if downloaded else "not_downloaded"

        if downloaded:
            progress = 1.0
        elif status == "downloading":
            progress = min(cache_bytes / expected_bytes, 0.99)
        else:
            progress = 0.0

        return {
            "id": model,
            "repository": repository,
            "status": status,
            "active": model == active_model,
            "downloaded_bytes": cache_bytes,
            "total_bytes": max(expected_bytes, cache_bytes) if downloaded else expected_bytes,
            "progress": progress,
            "error": job_error if status == "error" else None,
        }

    def list(
        self,
        *,
        active_model: str | None = None,
        runtime_status: str = "not_loaded",
    ) -> list[dict[str, Any]]:
        return [
            self.describe(
                model,
                active_model=active_model,
                runtime_status=runtime_status,
            )
            for model in MODEL_REPOSITORIES
        ]

    def start_download(self, model: str) -> None:
        repository = self._validate_model(model)
        if model_is_downloaded(model):
            return
        with self._lock:
            current = self._jobs.get(model)
            if current and current["status"] == "downloading":
                return
            self._jobs[model] = {"status": "downloading", "error": None}

        thread = threading.Thread(
            target=self._run_download,
            args=(model, repository),
            name=f"vrcs-model-{model}",
            daemon=True,
        )
        thread.start()

    def _run_download(self, model: str, repository: str) -> None:
        try:
            self._download(repository)
            if not model_is_downloaded(model):
                raise RuntimeError("下载已结束，但模型文件不完整")
        except Exception as exc:
            logger.exception("Failed to download ASR model %s", model)
            with self._lock:
                self._jobs[model] = {"status": "error", "error": str(exc)}
            return
        with self._lock:
            self._jobs.pop(model, None)

    def delete(self, model: str, *, active_model: str | None = None) -> None:
        self._validate_model(model)
        if model == active_model:
            raise ValueError("当前正在使用该模型，请先选择其他模型")
        status, _ = self._job_status(model)
        if status == "downloading":
            raise RuntimeError("模型正在下载，暂时不能删除")

        model_root = model_cache_path(model)
        if model_root and model_root.exists():
            shutil.rmtree(model_root)

        lock_root = model_root.parent / ".locks" / model_root.name if model_root else None
        if lock_root and lock_root.exists():
            shutil.rmtree(lock_root)
        with self._lock:
            self._jobs.pop(model, None)
