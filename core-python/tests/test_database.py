from app.database import Database
from app.models import Subtitle


def test_subtitles_are_limited_and_dictionary_is_seeded(tmp_path):
    database = Database(tmp_path / "test.db")
    database.initialize()
    for number in range(4):
        database.add_subtitle(Subtitle(text=f"line {number}"), limit=3)

    assert [item.text for item in database.subtitle_history()] == ["line 3", "line 2", "line 1"]
    assert database.lookup("hello")[0].definition == "used as a greeting"
    database.close()


def test_subtitle_source_is_persisted_and_old_tables_are_migrated(tmp_path):
    path = tmp_path / "old.db"
    database = Database(path)
    database.connection.execute(
        """CREATE TABLE subtitles (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            text TEXT NOT NULL,
            language TEXT,
            started_at REAL,
            ended_at REAL,
            created_at TEXT NOT NULL
        )"""
    )
    database.initialize()
    database.add_subtitle(Subtitle(text="my voice", source="microphone"), limit=10)

    saved = database.subtitle_history()[0]
    assert saved.source == "microphone"
    database.close()
