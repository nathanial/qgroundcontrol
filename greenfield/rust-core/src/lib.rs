mod discovery;
mod error;
mod events;
mod mavlink;

use std::time::Duration;

use discovery::snapshot_devices;
use error::{invalid_argument, timeout, CoreError, CoreResult};
use mavlink::{manager as mav_manager, resolve_link, simulated_descriptor, LinkConfig};
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
#[derive(Debug, Serialize, Clone)]
pub struct FlightModeStatus {
    pub label: String,
    pub base_mode: u8,
    pub custom_mode: u32,
}

impl From<domain::FlightMode> for FlightModeStatus {
    fn from(mode: domain::FlightMode) -> Self {
        Self {
            label: mode.label,
            base_mode: mode.base_mode,
            custom_mode: mode.custom_mode,
        }
    }
}

#[napi(object)]
#[derive(Debug, Serialize, Clone)]
pub struct BatteryStatus {
    pub voltage_v: f64,
    pub current_a: Option<f64>,
    pub remaining_percent: Option<f64>,
}

impl From<domain::BatteryStatus> for BatteryStatus {
    fn from(status: domain::BatteryStatus) -> Self {
        Self {
            voltage_v: status.voltage_v as f64,
            current_a: status.current_a.map(|value| value as f64),
            remaining_percent: status.remaining_percent.map(|value| value as f64),
        }
    }
}

#[napi(string_enum)]
#[derive(Debug, Serialize, PartialEq, Eq)]
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

impl From<domain::GpsFixType> for GpsFixType {
    fn from(value: domain::GpsFixType) -> Self {
        match value {
            domain::GpsFixType::NoFix => Self::NoFix,
            domain::GpsFixType::DeadReckoning => Self::DeadReckoning,
            domain::GpsFixType::Fix2D => Self::Fix2D,
            domain::GpsFixType::Fix3D => Self::Fix3D,
            domain::GpsFixType::DGps => Self::DGps,
            domain::GpsFixType::RtkFloat => Self::RtkFloat,
            domain::GpsFixType::RtkFixed => Self::RtkFixed,
            domain::GpsFixType::StaticHold => Self::StaticHold,
            domain::GpsFixType::Other => Self::Other,
        }
    }
}

#[napi(object)]
#[derive(Debug, Serialize, Clone)]
pub struct GpsStatus {
    pub fix_type: GpsFixType,
    pub satellites_visible: u8,
    pub latitude_deg: Option<f64>,
    pub longitude_deg: Option<f64>,
    pub altitude_m: Option<f64>,
    pub hdop: Option<f64>,
    pub vdop: Option<f64>,
}

impl From<domain::GpsStatus> for GpsStatus {
    fn from(status: domain::GpsStatus) -> Self {
        Self {
            fix_type: status.fix_type.into(),
            satellites_visible: status.satellites_visible,
            latitude_deg: status.latitude_deg,
            longitude_deg: status.longitude_deg,
            altitude_m: status.altitude_m,
            hdop: status.hdop.map(|value| value as f64),
            vdop: status.vdop.map(|value| value as f64),
        }
    }
}

#[napi(object)]
pub struct VehicleStatus {
    pub vehicle_id: String,
    pub vehicle_type: VehicleType,
    pub arming_state: ArmingState,
    pub heartbeat_millis: u32,
    pub flight_mode: Option<FlightModeStatus>,
    pub battery: Option<BatteryStatus>,
    pub gps: Option<GpsStatus>,
}

impl From<domain::VehicleStatus> for VehicleStatus {
    fn from(status: domain::VehicleStatus) -> Self {
        Self {
            vehicle_id: status.vehicle_id.0,
            vehicle_type: status.vehicle_type.into(),
            arming_state: status.arming_state.into(),
            heartbeat_millis: status.heartbeat_millis,
            flight_mode: status.flight_mode.map(Into::into),
            battery: status.battery.map(Into::into),
            gps: status.gps.map(Into::into),
        }
    }
}

#[napi(string_enum)]
pub enum LinkKind {
    Simulated,
    Serial,
    Udp,
}

impl From<domain::LinkKind> for LinkKind {
    fn from(value: domain::LinkKind) -> Self {
        match value {
            domain::LinkKind::Simulated => Self::Simulated,
            domain::LinkKind::Serial => Self::Serial,
            domain::LinkKind::Udp => Self::Udp,
        }
    }
}

impl From<LinkKind> for domain::LinkKind {
    fn from(value: LinkKind) -> Self {
        match value {
            LinkKind::Simulated => Self::Simulated,
            LinkKind::Serial => Self::Serial,
            LinkKind::Udp => Self::Udp,
        }
    }
}

#[napi(object)]
pub struct DeviceDescriptor {
    pub id: String,
    pub label: String,
    pub transport: LinkKind,
    pub serial_path: Option<String>,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub udp_bind: Option<String>,
    pub udp_target_host: Option<String>,
    pub udp_target_port: Option<u16>,
}

impl From<domain::DeviceDescriptor> for DeviceDescriptor {
    fn from(descriptor: domain::DeviceDescriptor) -> Self {
        let mut result = Self {
            id: descriptor.id,
            label: descriptor.label,
            transport: descriptor.transport.clone().into(),
            serial_path: None,
            manufacturer: None,
            product: None,
            vid: None,
            pid: None,
            udp_bind: None,
            udp_target_host: None,
            udp_target_port: None,
        };

        match descriptor.details {
            domain::DeviceDetails::Simulated => {}
            domain::DeviceDetails::Serial {
                path,
                manufacturer,
                product,
                vid,
                pid,
            } => {
                result.serial_path = Some(path);
                result.manufacturer = manufacturer;
                result.product = product;
                result.vid = vid;
                result.pid = pid;
            }
            domain::DeviceDetails::Udp {
                bind,
                target_host,
                target_port,
            } => {
                result.udp_bind = Some(bind);
                result.udp_target_host = target_host;
                result.udp_target_port = target_port;
            }
        }

        result
    }
}

#[napi(object)]
pub struct ConnectOptions {
    pub link: Option<LinkKind>,
    pub device_id: Option<String>,
    pub serial_path: Option<String>,
    pub serial_baud: Option<u32>,
    pub udp_bind: Option<String>,
    pub udp_target_host: Option<String>,
    pub udp_target_port: Option<u16>,
    pub label: Option<String>,
    pub force_simulated: Option<bool>,
}

#[napi(string_enum)]
pub enum ConnectionPhase {
    Idle,
    Discovering,
    Connecting,
    Connected,
    Disconnecting,
    Disconnected,
    Error,
}

impl From<domain::ConnectionPhase> for ConnectionPhase {
    fn from(value: domain::ConnectionPhase) -> Self {
        match value {
            domain::ConnectionPhase::Idle => Self::Idle,
            domain::ConnectionPhase::Discovering => Self::Discovering,
            domain::ConnectionPhase::Connecting => Self::Connecting,
            domain::ConnectionPhase::Connected => Self::Connected,
            domain::ConnectionPhase::Disconnecting => Self::Disconnecting,
            domain::ConnectionPhase::Disconnected => Self::Disconnected,
            domain::ConnectionPhase::Error => Self::Error,
        }
    }
}

#[napi(object)]
pub struct ConnectionStatus {
    pub phase: ConnectionPhase,
    pub message: Option<String>,
    pub device: Option<DeviceDescriptor>,
}

impl From<domain::ConnectionStatus> for ConnectionStatus {
    fn from(status: domain::ConnectionStatus) -> Self {
        Self {
            phase: status.phase.into(),
            message: status.message,
            device: status.device.map(Into::into),
        }
    }
}

#[napi(object)]
pub struct ParameterValue {
    pub name: String,
    pub value: f64,
    pub param_type: String,
    pub index: Option<u16>,
}

impl From<domain::ParameterValue> for ParameterValue {
    fn from(value: domain::ParameterValue) -> Self {
        Self {
            name: value.name,
            value: value.value as f64,
            param_type: value.param_type,
            index: value.index,
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
    let status = mav_manager().vehicle_status();

    if status.vehicle_id.as_str() == "UNKNOWN" {
        let mut bootstrap = domain::VehicleStatus::new(domain::VehicleId("SIM-01".into()));
        bootstrap.vehicle_type = domain::VehicleType::Multirotor;
        bootstrap.arming_state = domain::ArmingState::Disarmed;
        bootstrap.heartbeat_millis = 0;
        bootstrap.flight_mode = Some(domain::FlightMode {
            label: "Standby".into(),
            base_mode: 0,
            custom_mode: 0,
        });
        return Ok(bootstrap.into());
    }

    Ok(status.into())
}

#[napi]
pub fn version() -> Result<String> {
    Ok(env!("CARGO_PKG_VERSION").to_string())
}

#[napi]
pub fn simulate_failure() -> Result<()> {
    Err(CoreError::Other(anyhow::anyhow!("forced failure for diagnostics")).into())
}

#[napi]
pub fn describe_status_channel() -> Result<String> {
    Ok(String::from(
        "Rust core emits JSON envelopes with { level, kind, message } fields.",
    ))
}

#[napi]
pub fn register_event_sink(callback: JsFunction) -> Result<()> {
    events::register_sink(callback).map_err(Into::into)
}

#[napi]
pub async fn start_device_watch(interval_ms: Option<u32>) -> Result<()> {
    let clamped = interval_ms.unwrap_or(1_500).clamp(250, 10_000);
    discovery::start(Duration::from_millis(clamped as u64)).await;
    Ok(())
}

#[napi]
pub fn list_devices() -> Result<Vec<DeviceDescriptor>> {
    let devices: Vec<DeviceDescriptor> = snapshot_devices().into_iter().map(Into::into).collect();
    Ok(devices)
}

#[napi]
pub async fn connect_mavlink(options: Option<ConnectOptions>) -> Result<ConnectionStatus> {
    let options = options.unwrap_or(ConnectOptions {
        link: None,
        device_id: None,
        serial_path: None,
        serial_baud: None,
        udp_bind: None,
        udp_target_host: None,
        udp_target_port: None,
        label: None,
        force_simulated: None,
    });

    let descriptor = descriptor_from_options(&options)?;
    let mut link = resolve_link(&descriptor);

    if let LinkConfig::Serial { baud, .. } = &mut link {
        if let Some(custom) = options.serial_baud {
            *baud = custom;
        }
    }

    let status = mav_manager()
        .connect(link)
        .await
        .map_err::<Error, _>(Into::into)?;
    Ok(status.into())
}

#[napi]
pub async fn disconnect_mavlink() -> Result<()> {
    mav_manager().disconnect().await.map_err(Into::into)
}

#[napi]
pub async fn fetch_parameters(timeout_ms: Option<u32>) -> Result<Vec<ParameterValue>> {
    let timeout_duration = timeout_ms.map(|value| Duration::from_millis(value as u64));
    let params = mav_manager()
        .fetch_parameters(timeout_duration)
        .await
        .map_err::<Error, _>(Into::into)?;

    Ok(params.into_iter().map(Into::into).collect())
}

#[napi]
pub fn current_connection_status() -> Result<ConnectionStatus> {
    Ok(mav_manager().status().into())
}

#[napi]
pub fn cached_parameters() -> Result<Vec<ParameterValue>> {
    Ok(mav_manager()
        .cached_parameters()
        .into_iter()
        .map(Into::into)
        .collect())
}

fn descriptor_from_options(options: &ConnectOptions) -> CoreResult<domain::DeviceDescriptor> {
    if options.force_simulated.unwrap_or(false) {
        return Ok(simulated_descriptor());
    }

    let known_devices = snapshot_devices();

    if let Some(id) = &options.device_id {
        if let Some(device) = known_devices.iter().find(|d| &d.id == id) {
            return Ok(device.clone());
        }
    }

    match options.link.unwrap_or(LinkKind::Udp) {
        LinkKind::Simulated => Ok(simulated_descriptor()),
        LinkKind::Serial => {
            let path = options
                .serial_path
                .clone()
                .ok_or_else(|| invalid_argument("serial_path required for serial connections"))?;
            let label = options
                .label
                .clone()
                .unwrap_or_else(|| format!("Serial {path}"));
            Ok(domain::DeviceDescriptor {
                id: options
                    .device_id
                    .clone()
                    .unwrap_or_else(|| format!("serial:{path}")),
                label,
                transport: domain::LinkKind::Serial,
                details: domain::DeviceDetails::Serial {
                    path,
                    manufacturer: None,
                    product: None,
                    vid: None,
                    pid: None,
                },
            })
        }
        LinkKind::Udp => {
            let bind = options
                .udp_bind
                .clone()
                .or_else(|| {
                    known_devices.iter().find_map(|d| match &d.details {
                        domain::DeviceDetails::Udp { bind, .. } => Some(bind.clone()),
                        _ => None,
                    })
                })
                .unwrap_or_else(|| "0.0.0.0:14550".into());

            let label = options
                .label
                .clone()
                .unwrap_or_else(|| format!("UDP {bind}"));

            Ok(domain::DeviceDescriptor {
                id: options
                    .device_id
                    .clone()
                    .unwrap_or_else(|| format!("udp:{bind}")),
                label,
                transport: domain::LinkKind::Udp,
                details: domain::DeviceDetails::Udp {
                    bind,
                    target_host: options.udp_target_host.clone(),
                    target_port: options.udp_target_port,
                },
            })
        }
    }
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
