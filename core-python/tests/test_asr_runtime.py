import builtins
import sys
from types import ModuleType

from app.asr.runtime import prepare_faster_whisper_runtime


def test_installs_av_stub_when_pyav_is_not_packaged(monkeypatch) -> None:
    original_import = builtins.__import__

    def import_without_av(name, *args, **kwargs):
        if name == "av":
            raise ImportError("PyAV intentionally excluded from Release")
        return original_import(name, *args, **kwargs)

    monkeypatch.delitem(sys.modules, "av", raising=False)
    monkeypatch.setattr(builtins, "__import__", import_without_av)

    prepare_faster_whisper_runtime()

    assert isinstance(sys.modules["av"], ModuleType)
