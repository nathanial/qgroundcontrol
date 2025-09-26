import type { VehicleStatus } from '../../rust-core/index';

type RustEnvelope = {
  level?: string;
  message?: string;
  vehicle?: VehicleStatus;
  [key: string]: unknown;
};

const statusBadge = document.getElementById('status-badge');
const logContainer = document.getElementById('rust-log');
const vehicleStatusContainer = document.getElementById('vehicle-status');
const runDiagnosticsButton = document.getElementById('run-diagnostics');
const simulateFailureButton = document.getElementById('simulate-failure');

if (statusBadge) {
  statusBadge.textContent = 'Awaiting core signal…';
}

function renderVehicleStatus(status: VehicleStatus) {
  if (!vehicleStatusContainer) {
    return;
  }

  vehicleStatusContainer.innerHTML = `
    <div><strong>ID:</strong> ${status.vehicleId}</div>
    <div><strong>Type:</strong> ${status.vehicleType}</div>
    <div><strong>Arming:</strong> ${status.armingState}</div>
    <div><strong>Heartbeat:</strong> ${status.heartbeatMillis} ms</div>
  `;
}

window.backend.onRustMessage((payload: string) => {
  let entryText = payload;
  let level: string | undefined;

  try {
    const envelope = JSON.parse(payload) as RustEnvelope;
    if (typeof envelope.message === 'string') {
      entryText = envelope.message;
    }
    if (typeof envelope.level === 'string') {
      level = envelope.level;
    }
    if (typeof envelope.vehicle === 'object' && envelope.vehicle !== null) {
      renderVehicleStatus(envelope.vehicle as VehicleStatus);
    }
  } catch {
    // Ignore JSON parse failures and use the raw payload
  }

  if (statusBadge) {
    statusBadge.textContent = 'Connected to rust-core';
  }

  if (logContainer) {
    const entry = document.createElement('div');
    entry.className = 'log-entry';
    entry.dataset.level = level ?? 'info';
    entry.textContent = entryText;
    logContainer.prepend(entry);
  }
});

async function requestVehicleStatus(): Promise<void> {
  try {
    const vehicle = await window.backend.fetchVehicleStatus();
    renderVehicleStatus(vehicle);
  } catch (error) {
    console.error('Failed to load vehicle status', error);
  }
}

async function runDiagnostics(timeout?: number): Promise<void> {
  try {
    await window.backend.invokeDiagnostics(timeout);
  } catch (error) {
    console.error('Diagnostics failed', error);
  }
}

async function triggerFailure(): Promise<void> {
  try {
    await window.backend.simulateFailure();
  } catch (error) {
    console.error('simulateFailure error', error);
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

void requestVehicleStatus();
