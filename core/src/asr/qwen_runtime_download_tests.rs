use std::io::Write;
use std::path::Path;

use super::qwen_runtime_download::{assets, extract, installed};

fn zip_archive(path: &Path, omitted: Option<&str>) {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for name in assets::WINDOWS.required_files.iter().copied().filter(|n| *n != "LICENSE") {
        if Some(name) != omitted {
            zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(b"runtime fixture").unwrap();
        }
    }
    for name in ["../escaped.dll", "llama-cli.exe"] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"must not extract").unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn windows_runtime_keeps_legacy_layout_and_requires_its_dependencies() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("runtime.zip");
    let asset = assets::RuntimeAsset { licenses: &[], ..assets::WINDOWS };
    let directory = asset.directory(root.path());
    assert_eq!(directory, root.path().join("b11501"));
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join("LICENSE"), b"license fixture").unwrap();
    zip_archive(&source, Some("llama-server-impl.dll"));
    assert!(extract::extract_archive(&source, &directory, &asset).is_err());
    assert!(!extract::installed(&directory, &asset));
    extract::remove_owned_directory(&directory, &asset).unwrap();
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join("LICENSE"), b"license fixture").unwrap();
    zip_archive(&source, None);
    extract::extract_archive(&source, &directory, &asset).unwrap();
    assert!(extract::installed(&directory, &asset));
    assert!(!root.path().join("escaped.dll").exists());
    assert!(!directory.join("llama-cli.exe").exists());
    std::fs::write(directory.join("ggml-vulkan.dll"), b"").unwrap();
    assert!(!extract::installed(&directory, &asset));
}

type TarEntry<'a> = (&'a str, tar::EntryType, &'a str, &'a [u8], Option<u64>);
// Build raw tar headers so malicious paths are exercised even when tar's
// writer would refuse to create them through its normal path API.
fn tar_archive(path: &Path, entries: &[TarEntry<'_>]) {
    let gzip = flate2::write::GzEncoder::new(std::fs::File::create(path).unwrap(),
        flate2::Compression::default());
    let mut archive = tar::Builder::new(gzip);
    for (name, kind, target, contents, declared_size) in entries {
        let mut header = tar::Header::new_gnu();
        header.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
        header.set_entry_type(*kind);
        header.set_mode(0o7777);
        header.set_size(declared_size.unwrap_or(contents.len() as u64));
        if !target.is_empty() { header.set_link_name(target).unwrap(); }
        header.set_cksum();
        archive.append(&header, *contents).unwrap();
    }
    archive.into_inner().unwrap().finish().unwrap();
}

#[test]
fn linux_tar_rejects_traversal_oversize_and_wrong_elf_architecture() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("runtime.tar.gz");
    let cases: Vec<(&str, Vec<u8>, Option<u64>)> = vec![
        ("llama-b11501/../escaped", b"bad".to_vec(), None),
        ("/tmp/escaped", b"bad".to_vec(), None),
        ("llama-b11501/unused", Vec::new(), Some(128 * 1024 * 1024 + 1)),
        ("llama-b11501/llama-server", {
            let mut header = vec![0u8; 20];
            header[..6].copy_from_slice(b"\x7fELF\x02\x01");
            header[18..20].copy_from_slice(&183u16.to_le_bytes());
            header
        }, None),
    ];
    for (index, (name, contents, size)) in cases.iter().enumerate() {
        let stage = root.path().join(format!("stage-{index}"));
        std::fs::create_dir(&stage).unwrap();
        tar_archive(&source, &[(name, tar::EntryType::Regular, "", contents, *size)]);
        assert!(extract::extract_archive(&source, &stage, &assets::LINUX_X64).is_err());
        assert!(!stage.join("version.txt").exists());
    }
    assert!(!root.path().join("escaped").exists());
}

#[test]
fn linux_tar_rejects_external_dangling_and_cyclic_links() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("runtime.tar.gz");
    let stage = root.path().join("stage");
    std::fs::create_dir(&stage).unwrap();
    for target in ["../outside", "/tmp/outside", "libllama.so.0", "libllama.so"] {
        tar_archive(&source, &[("llama-b11501/libllama.so", tar::EntryType::Symlink, target, b"", None)]);
        assert!(extract::extract_archive(&source, &stage, &assets::LINUX_X64).is_err());
        assert!(!stage.join("libllama.so").symlink_metadata().is_ok());
    }
    tar_archive(&source, &[
        ("llama-b11501/libllama.so", tar::EntryType::Symlink, "libllama.so.0", b"", None),
        ("llama-b11501/libllama.so.0", tar::EntryType::Symlink, "libllama.so", b"", None),
    ]);
    assert!(extract::extract_archive(&source, &stage, &assets::LINUX_X64).is_err());
}

#[test]
fn truncated_gzip_is_never_marked_installed() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("runtime.tar.gz");
    std::fs::write(&source, b"\x1f\x8b\x08broken").unwrap();
    let stage = root.path().join("stage");
    std::fs::create_dir(&stage).unwrap();
    assert!(extract::extract_archive(&source, &stage, &assets::LINUX_X64).is_err());
    assert!(!stage.join("version.txt").exists());
}

#[cfg(unix)]
#[test]
fn linux_vulkan_installation_requires_the_gpu_backend() {
    let root = tempfile::tempdir().unwrap();
    let asset = &assets::LINUX_VULKAN_X64;
    let directory = asset.directory(root.path());
    std::fs::create_dir_all(&directory).unwrap();
    let mut elf = [0u8; 20];
    elf[..6].copy_from_slice(b"\x7fELF\x02\x01");
    elf[18..20].copy_from_slice(&62u16.to_le_bytes());
    for name in asset.required_files() {
        std::fs::write(directory.join(name), elf).unwrap();
    }
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(directory.join(asset.executable), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(directory.join("version.txt"), asset.archive.sha256).unwrap();
    assert!(extract::installed(&directory, asset));
    assert!(!extract::installed(&directory, &assets::LINUX_X64));
    std::fs::remove_file(directory.join("libggml-vulkan.so")).unwrap();
    assert!(!extract::installed(&directory, asset));
}

#[test]
fn failed_activation_restores_previous_installation() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("b11501");
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join("llama-server.exe"), b"previous usable executable").unwrap();
    let error = extract::activate(&root.path().join("missing-stage"), &directory, &assets::WINDOWS)
        .unwrap_err();
    assert!(error.contains("Could not activate"));
    assert_eq!(std::fs::read(directory.join("llama-server.exe")).unwrap(), b"previous usable executable");
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn runtime_asset_with_a_wrong_checksum_cannot_be_activated() {
    use axum::{routing::get, Router};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/runtime.zip", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/runtime.zip", get(|| async { "abd" })))
            .await.unwrap();
    });
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("runtime.zip");
    let (_sender, mut cancel) = tokio::sync::watch::channel(false);
    let result = super::qwen_models::download_file(
        &reqwest::Client::new(), reqwest::Url::parse(&url).unwrap(),
        super::qwen_models::AssetSpec {
            name: "runtime.zip", bytes: 3,
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        }, &target, &root.path().join("runtime.zip.part"), &mut cancel, |_| {},
    ).await;
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
    manager.start_qwen_runtime_download(Arc::clone(&runtime)).unwrap();
    manager.start_qwen_runtime_download(Arc::clone(&runtime)).unwrap();
    manager.cancel_qwen_runtime_download().await;
    assert!(!runtime.directory.exists());
    assert_eq!(std::fs::read_dir(runtime.directory.parent().unwrap()).unwrap().count(), 0);
    manager.move_model_dir(root.path().join("other-models")).unwrap();
}

#[tokio::test]
#[ignore = "downloads the pinned runtime from GitHub and launches it on the native architecture"]
async fn real_qwen_runtime_download_installs_and_launches() {
    use std::sync::Arc;
    use std::time::Duration;
    let root = tempfile::tempdir().unwrap();
    let manager = Arc::new(super::ModelManager::new(root.path().join("models")).unwrap());
    let runtime = Arc::new(super::QwenRuntime::new(root.path().join("runtimes/qwen")));
    manager.start_qwen_runtime_download(Arc::clone(&runtime)).unwrap();
    tokio::time::timeout(Duration::from_secs(180), async {
        loop {
            let installation = serde_json::to_value(manager.qwen_runtime_installation(&runtime)).unwrap();
            match installation["status"].as_str().unwrap() {
                "installed" => break,
                "error" => panic!("{}", installation["error"]),
                _ => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    }).await.unwrap();
    let executable = runtime.executable_path().unwrap();
    assert!(executable.starts_with(root.path()));
    assert!(installed(executable.parent().unwrap()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&executable).unwrap().permissions().mode() & 0o777, 0o755);
        assert!(std::fs::read_link(executable.parent().unwrap().join("libllama.so")).is_ok());
    }
    let output = super::qwen_runtime::runtime_command(&executable)
        .arg("--list-devices").output().await.unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    println!("{}", String::from_utf8_lossy(&output.stdout));
}
