import time

import pytest

from app.asr.model_manager import ModelManager


def wait_for_status(manager: ModelManager, model: str, expected: str) -> None:
    deadline = time.monotonic() + 2
    while time.monotonic() < deadline:
        if manager.describe(model)["status"] == expected:
            return
        time.sleep(0.01)
    pytest.fail(f"{model} did not reach {expected}")


def test_model_manager_downloads_lists_and_deletes_model(tmp_path, monkeypatch):
    cache = tmp_path / "hub"
    monkeypatch.setenv("HUGGINGFACE_HUB_CACHE", str(cache))

    def download(repository: str) -> None:
        snapshot = (
            cache
            / f"models--{repository.replace('/', '--')}"
            / "snapshots"
            / "revision"
        )
        snapshot.mkdir(parents=True)
        (snapshot / "model.bin").write_bytes(b"model")
        (snapshot / "config.json").write_text("{}", encoding="utf-8")

    manager = ModelManager(download)
    manager.start_download("tiny")
    wait_for_status(manager, "tiny", "downloaded")

    record = manager.describe("tiny")
    assert record["progress"] == 1.0
    assert record["downloaded_bytes"] > 0

    manager.delete("tiny", active_model="small")
    assert manager.describe("tiny")["status"] == "not_downloaded"


def test_model_manager_exposes_download_errors(tmp_path, monkeypatch):
    monkeypatch.setenv("HUGGINGFACE_HUB_CACHE", str(tmp_path / "hub"))

    def fail(_: str) -> None:
        raise RuntimeError("network unavailable")

    manager = ModelManager(fail)
    manager.start_download("base")
    wait_for_status(manager, "base", "error")

    record = manager.describe("base")
    assert record["error"] == "network unavailable"


def test_model_manager_protects_active_and_in_progress_models(
    tmp_path,
    monkeypatch,
):
    monkeypatch.setenv("HUGGINGFACE_HUB_CACHE", str(tmp_path / "hub"))
    manager = ModelManager()

    with pytest.raises(ValueError, match="当前正在使用"):
        manager.delete("small", active_model="small")

    with pytest.raises(ValueError, match="不支持"):
        manager.start_download("unknown")
