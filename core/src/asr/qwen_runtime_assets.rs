use std::path::{Path, PathBuf};

use super::super::qwen_models::AssetSpec;

pub(in crate::asr) const VERSION: &str = "b11501";
pub(in crate::asr) const LICENSE: AssetSpec = AssetSpec {
    name: "LICENSE",
    bytes: 1_078,
    sha256: "94f29bbed6a22c35b992c5c6ebf0e7c92f13b836b90f36f461c9cf2f0f1d010d",
};
pub(in crate::asr) const LICENSE_URL: &str = "https://raw.githubusercontent.com/ggml-org/llama.cpp/46baf1f1fec5a06d1e52122a9207ca978b720f95/LICENSE";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::asr) enum ArchiveFormat {
    Zip,
    TarGz,
}

pub(in crate::asr) struct RuntimeAsset {
    pub os: &'static str,
    pub arch: &'static str,
    pub backend: &'static str,
    pub version: &'static str,
    pub url: &'static str,
    pub format: ArchiveFormat,
    pub archive: AssetSpec,
    pub executable: &'static str,
    pub required_files: &'static [&'static str],
    pub licenses: &'static [AssetSpec],
}

const WINDOWS_FILES: &[&str] = &[
    "llama-server.exe", "llama-server-impl.dll", "llama.dll", "llama-common.dll",
    "mtmd.dll", "ggml.dll", "ggml-base.dll", "ggml-vulkan.dll", "ggml-cpu-x64.dll",
    "libomp.dll", "LICENSE", "LICENSE-LLVM-OpenMP",
];
const LINUX_FILES: &[&str] = &[
    "llama-server", "libllama-server-impl.so", "libllama.so", "libllama.so.0",
    "libllama.so.0.6.0", "libllama-common.so", "libllama-common.so.0",
    "libllama-common.so.0.6.0", "libmtmd.so", "libmtmd.so.0", "libmtmd.so.0.6.0",
    "libggml.so", "libggml.so.0", "libggml.so.0.26.0", "libggml-base.so",
    "libggml-base.so.0", "libggml-base.so.0.26.0", "LICENSE",
];

pub(in crate::asr) const WINDOWS: RuntimeAsset = RuntimeAsset {
    os: "windows", arch: "x86_64", backend: "vulkan", version: VERSION,
    url: "https://github.com/ggml-org/llama.cpp/releases/download/b11501/llama-b11501-bin-win-vulkan-x64.zip",
    format: ArchiveFormat::Zip,
    archive: AssetSpec {
        name: "runtime.zip", bytes: 33_425_118,
        sha256: "6bf677025ae9ccdf5ca5e1ee1961ecaf25759f8cf63e875925a83bfaf00b7475",
    },
    executable: "llama-server.exe", required_files: WINDOWS_FILES,
    licenses: &[LICENSE, AssetSpec {
        name: "LICENSE-LLVM-OpenMP", bytes: 19_741,
        sha256: "fdad1758a9e1f9d5a81e18879b3406772115edc92c24bfa36b70c654f325e8e4",
    }],
};

// Downloaded and inspected at this fixed revision. Linux CPU/Vulkan archives
// provide $ORIGIN RUNPATH; glibc, libstdc++, libgcc_s, libgomp and OpenSSL 3 stay
// system dependencies, as do the Vulkan loader and GPU driver for GPU execution.
// x64: GLIBC_2.34 / GLIBCXX_3.4.30; ARM64: GLIBC_2.38 / GLIBCXX_3.4.32.
// These fit Ubuntu 24.04; model/hardware acceptance is a separate gate.
pub(in crate::asr) const LINUX_X64: RuntimeAsset = RuntimeAsset {
    os: "linux", arch: "x86_64", backend: "cpu", version: VERSION,
    url: "https://github.com/ggml-org/llama.cpp/releases/download/b11501/llama-b11501-bin-ubuntu-x64.tar.gz",
    format: ArchiveFormat::TarGz,
    archive: AssetSpec {
        name: "runtime.tar.gz", bytes: 17_789_324,
        sha256: "45bbf2fb33275e5652f027326f2fffd0e32df5a41a64e7b86f7da9e0dc0879c5",
    },
    executable: "llama-server", required_files: LINUX_FILES, licenses: &[LICENSE],
};
pub(in crate::asr) const LINUX_ARM64: RuntimeAsset = RuntimeAsset {
    os: "linux", arch: "aarch64", backend: "cpu", version: VERSION,
    url: "https://github.com/ggml-org/llama.cpp/releases/download/b11501/llama-b11501-bin-ubuntu-arm64.tar.gz",
    format: ArchiveFormat::TarGz,
    archive: AssetSpec {
        name: "runtime.tar.gz", bytes: 13_785_721,
        sha256: "6e4d58453b74e29190293730076f8226ff8cdb7c1e2fd29f51dcc89e873dc701",
    },
    executable: "llama-server", required_files: LINUX_FILES, licenses: &[LICENSE],
};

pub(in crate::asr) const LINUX_VULKAN_X64: RuntimeAsset = RuntimeAsset {
    backend: "vulkan",
    url: "https://github.com/ggml-org/llama.cpp/releases/download/b11501/llama-b11501-bin-ubuntu-vulkan-x64.tar.gz",
    archive: AssetSpec {
        name: "runtime.tar.gz", bytes: 31_737_331,
        sha256: "f766be93272cd57afa3b4234aa34797bc2bbff02dadccb184d9b64f64e8a5e10",
    },
    ..LINUX_X64
};
pub(in crate::asr) const LINUX_VULKAN_ARM64: RuntimeAsset = RuntimeAsset {
    backend: "vulkan",
    url: "https://github.com/ggml-org/llama.cpp/releases/download/b11501/llama-b11501-bin-ubuntu-vulkan-arm64.tar.gz",
    archive: AssetSpec {
        name: "runtime.tar.gz", bytes: 24_949_039,
        sha256: "1c3065bca5fe9b2a4cae6805989ddd65fda200f2756ed6ff43e396a260570f83",
    },
    ..LINUX_ARM64
};

pub(in crate::asr) fn select(os: &str, arch: &str, backend: &str) -> Result<&'static RuntimeAsset, String> {
    match (os, arch, backend) {
        ("windows", "x86_64", "vulkan") => Ok(&WINDOWS),
        ("linux", "x86_64", "cpu") => Ok(&LINUX_X64),
        ("linux", "aarch64", "cpu") => Ok(&LINUX_ARM64),
        ("linux", "x86_64", "vulkan") => Ok(&LINUX_VULKAN_X64),
        ("linux", "aarch64", "vulkan") => Ok(&LINUX_VULKAN_ARM64),
        _ => Err(format!("No managed Qwen runtime for {os}/{arch}/{backend}")),
    }
}

pub(in crate::asr) fn current() -> Result<&'static RuntimeAsset, String> {
    if cfg!(target_os = "linux") && !cfg!(target_env = "gnu") {
        return Err("Managed Linux Qwen runtime requires glibc".into());
    }
    select(std::env::consts::OS, std::env::consts::ARCH,
        if cfg!(windows) || cfg!(feature = "vulkan") { "vulkan" } else { "cpu" })
}

impl RuntimeAsset {
    pub(in crate::asr) fn directory(&self, root: &Path) -> PathBuf {
        // Keep the existing Windows installation layout and reuse its marker.
        if self.os == "windows" {
            root.join(self.version)
        } else {
            root.join(self.os).join(self.arch).join(self.backend).join(self.version)
        }
    }

    pub(in crate::asr) fn runtime_file(&self, name: &str) -> bool {
        if self.required_files().any(|file| file == name) { return true; }
        if name.contains(['/', '\\']) { return false; }
        if self.os == "windows" {
            (name.starts_with("ggml-cpu-") && name.ends_with(".dll")) || name == "ggml-rpc.dll"
        } else {
            let variants: &[&str] = match self.arch {
                "x86_64" => &["x64", "sse42", "sandybridge", "ivybridge", "haswell", "skylakex",
                    "cascadelake", "cannonlake", "icelake", "cooperlake", "alderlake",
                    "sapphirerapids", "piledriver", "zen4"],
                "aarch64" => &["armv8.0_1", "armv8.2_1", "armv8.2_2", "armv8.2_3",
                    "armv8.6_1", "armv8.6_2", "armv9.2_1", "armv9.2_2"],
                _ => return false,
            };
            name.strip_prefix("libggml-cpu-").and_then(|n| n.strip_suffix(".so"))
                .is_some_and(|variant| variants.contains(&variant))
        }
    }

    pub(in crate::asr) fn required_files(&self) -> impl Iterator<Item = &'static str> + '_ {
        let cpu = match (self.os, self.arch) {
            ("linux", "x86_64") => Some("libggml-cpu-x64.so"),
            ("linux", "aarch64") => Some("libggml-cpu-armv8.0_1.so"),
            _ => None,
        };
        let vulkan = (self.os == "linux" && self.backend == "vulkan")
            .then_some("libggml-vulkan.so");
        self.required_files.iter().copied().chain(cpu).chain(vulkan)
    }
}
