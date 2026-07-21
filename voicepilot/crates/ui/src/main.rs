use voicepilot_ui::app;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("info".parse().unwrap()),
        )
        .init();

    let db_path = std::env::var("VOICEPILOT_DB").unwrap_or_else(|_| "voicepilot.db".to_string());
    let kernel = trust_kernel::kernel::TrustKernel::open_file(&db_path)
        .expect("failed to open kernel");

    app::run(kernel).expect("failed to run Tauri app");
}
