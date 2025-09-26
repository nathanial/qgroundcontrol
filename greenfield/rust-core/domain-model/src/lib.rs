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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VehicleStatus {
    pub vehicle_id: VehicleId,
    pub vehicle_type: VehicleType,
    pub arming_state: ArmingState,
    pub heartbeat_millis: u32,
    pub flight_mode: Option<FlightMode>,
    pub battery: Option<BatteryStatus>,
    pub gps: Option<GpsStatus>,
}

impl VehicleStatus {
    pub fn new(vehicle_id: VehicleId) -> Self {
        Self {
            vehicle_id,
            vehicle_type: VehicleType::Unknown,
            arming_state: ArmingState::Unknown,
            heartbeat_millis: 0,
            flight_mode: None,
            battery: None,
            gps: None,
        }
    }
}

impl Default for VehicleStatus {
    fn default() -> Self {
        Self::new(VehicleId("UNKNOWN".into()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlightMode {
    pub label: String,
    pub base_mode: u8,
    pub custom_mode: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BatteryStatus {
    pub voltage_v: f32,
    pub current_a: Option<f32>,
    pub remaining_percent: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GpsFixType {
    NoFix,
    DeadReckoning,
    Fix2D,
    Fix3D,
    DGps,
    RtkFloat,
    RtkFixed,
    StaticHold,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GpsStatus {
    pub fix_type: GpsFixType,
    pub satellites_visible: u8,
    pub latitude_deg: Option<f64>,
    pub longitude_deg: Option<f64>,
    pub altitude_m: Option<f64>,
    pub hdop: Option<f32>,
    pub vdop: Option<f32>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MissionFrame {
    Global,
    GlobalRelativeAlt,
    GlobalTerrainAlt,
    Mission,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MissionSyncStage {
    Idle,
    Downloading,
    Uploading,
    AwaitingAck,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MissionOperationKind {
    Upload,
    Download,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MissionOperationStatus {
    Success,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MissionOperationReport {
    pub operation: MissionOperationKind,
    pub status: MissionOperationStatus,
    pub message: Option<String>,
    pub revision: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MissionSyncStatus {
    pub stage: MissionSyncStage,
    pub index: Option<u16>,
    pub total: Option<u16>,
    pub message: Option<String>,
}

impl MissionSyncStatus {
    pub fn new(stage: MissionSyncStage) -> Self {
        Self {
            stage,
            index: None,
            total: None,
            message: None,
        }
    }

    pub fn with_progress(mut self, index: Option<u16>, total: Option<u16>) -> Self {
        self.index = index;
        self.total = total;
        self
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MissionCoordinate {
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub altitude_m: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MissionItem {
    pub seq: u16,
    pub command: u16,
    pub frame: MissionFrame,
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub altitude_m: f32,
    pub param1: f32,
    pub param2: f32,
    pub param3: f32,
    pub param4: f32,
    pub auto_continue: bool,
    pub is_current: bool,
}

impl MissionItem {
    pub fn new(seq: u16, latitude: f64, longitude: f64, altitude: f32) -> Self {
        Self {
            seq,
            command: 16,
            frame: MissionFrame::GlobalRelativeAlt,
            latitude_deg: latitude,
            longitude_deg: longitude,
            altitude_m: altitude,
            param1: 0.0,
            param2: 0.0,
            param3: 0.0,
            param4: 0.0,
            auto_continue: true,
            is_current: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MissionPlan {
    pub plan_id: String,
    pub revision: u32,
    pub items: Vec<MissionItem>,
    pub home: Option<MissionCoordinate>,
    pub last_modified_millis: i64,
    pub notes: Option<String>,
}

impl MissionPlan {
    pub fn new(plan_id: impl Into<String>) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        Self {
            plan_id: plan_id.into(),
            revision: 0,
            items: Vec::new(),
            home: None,
            last_modified_millis: now,
            notes: None,
        }
    }

    pub fn with_items(mut self, items: Vec<MissionItem>) -> Self {
        self.items = items;
        self.last_modified_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        self
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
