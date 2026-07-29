//! VRCS Core 独立运行入口。

#[tokio::main]
async fn main() {
    vrcs_core::init_tracing();
    let options = vrcs_core::CoreOptions::from_env();
    let handle = vrcs_core::start(options).await.unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(1);
    });

    let _ = tokio::signal::ctrl_c().await;
    if let Err(error) = handle.shutdown().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
