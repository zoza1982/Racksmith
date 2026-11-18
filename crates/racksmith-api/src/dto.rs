//! Wire types: the JSON request and response bodies of the HTTP API.
//!
//! These types are the API's public contract and are kept separate from the domain types, so
//! the domain can evolve without silently changing the wire format. Every conversion between
//! the two is written out explicitly here.

use racksmith_domain::{Hostname, MacAddress, Machine, MachineEvent, MachineId, MachineState};
use serde::{Deserialize, Serialize};

use crate::ApiError;

/// Body of `GET /healthz`.
#[derive(Debug, Serialize)]
pub(crate) struct HealthResponse {
    /// Always `"ok"` while the server is accepting requests.
    pub(crate) status: &'static str,
}

/// Body of `POST /api/v1/machines`.
///
/// Fields are plain strings so that validation failures become precise `422` errors rather
/// than generic deserialization errors.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateMachineRequest {
    /// MAC address of the primary boot interface, `:`- or `-`-separated.
    mac: String,
    /// Optional RFC 1123 hostname.
    #[serde(default)]
    hostname: Option<String>,
}

impl CreateMachineRequest {
    /// Validates the request into domain values.
    ///
    /// # Errors
    ///
    /// Returns [`ApiError::InvalidMac`] or [`ApiError::InvalidHostname`] for malformed fields.
    pub(crate) fn validate(self) -> Result<(MacAddress, Option<Hostname>), ApiError> {
        let mac = self.mac.parse()?;
        let hostname = self.hostname.map(Hostname::try_from).transpose()?;
        Ok((mac, hostname))
    }
}

/// Body of `POST /api/v1/machines/{id}/events`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EventRequest {
    /// The lifecycle event to apply, for example `"start_commissioning"`.
    pub(crate) event: MachineEvent,
}

/// A machine as returned by every machine endpoint.
#[derive(Debug, Serialize)]
pub(crate) struct MachineResponse {
    id: MachineId,
    mac: MacAddress,
    hostname: Option<Hostname>,
    state: MachineState,
}

impl From<Machine> for MachineResponse {
    fn from(machine: Machine) -> Self {
        Self {
            id: machine.id(),
            mac: machine.mac(),
            hostname: machine.hostname().cloned(),
            state: machine.state(),
        }
    }
}

/// Body of every error response: `{"error": {"code": "...", "message": "..."}}`.
#[derive(Debug, Serialize)]
pub(crate) struct ErrorResponse {
    error: ErrorDetail,
}

/// The `error` object of an [`ErrorResponse`].
#[derive(Debug, Serialize)]
struct ErrorDetail {
    /// Stable, machine-readable error code; see [`ApiError::code`].
    code: &'static str,
    /// Human-readable description. Its wording may change; match on `code` instead.
    message: String,
}

impl From<&ApiError> for ErrorResponse {
    fn from(error: &ApiError) -> Self {
        Self {
            error: ErrorDetail {
                code: error.code(),
                message: error.to_string(),
            },
        }
    }
}
