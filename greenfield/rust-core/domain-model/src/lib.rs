use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct VehicleId(pub String);

impl VehicleId {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum VehicleType {
    Multirotor,
    FixedWing,
    VTOL,
    Rover,
    Sub,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ArmingState {
    Disarmed,
    Arming,
    Armed,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VehicleStatus {
    pub vehicle_id: VehicleId,
    pub vehicle_type: VehicleType,
    pub arming_state: ArmingState,
    pub heartbeat_millis: u32,
}

impl VehicleStatus {
    pub fn new(vehicle_id: VehicleId) -> Self {
        Self {
            vehicle_id,
            vehicle_type: VehicleType::Unknown,
            arming_state: ArmingState::Unknown,
            heartbeat_millis: 0,
        }
    }
}

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("vehicle not found: {0}")]
    VehicleNotFound(String),
    #[error("operation timed out after {0} ms")]
    Timeout(u64),
}

pub type DomainResult<T> = std::result::Result<T, DomainError>;
