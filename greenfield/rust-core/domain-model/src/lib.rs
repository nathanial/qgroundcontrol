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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum LinkKind {
    Simulated,
    Serial,
    Udp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DeviceDetails {
    Simulated,
    Serial {
        path: String,
        manufacturer: Option<String>,
        product: Option<String>,
        vid: Option<u16>,
        pid: Option<u16>,
    },
    Udp {
        bind: String,
        target_host: Option<String>,
        target_port: Option<u16>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceDescriptor {
    pub id: String,
    pub label: String,
    pub transport: LinkKind,
    pub details: DeviceDetails,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConnectionPhase {
    Idle,
    Discovering,
    Connecting,
    Connected,
    Disconnecting,
    Disconnected,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionStatus {
    pub phase: ConnectionPhase,
    pub message: Option<String>,
    pub device: Option<DeviceDescriptor>,
}

impl ConnectionStatus {
    pub fn new(phase: ConnectionPhase) -> Self {
        Self {
            phase,
            message: None,
            device: None,
        }
    }

    pub fn with_device(mut self, device: DeviceDescriptor) -> Self {
        self.device = Some(device);
        self
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MissionLogEntry {
    pub timestamp_millis: i64,
    pub level: LogLevel,
    pub source: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParameterValue {
    pub name: String,
    pub value: f32,
    pub param_type: String,
    pub index: Option<u16>,
}

impl ParameterValue {
    pub fn new(name: impl Into<String>, value: f32) -> Self {
        Self {
            name: name.into(),
            value,
            param_type: "float".into(),
            index: None,
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
