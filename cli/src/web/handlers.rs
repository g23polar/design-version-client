use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use design_version_core as dsv;
use serde::{Deserialize, Serialize};

use super::server::AppState;

// -- Error handling -----------------------------------------------------------

pub(crate) struct AppError(dsv::DvcError);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            dsv::DvcError::NotFound(_) => StatusCode::NOT_FOUND,
            dsv::DvcError::InvalidArgument(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = serde_json::json!({ "error": self.0.to_string() });
        (status, Json(body)).into_response()
    }
}

impl From<dsv::DvcError> for AppError {
    fn from(e: dsv::DvcError) -> Self {
        AppError(e)
    }
}

type ApiResult<T> = Result<Json<T>, AppError>;

// -- List / Get snapshots -----------------------------------------------------

#[derive(Deserialize)]
pub struct ListParams {
    pub label: Option<String>,
    pub file: Option<String>,
    pub batch: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct ListResponse {
    snapshots: Vec<dsv::Snapshot>,
    count: usize,
    total_bytes: u64,
}

pub async fn list_snapshots(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<ListResponse> {
    let snaps = if let Some(ref pat) = params.label {
        dsv::list_by_label(&state.store, pat)?
    } else if let Some(ref pat) = params.file {
        dsv::list_by_file(&state.store, pat)?
    } else if let Some(ref bat) = params.batch {
        dsv::list_by_batch(&state.store, bat)?
    } else {
        dsv::list(&state.store)?
    };

    let total_bytes: u64 = snaps.iter().map(|s| s.file_size).sum();
    let count = snaps.len();

    Ok(Json(ListResponse {
        snapshots: snaps,
        count,
        total_bytes,
    }))
}

pub async fn get_snapshot(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<dsv::Snapshot> {
    let snap = dsv::get_snapshot(&state.store, id)?;
    Ok(Json(snap))
}

// -- Label --------------------------------------------------------------------

#[derive(Deserialize)]
pub struct LabelBody {
    pub label: String,
}

pub async fn update_label(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(body): Json<LabelBody>,
) -> ApiResult<serde_json::Value> {
    dsv::update_label(&state.store, id, &body.label)?;
    Ok(Json(
        serde_json::json!({ "ok": true, "id": id, "label": body.label }),
    ))
}

// -- Delete -------------------------------------------------------------------

#[derive(Deserialize)]
pub struct DeleteParams {
    pub confirm: Option<bool>,
}

pub async fn delete_snapshot(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Query(params): Query<DeleteParams>,
) -> ApiResult<serde_json::Value> {
    if params.confirm.unwrap_or(false) {
        let report = dsv::delete_snapshot(&state.store, id)?;
        Ok(Json(serde_json::json!({
            "deleted": true,
            "snapshots_deleted": report.snapshots_deleted,
            "blobs_deleted": report.blobs_deleted,
            "bytes_freed": report.bytes_freed,
        })))
    } else {
        let snap = dsv::get_snapshot(&state.store, id)?;
        Ok(Json(serde_json::json!({
            "deleted": false,
            "dry_run": true,
            "snapshot": snap,
        })))
    }
}

pub async fn delete_batch(
    State(state): State<Arc<AppState>>,
    Path(batch_id): Path<String>,
    Query(params): Query<DeleteParams>,
) -> ApiResult<serde_json::Value> {
    if params.confirm.unwrap_or(false) {
        let report = dsv::delete_batch(&state.store, &batch_id)?;
        Ok(Json(serde_json::json!({
            "deleted": true,
            "snapshots_deleted": report.snapshots_deleted,
            "blobs_deleted": report.blobs_deleted,
            "bytes_freed": report.bytes_freed,
        })))
    } else {
        let snaps = dsv::list_by_batch(&state.store, &batch_id)?;
        Ok(Json(serde_json::json!({
            "deleted": false,
            "dry_run": true,
            "batch_id": batch_id,
            "snapshot_count": snaps.len(),
            "snapshots": snaps,
        })))
    }
}

// -- Verify -------------------------------------------------------------------

pub async fn verify_all(State(state): State<Arc<AppState>>) -> ApiResult<dsv::VerifyReport> {
    let report = dsv::verify_all(&state.store)?;
    Ok(Json(report))
}

pub async fn verify_one(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<serde_json::Value> {
    match dsv::verify(&state.store, id) {
        Ok(()) => Ok(Json(serde_json::json!({ "id": id, "status": "ok" }))),
        Err(e) => Ok(Json(
            serde_json::json!({ "id": id, "status": "failed", "error": e.to_string() }),
        )),
    }
}

// -- Diff ---------------------------------------------------------------------

pub async fn diff(
    State(state): State<Arc<AppState>>,
    Path((id1, id2)): Path<(i64, i64)>,
) -> ApiResult<dsv::DiffReport> {
    let report = dsv::diff(&state.store, id1, id2)?;
    Ok(Json(report))
}
