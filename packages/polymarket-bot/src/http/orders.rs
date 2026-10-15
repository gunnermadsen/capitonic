//! Authenticated generic order controller. Execution dispatch belongs to the application.
use super::*;
use crate::execution::orders::{EvaluationRequest, ExecuteRequest, Receipt};
use axum::extract::rejection::JsonRejection;

pub(super) fn routes() -> Router<HttpState> {
    Router::new()
        .route("/orders/:process_id/preview", post(preview))
        .route("/orders/:process_id/dry-run", post(dry_run))
        .route("/orders/:process_id/execute", post(execute))
        .route("/orders/:process_id/order/:order_id", get(order))
}
fn body<T>(body: Result<Json<T>, JsonRejection>) -> Result<T, HttpError> {
    body.map(|Json(value)| value)
        .map_err(|_| HttpError::bad_request("invalid order request body"))
}
async fn preview(
    State(state): State<HttpState>,
    Path(id): Path<Uuid>,
    request: Result<Json<EvaluationRequest>, JsonRejection>,
) -> Result<Json<Receipt>, HttpError> {
    let request = body(request)?;
    request.validate()?;
    state
        .control
        .orders()?
        .preview(id, request, false)
        .await
        .map(Json)
}
async fn dry_run(
    State(state): State<HttpState>,
    Path(id): Path<Uuid>,
    request: Result<Json<EvaluationRequest>, JsonRejection>,
) -> Result<Json<Receipt>, HttpError> {
    let request = body(request)?;
    request.validate()?;
    state
        .control
        .orders()?
        .preview(id, request, true)
        .await
        .map(Json)
}
async fn execute(
    State(state): State<HttpState>,
    Path(id): Path<Uuid>,
    request: Result<Json<ExecuteRequest>, JsonRejection>,
) -> Result<Json<Receipt>, HttpError> {
    let request = body(request)?;
    EvaluationRequest {
        version: request.version,
        candidate: request.candidate.clone(),
    }
    .validate()?;
    if request.idempotency_key.is_nil() {
        return Err(HttpError::bad_request("idempotency_key must be non-nil"));
    }
    state.control.orders()?.execute(id, request).await.map(Json)
}
async fn order(
    State(state): State<HttpState>,
    Path((id, order_id)): Path<(Uuid, String)>,
) -> Result<Json<Receipt>, HttpError> {
    state.control.orders()?.order(id, order_id).await.map(Json)
}
