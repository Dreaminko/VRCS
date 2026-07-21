from __future__ import annotations

import asyncio
from typing import Any, Literal

import numpy as np

from ..models import AudioDevice


class AudioUnavailableError(RuntimeError):
    pass


class AudioCapture:
    """Small async wrapper around PyAudioWPatch WASAPI loopback capture."""

    def __init__(
        self,
        output_rate: int = 16_000,
        source: Literal["speaker", "microphone"] = "speaker",
    ) -> None:
        self.output_rate = output_rate
        self.source = source
        self._audio: Any = None
        self._stream: Any = None
        self._input_rate = output_rate
        self._channels = 1
        self._frames_per_buffer = 512
        self.device: AudioDevice | None = None

    @staticmethod
    def _module() -> Any:
        try:
            import pyaudiowpatch as pyaudio
        except ImportError as exc:
            raise AudioUnavailableError(
                "Audio support is not installed. Run: pip install -e '.[audio]'"
            ) from exc
        return pyaudio

    def list_devices(self) -> list[AudioDevice]:
        pyaudio = self._module()
        with pyaudio.PyAudio() as audio:
            default_loopback = audio.get_default_wasapi_loopback()
            default_loopback_id = int(default_loopback["index"])
            loopbacks = list(audio.get_loopback_device_info_generator())
            loopback_ids = {int(item["index"]) for item in loopbacks}
            devices = [
                AudioDevice(
                    id=int(item["index"]),
                    name=str(item["name"]),
                    is_default=int(item["index"]) == default_loopback_id,
                    is_loopback=True,
                    sample_rate=int(item["defaultSampleRate"]),
                    channels=int(item["maxInputChannels"]),
                )
                for item in loopbacks
            ]
            wasapi_id = int(audio.get_host_api_info_by_type(pyaudio.paWASAPI)["index"])
            try:
                default_input_id = int(audio.get_default_input_device_info()["index"])
            except OSError:
                default_input_id = -1
            devices.extend(
                AudioDevice(
                    id=int(item["index"]),
                    name=str(item["name"]),
                    is_default=int(item["index"]) == default_input_id,
                    is_loopback=False,
                    sample_rate=int(item["defaultSampleRate"]),
                    channels=int(item["maxInputChannels"]),
                )
                for index in range(audio.get_device_count())
                if (item := audio.get_device_info_by_index(index))
                and int(item["hostApi"]) == wasapi_id
                and int(item["maxInputChannels"]) > 0
                and int(item["index"]) not in loopback_ids
            )
            return devices

    def start(self, device_id: int | None = None) -> AudioDevice:
        if self._stream is not None:
            raise RuntimeError("Audio capture is already running")
        pyaudio = self._module()
        self._audio = pyaudio.PyAudio()
        if device_id is not None:
            info = self._audio.get_device_info_by_index(device_id)
        elif self.source == "speaker":
            info = self._audio.get_default_wasapi_loopback()
        else:
            info = self._audio.get_default_input_device_info()
        self._input_rate = int(info["defaultSampleRate"])
        self._channels = max(1, int(info["maxInputChannels"]))
        self._frames_per_buffer = round(self._input_rate * 512 / self.output_rate)
        self._stream = self._audio.open(
            format=pyaudio.paInt16,
            channels=self._channels,
            rate=self._input_rate,
            input=True,
            input_device_index=int(info["index"]),
            frames_per_buffer=self._frames_per_buffer,
        )
        self.device = AudioDevice(
            id=int(info["index"]),
            name=str(info["name"]),
            is_default=device_id is None,
            is_loopback=self.source == "speaker",
            sample_rate=self._input_rate,
            channels=self._channels,
        )
        return self.device

    async def read(self) -> np.ndarray:
        if self._stream is None:
            raise RuntimeError("Audio capture is not running")
        loop = asyncio.get_running_loop()
        read_future = loop.run_in_executor(
            None,
            self._stream.read,
            self._frames_per_buffer,
            False,
        )
        try:
            raw = await asyncio.shield(read_future)
        except asyncio.CancelledError:
            try:
                await read_future
            except Exception:
                pass
            raise
        audio = np.frombuffer(raw, dtype=np.int16).astype(np.float32) / 32768.0
        if self._channels > 1:
            audio = audio.reshape(-1, self._channels).mean(axis=1)
        if self._input_rate != self.output_rate and len(audio) > 1:
            size = round(len(audio) * self.output_rate / self._input_rate)
            source = np.linspace(0.0, 1.0, num=len(audio), endpoint=False)
            target = np.linspace(0.0, 1.0, num=size, endpoint=False)
            audio = np.interp(target, source, audio).astype(np.float32)
        return audio

    def stop(self) -> None:
        if self._stream is not None:
            self._stream.stop_stream()
            self._stream.close()
            self._stream = None
        if self._audio is not None:
            self._audio.terminate()
            self._audio = None
        self.device = None
