use std::sync::Arc;
use std::time::{Duration, Instant};

use mavlink::common::{self, MavMessage, MavState, MavType};
use mavlink::connect_async;
use once_cell::sync::Lazy;
use parking_lot::{Mutex, RwLock};
use qgc_domain as domain;
use tokio::sync::{mpsc, oneshot};

use crate::error::{invalid_argument, timeout, CoreError, CoreResult};
use crate::events::{self, CoreEvent};

const DEFAULT_PARAMETER_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub enum LinkConfig {
    Simulated,
    Udp {
        descriptor: domain::DeviceDescriptor,
    },
    Serial {
        descriptor: domain::DeviceDescriptor,
        baud: u32,
    },
}

struct PendingParameters {
    expected: Option<usize>,
    values: Vec<domain::ParameterValue>,
    responder: oneshot::Sender<CoreResult<Vec<domain::ParameterValue>>>,
    deadline: Instant,
}

impl PendingParameters {
    fn new(
        timeout: Duration,
        responder: oneshot::Sender<CoreResult<Vec<domain::ParameterValue>>>,
    ) -> Self {
        Self {
            expected: None,
            values: Vec::new(),
            responder,
            deadline: Instant::now() + timeout,
        }
    }

    fn push(&mut self, value: domain::ParameterValue, expected: Option<usize>) {
        if let Some(expected) = expected {
            self.expected = Some(expected);
        }
        self.values.push(value);
    }

    fn is_complete(&self) -> bool {
        self.expected
            .map(|expected| self.values.len() >= expected)
            .unwrap_or(false)
    }

    fn is_expired(&self) -> bool {
        Instant::now() > self.deadline
    }

    fn finish(self) {
        let _ = self.responder.send(Ok(self.values));
    }

    fn fail(self, error: CoreError) {
        let _ = self.responder.send(Err(error));
    }
}

struct SessionState {
    descriptor: domain::DeviceDescriptor,
    status: RwLock<domain::ConnectionStatus>,
    autopilot_ids: Mutex<Option<(u8, u8)>>,
    last_heartbeat: Mutex<Option<Instant>>,
    parameter_cache: RwLock<Vec<domain::ParameterValue>>,
    pending_parameters: Mutex<Option<PendingParameters>>,
}

impl SessionState {
    fn new(descriptor: domain::DeviceDescriptor) -> Arc<Self> {
        Arc::new(Self {
            status: RwLock::new(
                domain::ConnectionStatus::new(domain::ConnectionPhase::Connecting)
                    .with_device(descriptor.clone()),
            ),
            descriptor,
            autopilot_ids: Mutex::new(None),
            last_heartbeat: Mutex::new(None),
            parameter_cache: RwLock::new(Vec::new()),
            pending_parameters: Mutex::new(None),
        })
    }

    fn status(&self) -> domain::ConnectionStatus {
        self.status.read().clone()
    }

    fn set_status(&self, phase: domain::ConnectionPhase, message: Option<String>) {
        let mut guard = self.status.write();
        guard.phase = phase;
        guard.message = message;
        guard.device = Some(self.descriptor.clone());
        events::emit(CoreEvent::ConnectionStatus {
            status: guard.clone(),
        });
    }

    fn record_heartbeat_interval(&self) -> u32 {
        let mut guard = self.last_heartbeat.lock();
        let now = Instant::now();
        let elapsed = guard
            .map(|previous| now.saturating_duration_since(previous).as_millis() as u32)
            .unwrap_or_default();
        *guard = Some(now);
        elapsed
    }

    fn set_autopilot_ids(&self, system_id: u8, component_id: u8) {
        *self.autopilot_ids.lock() = Some((system_id, component_id));
    }

    fn autopilot_ids(&self) -> (u8, u8) {
        self.autopilot_ids.lock().clone().unwrap_or((
            1,
            mavlink::common::MavComponent::MAV_COMP_ID_AUTOPILOT1 as u8,
        ))
    }

    fn begin_parameter_request(
        &self,
        timeout: Duration,
        responder: oneshot::Sender<CoreResult<Vec<domain::ParameterValue>>>,
    ) -> Result<(), oneshot::Sender<CoreResult<Vec<domain::ParameterValue>>>> {
        let mut guard = self.pending_parameters.lock();
        if guard.is_some() {
            Err(responder)
        } else {
            *guard = Some(PendingParameters::new(timeout, responder));
            Ok(())
        }
    }

    fn on_parameter_value(
        &self,
        value: domain::ParameterValue,
        expected: Option<usize>,
    ) -> Option<Vec<domain::ParameterValue>> {
        let mut_guard = &mut *self.pending_parameters.lock();
        if let Some(pending) = mut_guard.as_mut() {
            pending.push(value, expected);
            events::emit(CoreEvent::ParameterProgress {
                received: pending.values.len(),
                expected: pending.expected,
            });
            if pending.is_complete() {
                let values = pending.values.clone();
                *self.parameter_cache.write() = values.clone();
                let pending = mut_guard.take().unwrap();
                pending.finish();
                return Some(values);
            }
        }

        None
    }

    fn fail_parameter_request(&self, error: CoreError) {
        if let Some(pending) = self.pending_parameters.lock().take() {
            pending.fail(error);
        }
    }

    fn expire_parameter_request_if_needed(&self) {
        let mut guard = self.pending_parameters.lock();
        if let Some(pending) = guard.as_ref() {
            if pending.is_expired() {
                let pending = guard.take().unwrap();
                pending.fail(timeout(DEFAULT_PARAMETER_TIMEOUT));
            }
        }
    }

    fn parameter_cache(&self) -> Vec<domain::ParameterValue> {
        self.parameter_cache.read().clone()
    }
}

enum SessionCommand {
    Disconnect {
        respond_to: oneshot::Sender<CoreResult<()>>,
    },
    FetchParameters {
        respond_to: oneshot::Sender<CoreResult<Vec<domain::ParameterValue>>>,
        timeout: Duration,
    },
}

struct MavlinkSession {
    state: Arc<SessionState>,
    command_tx: mpsc::Sender<SessionCommand>,
}

impl MavlinkSession {
    fn status(&self) -> domain::ConnectionStatus {
        self.state.status()
    }

    fn parameter_cache(&self) -> Vec<domain::ParameterValue> {
        self.state.parameter_cache()
    }
}

pub struct MavlinkManager {
    session: Mutex<Option<Arc<MavlinkSession>>>,
}

static MANAGER: Lazy<MavlinkManager> = Lazy::new(|| MavlinkManager {
    session: Mutex::new(None),
});

impl MavlinkManager {
    pub fn instance() -> &'static Self {
        &MANAGER
    }

    pub fn status(&self) -> domain::ConnectionStatus {
        self.session
            .lock()
            .as_ref()
            .map(|session| session.status())
            .unwrap_or_else(|| domain::ConnectionStatus::new(domain::ConnectionPhase::Idle))
    }

    pub fn cached_parameters(&self) -> Vec<domain::ParameterValue> {
        self.session
            .lock()
            .as_ref()
            .map(|session| session.parameter_cache())
            .unwrap_or_default()
    }

    pub async fn connect(&self, config: LinkConfig) -> CoreResult<domain::ConnectionStatus> {
        if self.session.lock().is_some() {
            return Err(invalid_argument("MAVLink session already active"));
        }

        let session = match config.clone() {
            LinkConfig::Simulated => spawn_simulated_session().await?,
            LinkConfig::Udp { .. } => spawn_udp_session(config).await?,
            LinkConfig::Serial { .. } => spawn_serial_session(config).await?,
        };

        let session = Arc::new(session);
        let status = session.status();
        *self.session.lock() = Some(session);
        Ok(status)
    }

    pub async fn disconnect(&self) -> CoreResult<()> {
        let session = self.session.lock().take();
        if let Some(session) = session {
            let (tx, rx) = oneshot::channel();
            session
                .command_tx
                .send(SessionCommand::Disconnect { respond_to: tx })
                .await
                .map_err(|_| invalid_argument("connection already closed"))?;
            rx.await
                .unwrap_or_else(|_| Err(invalid_argument("connection closed")))?
        }
        Ok(())
    }

    pub async fn fetch_parameters(
        &self,
        timeout_override: Option<Duration>,
    ) -> CoreResult<Vec<domain::ParameterValue>> {
        let timeout_limit = timeout_override.unwrap_or(DEFAULT_PARAMETER_TIMEOUT);
        let session = self
            .session
            .lock()
            .as_ref()
            .cloned()
            .ok_or_else(|| invalid_argument("no active MAVLink session"))?;

        let (tx, rx) = oneshot::channel();
        session
            .command_tx
            .send(SessionCommand::FetchParameters {
                respond_to: tx,
                timeout: timeout_limit,
            })
            .await
            .map_err(|_| invalid_argument("connection closed"))?;

        let response = tokio::time::timeout(timeout_limit + Duration::from_secs(1), rx)
            .await
            .map_err(|_| timeout(timeout_limit))?;

        response.map_err(|_| invalid_argument("connection closed"))?
    }
}

pub fn manager() -> &'static MavlinkManager {
    MavlinkManager::instance()
}

pub fn resolve_link(config: &domain::DeviceDescriptor) -> LinkConfig {
    match &config.transport {
        domain::LinkKind::Simulated => LinkConfig::Simulated,
        domain::LinkKind::Serial => {
            if matches!(config.details, domain::DeviceDetails::Serial { .. }) {
                LinkConfig::Serial {
                    descriptor: config.clone(),
                    baud: 115200,
                }
            } else {
                LinkConfig::Simulated
            }
        }
        domain::LinkKind::Udp => {
            if matches!(config.details, domain::DeviceDetails::Udp { .. }) {
                LinkConfig::Udp {
                    descriptor: config.clone(),
                }
            } else {
                LinkConfig::Udp {
                    descriptor: domain::DeviceDescriptor {
                        id: "udp:0.0.0.0:14550".into(),
                        label: "UDP 0.0.0.0:14550".into(),
                        transport: domain::LinkKind::Udp,
                        details: domain::DeviceDetails::Udp {
                            bind: "0.0.0.0:14550".into(),
                            target_host: None,
                            target_port: None,
                        },
                    },
                }
            }
        }
    }
}

async fn spawn_simulated_session() -> CoreResult<MavlinkSession> {
    let descriptor = simulated_descriptor();

    let state = SessionState::new(descriptor.clone());
    state.set_status(
        domain::ConnectionPhase::Connected,
        Some("simulated link".into()),
    );

    let (command_tx, mut command_rx) = mpsc::channel(8);

    tokio::spawn({
        let state = state.clone();
        async move {
            let mut heartbeat_interval = tokio::time::interval(Duration::from_millis(700));
            let mut counter = 0u32;
            loop {
                tokio::select! {
                    Some(command) = command_rx.recv() => match command {
                        SessionCommand::Disconnect { respond_to } => {
                            state.set_status(domain::ConnectionPhase::Disconnected, Some("simulator stopped".into()));
                            let _ = respond_to.send(Ok(()));
                            break;
                        }
                        SessionCommand::FetchParameters { respond_to, .. } => {
                            let params = simulated_parameters();
                            let _ = respond_to.send(Ok(params.clone()));
                            events::emit(CoreEvent::ParameterProgress { received: params.len(), expected: Some(params.len()) });
                            events::emit(CoreEvent::ParameterBatch { parameters: params });
                        }
                    },
                    _ = heartbeat_interval.tick() => {
                        counter = counter.wrapping_add(1);
                        let heartbeat = domain::VehicleStatus {
                            vehicle_id: domain::VehicleId("SIM-01".into()),
                            vehicle_type: domain::VehicleType::Multirotor,
                            arming_state: domain::ArmingState::Disarmed,
                            heartbeat_millis: state.record_heartbeat_interval().max(500),
                        };
                        events::emit(CoreEvent::Heartbeat { status: heartbeat });

                        if counter % 5 == 0 {
                            events::emit(CoreEvent::Diagnostics {
                                level: domain::LogLevel::Info,
                                message: "simulated telemetry ok".into(),
                            });
                        }
                    }
                }
            }
        }
    });

    Ok(MavlinkSession { state, command_tx })
}

async fn spawn_udp_session(config: LinkConfig) -> CoreResult<MavlinkSession> {
    let descriptor = match config {
        LinkConfig::Udp { descriptor } => descriptor,
        _ => unreachable!(),
    };

    let bind = match &descriptor.details {
        domain::DeviceDetails::Udp { bind, .. } => bind.clone(),
        _ => "0.0.0.0:14550".into(),
    };

    let uri = format!("udpin:{bind}");
    let connection = connect_async::<MavMessage>(&uri)
        .await
        .map_err(|err| CoreError::Other(anyhow::anyhow!(err.to_string())))?;

    let state = SessionState::new(descriptor.clone());
    state.set_status(
        domain::ConnectionPhase::Connecting,
        Some("awaiting heartbeat".into()),
    );

    let (command_tx, mut command_rx) = mpsc::channel(32);

    tokio::spawn({
        let state = state.clone();
        async move {
            let connection = connection;
            loop {
                tokio::select! {
                    Some(command) = command_rx.recv() => match command {
                        SessionCommand::Disconnect { respond_to } => {
                            state.set_status(domain::ConnectionPhase::Disconnecting, Some("closing link".into()));
                            state.fail_parameter_request(invalid_argument("connection closed"));
                            let _ = respond_to.send(Ok(()));
                            break;
                        }
                        SessionCommand::FetchParameters { respond_to, timeout } => {
                            match state.begin_parameter_request(timeout, respond_to) {
                                Ok(()) => {
                                    let (target_system, target_component) = state.autopilot_ids();
                                    let request = MavMessage::PARAM_REQUEST_LIST(common::PARAM_REQUEST_LIST_DATA {
                                        target_system,
                                        target_component,
                                    });
                                    if let Err(err) = connection.send_default(&request).await {
                                        state.fail_parameter_request(CoreError::Other(anyhow::anyhow!(err.to_string())));
                                    } else {
                                        events::emit(CoreEvent::ParameterProgress { received: 0, expected: None });
                                    }
                                }
                                Err(responder) => {
                                    let _ = responder.send(Err(invalid_argument("parameter request already running")));
                                }
                            }
                        }
                    },
                    result = connection.recv() => match result {
                        Ok((header, message)) => {
                            handle_mavlink_message(&state, header, message);
                        }
                        Err(err) => {
                            state.set_status(domain::ConnectionPhase::Error, Some(format!("recv error: {err}")));
                            state.fail_parameter_request(CoreError::Other(anyhow::anyhow!(err.to_string())));
                            break;
                        }
                    }
                }

                state.expire_parameter_request_if_needed();
            }

            state.set_status(
                domain::ConnectionPhase::Disconnected,
                Some("link closed".into()),
            );
        }
    });

    Ok(MavlinkSession { state, command_tx })
}

async fn spawn_serial_session(config: LinkConfig) -> CoreResult<MavlinkSession> {
    let (descriptor, baud) = match config {
        LinkConfig::Serial { descriptor, baud } => (descriptor, baud),
        _ => unreachable!(),
    };

    let path = match &descriptor.details {
        domain::DeviceDetails::Serial { path, .. } => path.clone(),
        _ => return Err(invalid_argument("serial descriptor missing path")),
    };

    let uri = format!("serial:{path}:{baud}");
    let connection = connect_async::<MavMessage>(&uri)
        .await
        .map_err(|err| CoreError::Other(anyhow::anyhow!(err.to_string())))?;

    let state = SessionState::new(descriptor.clone());
    state.set_status(
        domain::ConnectionPhase::Connecting,
        Some("awaiting heartbeat".into()),
    );

    let (command_tx, mut command_rx) = mpsc::channel(32);

    tokio::spawn({
        let state = state.clone();
        async move {
            let connection = connection;
            loop {
                tokio::select! {
                    Some(command) = command_rx.recv() => match command {
                        SessionCommand::Disconnect { respond_to } => {
                            state.set_status(domain::ConnectionPhase::Disconnecting, Some("closing serial link".into()));
                            state.fail_parameter_request(invalid_argument("connection closed"));
                            let _ = respond_to.send(Ok(()));
                            break;
                        }
                        SessionCommand::FetchParameters { respond_to, timeout } => {
                            match state.begin_parameter_request(timeout, respond_to) {
                                Ok(()) => {
                                    let (target_system, target_component) = state.autopilot_ids();
                                    let request = MavMessage::PARAM_REQUEST_LIST(common::PARAM_REQUEST_LIST_DATA {
                                        target_system,
                                        target_component,
                                    });
                                    if let Err(err) = connection.send_default(&request).await {
                                        state.fail_parameter_request(CoreError::Other(anyhow::anyhow!(err.to_string())));
                                    } else {
                                        events::emit(CoreEvent::ParameterProgress { received: 0, expected: None });
                                    }
                                }
                                Err(responder) => {
                                    let _ = responder.send(Err(invalid_argument("parameter request already running")));
                                }
                            }
                        }
                    },
                    result = connection.recv() => match result {
                        Ok((header, message)) => {
                            handle_mavlink_message(&state, header, message);
                        }
                        Err(err) => {
                            state.set_status(domain::ConnectionPhase::Error, Some(format!("serial recv error: {err}")));
                            state.fail_parameter_request(CoreError::Other(anyhow::anyhow!(err.to_string())));
                            break;
                        }
                    }
                }

                state.expire_parameter_request_if_needed();
            }

            state.set_status(
                domain::ConnectionPhase::Disconnected,
                Some("serial link closed".into()),
            );
        }
    });

    Ok(MavlinkSession { state, command_tx })
}

fn handle_mavlink_message(
    state: &Arc<SessionState>,
    header: mavlink::MavHeader,
    message: MavMessage,
) {
    match message {
        MavMessage::HEARTBEAT(data) => {
            state.set_autopilot_ids(header.system_id, header.component_id);
            state.set_status(
                domain::ConnectionPhase::Connected,
                Some("heartbeat received".into()),
            );

            let heartbeat = domain::VehicleStatus {
                vehicle_id: domain::VehicleId(format!("SYS-{}", header.system_id)),
                vehicle_type: map_vehicle_type(data.mavtype),
                arming_state: map_arming_state(data.system_status),
                heartbeat_millis: state.record_heartbeat_interval(),
            };
            events::emit(CoreEvent::Heartbeat { status: heartbeat });
        }
        MavMessage::STATUSTEXT(data) => {
            let severity = map_statustext_severity(data.severity);
            let message = trim_zero_terminated(&data.text);
            events::emit_log(severity, "mavlink", message);
        }
        MavMessage::PARAM_VALUE(data) => {
            let value = domain::ParameterValue {
                name: trim_zero_terminated(&data.param_id),
                value: data.param_value,
                param_type: format!("{:?}", data.param_type),
                index: Some(data.param_index as u16),
            };
            let expected = if data.param_count > 0 {
                Some(data.param_count as usize)
            } else {
                None
            };

            if let Some(completed) = state.on_parameter_value(value, expected) {
                events::emit(CoreEvent::ParameterBatch {
                    parameters: completed,
                });
            }
        }
        _ => {}
    }
}

fn map_vehicle_type(value: MavType) -> domain::VehicleType {
    match value {
        MavType::MAV_TYPE_FIXED_WING => domain::VehicleType::FixedWing,
        MavType::MAV_TYPE_GENERIC
        | MavType::MAV_TYPE_QUADROTOR
        | MavType::MAV_TYPE_HEXAROTOR
        | MavType::MAV_TYPE_OCTOROTOR
        | MavType::MAV_TYPE_COAXIAL
        | MavType::MAV_TYPE_TRICOPTER
        | MavType::MAV_TYPE_HELICOPTER => domain::VehicleType::Multirotor,
        MavType::MAV_TYPE_GROUND_ROVER | MavType::MAV_TYPE_SURFACE_BOAT => {
            domain::VehicleType::Rover
        }
        MavType::MAV_TYPE_SUBMARINE => domain::VehicleType::Sub,
        _ => domain::VehicleType::Unknown,
    }
}

fn map_arming_state(value: MavState) -> domain::ArmingState {
    match value {
        MavState::MAV_STATE_UNINIT | MavState::MAV_STATE_STANDBY | MavState::MAV_STATE_BOOT => {
            domain::ArmingState::Disarmed
        }
        MavState::MAV_STATE_CALIBRATING => domain::ArmingState::Arming,
        MavState::MAV_STATE_ACTIVE
        | MavState::MAV_STATE_CRITICAL
        | MavState::MAV_STATE_EMERGENCY => domain::ArmingState::Armed,
        _ => domain::ArmingState::Unknown,
    }
}

#[allow(unreachable_patterns)]
fn map_statustext_severity(value: common::MavSeverity) -> domain::LogLevel {
    match value {
        common::MavSeverity::MAV_SEVERITY_EMERGENCY
        | common::MavSeverity::MAV_SEVERITY_ALERT
        | common::MavSeverity::MAV_SEVERITY_CRITICAL
        | common::MavSeverity::MAV_SEVERITY_ERROR => domain::LogLevel::Error,
        common::MavSeverity::MAV_SEVERITY_WARNING => domain::LogLevel::Warn,
        common::MavSeverity::MAV_SEVERITY_NOTICE | common::MavSeverity::MAV_SEVERITY_INFO => {
            domain::LogLevel::Info
        }
        common::MavSeverity::MAV_SEVERITY_DEBUG => domain::LogLevel::Debug,
        _ => domain::LogLevel::Trace,
    }
}

fn simulated_parameters() -> Vec<domain::ParameterValue> {
    vec![
        domain::ParameterValue {
            name: "SYSID_THISMAV".into(),
            value: 1.0,
            param_type: "int32".into(),
            index: Some(0),
        },
        domain::ParameterValue {
            name: "MPC_THR_MIN".into(),
            value: 0.12,
            param_type: "float".into(),
            index: Some(1),
        },
        domain::ParameterValue {
            name: "FW_AIRSPD_TRIM".into(),
            value: 13.5,
            param_type: "float".into(),
            index: Some(2),
        },
    ]
}

fn trim_zero_terminated(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_string()
}

pub fn simulated_descriptor() -> domain::DeviceDescriptor {
    domain::DeviceDescriptor {
        id: "simulator:virtual".into(),
        label: "Software Simulator".into(),
        transport: domain::LinkKind::Simulated,
        details: domain::DeviceDetails::Simulated,
    }
}
