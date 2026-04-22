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

use crate::cara::CaraPipeline;
use crate::storage::Storage;
use crate::api::routes::{ApiState, create_api_router};

/// Configuration for the web server.
#[derive(Debug, Clone)]
pub struct WebConfig {
    pub host: String,
    pub port: u16,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
        }
    }
}

/// Web server instance.
pub struct WebServer {
    config: WebConfig,
    storage: Arc<Storage>,
    cara: Arc<CaraPipeline>,
}

impl WebServer {
    pub fn new(config: WebConfig, storage: Arc<Storage>, cara: Arc<CaraPipeline>) -> Self {
        Self { config, storage, cara }
    }

    pub async fn run(&self) -> Result<()> {
        let state = ApiState {
            storage: self.storage.clone(),
            cara: self.cara.clone(),
        };

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

        let serve_dir = tokio::fs::read_dir("static").await;
        if serve_dir.is_ok() {
            info!("Serving static files from ./static directory");
        }

        let addr = format!("{}:{}", self.config.host, self.config.port);
        let listener = TcpListener::bind(&addr).await?;

        info!("🌐 Web server starting at http://{}", addr);
        info!("📊 Dashboard available at http://{}/", addr);
        info!("🔌 API endpoints available at http://{}/api/", addr);

        axum::serve(listener, app).await?;

        Ok(())
    }
}
