import type {
  DeviceDescriptor,
  ConnectionStatus,
  ParameterValue,
  VehicleStatus,
  StatusMessage
} from '../../rust-core/index';

type RustEnvelope = {
  level?: string;
  message?: string;
  kind?: string;
  event?: string;
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

const statusBadge = document.getElementById('status-badge');
const logContainer = document.getElementById('rust-log');
const vehicleStatusContainer = document.getElementById('vehicle-status');
const connectionStatusContainer = document.getElementById('connection-status');
const parameterStatsContainer = document.getElementById('parameter-stats');
const runDiagnosticsButton = document.getElementById('run-diagnostics');
const simulateFailureButton = document.getElementById('simulate-failure');
const refreshDevicesButton = document.getElementById('refresh-devices');
const disconnectButton = document.getElementById('disconnect-link');
const fetchParametersButton = document.getElementById('fetch-parameters');
const deviceTableBody = document.getElementById('device-table');

const devices = new Map<string, DeviceDescriptor>();
let connectionStatus: ConnectionStatus | null = null;
let latestVehicle: VehicleStatus | null = null;
const parameterState: { received: number; expected?: number; count: number; lastUpdated?: string } = {
  received: 0,
  expected: undefined,
  count: 0,
  lastUpdated: undefined
};

if (statusBadge) {
  statusBadge.textContent = 'Awaiting core signal…';
}

backend.onRustMessage((payload: string) => {
  let parsed: RustEnvelope | CoreEvent | null = null;

  try {
    parsed = JSON.parse(payload) as RustEnvelope | CoreEvent;
  } catch {
    appendLog('info', payload);
    return;
  }

  if (parsed && typeof parsed === 'object' && 'event' in parsed && typeof parsed.event === 'string') {
    handleCoreEvent(parsed as CoreEvent);
    return;
  }

  const envelope = parsed as RustEnvelope;
  if (envelope.message) {
    appendLog(envelope.level ?? 'info', envelope.message);
  }

  if (envelope.vehicle && typeof envelope.vehicle === 'object') {
    updateVehicleStatus(envelope.vehicle as VehicleStatus);
  }

  if (statusBadge) {
    statusBadge.textContent = 'Connected to rust-core';
  }
});

async function refreshDeviceList(): Promise<void> {
  try {
    const list = await backend.listDevices();
    devices.clear();
    list.forEach((device) => devices.set(device.id, device));
    renderDeviceTable();
  } catch (error) {
    console.error('Failed to list devices', error);
  }
}

function handleCoreEvent(event: CoreEvent): void {
  switch (event.event) {
    case 'discovery_snapshot': {
      const snapshot = Array.isArray(event.devices)
        ? (event.devices as DeviceDescriptor[])
        : [];
      devices.clear();
      snapshot.forEach((device: DeviceDescriptor) => devices.set(device.id, device));
      renderDeviceTable();
      break;
    }
    case 'device_discovered': {
      const device = event.device as DeviceDescriptor | undefined;
      if (device) {
        devices.set(device.id, device);
        renderDeviceTable();
      }
      break;
    }
    case 'device_lost': {
      const device = event.device as DeviceDescriptor | undefined;
      if (device) {
        devices.delete(device.id);
        renderDeviceTable();
      }
      break;
    }
    case 'connection_status': {
      connectionStatus = event.status as ConnectionStatus;
      renderConnectionStatus();
      break;
    }
    case 'heartbeat': {
      const status = event.status as VehicleStatus | undefined;
      if (status) {
        updateVehicleStatus(status);
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
        const timestamp = new Date(entry.timestamp_millis).toLocaleTimeString();
        appendLog(entry.level.toLowerCase(), `[${timestamp}] ${entry.source}: ${entry.message}`);
      }
      break;
    }
    case 'diagnostics': {
      const level = (event.level as string | undefined) ?? 'debug';
      const message = (event.message as string | undefined) ?? 'diagnostic event';
      appendLog(level.toLowerCase(), message);
      break;
    }
    case 'parameter_progress': {
      const received = typeof event.received === 'number' ? event.received : parameterState.received;
      const expected = typeof event.expected === 'number' ? event.expected : parameterState.expected;
      parameterState.received = received;
      parameterState.expected = expected;
      renderParameterStats();
      break;
    }
    case 'parameter_batch': {
      const parameters = (event.parameters as ParameterValue[]) ?? [];
      parameterState.count = parameters.length;
      parameterState.lastUpdated = new Date().toLocaleTimeString();
      renderParameterStats();
      appendLog('info', `Parameter batch received (${parameters.length} values)`);
      break;
    }
    default:
      appendLog('debug', `Unhandled core event: ${event.event}`);
      break;
  }

  if (statusBadge) {
    statusBadge.textContent = 'Connected to rust-core';
  }
}

function renderDeviceTable(): void {
  if (!deviceTableBody) {
    return;
  }

  deviceTableBody.innerHTML = '';
  const rows = Array.from(devices.values())
    .map((device) => device as DeviceDescriptor)
    .sort((a, b) => a.id.localeCompare(b.id));

  rows.forEach((device: DeviceDescriptor) => {
    const tr = document.createElement('tr');
    if (connectionStatus?.device?.id === device.id) {
      tr.dataset.active = 'true';
    }

    const details = summarizeDeviceDetails(device);

    tr.innerHTML = `
      <td>${device.id}</td>
      <td>${device.label}</td>
      <td>${device.transport}</td>
      <td>${details}</td>
      <td class="device-actions"></td>
    `;

    const actionsCell = tr.querySelector('.device-actions');
    if (actionsCell) {
      const button = document.createElement('button');
      button.textContent = connectionStatus?.device?.id === device.id ? 'Reconnect' : 'Connect';
      button.addEventListener('click', () => {
        void handleConnect(device.id);
      });
      actionsCell.appendChild(button);
    }

    deviceTableBody.appendChild(tr);
  });
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

function renderConnectionStatus(): void {
  if (!connectionStatusContainer) {
    return;
  }

  if (!connectionStatus) {
    connectionStatusContainer.textContent = 'Idle';
    return;
  }

  const { phase, message, device } = connectionStatus;
  const details = [phase, message, device?.label ?? device?.id]
    .filter((item) => typeof item === 'string' && item.length > 0)
    .join(' • ');
  connectionStatusContainer.textContent = details || 'Unknown';
  renderDeviceTable();
}

function updateVehicleStatus(status: VehicleStatus): void {
  latestVehicle = status;
  if (!vehicleStatusContainer) {
    return;
  }

  vehicleStatusContainer.innerHTML = `
    <div><strong>ID:</strong> ${status.vehicleId}</div>
    <div><strong>Type:</strong> ${status.vehicleType}</div>
    <div><strong>Arming:</strong> ${status.armingState}</div>
    <div><strong>Heartbeat:</strong> ${status.heartbeatMillis || '—'} ms</div>
  `;
}

function renderParameterStats(): void {
  if (!parameterStatsContainer) {
    return;
  }

  const { received, expected, count, lastUpdated } = parameterState;
  const progress = expected ? `${received}/${expected}` : `${received}`;
  const updated = lastUpdated ? `Last updated ${lastUpdated}` : 'No completed batch yet';

  parameterStatsContainer.innerHTML = `
    <div><strong>Progress:</strong> ${progress}</div>
    <div><strong>Cached Parameters:</strong> ${count}</div>
    <div>${updated}</div>
  `;
}

function appendLog(level: string, message: string): void {
  if (!logContainer) {
    return;
  }

  const entry = document.createElement('div');
  entry.className = 'log-entry';
  entry.dataset.level = level.toLowerCase();
  entry.textContent = message;
  logContainer.prepend(entry);
  while (logContainer.children.length > 200) {
    logContainer.removeChild(logContainer.lastChild as Node);
  }
}

async function runDiagnostics(timeout?: number): Promise<void> {
  try {
    await backend.invokeDiagnostics(timeout);
  } catch (error) {
    console.error('Diagnostics failed', error);
  }
}

async function triggerFailure(): Promise<void> {
  try {
    await backend.simulateFailure();
  } catch (error) {
    console.error('simulateFailure error', error);
  }
}

async function handleConnect(deviceId: string): Promise<void> {
  try {
      const status = await backend.connect({ deviceId });
    connectionStatus = status;
    renderConnectionStatus();
  } catch (error) {
    console.error('Connect failed', error);
  }
}

async function handleDisconnect(): Promise<void> {
  try {
    await backend.disconnect();
  } catch (error) {
    console.error('Disconnect failed', error);
  }
}

async function handleFetchParameters(): Promise<void> {
  try {
    const params = await backend.fetchParameters(5_000);
    parameterState.count = params.length;
    parameterState.lastUpdated = new Date().toLocaleTimeString();
    renderParameterStats();
    appendLog('info', `Fetched ${params.length} parameters via IPC command`);
  } catch (error) {
    console.error('fetchParameters failed', error);
  }
}

if (runDiagnosticsButton) {
  runDiagnosticsButton.addEventListener('click', () => {
    void runDiagnostics(250);
  });
}

if (simulateFailureButton) {
  simulateFailureButton.addEventListener('click', () => {
    void triggerFailure();
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

void refreshDeviceList();
void backend.fetchVehicleStatus().then(updateVehicleStatus).catch((error) => {
  console.error('Failed to load vehicle status', error);
});
void backend.getConnectionStatus().then((status: ConnectionStatus) => {
  connectionStatus = status;
  renderConnectionStatus();
});
void backend.getCachedParameters().then((params: ParameterValue[]) => {
  parameterState.count = params.length;
  if (params.length > 0) {
    parameterState.lastUpdated = new Date().toLocaleTimeString();
  }
  renderParameterStats();
});
