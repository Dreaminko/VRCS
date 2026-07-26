from __future__ import annotations

import json
import os
import tempfile
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any, Literal


SCHEMA_VERSION = 2
OutputMode = Literal["system", "vrchat"]
MicrophoneMode = Literal["default", "device", "disabled"]


@dataclass(slots=True)
class ServerConfig:
    host: str = "127.0.0.1"
    port: int = 8765


@dataclass(slots=True)
class StorageConfig:
    database_path: str = "data/vrcs.db"
    subtitle_history_limit: int = 500


@dataclass(slots=True)
class OutputConfig:
    mode: OutputMode = "system"
    device_id: int | None = None


@dataclass(slots=True)
class MicrophoneConfig:
    mode: MicrophoneMode = "disabled"
    device_id: int | None = None


@dataclass(slots=True)
class AudioConfig:
    sample_rate: int = 16_000
    output: OutputConfig = field(default_factory=OutputConfig)
    microphone: MicrophoneConfig = field(default_factory=MicrophoneConfig)


@dataclass(slots=True)
class AsrConfig:
    model: str = "small"
    language: str = "auto"
    device: str = "auto"
    compute_type: str = "int8"


@dataclass(slots=True)
class AppConfig:
    schema_version: int = SCHEMA_VERSION
    server: ServerConfig = field(default_factory=ServerConfig)
    storage: StorageConfig = field(default_factory=StorageConfig)
    audio: AudioConfig = field(default_factory=AudioConfig)
    asr: AsrConfig = field(default_factory=AsrConfig)


def _dataclass_values(instance: Any, raw: Any) -> dict[str, Any]:
    if not isinstance(raw, dict):
        return asdict(instance)
    return {
        key: raw.get(key, value)
        for key, value in asdict(instance).items()
    }


def _decode_v2(raw: dict[str, Any]) -> AppConfig:
    defaults = AppConfig()
    audio_raw = raw.get("audio", {})
    if not isinstance(audio_raw, dict):
        audio_raw = {}
    return AppConfig(
        schema_version=SCHEMA_VERSION,
        server=ServerConfig(**_dataclass_values(defaults.server, raw.get("server"))),
        storage=StorageConfig(**_dataclass_values(defaults.storage, raw.get("storage"))),
        audio=AudioConfig(
            sample_rate=int(audio_raw.get("sample_rate", defaults.audio.sample_rate)),
            output=OutputConfig(
                **_dataclass_values(defaults.audio.output, audio_raw.get("output"))
            ),
            microphone=MicrophoneConfig(
                **_dataclass_values(
                    defaults.audio.microphone,
                    audio_raw.get("microphone"),
                )
            ),
        ),
        asr=AsrConfig(**_dataclass_values(defaults.asr, raw.get("asr"))),
    )


def config_from_dict(raw: dict[str, Any]) -> AppConfig:
    if raw.get("schema_version") != SCHEMA_VERSION:
        raise ValueError(f"Expected configuration schema v{SCHEMA_VERSION}")
    return _decode_v2(raw)


def _migrate_v1(raw: dict[str, Any]) -> AppConfig:
    defaults = AppConfig()
    microphone_device_id = raw.get("microphone_device_id")
    vrchat_only = bool(raw.get("vrchat_only", False))
    return AppConfig(
        server=ServerConfig(
            host=str(raw.get("host", defaults.server.host)),
            port=int(raw.get("port", defaults.server.port)),
        ),
        storage=StorageConfig(
            database_path=str(
                raw.get("database_path", defaults.storage.database_path)
            ),
            subtitle_history_limit=int(
                raw.get(
                    "subtitle_history_limit",
                    defaults.storage.subtitle_history_limit,
                )
            ),
        ),
        audio=AudioConfig(
            sample_rate=int(raw.get("sample_rate", defaults.audio.sample_rate)),
            output=OutputConfig(
                mode="vrchat" if vrchat_only else "system",
                device_id=None if vrchat_only else raw.get("audio_device_id"),
            ),
            microphone=MicrophoneConfig(
                mode="device" if microphone_device_id is not None else "disabled",
                device_id=microphone_device_id,
            ),
        ),
        asr=AsrConfig(**_dataclass_values(defaults.asr, raw.get("asr"))),
    )


def load_config(path: Path) -> AppConfig:
    if not path.exists():
        config = AppConfig()
        save_config(path, config)
        return config
    raw = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(raw, dict):
        raise ValueError("Configuration root must be an object")
    version = raw.get("schema_version", 1)
    if version not in {1, SCHEMA_VERSION}:
        raise ValueError(f"Unsupported configuration schema v{version}")
    migrated = version == 1
    config = _migrate_v1(raw) if migrated else config_from_dict(raw)
    if migrated:
        save_config(path, config)
    return config


def save_config(path: Path, config: AppConfig) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = json.dumps(asdict(config), indent=2, ensure_ascii=False)
    descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{path.name}.",
        suffix=".tmp",
        dir=path.parent,
        text=True,
    )
    temporary_path = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8", newline="\n") as stream:
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary_path, path)
    except Exception:
        temporary_path.unlink(missing_ok=True)
        raise
