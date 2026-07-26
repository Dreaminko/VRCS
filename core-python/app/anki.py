from __future__ import annotations

import asyncio
import html
import json
import re
from dataclasses import dataclass
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

from .config import AnkiConfig
from .models import CardRequest


ANKI_CONNECT_API_VERSION = 6
ANKI_CONNECT_HOST = "127.0.0.1"
_NOTE_FONT_STACK = (
    "-apple-system, BlinkMacSystemFont, 'Segoe UI', 'PingFang SC', "
    "'Hiragino Sans GB', 'Noto Sans CJK SC', sans-serif"
)
_DICTIONARY_HEADING = re.compile(r"^【(?P<label>.+)】$")
_NUMBERED_GLOSS = re.compile(r"^\d+[.)、]\s*(?P<text>.+)$")


class AnkiError(RuntimeError):
    status_code = 502
    code = "anki_error"


class AnkiUnavailableError(AnkiError):
    status_code = 503
    code = "unavailable"


class AnkiConfigurationError(AnkiError):
    status_code = 422
    code = "invalid_configuration"


class AnkiDuplicateError(AnkiError):
    status_code = 409
    code = "duplicate"


class AnkiProtocolError(AnkiError):
    code = "protocol_error"


@dataclass(slots=True)
class AnkiDiscovery:
    version: int
    decks: list[str]
    models: list[str]
    fields: list[str]
    configuration_valid: bool
    error_code: str | None
    message: str


def _endpoint(config: AnkiConfig) -> str:
    if not 1 <= config.port <= 65_535:
        raise AnkiConfigurationError("AnkiConnect 端口必须在 1 到 65535 之间")
    return f"http://{ANKI_CONNECT_HOST}:{config.port}"


def _invoke(
    config: AnkiConfig,
    action: str,
    params: dict[str, Any] | None = None,
) -> Any:
    payload: dict[str, Any] = {
        "action": action,
        "version": ANKI_CONNECT_API_VERSION,
    }
    if params is not None:
        payload["params"] = params
    request = Request(
        _endpoint(config),
        data=json.dumps(payload, ensure_ascii=False).encode("utf-8"),
        headers={"Content-Type": "application/json"},
    )
    try:
        with urlopen(request, timeout=5) as response:  # noqa: S310 - fixed localhost endpoint
            result = json.load(response)
    except HTTPError as exc:
        raise AnkiUnavailableError(
            f"端口 {config.port} 没有响应 AnkiConnect，请检查端口是否被 VRCS Core 或其他服务占用"
        ) from exc
    except (URLError, TimeoutError, OSError) as exc:
        raise AnkiUnavailableError(
            f"无法连接 AnkiConnect（127.0.0.1:{config.port}），请先启动 Anki"
        ) from exc
    except (json.JSONDecodeError, UnicodeDecodeError) as exc:
        raise AnkiProtocolError("本地端口返回了无法识别的响应，不像 AnkiConnect") from exc

    if not isinstance(result, dict) or "result" not in result or "error" not in result:
        raise AnkiProtocolError("本地端口返回了无法识别的响应，不像 AnkiConnect")
    if result["error"] is not None:
        raise AnkiProtocolError(str(result["error"]))
    return result["result"]


def _string_list(value: Any, label: str) -> list[str]:
    if not isinstance(value, list) or any(not isinstance(item, str) for item in value):
        raise AnkiProtocolError(f"AnkiConnect 返回的{label}格式无效")
    return value


def _discover(config: AnkiConfig) -> AnkiDiscovery:
    version_result = _invoke(config, "version")
    if not isinstance(version_result, int):
        raise AnkiProtocolError("AnkiConnect 返回了无效版本号")
    if version_result < ANKI_CONNECT_API_VERSION:
        return AnkiDiscovery(
            version=version_result,
            decks=[],
            models=[],
            fields=[],
            configuration_valid=False,
            error_code="incompatible_version",
            message=f"AnkiConnect API v{version_result} 过旧，需要 v{ANKI_CONNECT_API_VERSION} 或更高版本",
        )

    catalog = _invoke(
        config,
        "multi",
        {
            "actions": [
                {"action": "deckNames"},
                {"action": "modelNames"},
            ]
        },
    )
    if not isinstance(catalog, list) or len(catalog) != 2:
        raise AnkiProtocolError("AnkiConnect 返回的牌组或笔记类型列表无效")
    decks = _string_list(catalog[0], "牌组列表")
    models = _string_list(catalog[1], "笔记类型列表")
    fields = (
        _string_list(
            _invoke(config, "modelFieldNames", {"modelName": config.model}),
            "字段列表",
        )
        if config.model in models
        else []
    )

    if config.deck not in decks:
        return AnkiDiscovery(
            version_result,
            decks,
            models,
            fields,
            False,
            "missing_deck",
            f"找不到牌组“{config.deck}”，请先在 Anki 中创建或选择其他牌组",
        )
    if config.model not in models:
        return AnkiDiscovery(
            version_result,
            decks,
            models,
            fields,
            False,
            "missing_model",
            f"找不到笔记类型“{config.model}”",
        )
    missing_fields = [
        name for name in (config.front_field, config.back_field) if name not in fields
    ]
    if missing_fields:
        return AnkiDiscovery(
            version_result,
            decks,
            models,
            fields,
            False,
            "missing_field",
            f"笔记类型“{config.model}”缺少字段：{'、'.join(missing_fields)}",
        )
    if config.front_field == config.back_field:
        return AnkiDiscovery(
            version_result,
            decks,
            models,
            fields,
            False,
            "duplicate_field_mapping",
            "正面和背面不能映射到同一个字段",
        )
    return AnkiDiscovery(
        version_result,
        decks,
        models,
        fields,
        True,
        None,
        "AnkiConnect 已连接，制卡配置有效",
    )


def _status(config: AnkiConfig) -> dict[str, object]:
    try:
        discovery = _discover(config)
    except AnkiError as exc:
        return {
            "connected": False,
            "version": None,
            "decks": [],
            "models": [],
            "fields": [],
            "configuration_valid": False,
            "error_code": exc.code,
            "message": str(exc),
        }
    return {
        "connected": True,
        "version": discovery.version,
        "decks": discovery.decks,
        "models": discovery.models,
        "fields": discovery.fields,
        "configuration_valid": discovery.configuration_valid,
        "error_code": discovery.error_code,
        "message": discovery.message,
    }


def _escaped_lines(value: str) -> str:
    return (
        html.escape(value, quote=True)
        .replace("\r\n", "\n")
        .replace("\r", "\n")
        .replace("\n", "<br>")
    )


def _section_label(label: str) -> str:
    return (
        '<div class="vrcs-section-label" '
        'style="display:flex;align-items:center;gap:0.45rem;'
        'margin-bottom:0.75rem;font-size:0.75rem;font-weight:700;'
        'line-height:1.4;opacity:0.62;">'
        '<span aria-hidden="true" style="display:inline-block;width:0.42rem;'
        'height:0.42rem;border-radius:999px;background:#3d73a8;"></span>'
        f"{label}</div>"
    )


def _definition_html(value: str) -> str:
    normalized = value.replace("\r\n", "\n").replace("\r", "\n").strip()
    rendered_blocks: list[str] = []
    for raw_block in re.split(r"\n\s*\n", normalized):
        lines = [line.strip() for line in raw_block.splitlines() if line.strip()]
        if not lines:
            continue

        heading_match = _DICTIONARY_HEADING.fullmatch(lines[0])
        content_lines = lines[1:] if heading_match else lines
        glosses = [_NUMBERED_GLOSS.fullmatch(line) for line in content_lines]
        is_numbered_list = bool(glosses) and all(match is not None for match in glosses)
        block_margin = "0" if not rendered_blocks else "1.15rem"
        block_parts = [
            f'<div class="vrcs-definition-block" style="margin-top:{block_margin};">'
        ]

        if heading_match:
            block_parts.append(
                '<div class="vrcs-dictionary-label" '
                'style="display:inline-block;padding:0.2rem 0.52rem;'
                'border-radius:999px;background:rgba(61,115,168,0.12);'
                'font-size:0.76rem;font-weight:600;line-height:1.45;'
                'opacity:0.82;">'
                f"{html.escape(heading_match.group('label'), quote=True)}</div>"
            )

        if is_numbered_list:
            block_parts.append(
                '<ol class="vrcs-gloss-list" '
                'style="margin:0.65rem 0 0;padding-left:1.45rem;">'
            )
            block_parts.extend(
                '<li style="margin:0.34rem 0;padding-left:0.18rem;">'
                f"{html.escape(match.group('text'), quote=True)}</li>"
                for match in glosses
                if match is not None
            )
            block_parts.append("</ol>")
        elif content_lines:
            content = "<br>".join(
                html.escape(line, quote=True) for line in content_lines
            )
            content_margin = "0.62rem" if heading_match else "0"
            block_parts.append(
                '<div class="vrcs-definition-text" '
                f'style="margin-top:{content_margin};">{content}</div>'
            )

        block_parts.append("</div>")
        rendered_blocks.append("".join(block_parts))

    return "".join(rendered_blocks)


def _note(card: CardRequest, config: AnkiConfig) -> dict[str, object]:
    front_parts = [
        '<div class="vrcs-note vrcs-note-front" '
        f'style="max-width:42rem;margin:0 auto;padding:0.35rem 0;'
        f"text-align:center;font-family:{_NOTE_FONT_STACK};color:inherit;"
        'overflow-wrap:anywhere;">',
        '<div class="vrcs-term" '
        'style="font-size:2rem;font-weight:700;line-height:1.25;">'
        f"{_escaped_lines(card.term)}</div>",
    ]
    if card.reading:
        front_parts.append(
            '<div class="vrcs-reading" '
            'style="margin-top:0.48rem;font-size:0.95rem;line-height:1.5;'
            'opacity:0.62;">'
            f"{_escaped_lines(card.reading)}</div>"
        )
    front_parts.append("</div>")
    front = "".join(front_parts)

    back_parts = [
        '<div class="vrcs-note vrcs-note-back" '
        f'style="max-width:42rem;margin:0 auto;text-align:left;'
        f"font-family:{_NOTE_FONT_STACK};font-size:1rem;line-height:1.72;"
        'color:inherit;overflow-wrap:anywhere;">',
        '<section class="vrcs-definition">',
        _section_label("释义"),
        '<div class="vrcs-definition-content">',
        _definition_html(card.definition),
        "</div></section>",
    ]
    if card.context:
        back_parts.append(
            '<section class="vrcs-context" '
            'style="margin-top:1.45rem;padding-top:1rem;'
            'border-top:1px solid rgba(127,127,127,0.24);">'
            f"{_section_label('语境')}"
            '<div class="vrcs-context-content" '
            'style="padding:0.82rem 0.95rem;border-radius:0.65rem;'
            'background:rgba(61,115,168,0.08);font-size:0.95rem;'
            'line-height:1.7;">'
            f"{_escaped_lines(card.context)}</div></section>"
        )
    metadata = " · ".join(
        _escaped_lines(value)
        for value in (card.dictionary, card.language.upper() if card.language else None)
        if value
    )
    if metadata:
        back_parts.append(
            '<footer class="vrcs-source" '
            'style="margin-top:1.15rem;font-size:0.76rem;line-height:1.5;'
            f'opacity:0.56;">{metadata}</footer>'
        )
    back_parts.append("</div>")

    return {
        "deckName": config.deck,
        "modelName": config.model,
        "fields": {
            config.front_field: front,
            config.back_field: "".join(back_parts),
        },
        "options": {"allowDuplicate": False},
        "tags": ["vrcs"],
    }


def _post_card(card: CardRequest, config: AnkiConfig) -> int:
    discovery = _discover(config)
    if not discovery.configuration_valid:
        raise AnkiConfigurationError(discovery.message)

    note = _note(card, config)
    can_add = _invoke(config, "canAddNotes", {"notes": [note]})
    if not isinstance(can_add, list) or len(can_add) != 1 or not isinstance(can_add[0], bool):
        raise AnkiProtocolError("AnkiConnect 返回的制卡校验结果无效")
    if not can_add[0]:
        raise AnkiDuplicateError("这条笔记已存在，未重复添加")

    try:
        result = _invoke(config, "addNote", {"note": note})
    except AnkiProtocolError as exc:
        if "duplicate" in str(exc).lower():
            raise AnkiDuplicateError("这条笔记已存在，未重复添加") from exc
        raise
    if not isinstance(result, int):
        raise AnkiProtocolError("AnkiConnect 未返回有效的笔记 ID")
    return result


async def anki_status(config: AnkiConfig) -> dict[str, object]:
    return await asyncio.to_thread(_status, config)


async def create_card(card: CardRequest, config: AnkiConfig) -> int:
    return await asyncio.to_thread(_post_card, card, config)
