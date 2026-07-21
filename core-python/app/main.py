from __future__ import annotations

import asyncio
import os
from contextlib import asynccontextmanager
from dataclasses import asdict
from pathlib import Path

import uvicorn
from fastapi import FastAPI, HTTPException, Query, Request, WebSocket, WebSocketDisconnect
from fastapi.middleware.cors import CORSMiddleware

from .anki import AnkiError, create_card
from .asr import WhisperTranscriber
from .audio.capture import AudioCapture, AudioUnavailableError
from .config import AppConfig, AsrConfig, load_config, save_config
from .database import Database
from .dictionary import MAX_ARCHIVE_BYTES, YomitanDictionaryError
from .models import AsrSettings, CaptureRequest, CardRequest, SettingsUpdate
from .pipeline import TranscriptionPipeline
from .subtitles import SubtitleStore
from .vad import SpeechSegmenter, VoiceDetector


ROOT = Path(__file__).resolve().parent.parent
CONFIG_PATH = Path(os.environ.get("VRCS_CONFIG", ROOT / "config.json"))


class AppState:
    def __init__(self, config_path: Path = CONFIG_PATH) -> None:
        self.config_path = config_path
        self.config = load_config(config_path)
        database_path = Path(self.config.database_path)
        if not database_path.is_absolute():
            database_path = config_path.parent / database_path
        self.database = Database(database_path)
        self.database.initialize()
        self.subtitles = SubtitleStore(self.database, self.config.subtitle_history_limit)
        self.capture = AudioCapture(self.config.sample_rate, source="speaker")
        self.microphone_capture = AudioCapture(self.config.sample_rate, source="microphone")
        self.detector = VoiceDetector()
        self.microphone_detector = VoiceDetector()
        self.transcriber = WhisperTranscriber(self.config.asr)
        self.pipeline = TranscriptionPipeline(
            self.capture,
            self.detector,
            SpeechSegmenter(self.config.sample_rate),
            self.transcriber,
            self.subtitles,
            source="speaker",
        )
        self.microphone_pipeline = TranscriptionPipeline(
            self.microphone_capture,
            self.microphone_detector,
            SpeechSegmenter(self.config.sample_rate),
            self.transcriber,
            self.subtitles,
            source="microphone",
        )

    async def close(self) -> None:
        await asyncio.gather(self.pipeline.stop(), self.microphone_pipeline.stop())
        self.database.close()


def create_app(config_path: Path = CONFIG_PATH) -> FastAPI:
    @asynccontextmanager
    async def lifespan(app: FastAPI):
        app.state.core = AppState(config_path)
        yield
        await app.state.core.close()

    app = FastAPI(title="VRCS Core", version="0.1.0", lifespan=lifespan)
    app.add_middleware(
        CORSMiddleware,
        allow_origins=["*"],
        allow_methods=["*"],
        allow_headers=["*"],
    )

    def state() -> AppState:
        return app.state.core

    @app.get("/health")
    async def health() -> dict[str, object]:
        core = state()
        return {
            "status": "ok",
            "capture_running": core.pipeline.running or core.microphone_pipeline.running,
            "audio_device": core.capture.device.model_dump() if core.capture.device else None,
            "microphone_device": (
                core.microphone_capture.device.model_dump()
                if core.microphone_capture.device
                else None
            ),
            "asr_status": core.transcriber.status,
            "vad_backend": core.detector.backend,
            "last_error": core.pipeline.last_error or core.microphone_pipeline.last_error,
        }

    @app.get("/api/audio/devices")
    async def audio_devices() -> list[dict[str, object]]:
        try:
            return [item.model_dump() for item in state().capture.list_devices()]
        except AudioUnavailableError as exc:
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        except Exception as exc:
            raise HTTPException(status_code=500, detail=f"Failed to enumerate audio devices: {exc}") from exc

    @app.post("/api/capture/start")
    async def start_capture(request: CaptureRequest) -> dict[str, object]:
        core = state()
        device_id = request.device_id if request.device_id is not None else core.config.audio_device_id
        microphone_id = (
            request.microphone_device_id
            if request.microphone_device_id is not None
            else core.config.microphone_device_id
        )
        try:
            device = core.pipeline.start(device_id)
            microphone = (
                core.microphone_pipeline.start(microphone_id)
                if microphone_id is not None
                else None
            )
        except AudioUnavailableError as exc:
            await asyncio.gather(core.pipeline.stop(), core.microphone_pipeline.stop())
            raise HTTPException(status_code=503, detail=str(exc)) from exc
        except RuntimeError as exc:
            await asyncio.gather(core.pipeline.stop(), core.microphone_pipeline.stop())
            raise HTTPException(status_code=409, detail=str(exc)) from exc
        except Exception as exc:
            await asyncio.gather(core.pipeline.stop(), core.microphone_pipeline.stop())
            raise HTTPException(status_code=500, detail=f"Failed to start capture: {exc}") from exc
        return {
            "running": True,
            "device": device.model_dump(),
            "microphone_device": microphone.model_dump() if microphone else None,
        }

    @app.post("/api/capture/stop")
    async def stop_capture() -> dict[str, bool]:
        core = state()
        await asyncio.gather(core.pipeline.stop(), core.microphone_pipeline.stop())
        return {"running": False}

    @app.get("/api/subtitles")
    async def subtitle_history(limit: int = Query(500, ge=1, le=500)) -> list[dict[str, object]]:
        return [item.model_dump(mode="json") for item in state().subtitles.history(limit)]

    @app.get("/api/settings")
    async def settings() -> dict[str, object]:
        return asdict(state().config)

    @app.put("/api/settings")
    async def update_settings(update: SettingsUpdate) -> dict[str, object]:
        core = state()
        if core.pipeline.running or core.microphone_pipeline.running:
            raise HTTPException(status_code=409, detail="Stop capture before changing settings")
        core.config.audio_device_id = update.audio_device_id
        core.config.microphone_device_id = update.microphone_device_id
        core.config.asr = AsrConfig(**update.asr.model_dump())
        core.transcriber.update(core.config.asr)
        save_config(core.config_path, core.config)
        return asdict(core.config)

    @app.get("/api/dictionary")
    async def dictionary_lookup(q: str = Query(min_length=1, max_length=100)) -> list[dict[str, object]]:
        return [item.model_dump() for item in state().database.lookup(q)]

    @app.get("/api/dictionaries")
    async def dictionaries() -> list[dict[str, object]]:
        return [item.model_dump() for item in state().database.dictionary_sources()]

    @app.post("/api/dictionaries/import")
    async def import_dictionary(request: Request) -> dict[str, object]:
        content_length = request.headers.get("content-length")
        if content_length:
            try:
                if int(content_length) > MAX_ARCHIVE_BYTES:
                    raise HTTPException(status_code=413, detail="词典压缩包超过 512 MB 限制")
            except ValueError:
                raise HTTPException(status_code=400, detail="Content-Length 请求头无效") from None
        archive = await request.body()
        if len(archive) > MAX_ARCHIVE_BYTES:
            raise HTTPException(status_code=413, detail="词典压缩包超过 512 MB 限制")
        try:
            imported = state().database.import_yomitan(archive)
        except (YomitanDictionaryError, ValueError) as exc:
            raise HTTPException(status_code=422, detail=str(exc)) from exc
        return imported.model_dump()

    @app.delete("/api/dictionaries/{source_id}")
    async def delete_dictionary(source_id: int) -> dict[str, bool]:
        if not state().database.delete_dictionary_source(source_id):
            raise HTTPException(status_code=404, detail="词典不存在")
        return {"deleted": True}

    @app.post("/api/anki/cards")
    async def add_anki_card(card: CardRequest) -> dict[str, int]:
        try:
            note_id = await create_card(card)
        except (AnkiError, OSError, ValueError) as exc:
            raise HTTPException(status_code=502, detail=f"AnkiConnect error: {exc}") from exc
        return {"note_id": note_id}

    @app.websocket("/ws")
    async def subtitle_socket(websocket: WebSocket) -> None:
        await websocket.accept()
        core = state()
        queue = core.subtitles.subscribe()
        try:
            await websocket.send_json({"type": "connected"})
            while True:
                subtitle = await queue.get()
                await websocket.send_json(
                    {"type": "subtitle", "subtitle": subtitle.model_dump(mode="json")}
                )
        except WebSocketDisconnect:
            pass
        finally:
            core.subtitles.unsubscribe(queue)

    return app


app = create_app()


def run() -> None:
    config: AppConfig = load_config(CONFIG_PATH)
    uvicorn.run("app.main:app", host=config.host, port=config.port, reload=False)


if __name__ == "__main__":
    run()
