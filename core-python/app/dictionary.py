from __future__ import annotations

import json
import re
from dataclasses import dataclass
from io import BytesIO
from pathlib import PurePosixPath
from typing import Any, Iterator
from zipfile import BadZipFile, ZipFile


MAX_ARCHIVE_BYTES = 512 * 1024 * 1024
MAX_UNCOMPRESSED_BYTES = 1024 * 1024 * 1024
MAX_JSON_FILE_BYTES = 256 * 1024 * 1024
TERM_BANK_PATTERN = re.compile(r"^term_bank_(\d+)\.json$")


class YomitanDictionaryError(ValueError):
    pass


@dataclass(frozen=True, slots=True)
class DictionaryMetadata:
    title: str
    revision: str
    source_language: str
    target_language: str | None
    format: int


@dataclass(frozen=True, slots=True)
class DictionaryRecord:
    term: str
    reading: str
    language: str
    definition: str
    score: float


def _text_content(value: Any) -> str:
    if isinstance(value, str):
        return value.strip()
    if isinstance(value, list):
        parts = [_text_content(item) for item in value]
        return "".join(part for part in parts if part)
    if not isinstance(value, dict):
        return ""

    entry_type = value.get("type")
    if entry_type == "text":
        return str(value.get("text", "")).strip()
    if entry_type == "image" or value.get("tag") == "img":
        return str(value.get("alt") or value.get("description") or value.get("title") or "").strip()
    if value.get("tag") == "br":
        return "\n"
    if "content" in value:
        return _text_content(value["content"])
    return ""


class YomitanDictionaryImporter:
    def __init__(self, archive: bytes) -> None:
        if not archive:
            raise YomitanDictionaryError("词典文件为空")
        if len(archive) > MAX_ARCHIVE_BYTES:
            raise YomitanDictionaryError("词典压缩包超过 512 MB 限制")
        self._archive = archive
        self._prefix = ""
        self._term_files: list[str] = []
        self.metadata = self._inspect()

    def _inspect(self) -> DictionaryMetadata:
        try:
            with ZipFile(BytesIO(self._archive)) as package:
                files = [item for item in package.infolist() if not item.is_dir()]
                if sum(item.file_size for item in files) > MAX_UNCOMPRESSED_BYTES:
                    raise YomitanDictionaryError("词典解压后超过 1 GB 限制")

                indexes = [item.filename for item in files if PurePosixPath(item.filename).name == "index.json"]
                if not indexes:
                    raise YomitanDictionaryError("不是有效的 Yomitan 词典：缺少 index.json")
                index_name = min(indexes, key=lambda name: (len(PurePosixPath(name).parts), len(name)))
                parent = PurePosixPath(index_name).parent
                self._prefix = "" if str(parent) == "." else f"{parent.as_posix()}/"

                banks: list[tuple[int, str]] = []
                for item in files:
                    if not item.filename.startswith(self._prefix):
                        continue
                    relative = item.filename[len(self._prefix):]
                    match = TERM_BANK_PATTERN.fullmatch(relative)
                    if match:
                        if item.file_size > MAX_JSON_FILE_BYTES:
                            raise YomitanDictionaryError(f"词条文件过大：{relative}")
                        banks.append((int(match.group(1)), item.filename))
                if not banks:
                    raise YomitanDictionaryError("不是有效的 Yomitan 词典：缺少 term_bank_*.json")
                self._term_files = [name for _, name in sorted(banks)]

                index = self._read_json(package, index_name)
        except BadZipFile as exc:
            raise YomitanDictionaryError("词典文件不是有效的 ZIP 压缩包") from exc

        if not isinstance(index, dict):
            raise YomitanDictionaryError("index.json 内容无效")
        title = str(index.get("title", "")).strip()
        revision = str(index.get("revision", "")).strip()
        try:
            format_version = int(index.get("format", index.get("version", 0)))
        except (TypeError, ValueError) as exc:
            raise YomitanDictionaryError("index.json 中的词典格式版本无效") from exc
        if not title or not revision or format_version not in {1, 2, 3}:
            raise YomitanDictionaryError("仅支持包含标题、修订号且格式为 1 到 3 的 Yomitan 词典")

        source_language = str(index.get("sourceLanguage") or "ja").strip() or "ja"
        target = str(index.get("targetLanguage") or "").strip() or None
        return DictionaryMetadata(title, revision, source_language, target, format_version)

    @staticmethod
    def _read_json(package: ZipFile, name: str) -> Any:
        try:
            with package.open(name) as source:
                return json.loads(source.read().decode("utf-8-sig"))
        except (UnicodeDecodeError, json.JSONDecodeError, KeyError) as exc:
            raise YomitanDictionaryError(f"无法读取 {name}") from exc

    def entries(self) -> Iterator[DictionaryRecord]:
        with ZipFile(BytesIO(self._archive)) as package:
            for name in self._term_files:
                bank = self._read_json(package, name)
                if not isinstance(bank, list):
                    raise YomitanDictionaryError(f"{name} 必须包含词条数组")
                for index, raw in enumerate(bank):
                    if not isinstance(raw, list) or len(raw) < 6:
                        raise YomitanDictionaryError(f"{name} 的第 {index + 1} 条词条格式无效")
                    term = str(raw[0]).strip()
                    reading = str(raw[1]).strip()
                    glossary = raw[5]
                    if not term or not isinstance(glossary, list):
                        continue
                    definitions = [_text_content(item) for item in glossary]
                    definition = "\n".join(item for item in definitions if item).strip()
                    if not definition:
                        continue
                    try:
                        score = float(raw[4])
                    except (TypeError, ValueError):
                        score = 0
                    yield DictionaryRecord(
                        term=term,
                        reading=reading,
                        language=self.metadata.source_language,
                        definition=definition[:32_000],
                        score=score,
                    )
