#![forbid(unsafe_code)]

use atihsrah::config;
use atihsrah::http;
use atihsrah::processor;
use atihsrah::state;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // Healthcheck CLI mode for container HEALTHCHECK without requiring curl or shell
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && (args[1] == "--healthcheck" || args[1] == "healthcheck") {
        let target_url = if args.len() > 2 {
            args[2].clone()
        } else {
            let port = std::env::var("ATIHSRAH_BIND")
                .ok()
                .and_then(|b| b.rsplit_once(':').map(|(_, p)| p.to_string()))
                .unwrap_or_else(|| "8080".to_string());
            format!("http://127.0.0.1:{}/health", port)
        };

        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()
        {
            Ok(c) => c,
            Err(_) => std::process::exit(1),
        };

        match client.get(&target_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                println!("OK");
                std::process::exit(0);
            }
            Ok(resp) => {
                eprintln!("Healthcheck failed with HTTP status: {}", resp.status());
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("Healthcheck connection failed: {}", e);
                std::process::exit(1);
            }
        }
    }

    // URL Signing CLI helper (enables zero-dependency signing from containers or CLI)
    if args.len() > 1 && (args[1] == "--sign" || args[1] == "sign") {
        let key_hex = std::env::var("ATIHSRAH_SIGNING_KEY").unwrap_or_default();
        let key_bytes = match hex::decode(key_hex.trim()) {
            Ok(k) if k.len() >= 32 => k,
            _ => {
                eprintln!("Error: Valid 32-byte hex ATIHSRAH_SIGNING_KEY required in environment");
                std::process::exit(1);
            }
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let exp = now + 3600;

        let mut params: Vec<(String, String)> = Vec::new();
        if args.len() > 2 {
            for pair in args[2].split('&') {
                if let Some((k, v)) = pair.split_once('=') {
                    params.push((k.to_string(), v.to_string()));
                }
            }
        }
        if !params.iter().any(|(k, _)| k == "exp") {
            params.push(("exp".to_string(), exp.to_string()));
        }

        let canonical = atihsrah::auth::signing::build_canonical_string(exp, &params);
        let sig = atihsrah::auth::signing::compute_signature(&key_bytes, &canonical);

        let mut query_parts: Vec<String> = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, atihsrah::auth::signing::rfc3986_encode(v)))
            .collect();
        query_parts.push(format!("sig={sig}"));
        let query_str = query_parts.join("&");
        println!("/transform?{query_str}");
        std::process::exit(0);
    }

    // Determine log level before loading full config
    let log_level = std::env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string());
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&log_level));

    // Initialize structured JSON logging with target field
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .with_target(true)
        .with_current_span(true)
        .init();

    tracing::info!("Initializing Atihsrah image proxy...");

    // 1. Load and validate configuration
    let config = match config::Config::from_env() {
        Ok(cfg) => cfg,
        Err(err) => {
            tracing::error!(error = %err, "Startup configuration error");
            eprintln!("Error: {err}");
            std::process::exit(1);
        }
    };

    tracing::info!(
        bind_addr = %config.bind_addr,
        auth_mode = ?config.auth_mode,
        allowed_origins_count = config.allowed_origins.len(),
        "Configuration validated successfully"
    );

    // Warn loudly if running in unauthenticated public mode
    if config.auth_mode == config::AuthMode::Public {
        tracing::warn!(
            "SECURITY WARNING: Running in PUBLIC auth mode. Image transformation endpoints are unauthenticated and open to abuse. Set ATIHSRAH_AUTH_MODE=signed in production."
        );
    }

    // 2. Instantiate ImageProcessor (ImageRsProcessor)
    let processor = Arc::new(processor::image_rs::ImageRsProcessor::new_with_config(
        &config,
    ));

    // 3. Initialize application state
    let state = match state::AppState::new(Arc::new(config.clone()), processor) {
        Ok(s) => Arc::new(s),
        Err(err) => {
            tracing::error!(error = %err, "Failed to initialize application state");
            eprintln!("Initialization error: {err}");
            std::process::exit(1);
        }
    };

    // 4. Initialize Prometheus metrics recorder
    let prometheus_handle = atihsrah::observability::init_prometheus();

    // Spawn Prometheus metrics server on isolated ATIHSRAH_METRICS_BIND
    let metrics_router =
        atihsrah::observability::create_metrics_router(prometheus_handle, Some(state.clone()));
    match tokio::net::TcpListener::bind(config.metrics_bind).await {
        Ok(metrics_listener) => {
            tracing::info!(
                metrics_bind = %config.metrics_bind,
                "Prometheus metrics server listening"
            );
            tokio::spawn(async move {
                if let Err(err) = axum::serve(metrics_listener, metrics_router).await {
                    tracing::error!(error = %err, "Metrics server encountered an error");
                }
            });
        }
        Err(err) => {
            tracing::error!(
                error = %err,
                metrics_bind = %config.metrics_bind,
                "Failed to bind Prometheus metrics listener"
            );
            eprintln!(
                "Failed to bind metrics listener to {}: {err}",
                config.metrics_bind
            );
            std::process::exit(1);
        }
    };

    // 5. Construct Axum HTTP router
    let app = http::create_router(state.clone());

    // 6. Bind listener
    let listener = match tokio::net::TcpListener::bind(config.bind_addr).await {
        Ok(l) => l,
        Err(err) => {
            tracing::error!(error = %err, bind_addr = %config.bind_addr, "Failed to bind TCP listener");
            eprintln!("Failed to bind TCP listener to {}: {err}", config.bind_addr);
            std::process::exit(1);
        }
    };

    // 7. Mark server as ready
    state.set_ready(true);

    tracing::info!(
        address = %config.bind_addr,
        "Atihsrah image proxy is ready and listening"
    );

    // 8. Prepare graceful shutdown signal handler (SIGTERM / SIGINT)
    let shutdown_state = state.clone();
    let shutdown_signal = async move {
        let ctrl_c = async {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to install Ctrl+C handler");
        };

        #[cfg(unix)]
        let terminate = async {
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(mut sig) => {
                    sig.recv().await;
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to install SIGTERM handler");
                    std::future::pending::<()>().await;
                }
            }
        };

        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();

        tokio::select! {
            _ = ctrl_c => {},
            _ = terminate => {},
        }

        tracing::info!(
            "Shutdown signal received. Flipping /ready to 503 and draining in-flight requests..."
        );
        shutdown_state.start_draining();
    };

    // 9. Serve incoming requests with graceful drain
    let make_service = app.into_make_service_with_connect_info::<std::net::SocketAddr>();
    let server = axum::serve(listener, make_service).with_graceful_shutdown(shutdown_signal);

    if let Err(err) = server.await {
        tracing::error!(error = %err, "Server terminated unexpectedly");
        std::process::exit(1);
    }

    tracing::info!("Graceful shutdown complete.");
}
