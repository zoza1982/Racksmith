//! The typed error of every HTTP handler and its mapping to JSON error responses.

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use racksmith_domain::{ParseHostnameError, ParseMacAddressError, ParseMachineIdError};

use crate::RegistryError;
use crate::dto::ErrorResponse;

/// Every way an API request can fail.
///
/// Each variant has a fixed HTTP status ([`ApiError::status`]) and a stable machine-readable
/// code ([`ApiError::code`]); together with the message they form the response body
/// `{"error": {"code": "...", "message": "..."}}`.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// The registry rejected the operation.
    #[error(transparent)]
    Registry(#[from] RegistryError),
    /// The machine id in the path is malformed, so it cannot name any machine.
    #[error(transparent)]
    InvalidMachineId(#[from] ParseMachineIdError),
    /// The `mac` field of a request is malformed.
    #[error(transparent)]
    InvalidMac(#[from] ParseMacAddressError),
    /// The `hostname` field of a request is malformed.
    #[error(transparent)]
    InvalidHostname(#[from] ParseHostnameError),
    /// The request body is not the expected JSON.
    #[error(transparent)]
    InvalidBody(#[from] JsonRejection),
    /// The request path could not be decoded.
    #[error(transparent)]
    InvalidPath(#[from] PathRejection),
    /// No route matches the request path.
    #[error("no such endpoint")]
    RouteNotFound,
    /// The route exists but does not support the request method.
    #[error("method not allowed for this endpoint")]
    MethodNotAllowed,
}

impl ApiError {
    /// Returns the HTTP status code of this error.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Registry(RegistryError::NotFound(_))
            | Self::InvalidMachineId(_)
            | Self::RouteNotFound => StatusCode::NOT_FOUND,
            Self::Registry(RegistryError::DuplicateMac(_) | RegistryError::Transition(_)) => {
                StatusCode::CONFLICT
            }
            Self::Registry(RegistryError::IdSpaceExhausted) => StatusCode::INSUFFICIENT_STORAGE,
            Self::InvalidMac(_) | Self::InvalidHostname(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::InvalidBody(rejection) => rejection.status(),
            Self::InvalidPath(rejection) => rejection.status(),
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
        }
    }

    /// Returns the stable, machine-readable error code sent in the response body.
    ///
    /// Clients should match on this code rather than on the message. Codes are part of the
    /// API contract: never change or reuse one.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Registry(RegistryError::NotFound(_)) => "machine_not_found",
            Self::Registry(RegistryError::DuplicateMac(_)) => "duplicate_mac_address",
            Self::Registry(RegistryError::Transition(_)) => "invalid_transition",
            Self::Registry(RegistryError::IdSpaceExhausted) => "id_space_exhausted",
            Self::InvalidMachineId(_) => "invalid_machine_id",
            Self::InvalidMac(_) => "invalid_mac_address",
            Self::InvalidHostname(_) => "invalid_hostname",
            Self::InvalidBody(_) => "invalid_request_body",
            Self::InvalidPath(_) => "invalid_path",
            Self::RouteNotFound => "route_not_found",
            Self::MethodNotAllowed => "method_not_allowed",
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        if status.is_server_error() {
            tracing::error!(error = %self, code = self.code(), "request failed");
        }
        (status, Json(ErrorResponse::from(&self))).into_response()
    }
}
