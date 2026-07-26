from pathlib import PurePath

from PyInstaller.utils.hooks import collect_data_files, collect_dynamic_libs


datas = collect_data_files(
    "faster_whisper",
    includes=["assets/silero_vad_v6.onnx"],
)
binaries = collect_dynamic_libs("ctranslate2")
hiddenimports = []

a = Analysis(
    ["run_core.py"],
    pathex=["."],
    binaries=binaries,
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[
        "pytest",
        "torch",
        "torchaudio",
        "silero_vad",
        "av",
        "hf_xet",
        "jinja2",
        "markupsafe",
        "watchfiles",
        "httptools",
        "dotenv",
        "onnxruntime.tools",
        "onnxruntime.quantization",
        "onnxruntime.transformers",
    ],
    noarchive=False,
    optimize=1,
)

# Do not ship Python's build-time MSVC runtime beside the executable. App-local
# DLLs take precedence over the supported system Redistributable and can be too
# old for newer native Python wheels. Release users must install the
# latest Microsoft Visual C++ v14 x64 Redistributable documented in README.md.
msvc_runtime_files = {
    "msvcp140.dll",
    "msvcp140_1.dll",
    "msvcp140_2.dll",
    "msvcp140_atomic_wait.dll",
    "msvcp140_codecvt_ids.dll",
    "vcruntime140.dll",
    "vcruntime140_1.dll",
    "concrt140.dll",
}
a.binaries = [
    entry
    for entry in a.binaries
    if PurePath(entry[0]).name.lower() not in msvc_runtime_files
]
pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="vrcs-core",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=False,
    console=False,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
    contents_directory="vrcs-core-runtime",
)

coll = COLLECT(
    exe,
    a.binaries,
    a.datas,
    strip=False,
    upx=False,
    upx_exclude=[],
    name="vrcs-core",
)
