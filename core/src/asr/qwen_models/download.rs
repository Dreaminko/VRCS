use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::watch;

use super::package::{cache_verified_asset, managed_parent, verify_asset};
use super::{activate_staging, staging_dir, AssetSpec, PackageSpec};

const DOWNLOAD_ATTEMPTS: usize = 4;

pub(crate) async fn download_into_staging(
    root: &Path,
    spec: PackageSpec,
    base_url: &str,
    client: &reqwest::Client,
    sources: &crate::downloads::DownloadSources,
    mut cancel: watch::Receiver<bool>,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    let stage = staging_dir(root, spec);
    managed_parent(root, spec, true)?;
    match tokio::fs::create_dir(&stage).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = tokio::fs::symlink_metadata(&stage)
                .await
                .map_err(|error| error.to_string())?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err("Qwen ASR staging path is not a managed directory".into());
            }
        }
        Err(error) => {
            return Err(format!(
                "Could not create Qwen ASR staging directory: {error}"
            ))
        }
    }
    let total = spec.files.iter().map(|file| file.bytes).sum();
    let mut completed = 0u64;
    on_progress(completed, total);
    for asset in spec.files {
        if *cancel.borrow() {
            return Err("Model download was cancelled".into());
        }
        let path = stage.join(asset.name);
        let check = path.clone();
        if tokio::task::spawn_blocking(move || verify_asset(&check, *asset, false))
            .await
            .map_err(|error| format!("Qwen ASR verification task failed: {error}"))??
        {
            completed += asset.bytes;
            on_progress(completed, total);
            continue;
        }
        let temporary = stage.join(format!(".{}.part", asset.name));
        let mut url = reqwest::Url::parse(base_url)
            .map_err(|error| format!("Invalid Qwen ASR download URL: {error}"))?;
        url.path_segments_mut()
            .map_err(|_| "Invalid Qwen ASR download URL".to_string())?
            .push(asset.name);
        let urls = tokio::select! {
            _ = cancel.changed() => return Err("Model download was cancelled".into()),
            urls = sources.urls(url.as_str()) => urls,
        };
        download_file_from_sources(
            client,
            &urls,
            *asset,
            &path,
            &temporary,
            &mut cancel,
            |bytes| on_progress(completed + bytes, total),
        )
        .await?;
        completed += asset.bytes;
    }
    if *cancel.borrow() {
        return Err("Model download was cancelled".into());
    }
    activate_staging(root, spec)
}

async fn download_file_from_sources(
    client: &reqwest::Client,
    urls: &[String],
    asset: AssetSpec,
    path: &Path,
    temporary: &PathBuf,
    cancel: &mut watch::Receiver<bool>,
    mut on_progress: impl FnMut(u64),
) -> Result<(), String> {
    let mut last_error = "No model download source is available".to_string();
    for url in urls {
        if *cancel.borrow() {
            return Err("Model download was cancelled".into());
        }
        on_progress(0);
        let url = reqwest::Url::parse(url).map_err(|error| error.to_string())?;
        match download_file(
            client,
            url,
            asset,
            path,
            temporary,
            cancel,
            &mut on_progress,
        )
        .await
        {
            Ok(()) => return Ok(()),
            Err(error) => {
                let _ = tokio::fs::remove_file(temporary).await;
                last_error = error;
                if *cancel.borrow() {
                    break;
                }
                tracing::warn!(asset = asset.name, %last_error, "model download source failed");
            }
        }
    }
    Err(last_error)
}

fn content_range_length(value: &str, offset: u64, total: u64) -> Option<u64> {
    let (range, size) = value.strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let start = start.parse::<u64>().ok()?;
    let end = end.parse::<u64>().ok()?;
    if start != offset || end < start || end >= total || size.parse::<u64>().ok()? != total {
        return None;
    }
    Some(end - start + 1)
}

pub(crate) async fn download_file(
    client: &reqwest::Client,
    url: reqwest::Url,
    asset: AssetSpec,
    path: &Path,
    temporary: &PathBuf,
    cancel: &mut watch::Receiver<bool>,
    mut on_progress: impl FnMut(u64),
) -> Result<(), String> {
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(temporary)
        .await
        .map_err(|error| format!("Could not create model download file: {error}"))?;
    let mut downloaded = 0u64;
    let mut digest = Sha256::new();
    let mut last_error = format!("Model asset {} is incomplete", asset.name);
    'attempts: for attempt in 0..DOWNLOAD_ATTEMPTS {
        if *cancel.borrow() {
            return Err("Model download was cancelled".into());
        }
        if attempt > 0 {
            tokio::select! {
                _ = cancel.changed() => return Err("Model download was cancelled".into()),
                _ = tokio::time::sleep(Duration::from_millis(500 * attempt as u64)) => {}
            }
        }
        file.seek(SeekFrom::Start(downloaded))
            .await
            .map_err(|error| format!("Could not resume model download: {error}"))?;
        let mut request = client.get(url.clone());
        if downloaded > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={downloaded}-"));
        }
        let response = tokio::select! {
            _ = cancel.changed() => return Err("Model download was cancelled".into()),
            response = request.send() => response,
        }
        .and_then(reqwest::Response::error_for_status);
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                let retryable = match error.status() {
                    Some(status) => {
                        status.is_server_error() || matches!(status.as_u16(), 408 | 429)
                    }
                    None => error.is_connect() || error.is_timeout() || error.is_request(),
                };
                last_error = format!(
                    "Could not download model asset {}: {}",
                    asset.name,
                    error.without_url()
                );
                if retryable {
                    continue;
                }
                return Err(last_error);
            }
        };
        if downloaded > 0 && response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
            // A source that ignores the range must restart, not mix responses.
            file.set_len(0)
                .await
                .map_err(|error| format!("Could not restart model download: {error}"))?;
            file.seek(SeekFrom::Start(0))
                .await
                .map_err(|error| format!("Could not restart model download: {error}"))?;
            downloaded = 0;
            digest = Sha256::new();
        }
        let expected_bytes = if response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
            let length = response
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| content_range_length(value, downloaded, asset.bytes));
            let Some(length) = length else {
                last_error = format!("Model asset {} has an unexpected Content-Range", asset.name);
                continue;
            };
            length
        } else {
            asset.bytes - downloaded
        };
        if response
            .content_length()
            .is_some_and(|size| size != expected_bytes)
        {
            return Err(format!("Model asset {} has an unexpected size", asset.name));
        }
        let mut stream = response.bytes_stream();
        loop {
            let chunk = tokio::select! {
                _ = cancel.changed() => return Err("Model download was cancelled".into()),
                chunk = stream.next() => chunk,
            };
            let Some(chunk) = chunk else { break };
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    last_error = format!("Model download was interrupted: {}", error.without_url());
                    continue 'attempts;
                }
            };
            downloaded = downloaded
                .checked_add(chunk.len() as u64)
                .ok_or_else(|| "Model download size overflow".to_string())?;
            if downloaded > asset.bytes {
                return Err(format!(
                    "Model asset {} exceeds its expected size",
                    asset.name
                ));
            }
            file.write_all(&chunk)
                .await
                .map_err(|error| format!("Could not write model download: {error}"))?;
            digest.update(&chunk);
            on_progress(downloaded);
        }
        if downloaded == asset.bytes {
            break;
        }
        last_error = format!("Model asset {} is incomplete", asset.name);
    }
    if downloaded != asset.bytes {
        return Err(last_error);
    }
    let digest = format!("{:x}", digest.finalize());
    if digest != asset.sha256 {
        return Err(format!(
            "Model asset {} failed SHA-256 verification",
            asset.name
        ));
    }
    file.sync_all()
        .await
        .map_err(|error| format!("Could not save model download: {error}"))?;
    drop(file);
    if path.exists() {
        tokio::fs::remove_file(path)
            .await
            .map_err(|error| format!("Could not replace invalid model asset: {error}"))?;
    }
    tokio::fs::rename(temporary, path)
        .await
        .map_err(|error| format!("Could not stage model asset: {error}"))?;
    cache_verified_asset(path, asset, digest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        http::{HeaderMap, StatusCode},
        routing::get,
        Router,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    const ASSET: AssetSpec = AssetSpec {
        name: "fixture.gguf",
        bytes: 3,
        sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    };

    #[tokio::test]
    async fn download_sends_app_user_agent_after_a_redirect() {
        let app = Router::new()
            .route(
                "/repo",
                get(|| async { axum::response::Redirect::temporary("/cdn") }),
            )
            .route(
                "/cdn",
                get(|headers: HeaderMap| async move {
                    if headers
                        .get("user-agent")
                        .and_then(|value| value.to_str().ok())
                        == Some(concat!("VRCS/", env!("CARGO_PKG_VERSION")))
                    {
                        (StatusCode::OK, "abc")
                    } else {
                        (StatusCode::FORBIDDEN, "denied by UA ACL = blacklist")
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let root = tempfile::tempdir().unwrap();
        let manager = crate::asr::ModelManager::new(root.path().into()).unwrap();
        let path = root.path().join(ASSET.name);
        let temporary = root.path().join("fixture.part");
        let (_cancel_tx, mut cancel) = watch::channel(false);
        let result = download_file(
            &manager.client,
            format!("http://{address}/repo").parse().unwrap(),
            ASSET,
            &path,
            &temporary,
            &mut cancel,
            |_| {},
        )
        .await;
        server.abort();
        result.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"abc");
        assert!(!temporary.exists());
    }

    #[tokio::test]
    async fn interrupted_transfers_preserve_prefix_across_request_and_range_failures() {
        use tokio::io::AsyncReadExt;

        let cases = [
            ("body interruption", vec![]),
            ("connection failure and 503", vec![
                "",
                "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n",
            ]),
            ("request timeout", vec!["HTTP/1.1 408 Request Timeout\r\nContent-Length: 0\r\n\r\n"]),
            ("rate limit", vec!["HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\n\r\n"]),
            ("wrong range start", vec!["HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-0/3\r\nContent-Length: 1\r\n\r\na"]),
            ("wrong range total", vec!["HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 2-2/4\r\nContent-Length: 1\r\n\r\na"]),
            ("invalid range end", vec!["HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 2-3/3\r\nContent-Length: 2\r\n\r\naa"]),
            ("missing range", vec!["HTTP/1.1 206 Partial Content\r\nContent-Length: 1\r\n\r\na"]),
            ("malformed range", vec!["HTTP/1.1 206 Partial Content\r\nContent-Range: invalid\r\nContent-Length: 1\r\n\r\na"]),
        ];
        for (case, middle) in cases {
            let mut responses = vec!["HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nab"];
            responses.extend(middle);
            responses.push("HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 2-2/3\r\nContent-Length: 1\r\n\r\nc");
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let mut ranges = Vec::new();
                for response in responses {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut request = Vec::new();
                    let mut buffer = [0u8; 256];
                    loop {
                        let read = socket.read(&mut buffer).await.unwrap();
                        request.extend_from_slice(&buffer[..read]);
                        if read == 0 || request.windows(4).any(|window| window == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let text = String::from_utf8_lossy(&request).to_ascii_lowercase();
                    ranges.push(
                        text.lines()
                            .find(|line| line.starts_with("range:"))
                            .map(str::to_owned),
                    );
                    socket.write_all(response.as_bytes()).await.unwrap();
                    socket.shutdown().await.unwrap();
                }
                ranges
            });
            let root = tempfile::tempdir().unwrap();
            let manager = crate::asr::ModelManager::new(root.path().into()).unwrap();
            let path = root.path().join(ASSET.name);
            let temporary = root.path().join("fixture.part");
            let (_cancel_tx, mut cancel) = watch::channel(false);
            let result = tokio::time::timeout(
                Duration::from_secs(10),
                download_file(
                    &manager.client,
                    format!("http://{address}/asset").parse().unwrap(),
                    ASSET,
                    &path,
                    &temporary,
                    &mut cancel,
                    |_| {},
                ),
            )
            .await;
            if !matches!(&result, Ok(Ok(()))) {
                server.abort();
                panic!("{case}: {result:?}");
            }
            let ranges = server.await.unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), b"abc", "{case}");
            assert_eq!(ranges[0], None, "{case}");
            assert!(
                ranges[1..]
                    .iter()
                    .all(|range| range.as_deref() == Some("range: bytes=2-")),
                "{case}"
            );
        }
    }

    #[tokio::test]
    async fn fallback_retries_http_errors_and_bad_checksums_before_installing() {
        let hits = Arc::new(AtomicUsize::new(0));
        let served = Arc::clone(&hits);
        let app = Router::new()
            .route(
                "/blocked",
                get(|| async {
                    axum::response::Redirect::temporary("/denied?auth_key=test-signature")
                }),
            )
            .route("/denied", get(|| async { StatusCode::SERVICE_UNAVAILABLE }))
            .route("/corrupt", get(|| async { "abd" }))
            .route(
                "/official",
                get(move || {
                    served.fetch_add(1, Ordering::SeqCst);
                    async { "abc" }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let root = tempfile::tempdir().unwrap();
        let temporary = root.path().join("fixture.part");
        let path = root.path().join(ASSET.name);
        let client = reqwest::Client::new();
        let (_cancel_tx, mut cancel) = watch::channel(false);
        let error = download_file_from_sources(
            &client,
            &[format!("http://{address}/blocked")],
            ASSET,
            &path,
            &temporary,
            &mut cancel,
            |_| {},
        )
        .await
        .unwrap_err();
        assert!(error.contains("503"));
        assert!(error.contains(ASSET.name));
        assert!(!error.contains("auth_key"));
        assert!(!error.contains("test-signature"));
        for failed in ["blocked", "corrupt"] {
            let urls = [
                format!("http://{address}/{failed}"),
                format!("http://{address}/official"),
            ];
            download_file_from_sources(
                &client,
                &urls,
                ASSET,
                &path,
                &temporary,
                &mut cancel,
                |_| {},
            )
            .await
            .unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), b"abc");
            assert!(!temporary.exists());
        }
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        server.abort();
    }

    #[tokio::test]
    async fn cancellation_during_a_request_never_starts_the_fallback() {
        let hits = Arc::new(AtomicUsize::new(0));
        let served = Arc::clone(&hits);
        let (cancel_tx, mut cancel) = watch::channel(false);
        let app = Router::new()
            .route(
                "/slow",
                get(move || {
                    let cancel_tx = cancel_tx.clone();
                    async move {
                        cancel_tx.send(true).unwrap();
                        std::future::pending::<String>().await
                    }
                }),
            )
            .route(
                "/official",
                get(move || {
                    served.fetch_add(1, Ordering::SeqCst);
                    async { "abc" }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let root = tempfile::tempdir().unwrap();
        let urls = [
            format!("http://{address}/slow"),
            format!("http://{address}/official"),
        ];
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            download_file_from_sources(
                &reqwest::Client::new(),
                &urls,
                ASSET,
                &root.path().join(ASSET.name),
                &root.path().join("fixture.part"),
                &mut cancel,
                |_| {},
            ),
        )
        .await
        .unwrap();
        assert!(result.unwrap_err().contains("cancelled"));
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        assert!(!root.path().join(ASSET.name).exists());
        server.abort();
    }
}
