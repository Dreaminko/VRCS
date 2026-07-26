import json

import pytest

from app.config import (
    AppConfig,
    MicrophoneConfig,
    OutputConfig,
    load_config,
    save_config,
)


def test_config_round_trip(tmp_path):
    path = tmp_path / "config.json"
    expected = AppConfig()
    expected.server.port = 9000
    expected.audio.output = OutputConfig(mode="vrchat")
    expected.audio.microphone = MicrophoneConfig(mode="default")

    save_config(path, expected)

    assert load_config(path) == expected
    assert json.loads(path.read_text(encoding="utf-8"))["schema_version"] == 2


def test_missing_config_is_created(tmp_path):
    path = tmp_path / "config.json"
    assert load_config(path) == AppConfig()
    assert path.exists()


def test_v1_config_is_migrated_and_rewritten_as_v2(tmp_path):
    path = tmp_path / "config.json"
    path.write_text(
        json.dumps(
            {
                "host": "127.0.0.2",
                "port": 9000,
                "database_path": "legacy.db",
                "audio_device_id": 12,
                "vrchat_only": False,
                "microphone_device_id": 34,
                "sample_rate": 16_000,
                "subtitle_history_limit": 800,
                "asr": {
                    "model": "base",
                    "language": "ja",
                    "device": "cpu",
                    "compute_type": "int8",
                },
            }
        ),
        encoding="utf-8",
    )

    migrated = load_config(path)

    assert migrated.schema_version == 2
    assert migrated.server.port == 9000
    assert migrated.storage.database_path == "legacy.db"
    assert migrated.audio.output == OutputConfig(mode="system", device_id=12)
    assert migrated.audio.microphone == MicrophoneConfig(mode="device", device_id=34)
    assert json.loads(path.read_text(encoding="utf-8"))["schema_version"] == 2


def test_atomic_save_keeps_previous_file_when_replace_fails(tmp_path, monkeypatch):
    path = tmp_path / "config.json"
    original = AppConfig()
    save_config(path, original)
    updated = AppConfig()
    updated.asr.model = "base"

    monkeypatch.setattr("app.config.os.replace", lambda *_: (_ for _ in ()).throw(OSError("locked")))

    with pytest.raises(OSError, match="locked"):
        save_config(path, updated)

    assert json.loads(path.read_text(encoding="utf-8"))["asr"]["model"] == "small"
    assert list(tmp_path.glob("*.tmp")) == []


def test_future_config_schema_is_not_treated_as_v1(tmp_path):
    path = tmp_path / "config.json"
    path.write_text('{"schema_version": 3}', encoding="utf-8")

    with pytest.raises(ValueError, match="Unsupported configuration schema v3"):
        load_config(path)
