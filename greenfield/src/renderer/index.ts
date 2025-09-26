import type {
  DeviceDescriptor,
  ConnectionStatus,
  ParameterValue,
  VehicleStatus,
  StatusMessage
} from '../../rust-core/index';
import { appStore, type ParameterProgressState, type LogEntry } from './state/store';

type RustEnvelope = {
  level?: string;
  message?: string;
  kind?: string;
  vehicle?: VehicleStatus;
  [key: string]: unknown;
};

type CoreEvent = {
  event: string;
  [key: string]: unknown;
};

type BackendAPI = {
  onRustMessage(callback: (message: string) => void): () => void;
  invokeDiagnostics(timeoutMs?: number): Promise<StatusMessage>;
  simulateFailure(): Promise<{ level: string; kind: string; message: string }>;
  fetchVehicleStatus(): Promise<VehicleStatus>;
  listDevices(): Promise<DeviceDescriptor[]>;
  connect(options?: Record<string, unknown>): Promise<ConnectionStatus>;
  disconnect(): Promise<void>;
  fetchParameters(timeoutMs?: number): Promise<ParameterValue[]>;
  getConnectionStatus(): Promise<ConnectionStatus>;
  getCachedParameters(): Promise<ParameterValue[]>;
};

const backend = window.backend as unknown as BackendAPI;
const store = appStore;

const statusBadge = document.getElementById('status-badge');
const logContainer = document.getElementById('rust-log');
const deviceTableBody = document.getElementById('device-table');
const connectionStatusContainer = document.getElementById('connection-status');
const vehicleStatusContainer = document.getElementById('vehicle-status');
const parameterStatsContainer = document.getElementById('parameter-stats');
const parameterTableBody = document.getElementById('parameter-table-body');
const parameterEmptyState = document.getElementById('parameter-empty');
const flightModeValue = document.getElementById('health-flight-mode');
const flightModeMeta = document.getElementById('health-flight-meta');
const heartbeatValue = document.getElementById('health-heartbeat');
const heartbeatMeta = document.getElementById('health-heartbeat-meta');
const gpsValue = document.getElementById('health-gps');
const gpsMeta = document.getElementById('health-gps-meta');
const batteryValue = document.getElementById('health-battery');
const batteryMeta = document.getElementById('health-battery-meta');
const parameterFilterInput = document.getElementById('parameter-filter') as HTMLInputElement | null;
const runDiagnosticsButton = document.getElementById('run-diagnostics');
const simulateFailureButton = document.getElementById('simulate-failure');
const refreshDevicesButton = document.getElementById('refresh-devices');
const disconnectButton = document.getElementById('disconnect-link');
const fetchParametersButton = document.getElementById('fetch-parameters');

const unsubscribes: Array<() => void> = [];
let parameterFilter = '';

if (statusBadge) {
  statusBadge.textContent = 'Awaiting core signal…';
}

function init(): void {
  setupStoreSubscriptions();
  setupUiHandlers();
  setupBackendSubscriptions();
  bootstrapState();
}

function setupStoreSubscriptions(): void {
  unsubscribes.push(
    store.subscribe(
      (state) => state.devices,
      (devices) => renderDeviceTable(devices)
    )
  );

  unsubscribes.push(
    store.subscribe(
      (state) => state.connectionStatus,
      (status) => renderConnectionStatus(status)
    )
  );

  unsubscribes.push(
    store.subscribe(
      (state) => state.vehicleStatus,
      (vehicle) => {
        renderVehicleSnapshot(vehicle);
        renderHealthDashboard(vehicle);
      }
    )
  );

  unsubscribes.push(
    store.subscribe(
      (state) => state.parameterProgress,
      (progress) => renderParameterStats(progress)
    )
  );

  unsubscribes.push(
    store.subscribe(
      (state) => state.parameterValues,
      (parameters) => renderParameterTable(parameters)
    )
  );

  unsubscribes.push(
    store.subscribe(
      (state) => state.logs,
      (logs) => renderLogs(logs)
    )
  );

  const current = store.getState();
  renderDeviceTable(current.devices);
  renderConnectionStatus(current.connectionStatus);
  renderVehicleSnapshot(current.vehicleStatus);
  renderHealthDashboard(current.vehicleStatus);
  renderParameterStats(current.parameterProgress);
  renderParameterTable(current.parameterValues);
  renderLogs(current.logs);
}

function setupUiHandlers(): void {
  if (parameterFilterInput) {
    parameterFilterInput.addEventListener('input', () => {
      parameterFilter = parameterFilterInput.value.trim().toLowerCase();
      renderParameterTable(store.getState().parameterValues);
    });
  }

  if (runDiagnosticsButton) {
    runDiagnosticsButton.addEventListener('click', () => {
      void backend.invokeDiagnostics(250).catch((error) => {
        logMessage('error', `Diagnostics failed: ${(error as Error).message}`);
      });
    });
  }

  if (simulateFailureButton) {
    simulateFailureButton.addEventListener('click', () => {
      void backend.simulateFailure().catch((error) => {
        logMessage('error', `simulateFailure error: ${(error as Error).message}`);
      });
    });
  }

  if (refreshDevicesButton) {
    refreshDevicesButton.addEventListener('click', () => {
      void refreshDeviceList();
    });
  }

  if (disconnectButton) {
    disconnectButton.addEventListener('click', () => {
      void handleDisconnect();
    });
  }

  if (fetchParametersButton) {
    fetchParametersButton.addEventListener('click', () => {
      void handleFetchParameters();
    });
  }
}

function setupBackendSubscriptions(): void {
  backend.onRustMessage((payload: string) => {
    if (statusBadge) {
      statusBadge.textContent = 'Connected to rust-core';
    }

    let parsed: RustEnvelope | CoreEvent | null = null;
    try {
      parsed = JSON.parse(payload) as RustEnvelope | CoreEvent;
    } catch {
      logMessage('info', payload);
      return;
    }

    if (parsed && typeof parsed === 'object' && 'event' in parsed && typeof parsed.event === 'string') {
      handleCoreEvent(parsed as CoreEvent);
      return;
    }

    const envelope = parsed as RustEnvelope;
    if (envelope.message) {
      logMessage(envelope.level ?? 'info', envelope.message);
    }

    if (envelope.vehicle && typeof envelope.vehicle === 'object') {
      store.getState().setVehicleStatus(envelope.vehicle as VehicleStatus);
    }
  });
}

async function bootstrapState(): Promise<void> {
  await Promise.all([
    refreshDeviceList(),
    backend
      .getConnectionStatus()
      .then((status) => store.getState().setConnectionStatus(status))
      .catch((error) => {
        logMessage('warn', `Failed to load connection status: ${(error as Error).message}`);
      }),
    backend
      .fetchVehicleStatus()
      .then((status) => store.getState().setVehicleStatus(status))
      .catch((error) => {
        logMessage('warn', `Failed to bootstrap vehicle: ${(error as Error).message}`);
      }),
    backend
      .getCachedParameters()
      .then((params) => {
        if (params.length > 0) {
          store.getState().setParameterValues(params, Date.now());
        }
      })
      .catch((error) => {
        logMessage('warn', `Failed to load cached parameters: ${(error as Error).message}`);
      })
  ]);
}

async function refreshDeviceList(): Promise<void> {
  try {
    const list = await backend.listDevices();
    store.getState().setDevices(list);
  } catch (error) {
    logMessage('error', `Failed to list devices: ${(error as Error).message}`);
  }
}

async function handleConnect(deviceId: string): Promise<void> {
  try {
    const status = await backend.connect({ deviceId });
    store.getState().setConnectionStatus(status);
  } catch (error) {
    logMessage('error', `Connect failed: ${(error as Error).message}`);
  }
}

async function handleDisconnect(): Promise<void> {
  try {
    await backend.disconnect();
    const status = await backend.getConnectionStatus();
    store.getState().setConnectionStatus(status);
  } catch (error) {
    logMessage('error', `Disconnect failed: ${(error as Error).message}`);
  }
}

async function handleFetchParameters(): Promise<void> {
  try {
    const params = await backend.fetchParameters(5_000);
    store.getState().setParameterValues(params, Date.now());
    logMessage('info', `Fetched ${params.length} parameters via IPC command`);
  } catch (error) {
    logMessage('error', `fetchParameters failed: ${(error as Error).message}`);
  }
}

function handleCoreEvent(event: CoreEvent): void {
  switch (event.event) {
    case 'discovery_snapshot': {
      const snapshot = Array.isArray(event.devices) ? (event.devices as DeviceDescriptor[]) : [];
      store.getState().setDevices(snapshot);
      break;
    }
    case 'device_discovered': {
      const device = event.device as DeviceDescriptor | undefined;
      if (device) {
        store.getState().upsertDevice(device);
      }
      break;
    }
    case 'device_lost': {
      const device = event.device as DeviceDescriptor | undefined;
      if (device) {
        store.getState().removeDevice(device.id);
      }
      break;
    }
    case 'connection_status': {
      const status = event.status as ConnectionStatus | undefined;
      if (status) {
        store.getState().setConnectionStatus(status);
      }
      break;
    }
    case 'heartbeat': {
      const status = event.status as VehicleStatus | undefined;
      if (status) {
        store.getState().setVehicleStatus(status);
      }
      break;
    }
    case 'mission_log': {
      const entry = event.entry as {
        level: string;
        source: string;
        message: string;
        timestamp_millis: number;
      };
      if (entry) {
        logMessage(entry.level.toLowerCase(), `[${entry.source}] ${entry.message}`, entry.timestamp_millis);
      }
      break;
    }
    case 'diagnostics': {
      const level = (event.level as string | undefined) ?? 'debug';
      const message = (event.message as string | undefined) ?? 'diagnostic event';
      logMessage(level.toLowerCase(), message);
      break;
    }
    case 'parameter_progress': {
      const received = typeof event.received === 'number' ? event.received : store.getState().parameterProgress.received;
      const expected = typeof event.expected === 'number' ? event.expected : store.getState().parameterProgress.expected;
      store.getState().setParameterProgress(received, expected);
      break;
    }
    case 'parameter_batch': {
      const parameters = (event.parameters as ParameterValue[]) ?? [];
      store.getState().setParameterValues(parameters, Date.now());
      logMessage('info', `Parameter batch received (${parameters.length} values)`);
      break;
    }
    default: {
      logMessage('debug', `Unhandled core event: ${event.event}`);
      break;
    }
  }
}

function renderDeviceTable(devices: DeviceDescriptor[]): void {
  if (!deviceTableBody) {
    return;
  }

  deviceTableBody.innerHTML = '';
  const fragment = document.createDocumentFragment();

  devices.forEach((device) => {
    const tr = document.createElement('tr');
    const connection = store.getState().connectionStatus;
    if (connection?.device?.id === device.id) {
      tr.dataset.active = 'true';
    }

    tr.innerHTML = `
      <td>${device.id}</td>
      <td>${device.label}</td>
      <td>${device.transport}</td>
      <td>${summarizeDeviceDetails(device)}</td>
      <td class="device-actions"></td>
    `;

    const actionsCell = tr.querySelector('.device-actions');
    if (actionsCell) {
      const button = document.createElement('button');
      button.textContent = connection?.device?.id === device.id ? 'Reconnect' : 'Connect';
      button.addEventListener('click', () => {
        void handleConnect(device.id);
      });
      actionsCell.appendChild(button);
    }

    fragment.appendChild(tr);
  });

  deviceTableBody.appendChild(fragment);
}

function summarizeDeviceDetails(device: DeviceDescriptor): string {
  if (device.transport === 'Serial' && device.serialPath) {
    return device.serialPath;
  }
  if (device.transport === 'Udp' && device.udpBind) {
    return `${device.udpBind}${device.udpTargetHost ? ` → ${device.udpTargetHost}` : ''}`;
  }
  return '—';
}

function renderConnectionStatus(status: ConnectionStatus | null): void {
  if (!connectionStatusContainer) {
    return;
  }

  if (!status) {
    connectionStatusContainer.textContent = 'Idle';
    return;
  }

  const { phase, message, device } = status;
  const details = [phase, message, device?.label ?? device?.id]
    .filter((item) => typeof item === 'string' && item.length > 0)
    .join(' • ');
  connectionStatusContainer.textContent = details || 'Unknown';
}

function renderVehicleSnapshot(vehicle: VehicleStatus | null): void {
  if (!vehicleStatusContainer) {
    return;
  }

  if (!vehicle) {
    vehicleStatusContainer.textContent = 'Waiting for heartbeat…';
    return;
  }

  vehicleStatusContainer.innerHTML = `
    <div><strong>Vehicle ID:</strong> ${vehicle.vehicleId}</div>
    <div><strong>Type:</strong> ${vehicle.vehicleType}</div>
    <div><strong>Arming:</strong> ${vehicle.armingState}</div>
    <div><strong>Heartbeat:</strong> ${vehicle.heartbeatMillis || '—'} ms</div>
  `;
}

function renderHealthDashboard(vehicle: VehicleStatus | null): void {
  if (!vehicle) {
    setText(flightModeValue, '—');
    setText(flightModeMeta, 'Waiting for data');
    setText(heartbeatValue, '—');
    setText(heartbeatMeta, 'No heartbeat yet');
    setText(gpsValue, '—');
    setText(gpsMeta, 'No fix');
    setText(batteryValue, '—');
    setText(batteryMeta, 'No telemetry');
    return;
  }

  if (vehicle.flightMode) {
    setText(flightModeValue, vehicle.flightMode.label);
    setText(
      flightModeMeta,
      `Base ${vehicle.flightMode.baseMode.toString(16)} · Custom ${vehicle.flightMode.customMode}`
    );
  } else {
    setText(flightModeValue, '—');
    setText(flightModeMeta, 'Unknown');
  }

  setText(heartbeatValue, vehicle.heartbeatMillis ? `${vehicle.heartbeatMillis} ms` : '—');
  setText(heartbeatMeta, `Vehicle ${vehicle.vehicleId}`);

  if (vehicle.gps) {
    const { fixType, satellitesVisible, latitudeDeg, longitudeDeg } = normalizeGps(vehicle.gps);
    setText(gpsValue, `${fixType}`);
    setText(
      gpsMeta,
      `${satellitesVisible} sats${
        latitudeDeg && longitudeDeg ? ` • ${latitudeDeg.toFixed(5)}, ${longitudeDeg.toFixed(5)}` : ''
      }`
    );
  } else {
    setText(gpsValue, '—');
    setText(gpsMeta, 'No fix');
  }

  if (vehicle.battery) {
    setText(batteryValue, `${vehicle.battery.voltageV.toFixed(2)} V`);
    const remaining = vehicle.battery.remainingPercent;
    const current = vehicle.battery.currentA;
    setText(
      batteryMeta,
      `${remaining !== undefined ? `${remaining.toFixed(0)}%` : '—'} remaining${
        current !== undefined ? ` • ${current.toFixed(1)} A` : ''
      }`
    );
  } else {
    setText(batteryValue, '—');
    setText(batteryMeta, 'Telemetry pending');
  }
}

function normalizeGps(gps: NonNullable<VehicleStatus['gps']>): {
  fixType: string;
  satellitesVisible: number;
  latitudeDeg?: number;
  longitudeDeg?: number;
} {
  return {
    fixType: gps.fixType,
    satellitesVisible: gps.satellitesVisible,
    latitudeDeg: gps.latitudeDeg ?? undefined,
    longitudeDeg: gps.longitudeDeg ?? undefined
  };
}

function renderParameterStats(progress: ParameterProgressState): void {
  if (!parameterStatsContainer) {
    return;
  }

  const total = store.getState().parameterValues.length;
  const progressLabel = progress.expected
    ? `${progress.received}/${progress.expected}`
    : `${progress.received}`;
  const updated = progress.lastUpdated
    ? new Date(progress.lastUpdated).toLocaleTimeString()
    : 'No completed batch yet';

  parameterStatsContainer.innerHTML = `
    <div><strong>Progress:</strong> ${progressLabel}</div>
    <div><strong>Cached Parameters:</strong> ${total}</div>
    <div>${updated}</div>
  `;
}

function renderParameterTable(parameters: ParameterValue[]): void {
  if (!parameterTableBody || !parameterEmptyState) {
    return;
  }

  const normalizedFilter = parameterFilter;
  const filtered = normalizedFilter
    ? parameters.filter((param) => param.name.toLowerCase().includes(normalizedFilter))
    : parameters;

  parameterTableBody.innerHTML = '';
  parameterEmptyState.classList.toggle('hidden', filtered.length > 0);

  const fragment = document.createDocumentFragment();
  filtered.slice(0, 500).forEach((param) => {
    const tr = document.createElement('tr');
    tr.innerHTML = `
      <td>${param.index ?? '—'}</td>
      <td>${param.name}</td>
      <td>${formatParameterValue(param.value)}</td>
      <td>${param.paramType}</td>
    `;
    fragment.appendChild(tr);
  });

  parameterTableBody.appendChild(fragment);
}

function formatParameterValue(value: number): string {
  if (Number.isInteger(value)) {
    return value.toString();
  }

  return value.toFixed(3).replace(/0+$/, '').replace(/\.$/, '');
}

function renderLogs(logs: LogEntry[]): void {
  if (!logContainer) {
    return;
  }

  logContainer.innerHTML = '';
  const fragment = document.createDocumentFragment();

  logs.forEach((entry) => {
    const div = document.createElement('div');
    div.className = 'log-entry';
    div.dataset.level = entry.level;
    const timestamp = new Date(entry.timestamp).toLocaleTimeString();
    div.textContent = `[${timestamp}] ${entry.message}`;
    fragment.appendChild(div);
  });

  logContainer.appendChild(fragment);
}

function setText(element: HTMLElement | null, value: string): void {
  if (element) {
    element.textContent = value;
  }
}

function logMessage(level: string, message: string, timestampMillis?: number): void {
  const entry: LogEntry = {
    id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
    level: level.toLowerCase(),
    message,
    timestamp: timestampMillis ?? Date.now()
  };
  store.getState().appendLog(entry);
}

init();
