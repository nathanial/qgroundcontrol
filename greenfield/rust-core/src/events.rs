use std::time::{SystemTime, UNIX_EPOCH};

use once_cell::sync::Lazy;
use parking_lot::RwLock;
use qgc_domain as domain;
use serde::Serialize;

use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi::{Env, JsFunction, Result as NapiResult};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum CoreEvent {
    DeviceDiscovered {
        device: domain::DeviceDescriptor,
    },
    DeviceLost {
        device: domain::DeviceDescriptor,
    },
    DiscoverySnapshot {
        devices: Vec<domain::DeviceDescriptor>,
    },
    ConnectionStatus {
        status: domain::ConnectionStatus,
    },
    Heartbeat {
        status: domain::VehicleStatus,
    },
    MissionLog {
        entry: domain::MissionLogEntry,
    },
    ParameterBatch {
        parameters: Vec<domain::ParameterValue>,
    },
    ParameterProgress {
        received: usize,
        expected: Option<usize>,
    },
    Diagnostics {
        level: domain::LogLevel,
        message: String,
    },
    MissionPlan {
        plan: domain::MissionPlan,
    },
    MissionSync {
        status: domain::MissionSyncStatus,
    },
    MissionOperation {
        report: domain::MissionOperationReport,
    },
}

static EVENT_SINK: Lazy<RwLock<Option<ThreadsafeFunction<serde_json::Value>>>> =
    Lazy::new(|| RwLock::new(None));

pub fn register_sink(callback: JsFunction) -> NapiResult<()> {
    let tsfn: ThreadsafeFunction<serde_json::Value> =
        callback.create_threadsafe_function(0, |ctx| {
            let env: Env = ctx.env;
            let js_value = env.to_js_value(&ctx.value)?;
            Ok(vec![js_value])
        })?;

    let mut guard = EVENT_SINK.write();
    *guard = Some(tsfn);

    Ok(())
}

pub fn emit(event: CoreEvent) {
    if let Ok(value) = serde_json::to_value(event) {
        if let Some(tsfn) = EVENT_SINK.read().as_ref() {
            let _ = tsfn.call(Ok(value), ThreadsafeFunctionCallMode::NonBlocking);
        }
    }
}

pub fn emit_log(level: domain::LogLevel, source: &str, message: impl Into<String>) {
    let entry = domain::MissionLogEntry {
        timestamp_millis: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64,
        level: level.clone(),
        source: source.to_string(),
        message: message.into(),
    };

    emit(CoreEvent::MissionLog { entry });
}

pub fn emit_mission_plan(plan: domain::MissionPlan) {
    emit(CoreEvent::MissionPlan { plan });
}

pub fn emit_mission_sync(status: domain::MissionSyncStatus) {
    emit(CoreEvent::MissionSync { status });
}

pub fn emit_mission_operation(report: domain::MissionOperationReport) {
    emit(CoreEvent::MissionOperation { report });
}
