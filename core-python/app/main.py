from __future__ import annotations

import asyncio
import logging
import os
import secrets
import sys
from contextlib import asynccontextmanager
from dataclasses import asdict
from logging.handlers import RotatingFileHandler
from pathlib import Path

import uvicorn
from fastapi import FastAPI, HTTPException, Query, Request, WebSocket, WebSocketDisconnect
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import JSONResponse

from .anki import AnkiError, anki_status, create_card
from .asr import WhisperTranscriber
from .asr.capabilities import asr_capabilities, validate_asr_config
from .asr.model_manager import ModelManager
from .audio.capture import AudioCapture, AudioUnavailableError
from .config import AppConfig, config_from_dict, load_config, save_config
from .database import Database
from .dictionary import MAX_ARCHIVE_BYTES, YomitanDictionaryError
from .models import CardRequest, SettingsUpdate
from .pipeline import TranscriptionPipeline
from .subtitles import SubtitleStore
from .vad import SpeechSegmenter, VoiceDetector


ROOT = Path(__file__).resolve().parent.parent


def _default_config_path() -> Path:
    configured = os.environ.get("VRCS_CONFIG")
    if configured:
        return Path(configured)
    local_app_data = os.environ.get("LOCALAPPDATA")
    if getattr(sys, "frozen", False) and local_app_data:
        return Path(local_app_data) / ".vrcs" / "config.json"
    return ROOT / "config.json"


CONFIG_PATH = _default_config_path()
CORE_VERSION = "0.1.0"
ALLOWED_ORIGINS = [
    "http://tauri.localhost",
    "https://tauri.localhost",
    "tauri://localhost",
    "http://localhost:1420",
]


class AppState:
    def __init__(self, config_path: Path = CONFIG_PATH) -> None:
        self.config_path = config_path
        self.config = load_config(config_path)
        database_path = Path(self.config.storage.database_path)
        if not database_path.is_absolute():
            database_path = config_path.parent / database_path
        self.database = Database(database_path)
        self.database.initialize()
        self.subtitles = SubtitleStore(
            self.database,
            self.config.storage.subtitle_history_limit,
        )
        self.capture = AudioCapture(self.config.audio.sample_rate, source="speaker")
        self.microphone_capture = AudioCapture(
            self.config.audio.sample_rate,
            source="microphone",
        )
        self.detector = VoiceDetector()
        self.microphone_detector = VoiceDetector()
        self.transcriber = WhisperTranscriber(self.config.asr)
        self.model_manager = ModelManager()
        self.pipeline = TranscriptionPipeline(
            self.capture,
            self.detector,
            self.create_segmenter(self.config),
            self.transcriber,
            self.subtitles,
            source="speaker",
        )
        self.microphone_pipeline = TranscriptionPipeline(
            self.microphone_capture,
            self.microphone_detector,
            self.create_segmenter(self.config),
            self.transcriber,
            self.subtitles,
            source="microphone",
        )
        self.settings_lock = asyncio.Lock()

    @staticmethod
    def create_segmenter(config: AppConfig) -> SpeechSegmenter:
        return SpeechSegmenter(
            config.audio.sample_rate,
            silence_seconds=config.vad.silence_seconds,
            max_speech_seconds=config.vad.max_speech_seconds,
        )

    async def close(self) -> None:
        await asyncio.gather(self.pipeline.stop(), self.microphone_pipeline.stop())
        self.database.close()


def create_app(
    config_path: Path = CONFIG_PATH,
    session_token: str | None = None,
) -> FastAPI:
    @asynccontextmanager
    async def lifespan(app: FastAPI):
        app.state.core = AppState(config_path)
        yield
        await app.state.core.close()

    app = FastAPI(title="VRCS Core", version=CORE_VERSION, lifespan=lifespan)
    app.add_middleware(
        CORSMiddleware,
        allow_origins=ALLOWED_ORIGINS,
        allow_methods=["*"],
        allow_headers=["Authorization", "Content-Type"],
    )

    @app.middleware("http")
    async def authenticate(request: Request, call_next):
        if session_token and request.method != "OPTIONS":
            supplied = request.headers.get("authorization", "")
            expected = f"Bearer {session_token}"
            if not secrets.compare_digest(supplied, expected):
                return JSONResponse(status_code=401, content={"detail": "Unauthorized"})
        return await call_next(request)

    def state() -> AppState:
        return app.state.core

    @app.get("/health")
    async def health() -> dict[str, object]:
        core = state()
        return {
            "status": "ok",
            "service": "vrcs-core",
            "version": CORE_VERSION,
            "config_schema": core.config.schema_version,
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
    async def start_capture() -> dict[str, object]:
        core = state()
        output = core.config.audio.output
        microphone_config = core.config.audio.microphone
        device_id = output.device_id if output.mode == "system" else None
        process_name = "VRChat.exe" if output.mode == "vrchat" else None
        try:
            device = core.pipeline.start(
                device_id,
                process_name=process_name,
            )
            if microphone_config.mode == "disabled":
                microphone = None
            else:
                microphone_id = (
                    microphone_config.device_id
                    if microphone_config.mode == "device"
                    else None
                )
                microphone = core.microphone_pipeline.start(microphone_id)
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
        candidate = config_from_dict(update.model_dump())
        if (
            core.pipeline.running or core.microphone_pipeline.running
        ) and (
            candidate.audio != core.config.audio
            or candidate.vad != core.config.vad
            or candidate.asr != core.config.asr
        ):
            raise HTTPException(
                status_code=409,
                detail="Stop capture before changing audio, segmentation, or recognition settings",
            )
        async with core.settings_lock:
            if candidate.server != core.config.server:
                raise HTTPException(
                    status_code=422,
                    detail="Core 地址属于启动配置，不能在运行中修改",
                )
            if candidate.storage != core.config.storage:
                raise HTTPException(
                    status_code=422,
                    detail="存储配置不能在运行中修改",
                )
            if candidate.audio.sample_rate != core.config.audio.sample_rate:
                raise HTTPException(
                    status_code=422,
                    detail="采样率不能在运行中修改",
                )
            try:
                if (
                    candidate.audio.output.mode == "system"
                    and candidate.audio.output.device_id is not None
                ):
                    core.capture.validate_device_id(
                        candidate.audio.output.device_id
                    )
                if candidate.audio.microphone.mode == "device":
                    core.microphone_capture.validate_device_id(
                        candidate.audio.microphone.device_id  # type: ignore[arg-type]
                    )
                validate_asr_config(candidate.asr)
                await asyncio.to_thread(save_config, core.config_path, candidate)
            except (AudioUnavailableError, ValueError) as exc:
                raise HTTPException(status_code=422, detail=str(exc)) from exc
            vad_changed = candidate.vad != core.config.vad
            core.config = candidate
            if vad_changed:
                core.pipeline.segmenter = core.create_segmenter(candidate)
                core.microphone_pipeline.segmenter = core.create_segmenter(candidate)
            core.transcriber.update(candidate.asr)
            return asdict(core.config)

    @app.get("/api/asr/capabilities")
    async def get_asr_capabilities() -> dict[str, object]:
        core = state()
        return asr_capabilities(core.config.asr, core.transcriber.status)

    @app.get("/api/asr/models")
    async def get_asr_models() -> list[dict[str, object]]:
        core = state()
        return core.model_manager.list(
            active_model=core.config.asr.model,
            runtime_status=core.transcriber.status,
        )

    @app.post("/api/asr/models/{model}/download", status_code=202)
    async def download_asr_model(model: str) -> dict[str, object]:
        core = state()
        try:
            core.model_manager.start_download(model)
            return core.model_manager.describe(
                model,
                active_model=core.config.asr.model,
                runtime_status=core.transcriber.status,
            )
        except ValueError as exc:
            raise HTTPException(status_code=404, detail=str(exc)) from exc

    @app.delete("/api/asr/models/{model}")
    async def delete_asr_model(model: str) -> dict[str, bool]:
        core = state()
        try:
            await asyncio.to_thread(
                core.model_manager.delete,
                model,
                active_model=core.config.asr.model,
            )
        except ValueError as exc:
            status_code = 409 if model == core.config.asr.model else 404
            raise HTTPException(status_code=status_code, detail=str(exc)) from exc
        except RuntimeError as exc:
            raise HTTPException(status_code=409, detail=str(exc)) from exc
        return {"deleted": True}

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

    @app.get("/api/anki/status")
    async def get_anki_status() -> dict[str, object]:
        return await anki_status(state().config.anki)

    @app.post("/api/anki/cards")
    async def add_anki_card(card: CardRequest) -> dict[str, int]:
        try:
            note_id = await create_card(card, state().config.anki)
        except AnkiError as exc:
            raise HTTPException(status_code=exc.status_code, detail=str(exc)) from exc
        return {"note_id": note_id}

    @app.websocket("/ws")
    async def subtitle_socket(websocket: WebSocket) -> None:
        if session_token and not secrets.compare_digest(
            websocket.query_params.get("token", ""), session_token
        ):
            await websocket.close(code=1008, reason="Unauthorized")
            return
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


def run() -> None:
    config: AppConfig = load_config(CONFIG_PATH)
    host = os.environ.get("VRCS_HOST", config.server.host)
    port = int(os.environ.get("VRCS_PORT", str(config.server.port)))
    session_token = os.environ.get("VRCS_SESSION_TOKEN")
    log_dir = Path(os.environ.get("VRCS_LOG_DIR", CONFIG_PATH.parent / "logs"))
    log_dir.mkdir(parents=True, exist_ok=True)
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
        handlers=[
            RotatingFileHandler(
                log_dir / "vrcs-core.log",
                maxBytes=5 * 1024 * 1024,
                backupCount=3,
                encoding="utf-8",
            )
        ],
        force=True,
    )
    runtime_app = create_app(CONFIG_PATH, session_token=session_token)
    uvicorn.run(
        runtime_app,
        host=host,
        port=port,
        http="h11",
        reload=False,
        log_config=None,
        access_log=False,
    )


if __name__ == "__main__":
    run()
