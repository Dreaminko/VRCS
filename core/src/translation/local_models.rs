use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock as StdRwLock};

use serde::{Deserialize, Serialize};
use tokio::sync::{watch, OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

use crate::asr::{
    download_model_file, paths_equivalent, verify_model_file, verify_model_file_forced,
    verify_model_file_readonly, ModelAssetSpec,
};

use super::plamo::{
    MODEL_BYTES, MODEL_FILENAME, MODEL_ID, MODEL_REPOSITORY, MODEL_REVISION, MODEL_SHA256,
};

const LICENSE_BYTES: u64 = 7_629;
const LICENSE_SHA256: &str = "d9979039b8f1a96fa81e1be61c4550d06f8542b25119e3f765c7c011a68597fb";
const ACCEPTANCE_FILE: &str = ".plamo-license-acceptance.json";
const MODEL: ModelAssetSpec = ModelAssetSpec {
    name: MODEL_FILENAME,
    bytes: MODEL_BYTES,
    sha256: MODEL_SHA256,
};
const LICENSE: ModelAssetSpec = ModelAssetSpec {
    name: "LICENSE-PLaMo-Community-en.txt",
    bytes: LICENSE_BYTES,
    sha256: LICENSE_SHA256,
};

#[derive(Clone)]
struct DownloadJob {
    status: &'static str,
    downloaded_bytes: u64,
    error: Option<String>,
    cancel: watch::Sender<bool>,
    done: watch::Sender<bool>,
}

impl DownloadJob {
    fn active(&self) -> bool {
        matches!(self.status, "downloading" | "verifying")
    }
}

pub(crate) struct LocalTranslationModels {
    root: StdRwLock<PathBuf>,
    sources: Arc<crate::downloads::DownloadSources>,
    client: reqwest::Client,
    job: Mutex<Option<DownloadJob>>,
    lifecycle: Arc<RwLock<()>>,
}

#[derive(Clone, Serialize)]
pub(crate) struct LocalTranslationModelStatus {
    pub(crate) id: &'static str,
    pub(crate) revision: &'static str,
    pub(crate) bytes: u64,
    pub(crate) sha256: &'static str,
    pub(crate) license: &'static str,
    pub(crate) license_url: &'static str,
    pub(crate) license_accepted: bool,
    pub(crate) status: &'static str,
    pub(crate) downloaded_bytes: u64,
    pub(crate) total_bytes: u64,
    pub(crate) progress: f64,
    pub(crate) error: Option<String>,
}

impl LocalTranslationModels {
    pub(crate) fn new(
        root: PathBuf,
        sources: Arc<crate::downloads::DownloadSources>,
    ) -> Result<Self, String> {
        std::fs::create_dir_all(&root)
            .map_err(|error| format!("Could not create model directory: {error}"))?;
        Ok(Self {
            root: StdRwLock::new(root),
            sources,
            client: crate::downloads::client_builder()
                .build()
                .map_err(|error| format!("Could not create model download client: {error}"))?,
            job: Mutex::new(None),
            lifecycle: Arc::new(RwLock::new(())),
        })
    }

    pub(crate) fn root(&self) -> PathBuf {
        self.root
            .read()
            .expect("translation model directory lock")
            .clone()
    }

    pub(crate) fn verified_model_path(&self) -> Result<PathBuf, String> {
        let root = self.root();
        if !license_acceptance_recorded(&root) {
            return Err("The PLaMo Community License has not been accepted".into());
        }
        if !verify_package(&root)? {
            return Err("The local PLaMo model is missing or failed verification".into());
        }
        Ok(model_path(&root))
    }

    pub(crate) async fn lock_shared(&self) -> OwnedRwLockReadGuard<()> {
        Arc::clone(&self.lifecycle).read_owned().await
    }

    pub(crate) async fn lock_exclusive(&self) -> OwnedRwLockWriteGuard<()> {
        Arc::clone(&self.lifecycle).write_owned().await
    }

    pub(crate) fn status(&self) -> LocalTranslationModelStatus {
        let package = package_dir(&self.root());
        let installed = verify_assets_readonly(&package, &[MODEL, LICENSE]).unwrap_or(false);
        let package_exists = package.exists();
        let job = self.job.lock().expect("translation model job lock").clone();
        let (status, downloaded_bytes, error) = match job {
            Some(job) if job.active() || job.error.is_some() => {
                (job.status, job.downloaded_bytes, job.error)
            }
            _ if installed => ("installed", MODEL_BYTES, None),
            _ if package_exists => ("corrupt", 0, None),
            _ => ("not_downloaded", 0, None),
        };
        LocalTranslationModelStatus {
            id: MODEL_ID,
            revision: MODEL_REVISION,
            bytes: MODEL_BYTES,
            sha256: MODEL_SHA256,
            license: "PLaMo Community License",
            license_url: "https://plamo.preferredai.jp/info/plamo-community-license-en",
            license_accepted: license_acceptance_recorded(&self.root()),
            status,
            downloaded_bytes,
            total_bytes: MODEL_BYTES,
            progress: if status == "installed" {
                1.0
            } else {
                (downloaded_bytes as f64 / MODEL_BYTES as f64).min(0.99)
            },
            error,
        }
    }

    pub(crate) fn start_download_locked(
        self: &Arc<Self>,
        accepted_revision: &str,
        lifecycle: OwnedRwLockWriteGuard<()>,
    ) -> Result<(), String> {
        if !cfg!(all(windows, target_arch = "x86_64")) {
            return Err("Local PLaMo translation supports Windows x64 only".into());
        }
        if accepted_revision != MODEL_REVISION {
            return Err(
                "The PLaMo Community License must be accepted for this model revision".into(),
            );
        }
        record_license_acceptance(&self.root(), accepted_revision)?;
        let (cancel, receiver) = watch::channel(false);
        let (done, _) = watch::channel(false);
        let installed = self.status().status == "installed";
        {
            let mut job = self.job.lock().expect("translation model job lock");
            if installed {
                return Ok(());
            }
            if job.as_ref().is_some_and(DownloadJob::active) {
                return Ok(());
            }
            *job = Some(DownloadJob {
                status: "downloading",
                downloaded_bytes: 0,
                error: None,
                cancel,
                done: done.clone(),
            });
        }
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            let _lifecycle = lifecycle;
            let result = manager.download(receiver.clone()).await;
            let mut job = manager.job.lock().expect("translation model job lock");
            if *receiver.borrow() || result.is_ok() {
                *job = None;
            } else if let Some(job) = job.as_mut() {
                job.status = "error";
                job.error = result.err();
            }
            // Store a permit if cancellation begins after the worker finishes.
            complete_download(&done);
        });
        Ok(())
    }

    pub(crate) async fn cancel(&self) {
        let job = self
            .job
            .lock()
            .expect("translation model job lock")
            .as_ref()
            .filter(|job| job.active())
            .cloned();
        if let Some(job) = job {
            cancel_job(job).await;
        }
    }

    pub(crate) fn verify_locked(
        &self,
        _lifecycle: &OwnedRwLockReadGuard<()>,
    ) -> Result<LocalTranslationModelStatus, String> {
        if !verify_assets_forced(&package_dir(&self.root()), &[MODEL, LICENSE])? {
            return Err("The local PLaMo model is missing or failed verification".into());
        }
        Ok(self.status())
    }

    pub(crate) fn delete_locked(
        &self,
        _lifecycle: &OwnedRwLockWriteGuard<()>,
    ) -> Result<bool, String> {
        if self
            .job
            .lock()
            .expect("translation model job lock")
            .as_ref()
            .is_some_and(DownloadJob::active)
        {
            return Err("Cancel the PLaMo model download before deleting it".into());
        }
        remove_managed_package(&package_dir(&self.root()), false)
    }

    pub(crate) fn move_root_locked(
        &self,
        root: PathBuf,
        _lifecycle: &OwnedRwLockWriteGuard<()>,
    ) -> Result<(), String> {
        let previous = self.root();
        if paths_equivalent(&previous, &root) {
            return Ok(());
        }
        if self
            .job
            .lock()
            .expect("translation model job lock")
            .as_ref()
            .is_some_and(DownloadJob::active)
        {
            return Err(
                "The model storage path cannot change during a translation model download".into(),
            );
        }
        let accepted = license_acceptance_recorded(&previous);
        if accepted {
            record_license_acceptance(&root, MODEL_REVISION)?;
        }
        if verify_package(&previous)? {
            std::fs::create_dir_all(translation_root(&root)).map_err(|error| error.to_string())?;
            let destination = package_dir(&root);
            if destination.exists() {
                if !verify_package(&root)? {
                    return Err("The destination contains an invalid PLaMo model".into());
                }
                remove_managed_package(&package_dir(&previous), false)?;
            } else {
                move_package(&package_dir(&previous), &destination)?;
            }
        }
        *self.root.write().expect("translation model directory lock") = root;
        Ok(())
    }

    async fn download(&self, cancel: watch::Receiver<bool>) -> Result<(), String> {
        let root = self.root();
        let target = package_dir(&root);
        let stage = target.with_file_name(format!(".{MODEL_REVISION}.staging"));
        let assets = [
            (
                MODEL,
                format!(
                    "https://huggingface.co/{MODEL_REPOSITORY}/resolve/{MODEL_REVISION}/{MODEL_FILENAME}"
                ),
            ),
            (
                LICENSE,
                format!(
                    "https://huggingface.co/{MODEL_REPOSITORY}/resolve/{MODEL_REVISION}/LICENSE/en"
                ),
            ),
        ];
        download_assets(
            &self.client,
            &self.sources,
            &stage,
            &target,
            &assets,
            cancel,
            |asset, bytes| {
                if asset.name == MODEL.name {
                    if let Some(job) = self
                        .job
                        .lock()
                        .expect("translation model job lock")
                        .as_mut()
                    {
                        job.downloaded_bytes = bytes;
                        if bytes == MODEL_BYTES {
                            job.status = "verifying";
                        }
                    }
                }
            },
        )
        .await
    }
}

fn complete_download(done: &watch::Sender<bool>) {
    done.send_replace(true);
}

async fn cancel_job(job: DownloadJob) {
    let _ = job.cancel.send(true);
    let mut done = job.done.subscribe();
    if !*done.borrow() {
        let _ = done.changed().await;
    }
}

async fn download_assets(
    client: &reqwest::Client,
    sources: &crate::downloads::DownloadSources,
    stage: &Path,
    target: &Path,
    assets: &[(ModelAssetSpec, String)],
    mut cancel: watch::Receiver<bool>,
    mut on_progress: impl FnMut(ModelAssetSpec, u64),
) -> Result<(), String> {
    let specs = assets.iter().map(|(asset, _)| *asset).collect::<Vec<_>>();
    remove_managed_assets(stage, true, &specs)?;
    tokio::fs::create_dir_all(stage)
        .await
        .map_err(|error| error.to_string())?;
    let result = async {
        for (asset, official_url) in assets {
            if *cancel.borrow() {
                return Err("PLaMo model download was cancelled".into());
            }
            let path = stage.join(asset.name);
            let temporary = stage.join(format!(".{}.part", asset.name));
            let urls = sources.urls(official_url).await;
            let mut last_error = "No PLaMo model download source is available".to_string();
            for url in urls {
                if *cancel.borrow() {
                    return Err("PLaMo model download was cancelled".into());
                }
                match download_model_file(
                    client,
                    reqwest::Url::parse(&url).map_err(|error| error.to_string())?,
                    *asset,
                    &path,
                    &temporary,
                    &mut cancel,
                    |bytes| on_progress(*asset, bytes),
                )
                .await
                {
                    Ok(()) => break,
                    Err(error) => {
                        let _ = tokio::fs::remove_file(&temporary).await;
                        last_error = error;
                    }
                }
            }
            if !path.is_file() {
                return Err(last_error);
            }
        }
        if *cancel.borrow() {
            return Err("PLaMo model download was cancelled".into());
        }
        remove_managed_assets(target, false, &specs)?;
        tokio::fs::rename(stage, target)
            .await
            .map_err(|error| format!("Could not activate local PLaMo model: {error}"))
    }
    .await;
    if result.is_err() {
        let _ = remove_managed_assets(stage, true, &specs);
    }
    result
}

fn translation_root(root: &Path) -> PathBuf {
    root.join("translation")
}

fn license_acceptance_path(root: &Path) -> PathBuf {
    translation_root(root).join(ACCEPTANCE_FILE)
}

#[derive(Deserialize, Serialize)]
struct LicenseAcceptance {
    revision: String,
}

fn license_acceptance_recorded(root: &Path) -> bool {
    std::fs::read(license_acceptance_path(root))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<LicenseAcceptance>(&bytes).ok())
        .is_some_and(|acceptance| acceptance.revision == MODEL_REVISION)
}

fn record_license_acceptance(root: &Path, revision: &str) -> Result<(), String> {
    let path = license_acceptance_path(root);
    if std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<LicenseAcceptance>(&bytes).ok())
        .is_some_and(|acceptance| acceptance.revision == revision)
    {
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| "The PLaMo license acceptance path has no parent".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = path.with_extension("tmp");
    let bytes = serde_json::to_vec(&LicenseAcceptance {
        revision: revision.into(),
    })
    .map_err(|error| error.to_string())?;
    std::fs::write(&temporary, bytes)
        .map_err(|error| format!("Could not record the PLaMo license acceptance: {error}"))?;
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|error| format!("Could not update the PLaMo license acceptance: {error}"))?;
    }
    std::fs::rename(&temporary, &path)
        .map_err(|error| format!("Could not save the PLaMo license acceptance: {error}"))
}

fn package_dir(root: &Path) -> PathBuf {
    translation_root(root).join(MODEL_ID).join(MODEL_REVISION)
}

fn model_path(root: &Path) -> PathBuf {
    package_dir(root).join(MODEL_FILENAME)
}

fn verify_package(root: &Path) -> Result<bool, String> {
    verify_assets(&package_dir(root), &[MODEL, LICENSE])
}

fn verify_assets(directory: &Path, assets: &[ModelAssetSpec]) -> Result<bool, String> {
    verify_assets_with_cache(directory, assets, true)
}

fn verify_assets_readonly(directory: &Path, assets: &[ModelAssetSpec]) -> Result<bool, String> {
    verify_assets_with_cache(directory, assets, false)
}

fn verify_assets_forced(directory: &Path, assets: &[ModelAssetSpec]) -> Result<bool, String> {
    if !assets_have_expected_size(directory, assets) {
        return Ok(false);
    }
    for asset in assets {
        if !verify_model_file_forced(&directory.join(asset.name), *asset)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn verify_assets_with_cache(
    directory: &Path,
    assets: &[ModelAssetSpec],
    write_cache: bool,
) -> Result<bool, String> {
    if !assets_have_expected_size(directory, assets) {
        return Ok(false);
    }
    for asset in assets {
        let path = directory.join(asset.name);
        let verified = if write_cache {
            verify_model_file(&path, *asset)?
        } else {
            verify_model_file_readonly(&path, *asset)?
        };
        if !verified {
            return Ok(false);
        }
    }
    Ok(true)
}

fn assets_have_expected_size(directory: &Path, assets: &[ModelAssetSpec]) -> bool {
    assets.iter().all(|asset| {
        let path = directory.join(asset.name);
        path.is_file()
            && path
                .metadata()
                .is_ok_and(|metadata| metadata.is_file() && metadata.len() == asset.bytes)
    })
}

fn move_package(source: &Path, destination: &Path) -> Result<(), String> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!("Could not create the local PLaMo model directory: {error}")
        })?;
    }
    match std::fs::rename(source, destination) {
        Ok(()) => return Ok(()),
        Err(error) if error.kind() != std::io::ErrorKind::CrossesDevices => {
            return Err(format!("Could not move the local PLaMo model: {error}"));
        }
        Err(_) => {}
    }
    copy_package(source, destination, &[MODEL, LICENSE])
}

fn copy_package(
    source: &Path,
    destination: &Path,
    assets: &[ModelAssetSpec],
) -> Result<(), String> {
    let temporary = destination.with_file_name(format!(".{MODEL_REVISION}.moving"));
    remove_managed_assets(&temporary, false, assets)?;
    std::fs::create_dir_all(&temporary).map_err(|error| error.to_string())?;
    let result = (|| {
        for asset in assets {
            std::fs::copy(source.join(asset.name), temporary.join(asset.name))
                .map_err(|error| format!("Could not copy the local PLaMo model: {error}"))?;
        }
        if !verify_assets(&temporary, assets)? {
            return Err("The copied local PLaMo model failed verification".into());
        }
        std::fs::rename(&temporary, destination)
            .map_err(|error| format!("Could not activate the moved local PLaMo model: {error}"))?;
        remove_managed_assets(source, false, assets)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = remove_managed_assets(&temporary, false, assets);
    }
    result
}

fn remove_managed_package(path: &Path, allow_partials: bool) -> Result<bool, String> {
    remove_managed_assets(path, allow_partials, &[MODEL, LICENSE])
}

fn remove_managed_assets(
    path: &Path,
    allow_partials: bool,
    assets: &[ModelAssetSpec],
) -> Result<bool, String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Local PLaMo model path is not a managed directory".into());
    }
    for entry in std::fs::read_dir(path).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let owned = assets.iter().any(|asset| {
            name == asset.name
                || name == format!("{}.vrcs-verified.json", asset.name)
                || (allow_partials && name == format!(".{}.part", asset.name))
        });
        if !owned
            || !entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_file()
        {
            return Err("Local PLaMo model path contains unmanaged files".into());
        }
    }
    std::fs::remove_dir_all(path).map_err(|error| error.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Router};
    use std::time::Duration;

    const FIXTURE: ModelAssetSpec = ModelAssetSpec {
        name: MODEL_FILENAME,
        bytes: 3,
        sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    };

    fn test_directory() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("vrcs-plamo-models-")
            .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
            .unwrap()
    }

    async fn fixture_server(body: &'static str) -> (String, tokio::task::JoinHandle<()>) {
        let app = Router::new().route("/model", get(move || async move { body }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (origin, server)
    }

    #[test]
    fn status_reports_the_pinned_model_before_installation() {
        let directory = tempfile::tempdir().unwrap();
        let manager = LocalTranslationModels::new(
            directory.path().into(),
            Arc::new(crate::downloads::DownloadSources::new(Default::default())),
        )
        .unwrap();
        let status = manager.status();
        assert_eq!(status.id, MODEL_ID);
        assert_eq!(status.status, "not_downloaded");
        assert_eq!(status.bytes, MODEL_BYTES);
        assert_eq!(status.revision, MODEL_REVISION);
    }

    #[tokio::test]
    async fn license_acceptance_is_bound_to_the_installed_revision() {
        let directory = tempfile::tempdir().unwrap();
        let manager = Arc::new(
            LocalTranslationModels::new(
                directory.path().into(),
                Arc::new(crate::downloads::DownloadSources::new(Default::default())),
            )
            .unwrap(),
        );
        let lifecycle = manager.lock_exclusive().await;
        let error = manager
            .start_download_locked("main", lifecycle)
            .unwrap_err();
        assert!(error.contains("accepted"));
    }

    #[test]
    fn license_acceptance_records_the_exact_model_revision() {
        let directory = tempfile::tempdir().unwrap();
        assert!(!license_acceptance_recorded(directory.path()));

        record_license_acceptance(directory.path(), MODEL_REVISION).unwrap();

        assert!(license_acceptance_recorded(directory.path()));
        let value: serde_json::Value = serde_json::from_slice(
            &std::fs::read(license_acceptance_path(directory.path())).unwrap(),
        )
        .unwrap();
        assert_eq!(value["revision"], MODEL_REVISION);
    }

    #[test]
    fn status_requires_the_license_as_well_as_the_model_file() {
        let directory = tempfile::tempdir().unwrap();
        let package = package_dir(directory.path());
        std::fs::create_dir_all(&package).unwrap();
        let model = std::fs::File::create(package.join(MODEL_FILENAME)).unwrap();
        model.set_len(MODEL_BYTES).unwrap();
        let manager = LocalTranslationModels::new(
            directory.path().into(),
            Arc::new(crate::downloads::DownloadSources::new(Default::default())),
        )
        .unwrap();

        assert_eq!(manager.status().status, "corrupt");
    }

    #[test]
    fn verified_runtime_requires_recorded_license_acceptance() {
        let directory = tempfile::tempdir().unwrap();
        let manager = LocalTranslationModels::new(
            directory.path().into(),
            Arc::new(crate::downloads::DownloadSources::new(Default::default())),
        )
        .unwrap();

        let error = manager.verified_model_path().unwrap_err();

        assert!(error.contains("License"));
    }

    #[test]
    fn managed_cleanup_owns_verification_sidecars() {
        let directory = tempfile::tempdir().unwrap();
        let package = directory.path().join("package");
        std::fs::create_dir_all(&package).unwrap();
        for name in [
            MODEL_FILENAME,
            "LICENSE-PLaMo-Community-en.txt",
            "plamo-3-translate-0.8b-Q4_K_M.gguf.vrcs-verified.json",
            "LICENSE-PLaMo-Community-en.txt.vrcs-verified.json",
        ] {
            std::fs::write(package.join(name), b"test").unwrap();
        }

        assert!(remove_managed_package(&package, false).unwrap());
        assert!(!package.exists());
    }

    #[test]
    fn explicit_verification_rehashes_cached_assets() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(FIXTURE.name);
        std::fs::write(&path, b"abd").unwrap();
        let modified_nanos = path
            .metadata()
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        let verification = path.with_file_name(format!("{}.vrcs-verified.json", FIXTURE.name));
        std::fs::write(
            verification,
            serde_json::to_vec(&serde_json::json!({
                "bytes": FIXTURE.bytes,
                "modified_nanos": modified_nanos,
                "sha256": FIXTURE.sha256,
            }))
            .unwrap(),
        )
        .unwrap();

        assert!(!verify_assets_forced(directory.path(), &[FIXTURE]).unwrap());
    }

    #[test]
    fn copied_packages_are_verified_in_their_temporary_directory() {
        let directory = tempfile::Builder::new()
            .prefix("vrcs-plamo-copy-")
            .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
            .unwrap();
        let source = directory.path().join("source");
        let destination = directory.path().join(MODEL_REVISION);
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("model.bin"), b"hello\n").unwrap();
        let asset = ModelAssetSpec {
            name: "model.bin",
            bytes: 6,
            sha256: "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03",
        };

        copy_package(&source, &destination, &[asset]).unwrap();

        assert_eq!(
            std::fs::read(destination.join("model.bin")).unwrap(),
            b"hello\n"
        );
        assert!(!source.exists());
    }

    #[test]
    fn moving_a_package_creates_its_destination_parent_directories() {
        let directory = test_directory();
        let source = directory.path().join("source");
        let destination = directory
            .path()
            .join("new-root")
            .join("translation")
            .join(MODEL_ID)
            .join(MODEL_REVISION);
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join(MODEL.name), b"model").unwrap();
        std::fs::write(source.join(LICENSE.name), b"license").unwrap();

        move_package(&source, &destination).unwrap();

        assert!(destination.join(MODEL.name).is_file());
        assert!(!source.exists());
    }

    #[tokio::test]
    async fn concurrent_cancel_waiters_all_finish_after_completion() {
        let (cancel, _) = watch::channel(false);
        let (done, _) = watch::channel(false);
        let job = DownloadJob {
            status: "downloading",
            downloaded_bytes: 0,
            error: None,
            cancel,
            done: done.clone(),
        };
        let first = tokio::spawn(cancel_job(job.clone()));
        let second = tokio::spawn(cancel_job(job));
        tokio::task::yield_now().await;

        done.send(true).unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            first.await.unwrap();
            second.await.unwrap();
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn completion_without_a_subscriber_releases_a_later_cancel_waiter() {
        let (cancel, _) = watch::channel(false);
        let (done, _) = watch::channel(false);
        complete_download(&done);
        let job = DownloadJob {
            status: "downloading",
            downloaded_bytes: 0,
            error: None,
            cancel,
            done,
        };

        tokio::time::timeout(Duration::from_secs(1), cancel_job(job))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn checksum_failure_never_replaces_an_existing_package() {
        let directory = test_directory();
        let stage = directory.path().join(".staging");
        let target = directory.path().join("installed");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("old-model"), b"keep").unwrap();
        let (origin, server) = fixture_server("abd").await;
        let (_cancel_tx, cancel) = watch::channel(false);

        let error = download_assets(
            &reqwest::Client::new(),
            &crate::downloads::DownloadSources::new(crate::config::ModelDownloadSource::Official),
            &stage,
            &target,
            &[(FIXTURE, format!("{origin}/model"))],
            cancel,
            |_, _| {},
        )
        .await
        .unwrap_err();

        assert!(error.contains("SHA-256"));
        assert_eq!(std::fs::read(target.join("old-model")).unwrap(), b"keep");
        assert!(!stage.exists());
        server.abort();
    }

    #[tokio::test]
    async fn cancellation_removes_staging_without_activating_the_model() {
        use std::sync::Arc;
        use tokio::sync::Notify;

        let directory = test_directory();
        let stage = directory.path().join(".staging");
        let target = directory.path().join("installed");
        let started = Arc::new(Notify::new());
        let request_started = Arc::clone(&started);
        let app = Router::new().route(
            "/model",
            get(move || {
                let request_started = Arc::clone(&request_started);
                async move {
                    request_started.notify_one();
                    std::future::pending::<&'static str>().await
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (cancel_tx, cancel) = watch::channel(false);
        let downloading = tokio::spawn({
            let stage = stage.clone();
            let target = target.clone();
            async move {
                download_assets(
                    &reqwest::Client::new(),
                    &crate::downloads::DownloadSources::new(
                        crate::config::ModelDownloadSource::Official,
                    ),
                    &stage,
                    &target,
                    &[(FIXTURE, format!("{origin}/model"))],
                    cancel,
                    |_, _| {},
                )
                .await
            }
        });
        started.notified().await;
        cancel_tx.send(true).unwrap();

        let error = downloading.await.unwrap().unwrap_err();

        assert!(error.contains("cancelled"));
        assert!(!target.exists());
        assert!(!stage.exists());
        server.abort();
    }

    #[tokio::test]
    async fn deleted_packages_can_be_downloaded_again() {
        let directory = test_directory();
        let manager = LocalTranslationModels::new(
            directory.path().into(),
            Arc::new(crate::downloads::DownloadSources::new(
                crate::config::ModelDownloadSource::Official,
            )),
        )
        .unwrap();
        let target = package_dir(directory.path());
        let stage = target.with_file_name(".staging");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join(MODEL_FILENAME), b"old").unwrap();
        std::fs::write(target.join(LICENSE.name), b"old").unwrap();
        let lifecycle = manager.lock_exclusive().await;
        assert!(manager.delete_locked(&lifecycle).unwrap());
        let (origin, server) = fixture_server("abc").await;
        for _ in 0..2 {
            let (_cancel_tx, cancel) = watch::channel(false);
            download_assets(
                &reqwest::Client::new(),
                &crate::downloads::DownloadSources::new(
                    crate::config::ModelDownloadSource::Official,
                ),
                &stage,
                &target,
                &[(FIXTURE, format!("{origin}/model"))],
                cancel,
                |_, _| {},
            )
            .await
            .unwrap();
            assert_eq!(std::fs::read(target.join(MODEL_FILENAME)).unwrap(), b"abc");
            assert!(manager.delete_locked(&lifecycle).unwrap());
        }
        server.abort();
    }
}
