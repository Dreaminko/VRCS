//! Standalone entrypoint for VRCS Core.

fn main() {
    #[cfg(target_os = "linux")]
    if std::env::args().nth(1).as_deref() == Some("--network-helper") {
        if let Err(error) = vrcs_core::run_network_helper(std::env::args().skip(2)) {
            eprintln!("vrcs-network-helper: {error}");
            std::process::exit(1);
        }
        return;
    }
    #[cfg(target_os = "linux")]
    if std::env::var_os("PKEXEC_UID").is_some() {
        eprintln!("Refusing to run VRCS Core under pkexec; only --network-helper is permitted.");
        std::process::exit(1);
    }
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(async_main());
}

async fn async_main() {
    let options = vrcs_core::CoreOptions::from_env();
    let token_was_supplied = options
        .session_token
        .as_deref()
        .is_some_and(|token| !token.trim().is_empty());
    let log_dir = std::env::var("VRCS_LOG_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            #[cfg(target_os = "linux")]
            return vrcs_core::linux_log_directory();
            #[cfg(not(target_os = "linux"))]
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

    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .unwrap_or_else(|error| {
                    eprintln!("{error}");
                    std::process::exit(1);
                });
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
    if let Err(error) = handle.shutdown().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
