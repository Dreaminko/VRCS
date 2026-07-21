from __future__ import annotations

from abc import ABC, abstractmethod
from pathlib import Path


class DictionaryImporter(ABC):
    """Extension point for Yomitan, StarDict, MDict and other formats."""

    @abstractmethod
    def import_file(self, path: Path) -> None:
        raise NotImplementedError

