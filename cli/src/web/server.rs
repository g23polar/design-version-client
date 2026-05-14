use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, put},
    Router,
};

use super::handlers;

/// Shared application state passed to every handler.
#[derive(Clone)]
pub struct AppState {
    pub store: PathBuf,
}

/// Start the web server and auto-open the browser.
pub async fn run(store: PathBuf, port: u16) -> anyhow::Result<()> {
    let state = Arc::new(AppState { store });

    let app = Router::new()
        // Static assets
        .route("/", get(index_html))
        .route("/style.css", get(style_css))
        .route("/app.js", get(app_js))
        // API endpoints
        .route("/api/snapshots", get(handlers::list_snapshots))
        .route("/api/snapshots/{id}", get(handlers::get_snapshot))
        .route("/api/snapshots/{id}/label", put(handlers::update_label))
        .route("/api/snapshots/{id}", delete(handlers::delete_snapshot))
        .route("/api/batches/{batch_id}", delete(handlers::delete_batch))
        .route("/api/verify", get(handlers::verify_all))
        .route("/api/verify/{id}", get(handlers::verify_one))
        .route("/api/diff/{id1}/{id2}", get(handlers::diff))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let url = format!("http://localhost:{port}");

    println!("Starting dsv web interface at {url}");

    // Auto-open browser (best-effort, don't fail if it doesn't work)
    let url_clone = url.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        if open::that(&url_clone).is_err() {
            eprintln!("Could not auto-open browser. Navigate to {url_clone}");
        }
    });

    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("Press Ctrl+C to stop.");
    axum::serve(listener, app).await?;

    Ok(())
}

// -- Static asset handlers ----------------------------------------------------

async fn index_html() -> Html<&'static str> {
    Html(include_str!("assets/index.html"))
}

async fn style_css() -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/css")],
        include_str!("assets/style.css"),
    )
        .into_response()
}

async fn app_js() -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/javascript")],
        include_str!("assets/app.js"),
    )
        .into_response()
}
