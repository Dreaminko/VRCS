use std::io::Write;
use std::path::Path;

use super::qwen_runtime_download::{extract_archive, installed, REQUIRED_FILES, SHA256};

fn archive(path: &Path, omitted: Option<&str>) {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for name in REQUIRED_FILES
        .iter()
        .copied()
        .filter(|name| *name != "LICENSE")
    {
        if Some(name) != omitted {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"runtime fixture").unwrap();
        }
    }
    for name in ["../escaped.dll", "llama-cli.exe"] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"must not extract").unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn runtime_archive_requires_all_dependencies_and_extracts_only_runtime_files() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("runtime.zip");
    let stage = root.path().join("stage");
    std::fs::create_dir(&stage).unwrap();
    std::fs::write(stage.join("LICENSE"), b"license fixture").unwrap();
    archive(&source, Some("llama-server-impl.dll"));
    assert!(extract_archive(&source, &stage).is_err());
    assert!(!installed(&stage));

    archive(&source, None);
    extract_archive(&source, &stage).unwrap();
    assert!(installed(&stage));
    assert!(!root.path().join("escaped.dll").exists());
    assert!(!stage.join("llama-cli.exe").exists());
    assert_eq!(
        std::fs::read_to_string(stage.join("version.txt"))
            .unwrap()
            .trim(),
        SHA256
    );
}

#[test]
fn runtime_is_not_available_with_a_missing_or_empty_dependency() {
    let root = tempfile::tempdir().unwrap();
    for name in REQUIRED_FILES {
        std::fs::write(root.path().join(name), b"fixture").unwrap();
    }
    std::fs::write(root.path().join("version.txt"), SHA256).unwrap();
    assert!(installed(root.path()));
    std::fs::write(root.path().join("ggml-vulkan.dll"), b"").unwrap();
    assert!(!installed(root.path()));
    std::fs::remove_file(root.path().join("ggml-vulkan.dll")).unwrap();
    assert!(!installed(root.path()));
}

#[tokio::test]
async fn runtime_asset_with_a_wrong_checksum_cannot_be_activated() {
    use axum::{routing::get, Router};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/runtime.zip", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route("/runtime.zip", get(|| async { "abd" })),
        )
        .await
        .unwrap();
    });
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("runtime.zip");
    let temporary = root.path().join("runtime.zip.part");
    let (_sender, mut cancel) = tokio::sync::watch::channel(false);
    let result = super::qwen_models::download_file(
        &reqwest::Client::new(),
        reqwest::Url::parse(&url).unwrap(),
        super::qwen_models::AssetSpec {
            name: "runtime.zip",
            bytes: 3,
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        },
        &target,
        &temporary,
        &mut cancel,
        |_| {},
    )
    .await;
    assert!(result.unwrap_err().contains("SHA-256"));
    assert!(!target.exists());
    server.abort();
}

#[tokio::test]
async fn cancelling_runtime_download_cleans_staging_and_releases_the_download_slot() {
    use std::sync::Arc;
    let root = tempfile::tempdir().unwrap();
    let manager = Arc::new(super::ModelManager::new(root.path().join("models")).unwrap());
    let runtime = Arc::new(super::QwenRuntime::new(root.path().join("runtimes/qwen")));
    manager
        .start_qwen_runtime_download(Arc::clone(&runtime))
        .unwrap();
    manager
        .start_qwen_runtime_download(Arc::clone(&runtime))
        .unwrap();
    manager.cancel_qwen_runtime_download().await;
    assert!(!runtime.directory.exists());
    assert_eq!(
        std::fs::read_dir(runtime.directory.parent().unwrap())
            .unwrap()
            .count(),
        0
    );
    manager
        .move_model_dir(root.path().join("other-models"))
        .unwrap();
}

#[tokio::test]
#[ignore = "downloads the pinned 32 MiB Windows runtime from GitHub"]
async fn real_qwen_runtime_download_installs_and_launches() {
    use std::sync::Arc;
    use std::time::Duration;
    let root = tempfile::tempdir().unwrap();
    let manager = Arc::new(super::ModelManager::new(root.path().join("models")).unwrap());
    let runtime = Arc::new(super::QwenRuntime::new(root.path().join("runtimes/qwen")));
    manager
        .start_qwen_runtime_download(Arc::clone(&runtime))
        .unwrap();
    tokio::time::timeout(Duration::from_secs(180), async {
        loop {
            let installation =
                serde_json::to_value(manager.qwen_runtime_installation(&runtime)).unwrap();
            match installation["status"].as_str().unwrap() {
                "installed" => break,
                "error" => panic!("{}", installation["error"]),
                _ => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    })
    .await
    .unwrap();
    let executable = runtime.executable_path().unwrap();
    assert!(executable.starts_with(root.path()));
    assert!(installed(executable.parent().unwrap()));
    let output = super::qwen_runtime::runtime_command(&executable)
        .arg("--list-devices")
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
}
