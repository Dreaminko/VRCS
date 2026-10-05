use std::path::Path;

use super::qwen_models::{
    activate_staging, download_into_staging, is_installed, package_dir, prepare_migration,
    staging_dir, AssetSpec, PackageSpec,
};

const FILES: [AssetSpec; 2] = [
    AssetSpec {
        name: "main.gguf",
        bytes: 3,
        sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    },
    AssetSpec {
        name: "mmproj.gguf",
        bytes: 3,
        sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    },
];

const FIXTURE: PackageSpec = PackageSpec {
    id: "fixture-q8",
    repository: "fixture/repo",
    revision: "revision-1",
    files: &FILES,
};

const WHISPER_FIXTURE: super::ModelSpec = super::ModelSpec {
    id: "fixture",
    filename: "ggml-fixture.bin",
    expected_bytes: 3,
    sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
};

fn write_file(directory: &Path, name: &str, bytes: &[u8]) {
    std::fs::create_dir_all(directory).unwrap();
    std::fs::write(directory.join(name), bytes).unwrap();
}

#[test]
fn qwen_package_becomes_visible_only_after_both_files_verify() {
    let root = tempfile::tempdir().unwrap();
    let stage = staging_dir(root.path(), FIXTURE);
    let installed = package_dir(root.path(), FIXTURE);

    write_file(&stage, "main.gguf", b"abc");
    assert!(activate_staging(root.path(), FIXTURE).is_err());
    assert!(!installed.exists());

    write_file(&stage, "mmproj.gguf", b"abd");
    assert!(activate_staging(root.path(), FIXTURE).is_err());
    assert!(!installed.exists());

    write_file(&stage, "mmproj.gguf", b"abc");
    activate_staging(root.path(), FIXTURE).unwrap();
    assert!(is_installed(root.path(), FIXTURE, true).unwrap());
    assert!(!stage.exists());

    write_file(&installed, "mmproj.gguf", b"abd");
    assert!(!is_installed(root.path(), FIXTURE, true).unwrap());
}

#[test]
fn qwen_package_does_not_activate_with_unmanaged_staging_files() {
    let root = tempfile::tempdir().unwrap();
    let stage = staging_dir(root.path(), FIXTURE);
    write_file(&stage, "main.gguf", b"abc");
    write_file(&stage, "mmproj.gguf", b"abc");
    write_file(&stage, "user-note.txt", b"keep this");

    assert!(activate_staging(root.path(), FIXTURE).is_err());
    assert!(!package_dir(root.path(), FIXTURE).exists());
    assert_eq!(
        std::fs::read(stage.join("user-note.txt")).unwrap(),
        b"keep this"
    );
}

#[test]
fn qwen_download_respects_an_existing_whisper_download() {
    use std::sync::Arc;

    let root = tempfile::tempdir().unwrap();
    let manager = Arc::new(super::ModelManager::new(root.path().to_path_buf()).unwrap());
    let (cancel, _) = tokio::sync::watch::channel(false);
    manager.jobs.lock().unwrap().insert(
        "tiny".into(),
        super::DownloadJob {
            status: "downloading",
            downloaded_bytes: 0,
            total_bytes: 100,
            error: None,
            cancel,
            done: Arc::new(tokio::sync::Notify::new()),
        },
    );
    assert!(manager.start_qwen_download("qwen3-asr-0.6b-q8_0").is_err());
}

#[test]
fn model_directory_change_cannot_leave_qwen_packages_behind() {
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let managed = source.path().join("qwen-asr").join("qwen3-asr-0.6b-q8_0");
    std::fs::create_dir_all(&managed).unwrap();
    std::fs::write(managed.join("marker"), b"owned by user").unwrap();
    let manager = super::ModelManager::new(source.path().to_path_buf()).unwrap();

    assert!(manager.move_model_dir(target.path().to_path_buf()).is_err());
    assert_eq!(manager.model_dir(), source.path());
    assert!(managed.join("marker").exists());
}

#[test]
fn qwen_delete_preserves_unmanaged_files() {
    let root = tempfile::tempdir().unwrap();
    let manager = super::ModelManager::new(root.path().to_path_buf()).unwrap();
    let spec = super::qwen_models::PACKAGES[0];
    let stage = staging_dir(root.path(), spec);
    write_file(&stage, "user-note.txt", b"keep this");

    assert!(manager.delete_qwen(spec.id).is_err());
    assert_eq!(
        std::fs::read(stage.join("user-note.txt")).unwrap(),
        b"keep this"
    );
}

#[test]
fn qwen_migration_keeps_the_source_until_commit_and_can_rollback() {
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let old_package = package_dir(source.path(), FIXTURE);
    let new_package = package_dir(target.path(), FIXTURE);
    write_file(&old_package, "main.gguf", b"abc");
    write_file(&old_package, "mmproj.gguf", b"abc");

    let migration = prepare_migration(source.path(), target.path(), &[FIXTURE]).unwrap();
    assert!(is_installed(source.path(), FIXTURE, true).unwrap());
    assert!(is_installed(target.path(), FIXTURE, true).unwrap());
    migration.rollback().unwrap();
    assert!(old_package.exists());
    assert!(!new_package.exists());

    let migration = prepare_migration(source.path(), target.path(), &[FIXTURE]).unwrap();
    migration.commit().unwrap();
    assert!(!old_package.exists());
    assert!(is_installed(target.path(), FIXTURE, true).unwrap());
}

#[test]
fn asr_directory_change_moves_whisper_and_qwen_together() {
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_file(source.path(), WHISPER_FIXTURE.filename, b"abc");
    let old_qwen = package_dir(source.path(), FIXTURE);
    write_file(&old_qwen, "main.gguf", b"abc");
    write_file(&old_qwen, "mmproj.gguf", b"abc");

    super::migration::move_model_dir_with_specs(
        source.path().to_path_buf(),
        target.path().to_path_buf(),
        &[WHISPER_FIXTURE],
        &[FIXTURE],
    )
    .unwrap();

    assert!(!source.path().join(WHISPER_FIXTURE.filename).exists());
    assert!(!old_qwen.exists());
    assert_eq!(
        std::fs::read(target.path().join(WHISPER_FIXTURE.filename)).unwrap(),
        b"abc"
    );
    assert!(is_installed(target.path(), FIXTURE, true).unwrap());

    super::migration::move_model_dir_with_specs(
        target.path().to_path_buf(),
        source.path().to_path_buf(),
        &[WHISPER_FIXTURE],
        &[FIXTURE],
    )
    .unwrap();
    assert!(is_installed(source.path(), FIXTURE, true).unwrap());
    assert!(!package_dir(target.path(), FIXTURE).exists());
    assert_eq!(
        std::fs::read(source.path().join(WHISPER_FIXTURE.filename)).unwrap(),
        b"abc"
    );
}

#[test]
fn whisper_migration_failure_rolls_back_new_qwen_target() {
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    write_file(source.path(), WHISPER_FIXTURE.filename, b"abc");
    write_file(target.path(), WHISPER_FIXTURE.filename, b"abd");
    let old_qwen = package_dir(source.path(), FIXTURE);
    write_file(&old_qwen, "main.gguf", b"abc");
    write_file(&old_qwen, "mmproj.gguf", b"abc");

    assert!(super::migration::move_model_dir_with_specs(
        source.path().to_path_buf(),
        target.path().to_path_buf(),
        &[WHISPER_FIXTURE],
        &[FIXTURE],
    )
    .is_err());

    assert_eq!(
        std::fs::read(source.path().join(WHISPER_FIXTURE.filename)).unwrap(),
        b"abc"
    );
    assert_eq!(
        std::fs::read(target.path().join(WHISPER_FIXTURE.filename)).unwrap(),
        b"abd"
    );
    assert!(is_installed(source.path(), FIXTURE, true).unwrap());
    assert!(!package_dir(target.path(), FIXTURE).exists());
}

#[tokio::test]
async fn qwen_download_reuses_verified_files_after_a_failed_companion_download() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    use axum::{http::StatusCode, routing::get, Router};

    let main_hits = Arc::new(AtomicUsize::new(0));
    let companion_fails = Arc::new(AtomicBool::new(true));
    let first_hits = Arc::clone(&main_hits);
    let second_fails = Arc::clone(&companion_fails);
    let app = Router::new()
        .route(
            "/main.gguf",
            get(move || {
                let hits = Arc::clone(&first_hits);
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    b"abc".to_vec()
                }
            }),
        )
        .route(
            "/mmproj.gguf",
            get(move || {
                let fails = Arc::clone(&second_fails);
                async move {
                    if fails.load(Ordering::SeqCst) {
                        (StatusCode::INTERNAL_SERVER_ERROR, b"error".to_vec())
                    } else {
                        (StatusCode::OK, b"abc".to_vec())
                    }
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let root = tempfile::tempdir().unwrap();
    let client = reqwest::Client::new();
    let (_cancel_sender, cancel) = tokio::sync::watch::channel(false);
    let url = format!("http://{address}");

    let first_error = download_into_staging(
        root.path(),
        FIXTURE,
        &url,
        &client,
        cancel.clone(),
        |_, _| {},
    )
    .await
    .unwrap_err();
    assert!(!package_dir(root.path(), FIXTURE).exists());
    assert!(
        staging_dir(root.path(), FIXTURE).join("main.gguf").exists(),
        "{first_error}"
    );

    companion_fails.store(false, Ordering::SeqCst);
    download_into_staging(root.path(), FIXTURE, &url, &client, cancel, |_, _| {})
        .await
        .unwrap();
    assert!(is_installed(root.path(), FIXTURE, true).unwrap());
    assert_eq!(main_hits.load(Ordering::SeqCst), 1);
    server.abort();
}
