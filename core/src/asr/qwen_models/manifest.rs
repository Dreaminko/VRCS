#[derive(Clone, Copy)]
pub(crate) struct AssetSpec {
    pub(crate) name: &'static str,
    pub(crate) bytes: u64,
    pub(crate) sha256: &'static str,
}

#[derive(Clone, Copy)]
pub(crate) struct PackageSpec {
    pub(crate) id: &'static str,
    pub(crate) repository: &'static str,
    pub(crate) revision: &'static str,
    pub(crate) files: &'static [AssetSpec],
}

const QWEN_06B_Q8_FILES: [AssetSpec; 2] = [
    AssetSpec {
        name: "Qwen3-ASR-0.6B-Q8_0.gguf",
        bytes: 804_749_248,
        sha256: "bca259818b50ca7c4c05e9bdb35a5dc04fa039653a6d6f3f0f331f96f6aa1971",
    },
    AssetSpec {
        name: "mmproj-Qwen3-ASR-0.6B-Q8_0.gguf",
        bytes: 214_392_480,
        sha256: "41a342b5e4c514e968cb756de6cd1b7be39eff43c44c57a2ef5fc6522e36603d",
    },
];

pub(crate) const PACKAGES: [PackageSpec; 1] = [PackageSpec {
    id: "qwen3-asr-0.6b-q8_0",
    repository: "ggml-org/Qwen3-ASR-0.6B-GGUF",
    revision: "928ab958557df9aa2ef1c93e0e83c7ad0933fae2",
    files: &QWEN_06B_Q8_FILES,
}];

pub(crate) fn package_spec(id: &str) -> Result<PackageSpec, String> {
    PACKAGES
        .iter()
        .copied()
        .find(|spec| spec.id == id)
        .ok_or_else(|| format!("Unsupported Qwen ASR package: {id}"))
}
