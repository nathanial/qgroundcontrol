use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mavlink::common::{
    self, MavCmd, MavFrame, MavMessage, MavMissionResult, MavModeFlag, MavState, MavType,
};
use mavlink::connect_async;
use num_traits::FromPrimitive;
use once_cell::sync::Lazy;
use parking_lot::{Mutex, RwLock};
use qgc_domain as domain;
use tokio::sync::{mpsc, oneshot};

use crate::error::{invalid_argument, timeout, CoreError, CoreResult};
use crate::events::{self, CoreEvent};

const DEFAULT_PARAMETER_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_MISSION_TIMEOUT: Duration = Duration::from_secs(15);

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

struct MissionUploadState {
    respond_to: oneshot::Sender<CoreResult<domain::MissionPlan>>,
    plan: domain::MissionPlan,
    previous_plan: domain::MissionPlan,
    next_seq: u16,
    total: u16,
    target_system: u8,
    target_component: u8,
}

struct MissionDownloadState {
    respond_to: oneshot::Sender<CoreResult<domain::MissionPlan>>,
    plan_id: String,
    items: Vec<domain::MissionItem>,
    expected: Option<u16>,
    next_request: u16,
    total: u16,
    target_system: u8,
    target_component: u8,
}

enum MissionTaskState {
    Upload(MissionUploadState),
    Download(MissionDownloadState),
}

struct MissionTask {
    deadline: Instant,
    state: MissionTaskState,
}

impl MissionDownloadState {
    fn as_plan(&self) -> domain::MissionPlan {
        let mut plan = domain::MissionPlan::new(self.plan_id.clone());
        plan.items = self.items.clone();
        plan
    }
}

struct SessionState {
    descriptor: domain::DeviceDescriptor,
    status: RwLock<domain::ConnectionStatus>,
    vehicle_status: RwLock<domain::VehicleStatus>,
    autopilot_ids: Mutex<Option<(u8, u8)>>,
    last_heartbeat: Mutex<Option<Instant>>,
    parameter_cache: RwLock<Vec<domain::ParameterValue>>,
    pending_parameters: Mutex<Option<PendingParameters>>,
    mission_plan: RwLock<domain::MissionPlan>,
    mission_revision: AtomicU32,
    pending_mission: Mutex<Option<MissionTask>>,
}

impl SessionState {
    fn new(descriptor: domain::DeviceDescriptor) -> Arc<Self> {
        let mut vehicle_status =
            domain::VehicleStatus::new(domain::VehicleId(descriptor.id.clone()));
        vehicle_status.vehicle_type = domain::VehicleType::Unknown;
        let plan_id = format!("mission-{}", descriptor.id);
        Arc::new(Self {
            status: RwLock::new(
                domain::ConnectionStatus::new(domain::ConnectionPhase::Connecting)
                    .with_device(descriptor.clone()),
            ),
            vehicle_status: RwLock::new(vehicle_status),
            descriptor,
            autopilot_ids: Mutex::new(None),
            last_heartbeat: Mutex::new(None),
            parameter_cache: RwLock::new(Vec::new()),
            pending_parameters: Mutex::new(None),
            mission_plan: RwLock::new(domain::MissionPlan::new(plan_id)),
            mission_revision: AtomicU32::new(0),
            pending_mission: Mutex::new(None),
        })
    }

    fn status(&self) -> domain::ConnectionStatus {
        self.status.read().clone()
    }

    fn vehicle_status(&self) -> domain::VehicleStatus {
        self.vehicle_status.read().clone()
    }

    fn update_vehicle_status<F>(&self, mut update: F)
    where
        F: FnMut(&mut domain::VehicleStatus),
    {
        let mut guard = self.vehicle_status.write();
        update(&mut guard);
        let snapshot = guard.clone();
        drop(guard);
        events::emit(CoreEvent::Heartbeat { status: snapshot });
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

    fn mission_plan(&self) -> domain::MissionPlan {
        let mut plan = self.mission_plan.read().clone();
        plan.revision = self.mission_revision.load(Ordering::SeqCst);
        plan
    }

    fn mission_revision(&self) -> u32 {
        self.mission_revision.load(Ordering::SeqCst)
    }

    fn replace_mission_plan(&self, mut plan: domain::MissionPlan) -> domain::MissionPlan {
        if plan.plan_id.is_empty() {
            plan.plan_id = self.mission_plan.read().plan_id.clone();
        }

        let sanitized = normalize_mission_plan(plan);
        let revision = self.mission_revision.fetch_add(1, Ordering::SeqCst) + 1;
        let mut stored = sanitized.clone();
        stored.revision = revision;
        stored.last_modified_millis = current_millis();
        {
            let mut guard = self.mission_plan.write();
            *guard = stored.clone();
        }
        events::emit_mission_plan(stored.clone());
        stored
    }

    fn start_mission_upload(
        &self,
        plan: domain::MissionPlan,
        timeout: Duration,
        respond_to: oneshot::Sender<CoreResult<domain::MissionPlan>>,
        target_system: u8,
        target_component: u8,
    ) -> Result<(), oneshot::Sender<CoreResult<domain::MissionPlan>>> {
        let mut guard = self.pending_mission.lock();
        if guard.is_some() {
            return Err(respond_to);
        }

        let plan = normalize_mission_plan(plan);
        let previous_plan = self.mission_plan.read().clone();
        let total = plan.items.len() as u16;

        *guard = Some(MissionTask {
            deadline: Instant::now() + timeout,
            state: MissionTaskState::Upload(MissionUploadState {
                respond_to,
                plan,
                previous_plan,
                next_seq: 0,
                total,
                target_system,
                target_component,
            }),
        });

        events::emit_mission_sync(
            domain::MissionSyncStatus::new(domain::MissionSyncStage::Uploading)
                .with_progress(Some(0), Some(total)),
        );

        Ok(())
    }

    fn start_mission_download(
        &self,
        timeout: Duration,
        respond_to: oneshot::Sender<CoreResult<domain::MissionPlan>>,
        target_system: u8,
        target_component: u8,
    ) -> Result<(), oneshot::Sender<CoreResult<domain::MissionPlan>>> {
        let mut guard = self.pending_mission.lock();
        if guard.is_some() {
            return Err(respond_to);
        }

        let plan_id = format!("mission-{}-dl", current_millis());

        *guard = Some(MissionTask {
            deadline: Instant::now() + timeout,
            state: MissionTaskState::Download(MissionDownloadState {
                respond_to,
                plan_id,
                items: Vec::new(),
                expected: None,
                next_request: 0,
                total: 0,
                target_system,
                target_component,
            }),
        });

        events::emit_mission_sync(
            domain::MissionSyncStatus::new(domain::MissionSyncStage::Downloading)
                .with_progress(Some(0), None),
        );

        Ok(())
    }

    fn mission_task_mut<R>(&self, f: impl FnOnce(&mut MissionTask) -> R) -> Option<R> {
        let mut guard = self.pending_mission.lock();
        guard.as_mut().map(f)
    }

    fn take_mission_task(&self) -> Option<MissionTask> {
        self.pending_mission.lock().take()
    }

    fn mission_task_expire_if_needed(&self) {
        let expired = {
            let mut guard = self.pending_mission.lock();
            if guard
                .as_ref()
                .map(|task| Instant::now() > task.deadline)
                .unwrap_or(false)
            {
                guard.take()
            } else {
                None
            }
        };

        if let Some(task) = expired {
            self.resolve_mission_failure(task, timeout(DEFAULT_MISSION_TIMEOUT));
        }
    }

    fn resolve_mission_success(&self, task: MissionTask, mut plan: domain::MissionPlan) {
        match task.state {
            MissionTaskState::Upload(upload) => {
                plan = normalize_mission_plan(plan);
                let stored = self.replace_mission_plan(plan);
                events::emit_mission_sync(
                    domain::MissionSyncStatus::new(domain::MissionSyncStage::Completed)
                        .with_progress(Some(upload.total), Some(upload.total)),
                );
                events::emit_mission_operation(domain::MissionOperationReport {
                    operation: domain::MissionOperationKind::Upload,
                    status: domain::MissionOperationStatus::Success,
                    message: Some("Mission upload acknowledged".into()),
                    revision: stored.revision,
                });
                let _ = upload.respond_to.send(Ok(stored));
            }
            MissionTaskState::Download(download) => {
                if plan.plan_id.is_empty() {
                    plan.plan_id = download.plan_id.clone();
                }
                plan = normalize_mission_plan(plan);
                let stored = self.replace_mission_plan(plan);
                events::emit_mission_sync(
                    domain::MissionSyncStatus::new(domain::MissionSyncStage::Completed)
                        .with_progress(Some(download.total), Some(download.total)),
                );
                events::emit_mission_operation(domain::MissionOperationReport {
                    operation: domain::MissionOperationKind::Download,
                    status: domain::MissionOperationStatus::Success,
                    message: Some("Mission download complete".into()),
                    revision: stored.revision,
                });
                let _ = download.respond_to.send(Ok(stored));
            }
        }
    }

    fn resolve_mission_failure(&self, task: MissionTask, error: CoreError) {
        let message = error.to_string();
        match task.state {
            MissionTaskState::Upload(upload) => {
                *self.mission_plan.write() = upload.previous_plan.clone();
                events::emit_mission_plan(self.mission_plan());
                events::emit_mission_sync(
                    domain::MissionSyncStatus::new(domain::MissionSyncStage::Failed)
                        .with_message(message.clone()),
                );
                events::emit_mission_operation(domain::MissionOperationReport {
                    operation: domain::MissionOperationKind::Upload,
                    status: domain::MissionOperationStatus::Failed,
                    message: Some(message.clone()),
                    revision: self.mission_revision(),
                });
                let _ = upload.respond_to.send(Err(error));
            }
            MissionTaskState::Download(download) => {
                events::emit_mission_sync(
                    domain::MissionSyncStatus::new(domain::MissionSyncStage::Failed)
                        .with_message(message.clone()),
                );
                events::emit_mission_operation(domain::MissionOperationReport {
                    operation: domain::MissionOperationKind::Download,
                    status: domain::MissionOperationStatus::Failed,
                    message: Some(message.clone()),
                    revision: self.mission_revision(),
                });
                let _ = download.respond_to.send(Err(error));
            }
        }
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
    UploadMission {
        plan: domain::MissionPlan,
        respond_to: oneshot::Sender<CoreResult<domain::MissionPlan>>,
        timeout: Duration,
    },
    DownloadMission {
        respond_to: oneshot::Sender<CoreResult<domain::MissionPlan>>,
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

    fn vehicle_status(&self) -> domain::VehicleStatus {
        self.state.vehicle_status()
    }

    fn parameter_cache(&self) -> Vec<domain::ParameterValue> {
        self.state.parameter_cache()
    }

    fn mission_plan(&self) -> domain::MissionPlan {
        self.state.mission_plan()
    }

    fn mission_revision(&self) -> u32 {
        self.state.mission_revision()
    }

    async fn download_mission(&self, timeout_budget: Duration) -> CoreResult<domain::MissionPlan> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(SessionCommand::DownloadMission {
                respond_to: tx,
                timeout: timeout_budget,
            })
            .await
            .map_err(|_| invalid_argument("connection closed"))?;

        let response = tokio::time::timeout(timeout_budget + Duration::from_secs(1), rx)
            .await
            .map_err(|_| timeout(timeout_budget))?;

        response.map_err(|_| invalid_argument("connection closed"))?
    }

    async fn upload_mission(
        &self,
        plan: domain::MissionPlan,
        timeout_budget: Duration,
    ) -> CoreResult<domain::MissionPlan> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(SessionCommand::UploadMission {
                plan,
                respond_to: tx,
                timeout: timeout_budget,
            })
            .await
            .map_err(|_| invalid_argument("connection closed"))?;

        let response = tokio::time::timeout(timeout_budget + Duration::from_secs(1), rx)
            .await
            .map_err(|_| timeout(timeout_budget))?;

        response.map_err(|_| invalid_argument("connection closed"))?
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

    pub fn vehicle_status(&self) -> domain::VehicleStatus {
        self.session
            .lock()
            .as_ref()
            .map(|session| session.vehicle_status())
            .unwrap_or_default()
    }

    pub fn cached_parameters(&self) -> Vec<domain::ParameterValue> {
        self.session
            .lock()
            .as_ref()
            .map(|session| session.parameter_cache())
            .unwrap_or_default()
    }

    pub fn mission_plan(&self) -> domain::MissionPlan {
        self.session
            .lock()
            .as_ref()
            .map(|session| session.mission_plan())
            .unwrap_or_else(|| domain::MissionPlan::new("mission-idle"))
    }

    pub fn cached_mission_plan(&self) -> domain::MissionPlan {
        self.mission_plan()
    }

    pub async fn download_mission(
        &self,
        timeout_override: Option<Duration>,
    ) -> CoreResult<domain::MissionPlan> {
        let timeout = timeout_override.unwrap_or(DEFAULT_MISSION_TIMEOUT);
        let session = self
            .session
            .lock()
            .as_ref()
            .cloned()
            .ok_or_else(|| invalid_argument("no active MAVLink session"))?;

        session.download_mission(timeout).await
    }

    pub async fn upload_mission(
        &self,
        mut plan: domain::MissionPlan,
        timeout_override: Option<Duration>,
    ) -> CoreResult<domain::MissionPlan> {
        let timeout = timeout_override.unwrap_or(DEFAULT_MISSION_TIMEOUT);
        let session = self
            .session
            .lock()
            .as_ref()
            .cloned()
            .ok_or_else(|| invalid_argument("no active MAVLink session"))?;

        let current_revision = session.mission_revision();
        if plan.revision != 0 && plan.revision != current_revision {
            return Err(invalid_argument(format!(
                "plan revision mismatch: expected {}, got {}",
                current_revision, plan.revision
            )));
        }

        if plan.plan_id.is_empty() {
            plan.plan_id = format!("mission-upload-{}", current_millis());
        }

        session.upload_mission(plan, timeout).await
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
                        SessionCommand::DownloadMission { respond_to, .. } => {
                            let plan = state.mission_plan();
                            events::emit_mission_sync(
                                domain::MissionSyncStatus::new(domain::MissionSyncStage::Downloading)
                                    .with_progress(Some(plan.items.len() as u16), Some(plan.items.len() as u16))
                                    .with_message("simulated mission ready"),
                            );
                            events::emit_mission_plan(plan.clone());
                            events::emit_mission_operation(domain::MissionOperationReport {
                                operation: domain::MissionOperationKind::Download,
                                status: domain::MissionOperationStatus::Success,
                                message: Some("Simulated mission provided".into()),
                                revision: plan.revision,
                            });
                            let _ = respond_to.send(Ok(plan));
                        }
                        SessionCommand::UploadMission { plan, respond_to, .. } => {
                            let plan = normalize_mission_plan(plan);
                            let plan = state.replace_mission_plan(plan);
                            events::emit_mission_plan(plan.clone());
                            events::emit_mission_sync(
                                domain::MissionSyncStatus::new(domain::MissionSyncStage::Completed)
                                    .with_progress(Some(plan.items.len() as u16), Some(plan.items.len() as u16))
                                    .with_message("simulated mission updated"),
                            );
                            events::emit_mission_operation(domain::MissionOperationReport {
                                operation: domain::MissionOperationKind::Upload,
                                status: domain::MissionOperationStatus::Success,
                                message: Some("Simulated mission accepted".into()),
                                revision: plan.revision,
                            });
                            let _ = respond_to.send(Ok(plan));
                        }
                    },
                    _ = heartbeat_interval.tick() => {
                        counter = counter.wrapping_add(1);
                        let heartbeat_ms = state.record_heartbeat_interval().max(500);
                        let percentage = (100.0 - (counter as f32 * 0.2)).clamp(5.0, 100.0);
                        let base_mode = (
                            common::MavModeFlag::MAV_MODE_FLAG_GUIDED_ENABLED.bits()
                                | common::MavModeFlag::MAV_MODE_FLAG_CUSTOM_MODE_ENABLED.bits()
                        ) as u8;

                        state.update_vehicle_status(|status| {
                            status.vehicle_id = domain::VehicleId("SIM-01".into());
                            status.vehicle_type = domain::VehicleType::Multirotor;
                            status.arming_state = domain::ArmingState::Disarmed;
                            status.heartbeat_millis = heartbeat_ms;
                            status.flight_mode = Some(domain::FlightMode {
                                label: "Simulated Hold".into(),
                                base_mode,
                                custom_mode: 0,
                            });
                            status.battery = Some(domain::BatteryStatus {
                                voltage_v: 11.4,
                                current_a: Some(2.1),
                                remaining_percent: Some(percentage),
                            });
                            status.gps = Some(domain::GpsStatus {
                                fix_type: domain::GpsFixType::Fix3D,
                                satellites_visible: 12,
                                latitude_deg: Some(37.334_5 + (counter as f64 * 0.000_001)),
                                longitude_deg: Some(-121.894_9 - (counter as f64 * 0.000_001)),
                                altitude_m: Some(15.0),
                                hdop: Some(0.9),
                                vdop: Some(1.1),
                            });
                        });

                        if counter % 5 == 0 {
                            events::emit(CoreEvent::Diagnostics {
                                level: domain::LogLevel::Info,
                                message: "simulated telemetry ok".into(),
                            });
                        }
                    }
                }

                state.mission_task_expire_if_needed();
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
                        SessionCommand::DownloadMission { respond_to, timeout } => {
                            let (target_system, target_component) = state.autopilot_ids();
                            match state.start_mission_download(timeout, respond_to, target_system, target_component) {
                                Ok(()) => {
                                    let request = MavMessage::MISSION_REQUEST_LIST(
                                        common::MISSION_REQUEST_LIST_DATA {
                                            target_system,
                                            target_component,
                                        },
                                    );
                                    if let Err(err) = connection.send_default(&request).await {
                                        if let Some(task) = state.take_mission_task() {
                                            state.resolve_mission_failure(
                                                task,
                                                CoreError::Other(anyhow::anyhow!(err.to_string())),
                                            );
                                        }
                                    }
                                }
                                Err(responder) => {
                                    let _ = responder.send(Err(invalid_argument("mission operation already running")));
                                }
                            }
                        }
                        SessionCommand::UploadMission { plan, respond_to, timeout } => {
                            let (target_system, target_component) = state.autopilot_ids();
                            let total = plan.items.len() as u16;
                            match state.start_mission_upload(plan, timeout, respond_to, target_system, target_component) {
                                Ok(()) => {
                                    let clear = MavMessage::MISSION_CLEAR_ALL(
                                        common::MISSION_CLEAR_ALL_DATA {
                                            target_system,
                                            target_component,
                                        },
                                    );
                                    if let Err(err) = connection.send_default(&clear).await {
                                        if let Some(task) = state.take_mission_task() {
                                            state.resolve_mission_failure(
                                                task,
                                                CoreError::Other(anyhow::anyhow!(err.to_string())),
                                            );
                                        }
                                        continue;
                                    }

                                    let count = MavMessage::MISSION_COUNT(
                                        common::MISSION_COUNT_DATA {
                                            target_system,
                                            target_component,
                                            count: total,
                                        },
                                    );
                                    if let Err(err) = connection.send_default(&count).await {
                                        if let Some(task) = state.take_mission_task() {
                                            state.resolve_mission_failure(
                                                task,
                                                CoreError::Other(anyhow::anyhow!(err.to_string())),
                                            );
                                        }
                                    }
                                }
                                Err(responder) => {
                                    let _ = responder.send(Err(invalid_argument("mission operation already running")));
                                }
                            }
                        }
                    },
                    result = connection.recv() => match result {
                        Ok((header, message)) => {
                            let outgoing = handle_mavlink_message(&state, header, message);
                            for msg in outgoing {
                                if let Err(err) = connection.send_default(&msg).await {
                                    state.set_status(
                                        domain::ConnectionPhase::Error,
                                        Some(format!("mission send error: {err}")),
                                    );
                                    if let Some(task) = state.take_mission_task() {
                                        state.resolve_mission_failure(
                                            task,
                                            CoreError::Other(anyhow::anyhow!(err.to_string())),
                                        );
                                    }
                                    break;
                                }
                            }
                        }
                        Err(err) => {
                            state.set_status(domain::ConnectionPhase::Error, Some(format!("recv error: {err}")));
                            state.fail_parameter_request(CoreError::Other(anyhow::anyhow!(err.to_string())));
                            break;
                        }
                    }
                }

                state.expire_parameter_request_if_needed();
                state.mission_task_expire_if_needed();
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
                        SessionCommand::DownloadMission { respond_to, timeout } => {
                            let (target_system, target_component) = state.autopilot_ids();
                            match state.start_mission_download(timeout, respond_to, target_system, target_component) {
                                Ok(()) => {
                                    let request = MavMessage::MISSION_REQUEST_LIST(
                                        common::MISSION_REQUEST_LIST_DATA {
                                            target_system,
                                            target_component,
                                        },
                                    );
                                    if let Err(err) = connection.send_default(&request).await {
                                        if let Some(task) = state.take_mission_task() {
                                            state.resolve_mission_failure(
                                                task,
                                                CoreError::Other(anyhow::anyhow!(err.to_string())),
                                            );
                                        }
                                    }
                                }
                                Err(responder) => {
                                    let _ = responder.send(Err(invalid_argument("mission operation already running")));
                                }
                            }
                        }
                        SessionCommand::UploadMission { plan, respond_to, timeout } => {
                            let (target_system, target_component) = state.autopilot_ids();
                            let total = plan.items.len() as u16;
                            match state.start_mission_upload(plan, timeout, respond_to, target_system, target_component) {
                                Ok(()) => {
                                    let clear = MavMessage::MISSION_CLEAR_ALL(
                                        common::MISSION_CLEAR_ALL_DATA {
                                            target_system,
                                            target_component,
                                        },
                                    );
                                    if let Err(err) = connection.send_default(&clear).await {
                                        if let Some(task) = state.take_mission_task() {
                                            state.resolve_mission_failure(
                                                task,
                                                CoreError::Other(anyhow::anyhow!(err.to_string())),
                                            );
                                        }
                                        continue;
                                    }

                                    let count = MavMessage::MISSION_COUNT(
                                        common::MISSION_COUNT_DATA {
                                            target_system,
                                            target_component,
                                            count: total,
                                        },
                                    );
                                    if let Err(err) = connection.send_default(&count).await {
                                        if let Some(task) = state.take_mission_task() {
                                            state.resolve_mission_failure(
                                                task,
                                                CoreError::Other(anyhow::anyhow!(err.to_string())),
                                            );
                                        }
                                    }
                                }
                                Err(responder) => {
                                    let _ = responder.send(Err(invalid_argument("mission operation already running")));
                                }
                            }
                        }
                    },
                    result = connection.recv() => match result {
                        Ok((header, message)) => {
                            let outgoing = handle_mavlink_message(&state, header, message);
                            for msg in outgoing {
                                if let Err(err) = connection.send_default(&msg).await {
                                    state.set_status(
                                        domain::ConnectionPhase::Error,
                                        Some(format!("mission send error: {err}")),
                                    );
                                    if let Some(task) = state.take_mission_task() {
                                        state.resolve_mission_failure(
                                            task,
                                            CoreError::Other(anyhow::anyhow!(err.to_string())),
                                        );
                                    }
                                    break;
                                }
                            }
                        }
                        Err(err) => {
                            state.set_status(domain::ConnectionPhase::Error, Some(format!("serial recv error: {err}")));
                            state.fail_parameter_request(CoreError::Other(anyhow::anyhow!(err.to_string())));
                            break;
                        }
                    }
                }

                state.expire_parameter_request_if_needed();
                state.mission_task_expire_if_needed();
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
) -> Vec<MavMessage> {
    enum DownloadAction {
        Request {
            seq: u16,
            target_system: u8,
            target_component: u8,
            total: u16,
        },
        Complete,
    }

    enum UploadAction {
        SendItem {
            message: common::MISSION_ITEM_INT_DATA,
            sent: u16,
            total: u16,
        },
    }

    let mut download_action: Option<DownloadAction> = None;
    let mut upload_action: Option<UploadAction> = None;
    let mut finalize_download = false;
    let mut finalize_upload: Option<Result<(), CoreError>> = None;

    let mut outgoing = Vec::new();
    match message {
        MavMessage::HEARTBEAT(data) => {
            state.set_autopilot_ids(header.system_id, header.component_id);
            state.set_status(
                domain::ConnectionPhase::Connected,
                Some("heartbeat received".into()),
            );

            let heartbeat_interval = state.record_heartbeat_interval();
            let heartbeat_ms = heartbeat_interval.max(100);

            state.update_vehicle_status(|status| {
                status.vehicle_id = domain::VehicleId(format!("SYS-{}", header.system_id));
                status.vehicle_type = map_vehicle_type(data.mavtype);
                status.arming_state = map_arming_state(data.system_status);
                status.heartbeat_millis = heartbeat_ms;
                status.flight_mode = Some(describe_flight_mode(data.base_mode, data.custom_mode));
            });
        }
        MavMessage::STATUSTEXT(data) => {
            let severity = map_statustext_severity(data.severity);
            let message = trim_zero_terminated(&data.text);
            events::emit_log(severity, "mavlink", message);
        }
        MavMessage::SYS_STATUS(data) => {
            let battery = battery_from_sys_status(&data);
            state.update_vehicle_status(|status| {
                status.battery = Some(battery.clone());
            });
        }
        MavMessage::GPS_RAW_INT(data) => {
            if let Some(gps) = gps_from_raw_int(&data) {
                state.update_vehicle_status(|status| {
                    status.gps = Some(gps.clone());
                });
            }
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
        MavMessage::MISSION_COUNT(data) => {
            let count = data.count as u16;
            let action = state
                .mission_task_mut(|task| {
                    if let MissionTaskState::Download(download) = &mut task.state {
                        download.expected = Some(count);
                        download.total = count;
                        download.next_request = 0;
                        events::emit_mission_sync(
                            domain::MissionSyncStatus::new(domain::MissionSyncStage::Downloading)
                                .with_progress(Some(0), Some(count)),
                        );
                        Some(DownloadAction::Request {
                            seq: 0,
                            target_system: download.target_system,
                            target_component: download.target_component,
                            total: count,
                        })
                    } else {
                        None
                    }
                })
                .flatten();

            match action {
                Some(DownloadAction::Request {
                    seq,
                    target_system,
                    target_component,
                    total,
                }) => {
                    if total == 0 {
                        finalize_download = true;
                    } else {
                        download_action = Some(DownloadAction::Request {
                            seq,
                            target_system,
                            target_component,
                            total,
                        });
                    }
                }
                _ => {}
            }
        }
        MavMessage::MISSION_ITEM_INT(data) => {
            let item = mission_item_from_message(&data);
            let action = state
                .mission_task_mut(|task| {
                    if let MissionTaskState::Download(download) = &mut task.state {
                        download.items.push(item.clone());
                        let received = download.items.len() as u16;
                        let total = download.expected.unwrap_or(download.total);
                        events::emit_mission_sync(
                            domain::MissionSyncStatus::new(domain::MissionSyncStage::Downloading)
                                .with_progress(Some(received.min(total)), Some(total)),
                        );
                        if total > 0 && received >= total {
                            Some(DownloadAction::Complete)
                        } else {
                            let next = received;
                            Some(DownloadAction::Request {
                                seq: next,
                                target_system: download.target_system,
                                target_component: download.target_component,
                                total,
                            })
                        }
                    } else {
                        None
                    }
                })
                .flatten();

            match action {
                Some(DownloadAction::Request {
                    seq,
                    target_system,
                    target_component,
                    total,
                }) => {
                    download_action = Some(DownloadAction::Request {
                        seq,
                        target_system,
                        target_component,
                        total,
                    });
                }
                Some(DownloadAction::Complete) => {
                    finalize_download = true;
                }
                None => {}
            }
        }
        MavMessage::MISSION_REQUEST(data) => {
            let seq = data.seq;
            upload_action = state
                .mission_task_mut(|task| {
                    if let MissionTaskState::Upload(upload) = &mut task.state {
                        if let Some(item) = upload.plan.items.get(seq as usize) {
                            let message = mission_item_to_message(
                                item,
                                upload.target_system,
                                upload.target_component,
                            );
                            upload.next_seq = seq + 1;
                            Some(UploadAction::SendItem {
                                message,
                                sent: upload.next_seq,
                                total: upload.total,
                            })
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .flatten();
        }
        MavMessage::MISSION_REQUEST_INT(data) => {
            let seq = data.seq;
            upload_action = state
                .mission_task_mut(|task| {
                    if let MissionTaskState::Upload(upload) = &mut task.state {
                        if let Some(item) = upload.plan.items.get(seq as usize) {
                            let message = mission_item_to_message(
                                item,
                                upload.target_system,
                                upload.target_component,
                            );
                            upload.next_seq = seq + 1;
                            Some(UploadAction::SendItem {
                                message,
                                sent: upload.next_seq,
                                total: upload.total,
                            })
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .flatten();
        }
        MavMessage::MISSION_ACK(data) => {
            let result = match data.mavtype {
                MavMissionResult::MAV_MISSION_ACCEPTED => Ok(()),
                other => Err(CoreError::Other(anyhow::anyhow!(format!(
                    "mission rejected with code {:?}",
                    other
                )))),
            };
            finalize_upload = Some(result);
        }
        _ => {}
    }

    if let Some(DownloadAction::Request {
        seq,
        target_system,
        target_component,
        total,
    }) = download_action
    {
        if total == 0 {
            finalize_download = true;
        } else {
            outgoing.push(MavMessage::MISSION_REQUEST_INT(
                common::MISSION_REQUEST_INT_DATA {
                    target_system,
                    target_component,
                    seq,
                },
            ));
        }
    }

    if let Some(UploadAction::SendItem {
        message,
        sent,
        total,
    }) = upload_action
    {
        outgoing.push(MavMessage::MISSION_ITEM_INT(message));
        events::emit_mission_sync(
            domain::MissionSyncStatus::new(domain::MissionSyncStage::Uploading)
                .with_progress(Some(sent.min(total)), Some(total)),
        );
    }

    if finalize_download {
        if let Some(task) = state.take_mission_task() {
            let plan = match &task.state {
                MissionTaskState::Download(download) => download.as_plan(),
                _ => domain::MissionPlan::new("mission-empty"),
            };
            state.resolve_mission_success(task, plan);
        }
    }

    if let Some(result) = finalize_upload {
        if let Some(task) = state.take_mission_task() {
            match result {
                Ok(()) => {
                    let plan_override = match &task.state {
                        MissionTaskState::Upload(upload) => Some(upload.plan.clone()),
                        _ => None,
                    };

                    if let Some(plan) = plan_override {
                        state.resolve_mission_success(task, plan);
                    } else {
                        state.resolve_mission_failure(
                            task,
                            CoreError::Other(anyhow::anyhow!(
                                "mission ack received without upload context"
                            )),
                        );
                    }
                }
                Err(error) => {
                    state.resolve_mission_failure(task, error);
                }
            }
        }
    }

    outgoing
}

fn current_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn mission_item_from_message(data: &common::MISSION_ITEM_INT_DATA) -> domain::MissionItem {
    domain::MissionItem {
        seq: data.seq,
        command: (data.command as u32) as u16,
        frame: mission_frame_from_mav(data.frame),
        latitude_deg: (data.x as f64) / 1e7,
        longitude_deg: (data.y as f64) / 1e7,
        altitude_m: data.z,
        param1: data.param1,
        param2: data.param2,
        param3: data.param3,
        param4: data.param4,
        auto_continue: data.autocontinue != 0,
        is_current: data.current != 0,
    }
}

fn mission_item_to_message(
    item: &domain::MissionItem,
    target_system: u8,
    target_component: u8,
) -> common::MISSION_ITEM_INT_DATA {
    let command = MavCmd::from_u32(item.command as u32).unwrap_or(MavCmd::MAV_CMD_NAV_WAYPOINT);
    common::MISSION_ITEM_INT_DATA {
        param1: item.param1,
        param2: item.param2,
        param3: item.param3,
        param4: item.param4,
        x: (normalize_latitude(item.latitude_deg) * 1e7).round() as i32,
        y: (normalize_longitude(item.longitude_deg) * 1e7).round() as i32,
        z: item.altitude_m,
        seq: item.seq,
        frame: mission_frame_to_mav(item.frame.clone()),
        current: if item.is_current { 1 } else { 0 },
        autocontinue: if item.auto_continue { 1 } else { 0 },
        command,
        target_system,
        target_component,
    }
}

fn mission_frame_to_mav(frame: domain::MissionFrame) -> MavFrame {
    match frame {
        domain::MissionFrame::Global => MavFrame::MAV_FRAME_GLOBAL,
        domain::MissionFrame::GlobalRelativeAlt => MavFrame::MAV_FRAME_GLOBAL_RELATIVE_ALT,
        domain::MissionFrame::GlobalTerrainAlt => MavFrame::MAV_FRAME_GLOBAL_TERRAIN_ALT,
        domain::MissionFrame::Mission => MavFrame::MAV_FRAME_MISSION,
    }
}

fn mission_frame_from_mav(value: MavFrame) -> domain::MissionFrame {
    match value {
        MavFrame::MAV_FRAME_GLOBAL => domain::MissionFrame::Global,
        MavFrame::MAV_FRAME_GLOBAL_TERRAIN_ALT => domain::MissionFrame::GlobalTerrainAlt,
        MavFrame::MAV_FRAME_MISSION => domain::MissionFrame::Mission,
        _ => domain::MissionFrame::GlobalRelativeAlt,
    }
}

fn normalize_mission_plan(mut plan: domain::MissionPlan) -> domain::MissionPlan {
    for (idx, item) in plan.items.iter_mut().enumerate() {
        item.seq = idx as u16;
        item.latitude_deg = normalize_latitude(item.latitude_deg);
        item.longitude_deg = normalize_longitude(item.longitude_deg);
        item.is_current = idx == 0;
        if !item.auto_continue {
            item.auto_continue = true;
        }
    }
    plan.last_modified_millis = current_millis();
    plan
}

fn normalize_latitude(value: f64) -> f64 {
    value.clamp(-90.0, 90.0)
}

fn normalize_longitude(value: f64) -> f64 {
    let mut lon = value;
    while lon > 180.0 {
        lon -= 360.0;
    }
    while lon < -180.0 {
        lon += 360.0;
    }
    lon
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

fn describe_flight_mode(base_mode: MavModeFlag, custom_mode: u32) -> domain::FlightMode {
    let base_bits = base_mode.bits();
    let mut labels: Vec<&str> = Vec::new();

    if base_bits & MavModeFlag::MAV_MODE_FLAG_AUTO_ENABLED.bits() != 0 {
        labels.push("Auto");
    }
    if base_bits & MavModeFlag::MAV_MODE_FLAG_GUIDED_ENABLED.bits() != 0 {
        labels.push("Guided");
    }
    if base_bits & MavModeFlag::MAV_MODE_FLAG_STABILIZE_ENABLED.bits() != 0 {
        labels.push("Stabilized");
    }
    if base_bits & MavModeFlag::MAV_MODE_FLAG_MANUAL_INPUT_ENABLED.bits() != 0 {
        labels.push("Manual");
    }
    if base_bits & MavModeFlag::MAV_MODE_FLAG_HIL_ENABLED.bits() != 0 {
        labels.push("HIL");
    }
    if labels.is_empty() {
        labels.push("Unknown");
    }

    if base_bits & MavModeFlag::MAV_MODE_FLAG_CUSTOM_MODE_ENABLED.bits() != 0 && custom_mode != 0 {
        labels.push("Custom");
    }

    let mut label = labels.join(" · ");
    if base_bits & MavModeFlag::MAV_MODE_FLAG_CUSTOM_MODE_ENABLED.bits() != 0 {
        label = format!("{} ({:#X})", label, custom_mode);
    }

    domain::FlightMode {
        label,
        base_mode: (base_bits & 0xFF) as u8,
        custom_mode,
    }
}

fn battery_from_sys_status(data: &common::SYS_STATUS_DATA) -> domain::BatteryStatus {
    let voltage_v = (data.voltage_battery as f32) / 1000.0;
    let current_a = if data.current_battery < 0 {
        None
    } else {
        Some(data.current_battery as f32 / 100.0)
    };
    let remaining_percent = if data.battery_remaining < 0 {
        None
    } else {
        Some(data.battery_remaining as f32)
    };

    domain::BatteryStatus {
        voltage_v,
        current_a,
        remaining_percent,
    }
}

fn gps_from_raw_int(data: &common::GPS_RAW_INT_DATA) -> Option<domain::GpsStatus> {
    let latitude_deg = if data.lat == 0 {
        None
    } else {
        Some(data.lat as f64 / 1e7)
    };
    let longitude_deg = if data.lon == 0 {
        None
    } else {
        Some(data.lon as f64 / 1e7)
    };
    let altitude_m = if data.alt == 0 {
        None
    } else {
        Some(data.alt as f64 / 1000.0)
    };

    let hdop = if data.eph == u16::MAX {
        None
    } else {
        Some(data.eph as f32 / 100.0)
    };
    let vdop = if data.epv == u16::MAX {
        None
    } else {
        Some(data.epv as f32 / 100.0)
    };

    let status = domain::GpsStatus {
        fix_type: map_gps_fix_type(data.fix_type as u8),
        satellites_visible: data.satellites_visible,
        latitude_deg,
        longitude_deg,
        altitude_m,
        hdop,
        vdop,
    };

    Some(status)
}

fn map_gps_fix_type<T>(value: T) -> domain::GpsFixType
where
    T: Into<u8>,
{
    match value.into() {
        0 | 1 => domain::GpsFixType::NoFix,
        2 => domain::GpsFixType::Fix2D,
        3 => domain::GpsFixType::Fix3D,
        4 => domain::GpsFixType::DGps,
        5 => domain::GpsFixType::RtkFloat,
        6 => domain::GpsFixType::RtkFixed,
        7 => domain::GpsFixType::StaticHold,
        8 => domain::GpsFixType::DeadReckoning,
        _ => domain::GpsFixType::Other,
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
