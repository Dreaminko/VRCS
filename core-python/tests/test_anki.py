import asyncio
import io
import json

import pytest

from app import anki
from app.config import AnkiConfig
from app.models import CardRequest


def test_invoke_is_restricted_to_configured_local_port(monkeypatch):
    captured = {}

    def fake_open(request, timeout):
        captured["url"] = request.full_url
        captured["payload"] = json.loads(request.data)
        captured["timeout"] = timeout
        return io.BytesIO(b'{"result": 6, "error": null}')

    monkeypatch.setattr(anki, "urlopen", fake_open)

    assert anki._invoke(AnkiConfig(port=8877), "version") == 6
    assert captured["url"] == "http://127.0.0.1:8877"
    assert captured["timeout"] == 5
    assert captured["payload"] == {"action": "version", "version": 6}


def _connected_invoke(captured: list[tuple[str, object]], can_add: bool = True):
    def fake_invoke(config, action, params=None):
        captured.append((action, params))
        if action == "version":
            return 6
        if action == "multi":
            return [["Default", "VRCS"], ["Basic", "Cloze"]]
        if action == "modelFieldNames":
            return ["Front", "Back"]
        if action == "canAddNotes":
            return [can_add]
        if action == "addNote":
            return 42
        raise AssertionError(f"unexpected action: {action}")

    return fake_invoke


def test_create_card_discovers_configuration_validates_and_escapes(monkeypatch):
    captured: list[tuple[str, object]] = []
    monkeypatch.setattr(anki, "_invoke", _connected_invoke(captured))

    note_id = asyncio.run(
        anki.create_card(
            CardRequest(
                term="<hello>",
                reading="h&llo",
                definition="【Local <Safe>】\n1. greeting\n2. <script>",
                context="say <hello>",
                dictionary="Local & Safe",
                language="en",
            ),
            AnkiConfig(),
        )
    )

    assert note_id == 42
    assert [action for action, _ in captured] == [
        "version",
        "multi",
        "modelFieldNames",
        "canAddNotes",
        "addNote",
    ]
    note = captured[-1][1]["note"]
    assert note["deckName"] == "VRCS"
    assert note["modelName"] == "Basic"
    assert "&lt;hello&gt;" in note["fields"]["Front"]
    assert "h&amp;llo" in note["fields"]["Front"]
    assert 'class="vrcs-note vrcs-note-front"' in note["fields"]["Front"]
    assert "font-size:2rem" in note["fields"]["Front"]
    assert "<script>" not in note["fields"]["Back"]
    assert 'class="vrcs-section-label"' in note["fields"]["Back"]
    assert 'class="vrcs-dictionary-label"' in note["fields"]["Back"]
    assert "Local &lt;Safe&gt;" in note["fields"]["Back"]
    assert 'class="vrcs-gloss-list"' in note["fields"]["Back"]
    assert ">greeting</li>" in note["fields"]["Back"]
    assert ">&lt;script&gt;</li>" in note["fields"]["Back"]
    assert 'class="vrcs-context-content"' in note["fields"]["Back"]
    assert "Local &amp; Safe · EN" in note["fields"]["Back"]


def test_plain_definition_keeps_paragraphs_and_line_breaks_readable():
    note = anki._note(
        CardRequest(
            term="hello",
            definition="first line\nsecond line\n\nnext paragraph",
        ),
        AnkiConfig(),
    )

    back = note["fields"]["Back"]
    assert back.count('class="vrcs-definition-block"') == 2
    assert "first line<br>second line" in back
    assert "next paragraph" in back
    assert "margin-top:1.15rem" in back


def test_duplicate_is_reported_before_add_note(monkeypatch):
    captured: list[tuple[str, object]] = []
    monkeypatch.setattr(anki, "_invoke", _connected_invoke(captured, can_add=False))

    with pytest.raises(anki.AnkiDuplicateError, match="已存在"):
        asyncio.run(
            anki.create_card(
                CardRequest(term="hello", definition="greeting"),
                AnkiConfig(),
            )
        )

    assert "addNote" not in [action for action, _ in captured]


def test_missing_mapping_is_a_configuration_error(monkeypatch):
    def fake_invoke(config, action, params=None):
        if action == "version":
            return 6
        if action == "multi":
            return [["VRCS"], ["Basic"]]
        if action == "modelFieldNames":
            return ["Question", "Answer"]
        raise AssertionError(action)

    monkeypatch.setattr(anki, "_invoke", fake_invoke)

    with pytest.raises(anki.AnkiConfigurationError, match="缺少字段"):
        asyncio.run(
            anki.create_card(
                CardRequest(term="hello", definition="greeting"),
                AnkiConfig(),
            )
        )


def test_status_keeps_anki_failure_out_of_the_rest_of_the_app(monkeypatch):
    monkeypatch.setattr(
        anki,
        "_discover",
        lambda _config: (_ for _ in ()).throw(
            anki.AnkiUnavailableError("请先启动 Anki")
        ),
    )

    status = asyncio.run(anki.anki_status(AnkiConfig()))

    assert status["connected"] is False
    assert status["configuration_valid"] is False
    assert status["error_code"] == "unavailable"
    assert status["message"] == "请先启动 Anki"


def test_status_reports_incompatible_version_without_losing_connection(monkeypatch):
    monkeypatch.setattr(anki, "_invoke", lambda *_args, **_kwargs: 5)

    status = asyncio.run(anki.anki_status(AnkiConfig()))

    assert status["connected"] is True
    assert status["version"] == 5
    assert status["configuration_valid"] is False
    assert status["error_code"] == "incompatible_version"
