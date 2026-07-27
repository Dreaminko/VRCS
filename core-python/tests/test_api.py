import json
from io import BytesIO
from zipfile import ZIP_DEFLATED, ZipFile

from fastapi.testclient import TestClient

import app.main as main_module
from app.main import create_app
from unittest.mock import Mock

from app.audio.capture import AudioUnavailableError
from app.anki import AnkiDuplicateError
from app.config import MicrophoneConfig, OutputConfig
from app.models import AudioDevice, Subtitle


def test_run_uses_packaged_h11_protocol(monkeypatch, tmp_path):
    observed: dict[str, object] = {}

    monkeypatch.setattr(main_module, "CONFIG_PATH", tmp_path / "config.json")
    monkeypatch.setenv("VRCS_LOG_DIR", str(tmp_path / "logs"))
    monkeypatch.setattr(
        main_module.uvicorn,
        "run",
        lambda app, **kwargs: observed.update(kwargs),
    )

    main_module.run()

    assert observed["http"] == "h11"


def dictionary_archive() -> bytes:
    output = BytesIO()
    with ZipFile(output, "w", ZIP_DEFLATED) as package:
        package.writestr(
            "index.json",
            json.dumps({"title": "API Dictionary", "revision": "1", "format": 3}),
        )
        package.writestr(
            "term_bank_1.json",
            json.dumps([["学ぶ", "まなぶ", "v5", "", 1, ["学习"], 1, ""]]),
        )
    return output.getvalue()


def test_health_history_and_settings(tmp_path):
    app = create_app(tmp_path / "config.json")
    with TestClient(app) as client:
        health = client.get("/health")
        assert health.status_code == 200
        assert health.json()["status"] == "ok"
        assert health.json()["config_schema"] == 3
        assert client.get("/api/subtitles").json() == []
        settings = client.get("/api/settings").json()
        assert settings["schema_version"] == 3
        assert settings["asr"]["model"] == "small"
        assert settings["audio"]["output"]["mode"] == "system"
        assert settings["vad"] == {
            "silence_seconds": 0.4,
            "max_speech_seconds": 6.0,
        }
        assert settings["server"]["port"] == 8766
        assert settings["anki"]["port"] == 8765


def test_session_token_protects_http_and_websocket(tmp_path):
    app = create_app(tmp_path / "config.json", session_token="release-token")
    with TestClient(app) as client:
        assert client.get("/health").status_code == 401
        health = client.get(
            "/health", headers={"Authorization": "Bearer release-token"}
        )
        assert health.status_code == 200
        assert health.json()["service"] == "vrcs-core"

        with client.websocket_connect("/ws?token=release-token") as websocket:
            assert websocket.receive_json() == {"type": "connected"}


def test_websocket_accepts_subscribers(tmp_path):
    app = create_app(tmp_path / "config.json")
    with TestClient(app) as client:
        with client.websocket_connect("/ws") as websocket:
            assert websocket.receive_json() == {"type": "connected"}
            client.portal.call(
                client.app.state.core.subtitles.publish,
                Subtitle(text="hello VRChat", source="microphone"),
            )
            message = websocket.receive_json()
            assert message["type"] == "subtitle"
            assert message["subtitle"]["text"] == "hello VRChat"
            assert message["subtitle"]["source"] == "microphone"


def test_capture_start_passes_microphone_device(tmp_path):
    app = create_app(tmp_path / "config.json")
    with TestClient(app) as client:
        core = client.app.state.core
        speaker = AudioDevice(
            id=10,
            name="speaker",
            is_loopback=True,
            sample_rate=48_000,
            channels=2,
        )
        microphone = AudioDevice(
            id=20,
            name="microphone",
            sample_rate=48_000,
            channels=1,
        )
        core.pipeline.start = Mock(return_value=speaker)
        core.microphone_pipeline.start = Mock(return_value=microphone)
        core.config.audio.output = OutputConfig(mode="system", device_id=10)
        core.config.audio.microphone = MicrophoneConfig(mode="device", device_id=20)

        response = client.post("/api/capture/start", json={})

        assert response.status_code == 200
        assert response.json()["microphone_device"]["id"] == 20
        core.pipeline.start.assert_called_once_with(10, process_name=None)
        core.microphone_pipeline.start.assert_called_once_with(20)


def test_vrchat_only_setting_uses_process_capture(tmp_path):
    app = create_app(tmp_path / "config.json")
    with TestClient(app) as client:
        core = client.app.state.core
        speaker = AudioDevice(
            id=-1,
            name="VRChat（仅应用音频）",
            is_loopback=True,
            sample_rate=16_000,
            channels=1,
        )
        core.pipeline.start = Mock(return_value=speaker)
        core.config.audio.output = OutputConfig(mode="vrchat")

        response = client.post("/api/capture/start", json={})
        assert response.status_code == 200
        core.pipeline.start.assert_called_once_with(
            None,
            process_name="VRChat.exe",
        )


def test_default_and_disabled_microphone_have_distinct_capture_semantics(tmp_path):
    app = create_app(tmp_path / "config.json")
    with TestClient(app) as client:
        core = client.app.state.core
        speaker = AudioDevice(
            id=10,
            name="speaker",
            is_loopback=True,
            sample_rate=48_000,
            channels=2,
        )
        microphone = AudioDevice(
            id=20,
            name="default microphone",
            sample_rate=48_000,
            channels=1,
        )
        core.pipeline.start = Mock(return_value=speaker)
        core.microphone_pipeline.start = Mock(return_value=microphone)
        core.config.audio.microphone = MicrophoneConfig(mode="default")

        assert client.post("/api/capture/start", json={}).status_code == 200
        core.microphone_pipeline.start.assert_called_once_with(None)

        client.post("/api/capture/stop")
        core.pipeline.start.reset_mock()
        core.microphone_pipeline.start.reset_mock()
        core.config.audio.microphone = MicrophoneConfig(mode="disabled")

        assert client.post("/api/capture/start", json={}).status_code == 200
        core.microphone_pipeline.start.assert_not_called()


def test_settings_apply_rejects_stale_device_without_changing_config(tmp_path):
    app = create_app(tmp_path / "config.json")
    with TestClient(app) as client:
        core = client.app.state.core
        original = client.get("/api/settings").json()
        update = json.loads(json.dumps(original))
        update["audio"]["output"] = {"mode": "system", "device_id": 999}
        core.capture.validate_device_id = Mock(
            side_effect=AudioUnavailableError("所选系统输出设备已失效，请重新选择")
        )

        response = client.put("/api/settings", json=update)

        assert response.status_code == 422
        assert client.get("/api/settings").json() == original


def test_settings_apply_commits_complete_v2_document(tmp_path):
    app = create_app(tmp_path / "config.json")
    with TestClient(app) as client:
        update = client.get("/api/settings").json()
        update["asr"]["model"] = "base"
        update["asr"]["device"] = "cpu"
        update["asr"]["compute_type"] = "int8"

        response = client.put("/api/settings", json=update)

        assert response.status_code == 200
        assert response.json()["asr"]["model"] == "base"
        persisted = json.loads((tmp_path / "config.json").read_text(encoding="utf-8"))
        assert persisted == response.json()


def test_settings_apply_updates_both_vad_segmenters(tmp_path):
    app = create_app(tmp_path / "config.json")
    with TestClient(app) as client:
        update = client.get("/api/settings").json()
        update["vad"] = {
            "silence_seconds": 0.3,
            "max_speech_seconds": 8.0,
        }

        response = client.put("/api/settings", json=update)

        assert response.status_code == 200
        assert response.json()["vad"] == update["vad"]
        core = client.app.state.core
        assert core.pipeline.segmenter.silence_samples == 4_800
        assert core.pipeline.segmenter.max_speech_samples == 128_000
        assert core.microphone_pipeline.segmenter.silence_samples == 4_800
        assert core.microphone_pipeline.segmenter.max_speech_samples == 128_000
        persisted = json.loads((tmp_path / "config.json").read_text(encoding="utf-8"))
        assert persisted["vad"] == update["vad"]


def test_settings_rejects_vad_values_outside_supported_range(tmp_path):
    app = create_app(tmp_path / "config.json")
    with TestClient(app) as client:
        update = client.get("/api/settings").json()
        update["vad"]["silence_seconds"] = 0

        response = client.put("/api/settings", json=update)

        assert response.status_code == 422


def test_asr_capabilities_expose_models_cuda_and_combinations(tmp_path):
    app = create_app(tmp_path / "config.json")
    with TestClient(app) as client:
        response = client.get("/api/asr/capabilities")

        assert response.status_code == 200
        payload = response.json()
        assert {model["id"] for model in payload["models"]} == {
            "tiny",
            "base",
            "small",
            "medium",
            "large-v3",
        }
        assert "cpu" in payload["compute_types"]
        assert "available" in payload["cuda"]


def test_anki_status_and_card_error_mapping(monkeypatch, tmp_path):
    async def fake_status(config):
        return {
            "connected": True,
            "version": 6,
            "decks": ["VRCS"],
            "models": ["Basic"],
            "fields": ["Front", "Back"],
            "configuration_valid": True,
            "error_code": None,
            "message": "ready",
        }

    async def duplicate_card(card, config):
        raise AnkiDuplicateError("这条笔记已存在，未重复添加")

    monkeypatch.setattr(main_module, "anki_status", fake_status)
    monkeypatch.setattr(main_module, "create_card", duplicate_card)

    app = create_app(tmp_path / "config.json")
    with TestClient(app) as client:
        status = client.get("/api/anki/status")
        assert status.status_code == 200
        assert status.json()["configuration_valid"] is True

        response = client.post(
            "/api/anki/cards",
            json={"term": "hello", "definition": "greeting"},
        )
        assert response.status_code == 409
        assert "已存在" in response.json()["detail"]


def test_imports_lists_looks_up_and_deletes_yomitan_dictionary(tmp_path):
    app = create_app(tmp_path / "config.json")
    with TestClient(app) as client:
        imported = client.post(
            "/api/dictionaries/import",
            content=dictionary_archive(),
            headers={"Content-Type": "application/zip"},
        )
        assert imported.status_code == 200
        source = imported.json()
        assert source["title"] == "API Dictionary"
        assert source["entry_count"] == 1

        assert client.get("/api/dictionaries").json()[0]["title"] == "API Dictionary"
        lookup = client.get("/api/dictionary", params={"q": "学ぶ"}).json()[0]
        assert lookup["reading"] == "まなぶ"
        assert lookup["definition"] == "学习"
        assert lookup["dictionary"] == "API Dictionary"

        deleted = client.delete(f"/api/dictionaries/{source['id']}")
        assert deleted.status_code == 200
        assert client.get("/api/dictionaries").json() == []
