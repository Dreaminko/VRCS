from __future__ import annotations

from datetime import datetime, timezone
from typing import Literal

from pydantic import BaseModel, ConfigDict, Field, model_validator


Language = Literal["auto", "en", "ja", "zh", "ko", "es", "fr", "de"]
DeviceType = Literal["auto", "cpu", "cuda"]
ComputeType = Literal["int8", "float16", "int8_float16"]
SubtitleSource = Literal["speaker", "microphone"]


class Subtitle(BaseModel):
    id: int | None = None
    text: str
    language: str | None = None
    started_at: float | None = None
    ended_at: float | None = None
    source: SubtitleSource = "speaker"
    created_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))


class AudioDevice(BaseModel):
    id: int
    name: str
    is_default: bool = False
    is_loopback: bool = False
    sample_rate: int
    channels: int


class AsrSettings(BaseModel):
    model_config = ConfigDict(extra="forbid")

    model: Literal["tiny", "base", "small", "medium", "large-v3"] = "small"
    language: Language = "auto"
    device: DeviceType = "auto"
    compute_type: ComputeType = "int8"


class ServerSettings(BaseModel):
    model_config = ConfigDict(extra="forbid")

    host: str = "127.0.0.1"
    port: int = Field(default=8766, ge=1, le=65_535)


class StorageSettings(BaseModel):
    model_config = ConfigDict(extra="forbid")

    database_path: str = "data/vrcs.db"
    subtitle_history_limit: int = Field(default=500, ge=1, le=10_000)


class OutputSettings(BaseModel):
    model_config = ConfigDict(extra="forbid")

    mode: Literal["system", "vrchat"] = "system"
    device_id: int | None = None

    @model_validator(mode="after")
    def validate_mode(self) -> "OutputSettings":
        if self.mode == "vrchat" and self.device_id is not None:
            raise ValueError("VRChat 模式不能指定系统输出设备")
        return self


class MicrophoneSettings(BaseModel):
    model_config = ConfigDict(extra="forbid")

    mode: Literal["default", "device", "disabled"] = "disabled"
    device_id: int | None = None

    @model_validator(mode="after")
    def validate_mode(self) -> "MicrophoneSettings":
        if self.mode == "device" and self.device_id is None:
            raise ValueError("指定麦克风模式必须选择设备")
        if self.mode != "device" and self.device_id is not None:
            raise ValueError("默认或关闭麦克风模式不能指定设备")
        return self


class AudioSettings(BaseModel):
    model_config = ConfigDict(extra="forbid")

    sample_rate: int = Field(default=16_000, ge=8_000, le=96_000)
    output: OutputSettings = Field(default_factory=OutputSettings)
    microphone: MicrophoneSettings = Field(default_factory=MicrophoneSettings)


class VadSettings(BaseModel):
    model_config = ConfigDict(extra="forbid")

    silence_seconds: float = Field(default=0.4, ge=0.1, le=2.0)
    max_speech_seconds: float = Field(default=6.0, ge=1.0, le=30.0)


class AnkiSettings(BaseModel):
    model_config = ConfigDict(extra="forbid")

    port: int = Field(default=8765, ge=1, le=65_535)
    deck: str = Field(default="VRCS", min_length=1, max_length=100)
    model: str = Field(default="Basic", min_length=1, max_length=100)
    front_field: str = Field(default="Front", min_length=1, max_length=100)
    back_field: str = Field(default="Back", min_length=1, max_length=100)

    @model_validator(mode="after")
    def validate_field_mapping(self) -> "AnkiSettings":
        if self.front_field == self.back_field:
            raise ValueError("Anki 正面和背面不能映射到同一个字段")
        return self


class SettingsUpdate(BaseModel):
    model_config = ConfigDict(extra="forbid")

    schema_version: Literal[3] = 3
    server: ServerSettings
    storage: StorageSettings
    audio: AudioSettings
    vad: VadSettings = Field(default_factory=VadSettings)
    asr: AsrSettings
    anki: AnkiSettings = Field(default_factory=AnkiSettings)


class CardRequest(BaseModel):
    model_config = ConfigDict(extra="forbid")

    term: str = Field(min_length=1, max_length=500)
    definition: str = Field(min_length=1, max_length=20_000)
    context: str = Field(default="", max_length=20_000)
    reading: str | None = Field(default=None, max_length=500)
    dictionary: str | None = Field(default=None, max_length=500)
    language: str | None = Field(default=None, max_length=20)


class DictionaryEntry(BaseModel):
    term: str
    language: str
    definition: str
    reading: str | None = None
    dictionary: str | None = None


class DictionarySource(BaseModel):
    id: int
    title: str
    revision: str
    source_language: str
    target_language: str | None = None
    entry_count: int
    imported_at: str
