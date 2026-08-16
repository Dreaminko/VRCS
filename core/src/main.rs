//! VRCS Core 独立运行入口。

#[tokio::main]
async fn main() {
    let options = vrcs_core::CoreOptions::from_env();
    let token_was_supplied = options
        .session_token
        .as_deref()
        .is_some_and(|token| !token.trim().is_empty());
    let log_dir = std::env::var("VRCS_LOG_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            options
                .config_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join("logs")
        });
    let _logging_guard = vrcs_core::init_tracing(Some(&log_dir)).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(1);
    });
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "VRCS Core starting");
    let handle = vrcs_core::start(options).await.unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(1);
    });
    if !token_was_supplied {
        eprintln!("VRCS Core session token: {}", handle.session_token());
    }

    let _ = tokio::signal::ctrl_c().await;
    if let Err(error) = handle.shutdown().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
