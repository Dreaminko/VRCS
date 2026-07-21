import asyncio
import io
import json

from app import anki
from app.models import CardRequest


def test_create_card_posts_expected_note(monkeypatch):
    captured = {}

    def fake_open(request, timeout):
        captured["url"] = request.full_url
        captured["payload"] = json.loads(request.data)
        captured["timeout"] = timeout
        return io.BytesIO(b'{"result": 42, "error": null}')

    monkeypatch.setattr(anki, "urlopen", fake_open)
    note_id = asyncio.run(
        anki.create_card(CardRequest(front="hello", back="greeting", context="hello there"))
    )

    assert note_id == 42
    assert captured["url"] == "http://127.0.0.1:8766"
    assert captured["timeout"] == 5
    note = captured["payload"]["params"]["note"]
    assert note["fields"]["Front"] == "hello"
    assert "hello there" in note["fields"]["Back"]
