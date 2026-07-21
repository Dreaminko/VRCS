from fastapi.testclient import TestClient

from app.main import create_app
from unittest.mock import Mock

from app.models import AudioDevice, Subtitle


def test_health_history_and_settings(tmp_path):
    app = create_app(tmp_path / "config.json")
    with TestClient(app) as client:
        health = client.get("/health")
        assert health.status_code == 200
        assert health.json()["status"] == "ok"
        assert client.get("/api/subtitles").json() == []
        assert client.get("/api/settings").json()["asr"]["model"] == "small"


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

        response = client.post(
            "/api/capture/start",
            json={"device_id": 10, "microphone_device_id": 20},
        )

        assert response.status_code == 200
        assert response.json()["microphone_device"]["id"] == 20
        core.pipeline.start.assert_called_once_with(10)
        core.microphone_pipeline.start.assert_called_once_with(20)
