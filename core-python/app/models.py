from __future__ import annotations

from datetime import datetime, timezone
from typing import Literal

from pydantic import BaseModel, Field


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
    model: Literal["tiny", "base", "small", "medium", "large-v3"] = "small"
    language: Language = "auto"
    device: DeviceType = "auto"
    compute_type: ComputeType = "int8"


class SettingsUpdate(BaseModel):
    asr: AsrSettings
    audio_device_id: int | None = None
    microphone_device_id: int | None = None


class CaptureRequest(BaseModel):
    device_id: int | None = None
    microphone_device_id: int | None = None


class CardRequest(BaseModel):
    front: str
    back: str
    context: str = ""
    deck: str = "VRCS"
    model: str = "Basic"


class DictionaryEntry(BaseModel):
    term: str
    language: str
    definition: str
