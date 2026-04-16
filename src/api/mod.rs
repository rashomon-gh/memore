//! Web API module for the Hindsight memory visualization dashboard.
//!
//! Provides a REST API for accessing memories, graph data, and analytics
//! through a web interface. Built with Axum for high-performance async HTTP.

pub mod models;
pub mod routes;

use anyhow::Result;
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;
use tower_http::services::ServeDir;
use tracing::info;

use crate::storage::Storage;
use crate::llm::LLMClient;
use crate::api::routes::{ApiState, create_api_router};

/// Configuration for the web server.
#[derive(Debug, Clone)]
pub struct WebConfig {
    pub host: String,
    pub port: u16,
    pub enabled: bool,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            enabled: false,
        }
    }
}

/// Web server instance.
pub struct WebServer {
    config: WebConfig,
    storage: Arc<Storage>,
    llm: Arc<LLMClient>,
    embedding_dim: usize,
}

impl WebServer {
    /// Create a new web server instance.
    pub fn new(config: WebConfig, storage: Arc<Storage>, llm: Arc<LLMClient>, embedding_dim: usize) -> Self {
        Self { config, storage, llm, embedding_dim }
    }

    /// Start the web server (blocks until shutdown).
    pub async fn run(&self) -> Result<()> {
        if !self.config.enabled {
            info!("Web server is disabled in configuration");
            return Ok(());
        }

        let state = ApiState {
            storage: self.storage.clone(),
            llm: self.llm.clone(),
            embedding_dim: self.embedding_dim,
        };

        // Build the application router with static file serving
        let app = create_api_router()
            .nest_service("/", ServeDir::new("static"))
            .layer(
                CorsLayer::new()
                    .allow_origin(Any)
                    .allow_methods(Any)
                    .allow_headers(Any),
            )
            .layer(CompressionLayer::new())
            .layer(TraceLayer::new_for_http())
            .with_state(state);

        // Serve static files from the static directory
        let serve_dir = tokio::fs::read_dir("static").await;
        if serve_dir.is_ok() {
            info!("Serving static files from ./static directory");
        }

        let addr = format!("{}:{}", self.config.host, self.config.port);
        let listener = TcpListener::bind(&addr).await?;

        info!("🌐 Web server starting at http://{}", addr);
        info!("📊 Dashboard available at http://{}/", addr);
        info!("🔌 API endpoints available at http://{}/api/", addr);

        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await?;

        Ok(())
    }
}

/// Signal handler for graceful shutdown.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install CTRL+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received CTRL+C, shutting down web server gracefully...");
        },
        _ = terminate => {
            info!("Received termination signal, shutting down web server gracefully...");
        },
    }
}
