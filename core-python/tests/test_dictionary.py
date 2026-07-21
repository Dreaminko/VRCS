import json
from io import BytesIO
from zipfile import ZIP_DEFLATED, ZipFile

import pytest

from app.database import Database
from app.dictionary import YomitanDictionaryError


def yomitan_archive() -> bytes:
    output = BytesIO()
    with ZipFile(output, "w", ZIP_DEFLATED) as package:
        package.writestr(
            "index.json",
            json.dumps(
                {
                    "title": "Test Japanese Dictionary",
                    "revision": "2026.07",
                    "format": 3,
                    "sourceLanguage": "ja",
                    "targetLanguage": "zh",
                }
            ),
        )
        package.writestr(
            "term_bank_1.json",
            json.dumps(
                [
                    ["便利", "べんり", "adj-na", "", 10, ["方便", {"type": "text", "text": "有用"}], 1, ""],
                    [
                        "辞書",
                        "じしょ",
                        "n",
                        "",
                        5,
                        [
                            {
                                "type": "structured-content",
                                "content": [
                                    {"tag": "span", "content": "词典"},
                                    {"tag": "br"},
                                    {"tag": "span", "content": "辞典"},
                                ],
                            }
                        ],
                        2,
                        "",
                    ],
                ]
            ),
        )
    return output.getvalue()


def test_imports_yomitan_terms_and_replaces_same_title(tmp_path):
    database = Database(tmp_path / "test.db")
    database.initialize()

    source = database.import_yomitan(yomitan_archive())
    assert source.title == "Test Japanese Dictionary"
    assert source.entry_count == 2
    assert source.source_language == "ja"
    assert source.target_language == "zh"

    entry = database.lookup("便利")[0]
    assert entry.reading == "べんり"
    assert entry.definition == "方便\n有用"
    assert entry.dictionary == source.title
    assert database.lookup("じしょ")[0].definition == "词典\n辞典"

    replacement = database.import_yomitan(yomitan_archive())
    assert replacement.id == source.id
    assert len(database.dictionary_sources()) == 1
    database.close()


def test_rejects_non_yomitan_zip(tmp_path):
    output = BytesIO()
    with ZipFile(output, "w") as package:
        package.writestr("other.json", "[]")

    database = Database(tmp_path / "test.db")
    database.initialize()
    with pytest.raises(YomitanDictionaryError, match="index.json"):
        database.import_yomitan(output.getvalue())
    database.close()


def test_exact_term_precedes_reading_matches_and_removes_duplicates(tmp_path):
    output = BytesIO()
    with ZipFile(output, "w", ZIP_DEFLATED) as package:
        package.writestr(
            "index.json",
            json.dumps({"title": "Lookup Priority", "revision": "1", "format": 3}),
        )
        package.writestr(
            "term_bank_1.json",
            json.dumps(
                [
                    ["あ", "", "", "", 10, ["感叹词"], 1, ""],
                    ["あ", "", "", "", 5, ["感叹词"], 2, ""],
                    ["亜", "あ", "", "", 8, ["亚、次等"], 3, ""],
                ]
            ),
        )

    database = Database(tmp_path / "test.db")
    database.initialize()
    database.import_yomitan(output.getvalue())

    entries = database.lookup("あ")
    assert [(entry.term, entry.definition) for entry in entries] == [("あ", "感叹词")]
    database.close()


def test_prefix_lookup_collapses_equivalent_spelling_variants(tmp_path):
    output = BytesIO()
    with ZipFile(output, "w", ZIP_DEFLATED) as package:
        package.writestr(
            "index.json",
            json.dumps({"title": "Spelling Variants", "revision": "1", "format": 3}),
        )
        package.writestr(
            "term_bank_1.json",
            json.dumps(
                [
                    ["話し相手", "はなしあいて", "", "", 10, ["交谈的对象"], 1, ""],
                    ["話しあいて", "はなしあいて", "", "", 5, ["交谈的对象"], 2, ""],
                ]
            ),
        )

    database = Database(tmp_path / "test.db")
    database.initialize()
    database.import_yomitan(output.getvalue())

    entries = database.lookup("話し")
    assert [(entry.term, entry.definition) for entry in entries] == [("話し相手", "交谈的对象")]
    database.close()
