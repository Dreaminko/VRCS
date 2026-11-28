# Third-party notices

VRCS distributes open-source runtime components. Their original copyright
notices and license terms remain applicable.

- Tauri — MIT OR Apache-2.0
- whatlang (local original-language inference) — MIT
- React and React DOM — MIT
- react-markdown and remark-gfm — MIT
- pulldown-cmark — MIT
- taffy — MIT
- cosmic-text — MIT OR Apache-2.0
- resvg — Apache-2.0 OR MIT
- unicode-segmentation — MIT OR Apache-2.0
- Axum and Tokio — MIT
- rusqlite and SQLite — MIT / Public Domain
- Vulkan Loader — Apache-2.0
- llama.cpp (Qwen ASR and local translation runtimes) — MIT
- PLaMo 3 Translate 0.8B GGUF — PLaMo Community License
- LLVM OpenMP runtime — Apache-2.0 WITH LLVM-exception
- webview2-com — MIT
- wasapi-rs — MIT
- pipewire-rs and libspa-rs — MIT
- secret-service — MIT OR Apache-2.0
- ashpd — MIT
- GTK/WebKitGTK Rust bindings — MIT
- png — MIT OR Apache-2.0
- tar and flate2 — MIT OR Apache-2.0
- Silero VAD — MIT
- Smart Turn — BSD-2-Clause
- ONNX Runtime — MIT
- PaddleOCR PP-OCRv6 models and dictionary — Apache-2.0
- PaddleX OCR processing reference — Apache-2.0
- rosc — MIT OR Apache-2.0
- mdns-sd — Apache-2.0 OR MIT
- rust-openvr and openvr-sys — MIT
- Valve OpenVR SDK — BSD-3-Clause
- openxr-rs and ash — MIT OR Apache-2.0
- ring — Apache-2.0 AND ISC
- libloading — ISC
- memchr — MIT OR Unlicense
- zeroize — MIT OR Apache-2.0

The Qwen ASR model and audio projector are downloaded through recognition settings
and remain subject to their upstream Apache-2.0 license. The llama.cpp runtime
is downloaded on demand through recognition settings. It includes its MIT
license; Windows assets also include the LLVM OpenMP license. Linux CPU/Vulkan assets
use system glibc, OpenSSL, libstdc++ and libgomp rather than bundling Windows DLLs;
Vulkan assets additionally use the system Vulkan loader and GPU driver.
The PLaMo 3 Translate model is downloaded on demand through translation
settings and remains subject to the upstream PLaMo Community License. Its
official license copy is retained with the model package.
The bundled Vulkan loader includes the VulkanRT license notices.
The Smart Turn model is downloaded only when semantic endpointing is enabled
and remains subject to the upstream BSD-2-Clause license.
The PP-OCRv6 small models and dictionary are downloaded through the OCR settings
and remain subject to the upstream Apache-2.0 license. Local image processing
follows the PaddleX reference algorithms, Copyright PaddlePaddle Authors.
