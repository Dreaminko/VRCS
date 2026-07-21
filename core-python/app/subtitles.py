from __future__ import annotations

import asyncio

from .database import Database
from .models import Subtitle


class SubtitleStore:
    def __init__(self, database: Database, limit: int) -> None:
        self.database = database
        self.limit = limit
        self._subscribers: set[asyncio.Queue[Subtitle]] = set()

    async def publish(self, subtitle: Subtitle) -> Subtitle:
        saved = self.database.add_subtitle(subtitle, self.limit)
        for queue in tuple(self._subscribers):
            if queue.full():
                queue.get_nowait()
            queue.put_nowait(saved)
        return saved

    def history(self, limit: int | None = None) -> list[Subtitle]:
        return self.database.subtitle_history(min(limit or self.limit, self.limit))

    def subscribe(self) -> asyncio.Queue[Subtitle]:
        queue: asyncio.Queue[Subtitle] = asyncio.Queue(maxsize=50)
        self._subscribers.add(queue)
        return queue

    def unsubscribe(self, queue: asyncio.Queue[Subtitle]) -> None:
        self._subscribers.discard(queue)

