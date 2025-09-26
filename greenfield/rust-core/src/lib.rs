mod error;

use std::time::Duration;

use error::{invalid_argument, timeout, CoreError, CoreResult};
use napi::bindgen_prelude::*;
use napi_derive::napi;
use qgc_domain as domain;
use serde::Serialize;
use tokio::time::sleep;

/// Message returned to the Electron shell.
#[napi(object)]
#[derive(Debug, Serialize, Clone)]
pub struct StatusMessage {
    pub kind: String,
    pub message: String,
}

impl StatusMessage {
    fn new(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
        }
    }
}

#[napi(string_enum)]
pub enum VehicleType {
    Multirotor,
    FixedWing,
    VTOL,
    Rover,
    Sub,
    Unknown,
}

impl From<domain::VehicleType> for VehicleType {
    fn from(value: domain::VehicleType) -> Self {
        match value {
            domain::VehicleType::Multirotor => Self::Multirotor,
            domain::VehicleType::FixedWing => Self::FixedWing,
            domain::VehicleType::VTOL => Self::VTOL,
            domain::VehicleType::Rover => Self::Rover,
            domain::VehicleType::Sub => Self::Sub,
            domain::VehicleType::Unknown => Self::Unknown,
        }
    }
}

#[napi(string_enum)]
pub enum ArmingState {
    Disarmed,
    Arming,
    Armed,
    Unknown,
}

impl From<domain::ArmingState> for ArmingState {
    fn from(value: domain::ArmingState) -> Self {
        match value {
            domain::ArmingState::Disarmed => Self::Disarmed,
            domain::ArmingState::Arming => Self::Arming,
            domain::ArmingState::Armed => Self::Armed,
            domain::ArmingState::Unknown => Self::Unknown,
        }
    }
}

#[napi(object)]
pub struct VehicleStatus {
    pub vehicle_id: String,
    pub vehicle_type: VehicleType,
    pub arming_state: ArmingState,
    pub heartbeat_millis: u32,
}

impl From<domain::VehicleStatus> for VehicleStatus {
    fn from(status: domain::VehicleStatus) -> Self {
        Self {
            vehicle_id: status.vehicle_id.0,
            vehicle_type: status.vehicle_type.into(),
            arming_state: status.arming_state.into(),
            heartbeat_millis: status.heartbeat_millis,
        }
    }
}

fn health_check_impl() -> CoreResult<StatusMessage> {
    Ok(StatusMessage::new("health", "rust-core napi module loaded"))
}

#[napi]
pub fn health_check() -> Result<StatusMessage> {
    health_check_impl().map_err(Into::into)
}

#[napi]
pub async fn run_diagnostics(timeout_ms: Option<u32>) -> Result<StatusMessage> {
    let timeout_value = timeout_ms.unwrap_or(50);
    if timeout_value == 0 {
        return Err(invalid_argument("timeout must be greater than 0 ms").into());
    }
    if timeout_value > 5_000 {
        let duration = Duration::from_millis(timeout_value as u64);
        return Err(timeout(duration).into());
    }

    let duration = Duration::from_millis(timeout_value as u64);
    sleep(duration).await;

    Ok(StatusMessage::new(
        "diagnostic",
        format!("completed after {} ms", timeout_value),
    ))
}

#[napi]
pub fn bootstrap_vehicle_status() -> Result<VehicleStatus> {
    let domain_status = domain::VehicleStatus {
        vehicle_id: domain::VehicleId("SIM-01".into()),
        vehicle_type: domain::VehicleType::Multirotor,
        arming_state: domain::ArmingState::Disarmed,
        heartbeat_millis: 120,
    };

    Ok(domain_status.into())
}

#[napi]
pub fn version() -> Result<String> {
    Ok(env!("CARGO_PKG_VERSION").to_string())
}

#[napi]
pub fn simulate_failure() -> Result<()> {
    Err(CoreError::Other(anyhow::anyhow!("forced failure for diagnostics")).into())
}

/// Provide richer error context for the renderer.
#[napi]
pub fn describe_status_channel() -> Result<String> {
    Ok(String::from(
        "Rust core emits JSON envelopes with { level, kind, message } fields.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_check_succeeds() {
        let status = health_check_impl().expect("status");
        assert_eq!(status.kind, "health");
    }

    #[tokio::test]
    async fn diagnostics_respects_timeout() {
        let result = run_diagnostics(Some(10)).await.expect("diagnostics");
        assert!(result.message.contains("10 ms"));
    }

    #[tokio::test]
    async fn diagnostics_rejects_large_timeout() {
        let err = run_diagnostics(Some(10_000))
            .await
            .expect_err("should reject");
        assert!(err.status == napi::Status::Cancelled);
    }
}
