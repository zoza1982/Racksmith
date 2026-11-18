//! The HTTP routes and their handlers.
//!
//! Handlers are thin: they extract and validate the request, call the machine registry, and
//! convert the result to a wire type from [`crate::dto`]. Extractor rejections are taken as
//! `Result`s and converted to [`ApiError`], so every error response has the same JSON shape.

use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use racksmith_domain::MachineId;

use crate::dto::{CreateMachineRequest, EventRequest, HealthResponse, MachineResponse};
use crate::{ApiError, AppState};

/// Path prefix of every versioned endpoint.
const API_V1: &str = "/api/v1";

/// Builds the API router with all routes bound to `state`.
pub fn router(state: AppState) -> Router {
    let machines = Router::new()
        .route("/machines", get(list_machines).post(create_machine))
        .route("/machines/{id}", get(get_machine))
        .route("/machines/{id}/events", post(apply_event));

    Router::new()
        .route("/healthz", get(health))
        .nest(API_V1, machines)
        .fallback(route_not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .with_state(state)
}

/// `GET /healthz`: liveness probe.
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

/// `GET /api/v1/machines`: every machine, ordered by id.
async fn list_machines(State(state): State<AppState>) -> Json<Vec<MachineResponse>> {
    let machines = state.registry().list().await;
    Json(machines.into_iter().map(MachineResponse::from).collect())
}

/// `POST /api/v1/machines`: registers a machine and answers `201 Created` with its location.
async fn create_machine(
    State(state): State<AppState>,
    body: Result<Json<CreateMachineRequest>, JsonRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Json(request) = body?;
    let (mac, hostname) = request.validate()?;
    let machine = state.registry().register(mac, hostname).await?;
    tracing::info!(machine = %machine.id(), %mac, "registered machine");

    let location = format!("{API_V1}/machines/{}", machine.id());
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, location)],
        Json(MachineResponse::from(machine)),
    ))
}

/// `GET /api/v1/machines/{id}`: a single machine.
async fn get_machine(
    State(state): State<AppState>,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<MachineResponse>, ApiError> {
    let id = parse_machine_id(path)?;
    let machine = state.registry().get(id).await?;
    Ok(Json(MachineResponse::from(machine)))
}

/// `POST /api/v1/machines/{id}/events`: applies a lifecycle event.
async fn apply_event(
    State(state): State<AppState>,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<EventRequest>, JsonRejection>,
) -> Result<Json<MachineResponse>, ApiError> {
    let id = parse_machine_id(path)?;
    let Json(EventRequest { event }) = body?;
    let machine = state.registry().apply(id, event).await?;
    tracing::info!(machine = %id, %event, state = %machine.state(), "applied event");
    Ok(Json(MachineResponse::from(machine)))
}

/// Fallback for paths that match no route.
async fn route_not_found() -> ApiError {
    ApiError::RouteNotFound
}

/// Fallback for known paths requested with an unsupported method.
async fn method_not_allowed() -> ApiError {
    ApiError::MethodNotAllowed
}

/// Parses the `{id}` path segment into a [`MachineId`].
fn parse_machine_id(path: Result<Path<String>, PathRejection>) -> Result<MachineId, ApiError> {
    let Path(raw) = path?;
    Ok(raw.parse()?)
}
