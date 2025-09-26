use std::collections::HashMap;
use std::time::Duration;

use once_cell::sync::Lazy;
use parking_lot::{Mutex, RwLock};
use qgc_domain as domain;
use serialport::{available_ports, SerialPortInfo, SerialPortType};
use tokio::sync::watch;

use crate::events::{self, CoreEvent};

const DEFAULT_UDP_ID: &str = "udp:14550";

struct DiscoveryState {
    devices: RwLock<HashMap<String, domain::DeviceDescriptor>>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    stop_tx: Mutex<Option<watch::Sender<bool>>>,
}

impl DiscoveryState {
    fn new() -> Self {
        Self {
            devices: RwLock::new(HashMap::new()),
            task: Mutex::new(None),
            stop_tx: Mutex::new(None),
        }
    }
}

static DISCOVERY: Lazy<DiscoveryState> = Lazy::new(DiscoveryState::new);

pub fn snapshot_devices() -> Vec<domain::DeviceDescriptor> {
    let mut devices: Vec<_> = DISCOVERY.devices.read().values().cloned().collect();

    if !devices.iter().any(|d| d.id == DEFAULT_UDP_ID) {
        devices.push(default_udp_descriptor());
    }

    devices
}

pub async fn start(interval: Duration) {
    let mut task_guard = DISCOVERY.task.lock();
    if task_guard.is_some() {
        return;
    }

    let (tx, rx) = watch::channel(false);
    *DISCOVERY.stop_tx.lock() = Some(tx);

    let handle = tokio::spawn(async move {
        let mut stop_rx = rx;
        loop {
            if let Err(error) = refresh_devices() {
                events::emit_log(
                    domain::LogLevel::Warn,
                    "discovery",
                    format!("device scan failed: {error}"),
                );
            }

            if stop_rx.has_changed().unwrap_or(false) && *stop_rx.borrow() {
                break;
            }

            tokio::select! {
                _ = stop_rx.changed() => {
                    if *stop_rx.borrow() {
                        break;
                    }
                }
                _ = tokio::time::sleep(interval) => {}
            }
        }
    });

    *task_guard = Some(handle);
}

#[allow(dead_code)]
pub async fn stop() {
    if let Some(tx) = DISCOVERY.stop_tx.lock().take() {
        let _ = tx.send(true);
    }

    if let Some(handle) = DISCOVERY.task.lock().take() {
        handle.abort();
    }
}

fn refresh_devices() -> Result<(), serialport::Error> {
    let mut current: HashMap<String, domain::DeviceDescriptor> = HashMap::new();

    for info in available_ports()? {
        let descriptor = descriptor_from_serial(&info);
        current.insert(descriptor.id.clone(), descriptor.clone());
    }

    current.insert(DEFAULT_UDP_ID.to_string(), default_udp_descriptor());

    let mut guard = DISCOVERY.devices.write();

    for (id, descriptor) in current.iter() {
        if !guard.contains_key(id) {
            events::emit(CoreEvent::DeviceDiscovered {
                device: descriptor.clone(),
            });
        }
    }

    for id in guard.keys().cloned().collect::<Vec<_>>() {
        if !current.contains_key(&id) {
            if let Some(device) = guard.remove(&id) {
                events::emit(CoreEvent::DeviceLost { device });
            }
        }
    }

    guard.extend(current.clone());

    events::emit(CoreEvent::DiscoverySnapshot {
        devices: guard.values().cloned().collect(),
    });

    Ok(())
}

fn descriptor_from_serial(info: &SerialPortInfo) -> domain::DeviceDescriptor {
    let (manufacturer, product, vid, pid) = match &info.port_type {
        SerialPortType::UsbPort(port) => (
            port.manufacturer.clone(),
            port.product.clone(),
            Some(port.vid),
            Some(port.pid),
        ),
        _ => (None, None, None, None),
    };

    domain::DeviceDescriptor {
        id: format!("serial:{}", info.port_name),
        label: info.port_name.clone(),
        transport: domain::LinkKind::Serial,
        details: domain::DeviceDetails::Serial {
            path: info.port_name.clone(),
            manufacturer,
            product,
            vid,
            pid,
        },
    }
}

fn default_udp_descriptor() -> domain::DeviceDescriptor {
    domain::DeviceDescriptor {
        id: DEFAULT_UDP_ID.to_string(),
        label: "UDP 0.0.0.0:14550".into(),
        transport: domain::LinkKind::Udp,
        details: domain::DeviceDetails::Udp {
            bind: "0.0.0.0:14550".into(),
            target_host: None,
            target_port: None,
        },
    }
}
