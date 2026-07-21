from __future__ import annotations

import json
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any


@dataclass(slots=True)
class AsrConfig:
    model: str = "small"
    language: str = "auto"
    device: str = "auto"
    compute_type: str = "int8"


@dataclass(slots=True)
class AppConfig:
    host: str = "127.0.0.1"
    port: int = 8765
    database_path: str = "data/vrcs.db"
    audio_device_id: int | None = None
    microphone_device_id: int | None = None
    sample_rate: int = 16_000
    subtitle_history_limit: int = 500
    asr: AsrConfig = field(default_factory=AsrConfig)


def _merge(config: AppConfig, raw: dict[str, Any]) -> AppConfig:
    asr = AsrConfig(**{**asdict(config.asr), **raw.get("asr", {})})
    scalar = {key: value for key, value in raw.items() if key != "asr" and hasattr(config, key)}
    return AppConfig(**{**asdict(config), **scalar, "asr": asr})


def load_config(path: Path) -> AppConfig:
    if not path.exists():
        config = AppConfig()
        save_config(path, config)
        return config
    return _merge(AppConfig(), json.loads(path.read_text(encoding="utf-8")))


def save_config(path: Path, config: AppConfig) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(asdict(config), indent=2, ensure_ascii=False), encoding="utf-8")
