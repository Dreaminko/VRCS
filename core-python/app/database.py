from __future__ import annotations

import sqlite3
from datetime import datetime, timezone
from pathlib import Path

from .dictionary import YomitanDictionaryImporter
from .models import DictionaryEntry, DictionarySource, Subtitle


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
        self.connection.execute("PRAGMA foreign_keys = ON")

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
            CREATE TABLE IF NOT EXISTS dictionary_sources (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL UNIQUE,
                revision TEXT NOT NULL,
                source_language TEXT NOT NULL,
                target_language TEXT,
                entry_count INTEGER NOT NULL DEFAULT 0,
                imported_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS dictionary_entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_id INTEGER NOT NULL REFERENCES dictionary_sources(id) ON DELETE CASCADE,
                term TEXT NOT NULL,
                reading TEXT NOT NULL DEFAULT '',
                language TEXT NOT NULL,
                definition TEXT NOT NULL,
                score REAL NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS dictionary_entries_term_idx
                ON dictionary_entries(term COLLATE NOCASE);
            CREATE INDEX IF NOT EXISTS dictionary_entries_reading_idx
                ON dictionary_entries(reading COLLATE NOCASE);
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
        query = term.strip()
        if not query:
            return []

        def imported_rows(predicate: str, value: str) -> list[sqlite3.Row]:
            candidates = self.connection.execute(
                f"""SELECT entries.term, entries.reading, entries.language, entries.definition,
                           sources.title AS dictionary
                    FROM dictionary_entries AS entries
                    JOIN dictionary_sources AS sources ON sources.id = entries.source_id
                    WHERE {predicate}
                    GROUP BY entries.source_id, entries.term, entries.reading,
                             entries.language, entries.definition, sources.title
                    ORDER BY MAX(entries.score) DESC, MIN(entries.id)
                    LIMIT ?""",
                (value, limit * 8),
            ).fetchall()
            rows: list[sqlite3.Row] = []
            seen: set[tuple[str, str, str, str]] = set()
            for row in candidates:
                signature = (
                    str(row["dictionary"]),
                    str(row["reading"]),
                    str(row["language"]),
                    str(row["definition"]),
                )
                if signature in seen:
                    continue
                seen.add(signature)
                rows.append(row)
                if len(rows) == limit:
                    break
            return rows

        exact_term = imported_rows("entries.term = ? COLLATE NOCASE", query)
        if exact_term:
            return [DictionaryEntry.model_validate(dict(row)) for row in exact_term]

        legacy = self.connection.execute(
            """SELECT term, NULL AS reading, language, definition, NULL AS dictionary
               FROM dictionary WHERE term = ? COLLATE NOCASE LIMIT ?""",
            (query, limit),
        ).fetchall()
        if legacy:
            return [DictionaryEntry.model_validate(dict(row)) for row in legacy]

        exact_reading = imported_rows("entries.reading = ? COLLATE NOCASE", query)
        if exact_reading:
            return [DictionaryEntry.model_validate(dict(row)) for row in exact_reading]

        prefix = imported_rows("entries.term LIKE ? COLLATE NOCASE", f"{query}%")
        if prefix:
            return [DictionaryEntry.model_validate(dict(row)) for row in prefix]

        legacy_prefix = self.connection.execute(
            """SELECT term, NULL AS reading, language, definition, NULL AS dictionary
               FROM dictionary WHERE term LIKE ? COLLATE NOCASE LIMIT ?""",
            (f"{query}%", limit),
        ).fetchall()
        if legacy_prefix:
            return [DictionaryEntry.model_validate(dict(row)) for row in legacy_prefix]

        reading_prefix = imported_rows("entries.reading LIKE ? COLLATE NOCASE", f"{query}%")
        return [DictionaryEntry.model_validate(dict(row)) for row in reading_prefix]

    def import_yomitan(self, archive: bytes) -> DictionarySource:
        importer = YomitanDictionaryImporter(archive)
        metadata = importer.metadata
        imported_at = datetime.now(timezone.utc).isoformat()
        count = 0
        with self.connection:
            self.connection.execute(
                """INSERT INTO dictionary_sources(
                       title, revision, source_language, target_language, entry_count, imported_at
                   ) VALUES (?, ?, ?, ?, 0, ?)
                   ON CONFLICT(title) DO UPDATE SET
                       revision = excluded.revision,
                       source_language = excluded.source_language,
                       target_language = excluded.target_language,
                       entry_count = 0,
                       imported_at = excluded.imported_at""",
                (
                    metadata.title,
                    metadata.revision,
                    metadata.source_language,
                    metadata.target_language,
                    imported_at,
                ),
            )
            row = self.connection.execute(
                "SELECT id FROM dictionary_sources WHERE title = ?", (metadata.title,)
            ).fetchone()
            source_id = int(row["id"])
            self.connection.execute("DELETE FROM dictionary_entries WHERE source_id = ?", (source_id,))

            batch: list[tuple[int, str, str, str, str, float]] = []
            for entry in importer.entries():
                batch.append(
                    (
                        source_id,
                        entry.term,
                        entry.reading,
                        entry.language,
                        entry.definition,
                        entry.score,
                    )
                )
                if len(batch) >= 1000:
                    self.connection.executemany(
                        """INSERT INTO dictionary_entries(
                               source_id, term, reading, language, definition, score
                           ) VALUES (?, ?, ?, ?, ?, ?)""",
                        batch,
                    )
                    count += len(batch)
                    batch.clear()
            if batch:
                self.connection.executemany(
                    """INSERT INTO dictionary_entries(
                           source_id, term, reading, language, definition, score
                       ) VALUES (?, ?, ?, ?, ?, ?)""",
                    batch,
                )
                count += len(batch)
            if not count:
                raise ValueError("Yomitan 词典中没有可导入的文本词条")
            self.connection.execute(
                "UPDATE dictionary_sources SET entry_count = ? WHERE id = ?", (count, source_id)
            )

        return self.dictionary_source(source_id)

    def dictionary_source(self, source_id: int) -> DictionarySource:
        row = self.connection.execute(
            "SELECT * FROM dictionary_sources WHERE id = ?", (source_id,)
        ).fetchone()
        if row is None:
            raise KeyError(source_id)
        return DictionarySource.model_validate(dict(row))

    def dictionary_sources(self) -> list[DictionarySource]:
        rows = self.connection.execute(
            "SELECT * FROM dictionary_sources ORDER BY imported_at DESC, id DESC"
        ).fetchall()
        return [DictionarySource.model_validate(dict(row)) for row in rows]

    def delete_dictionary_source(self, source_id: int) -> bool:
        with self.connection:
            cursor = self.connection.execute(
                "DELETE FROM dictionary_sources WHERE id = ?", (source_id,)
            )
        return cursor.rowcount > 0

    def close(self) -> None:
        self.connection.close()
