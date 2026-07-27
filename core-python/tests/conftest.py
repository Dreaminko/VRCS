import json
from io import BytesIO
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile


def build_yomitan_archive(
    title: str,
    revision: str,
    terms: list,
    *,
    source_language: str | None = None,
    target_language: str | None = None,
) -> bytes:
    index: dict[str, object] = {"title": title, "revision": revision, "format": 3}
    if source_language is not None:
        index["sourceLanguage"] = source_language
    if target_language is not None:
        index["targetLanguage"] = target_language

    output = BytesIO()
    with ZipFile(output, "w", ZIP_DEFLATED) as package:
        package.writestr("index.json", json.dumps(index))
        package.writestr("term_bank_1.json", json.dumps(terms))
    return output.getvalue()


def write_hf_snapshot(cache: Path, repository: str) -> Path:
    snapshot = (
        cache
        / f"models--{repository.replace('/', '--')}"
        / "snapshots"
        / "revision"
    )
    snapshot.mkdir(parents=True)
    (snapshot / "model.bin").write_bytes(b"model")
    (snapshot / "config.json").write_text("{}", encoding="utf-8")
    return snapshot
