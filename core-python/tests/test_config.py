import json

import pytest

from app.config import (
    AnkiConfig,
    AppConfig,
    MicrophoneConfig,
    OutputConfig,
    VadConfig,
    load_config,
    save_config,
)


def test_config_round_trip(tmp_path):
    path = tmp_path / "config.json"
    expected = AppConfig()
    expected.server.port = 18_766
    expected.audio.output = OutputConfig(mode="vrchat")
    expected.audio.microphone = MicrophoneConfig(mode="default")
    expected.vad = VadConfig(silence_seconds=0.3, max_speech_seconds=8.0)
    expected.anki = AnkiConfig(
        port=8877,
        deck="Learning",
        model="Vocabulary",
        front_field="Expression",
        back_field="Meaning",
    )

    save_config(path, expected)

    assert load_config(path) == expected
    assert json.loads(path.read_text(encoding="utf-8"))["schema_version"] == 3
    assert load_config(path).anki.deck == "Learning"


def test_missing_config_is_created(tmp_path):
    path = tmp_path / "config.json"
    assert load_config(path) == AppConfig()
    assert path.exists()


def test_v1_config_is_migrated_and_rewritten_as_v3(tmp_path):
    path = tmp_path / "config.json"
    path.write_text(
        json.dumps(
            {
                "host": "127.0.0.2",
                "port": 9_123,
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

    assert migrated.schema_version == 3
    assert migrated.server.port == 9_123
    assert migrated.storage.database_path == "legacy.db"
    assert migrated.audio.output == OutputConfig(mode="system", device_id=12)
    assert migrated.audio.microphone == MicrophoneConfig(mode="device", device_id=34)
    assert json.loads(path.read_text(encoding="utf-8"))["schema_version"] == 3


def test_v2_default_ports_are_migrated_and_rewritten(tmp_path):
    path = tmp_path / "config.json"
    path.write_text(
        json.dumps(
            {
                "schema_version": 2,
                "server": {"host": "127.0.0.1", "port": 8765},
                "anki": {
                    "port": 8766,
                    "deck": "VRCS",
                    "model": "Basic",
                    "front_field": "Front",
                    "back_field": "Back",
                },
            }
        ),
        encoding="utf-8",
    )

    migrated = load_config(path)

    assert migrated.schema_version == 3
    assert migrated.server.port == 8766
    assert migrated.anki.port == 8765
    persisted = json.loads(path.read_text(encoding="utf-8"))
    assert persisted["schema_version"] == 3
    assert persisted["server"]["port"] == 8766
    assert persisted["anki"]["port"] == 8765


def test_v2_custom_ports_are_preserved(tmp_path):
    path = tmp_path / "config.json"
    path.write_text(
        json.dumps(
            {
                "schema_version": 2,
                "server": {"host": "127.0.0.1", "port": 18_765},
                "anki": {
                    "port": 18_766,
                    "deck": "VRCS",
                    "model": "Basic",
                    "front_field": "Front",
                    "back_field": "Back",
                },
            }
        ),
        encoding="utf-8",
    )

    migrated = load_config(path)

    assert migrated.server.port == 18_765
    assert migrated.anki.port == 18_766


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
    path.write_text('{"schema_version": 4}', encoding="utf-8")

    with pytest.raises(ValueError, match="Unsupported configuration schema v4"):
        load_config(path)


def test_current_config_without_anki_section_uses_compatible_defaults(tmp_path):
    path = tmp_path / "config.json"
    save_config(path, AppConfig())
    payload = json.loads(path.read_text(encoding="utf-8"))
    payload.pop("anki", None)
    path.write_text(json.dumps(payload), encoding="utf-8")

    loaded = load_config(path)

    assert loaded.anki == AnkiConfig()


def test_current_config_without_vad_section_uses_low_latency_defaults(tmp_path):
    path = tmp_path / "config.json"
    save_config(path, AppConfig())
    payload = json.loads(path.read_text(encoding="utf-8"))
    payload.pop("vad")
    path.write_text(json.dumps(payload), encoding="utf-8")

    loaded = load_config(path)

    assert loaded.vad == VadConfig(silence_seconds=0.4, max_speech_seconds=6.0)


def test_current_config_rejects_vad_values_outside_supported_range(tmp_path):
    path = tmp_path / "config.json"
    save_config(path, AppConfig())
    payload = json.loads(path.read_text(encoding="utf-8"))
    payload["vad"]["max_speech_seconds"] = 60
    path.write_text(json.dumps(payload), encoding="utf-8")

    with pytest.raises(ValueError, match="max_speech_seconds"):
        load_config(path)


def test_default_ports_do_not_overlap_anki_or_vrchat_osc():
    config = AppConfig()

    assert config.server.port == 8766
    assert config.anki.port == 8765
    assert config.server.port not in {config.anki.port, 9000, 9001}
