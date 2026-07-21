from __future__ import annotations

import asyncio
import json
from urllib.request import Request, urlopen

from .models import CardRequest


class AnkiError(RuntimeError):
    pass


def _post_card(card: CardRequest) -> int:
    payload = {
        "action": "addNote",
        "version": 6,
        "params": {
            "note": {
                "deckName": card.deck,
                "modelName": card.model,
                "fields": {
                    "Front": card.front,
                    "Back": f"{card.back}<br><small>{card.context}</small>" if card.context else card.back,
                },
                "options": {"allowDuplicate": False},
                "tags": ["vrcs"],
            }
        },
    }
    request = Request(
        "http://127.0.0.1:8766",
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"},
    )
    with urlopen(request, timeout=5) as response:  # noqa: S310 - fixed localhost endpoint
        result = json.load(response)
    if result.get("error"):
        raise AnkiError(str(result["error"]))
    return int(result["result"])


async def create_card(card: CardRequest) -> int:
    return await asyncio.to_thread(_post_card, card)
