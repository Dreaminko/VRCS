from __future__ import annotations

import sqlite3
from pathlib import Path

from .models import DictionaryEntry, Subtitle


SEED_ENTRIES = (
    ("hello", "en", "used as a greeting"),
    ("world", "en", "the earth and all people and things on it"),
    ("こんにちは", "ja", "你好；日间问候语"),
    ("ありがとう", "ja", "谢谢"),
)


class Database:
    def __init__(self, path: Path) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        self.connection = sqlite3.connect(path, check_same_thread=False)
        self.connection.row_factory = sqlite3.Row

    def initialize(self) -> None:
        self.connection.executescript(
            """
            CREATE TABLE IF NOT EXISTS subtitles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                text TEXT NOT NULL,
                language TEXT,
                started_at REAL,
                ended_at REAL,
                source TEXT NOT NULL DEFAULT 'speaker',
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS dictionary (
                term TEXT NOT NULL,
                language TEXT NOT NULL,
                definition TEXT NOT NULL,
                PRIMARY KEY (term, language)
            );
            CREATE VIRTUAL TABLE IF NOT EXISTS dictionary_fts USING fts5(
                term, definition, content='dictionary', content_rowid='rowid'
            );
            CREATE TRIGGER IF NOT EXISTS dictionary_ai AFTER INSERT ON dictionary BEGIN
                INSERT INTO dictionary_fts(rowid, term, definition)
                VALUES (new.rowid, new.term, new.definition);
            END;
            """
        )
        columns = {
            str(row["name"])
            for row in self.connection.execute("PRAGMA table_info(subtitles)").fetchall()
        }
        if "source" not in columns:
            self.connection.execute(
                "ALTER TABLE subtitles ADD COLUMN source TEXT NOT NULL DEFAULT 'speaker'"
            )
        self.connection.executemany(
            "INSERT OR IGNORE INTO dictionary(term, language, definition) VALUES (?, ?, ?)",
            SEED_ENTRIES,
        )
        self.connection.commit()

    def add_subtitle(self, subtitle: Subtitle, limit: int) -> Subtitle:
        cursor = self.connection.execute(
            """INSERT INTO subtitles(text, language, started_at, ended_at, source, created_at)
               VALUES (?, ?, ?, ?, ?, ?)""",
            (
                subtitle.text,
                subtitle.language,
                subtitle.started_at,
                subtitle.ended_at,
                subtitle.source,
                subtitle.created_at.isoformat(),
            ),
        )
        self.connection.execute(
            "DELETE FROM subtitles WHERE id NOT IN (SELECT id FROM subtitles ORDER BY id DESC LIMIT ?)",
            (limit,),
        )
        self.connection.commit()
        return subtitle.model_copy(update={"id": cursor.lastrowid})

    def subtitle_history(self, limit: int = 500) -> list[Subtitle]:
        rows = self.connection.execute(
            "SELECT * FROM subtitles ORDER BY id DESC LIMIT ?", (limit,)
        ).fetchall()
        return [Subtitle.model_validate(dict(row)) for row in rows]

    def lookup(self, term: str, limit: int = 10) -> list[DictionaryEntry]:
        exact = self.connection.execute(
            "SELECT term, language, definition FROM dictionary WHERE lower(term) = lower(?)",
            (term.strip(),),
        ).fetchall()
        if exact:
            return [DictionaryEntry.model_validate(dict(row)) for row in exact]
        rows = self.connection.execute(
            "SELECT term, language, definition FROM dictionary WHERE term LIKE ? LIMIT ?",
            (f"{term.strip()}%", limit),
        ).fetchall()
        return [DictionaryEntry.model_validate(dict(row)) for row in rows]

    def close(self) -> None:
        self.connection.close()
