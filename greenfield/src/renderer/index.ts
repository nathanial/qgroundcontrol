import maplibregl from 'maplibre-gl';
import type {
  DeviceDescriptor,
  ConnectionStatus,
  ParameterValue,
  VehicleStatus,
  StatusMessage,
  MissionPlan,
  MissionItem,
  MissionSyncStatus,
  MissionOperationReport
} from '../../rust-core/index';
import { MissionFrame } from '../../rust-core/index';
import {
  appStore,
  createMissionDraftSkeleton,
  type ParameterProgressState,
  type LogEntry
} from './state/store';

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
  getMissionPlan(): Promise<MissionPlan>;
  getCachedMissionPlan(): Promise<MissionPlan>;
  downloadMission(timeoutMs?: number): Promise<MissionPlan>;
  uploadMission(plan: MissionPlan, timeoutMs?: number): Promise<MissionPlan>;
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
const missionStatusContainer = document.getElementById('mission-status');
const missionTableBody = document.getElementById('mission-table-body');
const missionEmptyState = document.getElementById('mission-empty');
const missionDownloadButton = document.getElementById('mission-download');
const missionUploadButton = document.getElementById('mission-upload');
const missionAddButton = document.getElementById('mission-add-waypoint');
const missionPatternButton = document.getElementById('mission-generate-pattern');
const missionSyncBadge = document.getElementById('mission-sync-status');
const missionMapContainer = document.getElementById('mission-map');

const unsubscribes: Array<() => void> = [];
let parameterFilter = '';
const missionSourceId = 'mission-plan';
const missionLineLayerId = 'mission-plan-line';
const missionPointLayerId = 'mission-plan-points';
let missionMap: maplibregl.Map | null = null;
let missionMapReady = false;
const DEFAULT_MISSION_ALTITUDE = 50;

if (statusBadge) {
  statusBadge.textContent = 'Awaiting core signal…';
}

function init(): void {
  setupStoreSubscriptions();
  setupUiHandlers();
  initializeMissionPlanner();
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

  unsubscribes.push(
    store.subscribe(
      (state) => state.missionDraft,
      (plan) => {
        renderMissionPlan(plan);
        updateMissionMap(plan);
      }
    )
  );

  unsubscribes.push(
    store.subscribe(
      (state) => state.missionSync,
      (status) => renderMissionSync(status)
    )
  );

  unsubscribes.push(
    store.subscribe(
      (state) => state.selectedMissionIndex,
      (index) => highlightSelectedMissionRow(index)
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
  renderMissionPlan(current.missionDraft ?? current.missionPlan);
  renderMissionSync(current.missionSync);
  updateMissionMap(current.missionDraft ?? current.missionPlan);
  highlightSelectedMissionRow(current.selectedMissionIndex);
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

  if (missionDownloadButton) {
    missionDownloadButton.addEventListener('click', () => {
      void handleMissionDownload();
    });
  }

  if (missionUploadButton) {
    missionUploadButton.addEventListener('click', () => {
      void handleMissionUpload();
    });
  }

  if (missionAddButton) {
    missionAddButton.addEventListener('click', () => {
      handleAddWaypoint();
    });
  }

  if (missionPatternButton) {
    missionPatternButton.addEventListener('click', () => {
      handleGeneratePattern();
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
      }),
    backend
      .getMissionPlan()
      .then((plan) => store.getState().setMissionPlan(plan))
      .catch((error) => {
        logMessage('warn', `Failed to fetch mission plan: ${(error as Error).message}`);
        return backend
          .getCachedMissionPlan()
          .then((plan) => store.getState().setMissionPlan(plan))
          .catch((cachedError) => {
            logMessage('debug', `No cached mission plan available: ${(cachedError as Error).message}`);
          });
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

function initializeMissionPlanner(): void {
  if (!missionMapContainer || missionMap) {
    return;
  }

  missionMap = new maplibregl.Map({
    container: missionMapContainer,
    style: 'https://demotiles.maplibre.org/style.json',
    center: [-121.8947, 37.3349],
    zoom: 13,
    pitch: 0,
    attributionControl: false
  });

  missionMap.addControl(new maplibregl.NavigationControl({ visualizePitch: false }), 'top-right');

  missionMap.on('load', () => {
    missionMapReady = true;
    if (!missionMap) {
      return;
    }
    missionMap.addSource(missionSourceId, {
      type: 'geojson',
      data: createMissionGeoJson(store.getState().missionDraft ?? store.getState().missionPlan)
    });

    missionMap.addLayer({
      id: missionLineLayerId,
      type: 'line',
      source: missionSourceId,
      paint: {
        'line-color': '#60a5fa',
        'line-width': 3
      }
    });

    missionMap.addLayer({
      id: missionPointLayerId,
      type: 'circle',
      source: missionSourceId,
      paint: {
        'circle-radius': 6,
        'circle-color': '#fbbf24',
        'circle-stroke-width': 1,
        'circle-stroke-color': '#1f2937'
      }
    });

    updateMissionMap(store.getState().missionDraft ?? store.getState().missionPlan);
  });

  missionMap.on('click', (event) => {
    store.getState().addMissionWaypoint({
      latitudeDeg: event.lngLat.lat,
      longitudeDeg: event.lngLat.lng
    });
  });
}

async function handleMissionDownload(): Promise<void> {
  try {
    const plan = await backend.downloadMission(15_000);
    store.getState().setMissionPlan(plan);
    logMessage('info', `Mission download complete (${plan.items.length} waypoints)`);
  } catch (error) {
    logMessage('error', `Mission download failed: ${(error as Error).message}`);
  }
}

async function handleMissionUpload(): Promise<void> {
  const draft = store.getState().missionDraft;
  if (!draft) {
    logMessage('warn', 'No mission draft available for upload');
    return;
  }

  try {
    const result = await backend.uploadMission(draft, 15_000);
    store.getState().setMissionPlan(result);
    logMessage('info', `Mission upload acknowledged (revision ${result.revision})`);
  } catch (error) {
    logMessage('error', `Mission upload failed: ${(error as Error).message}`);
  }
}

function handleAddWaypoint(): void {
  const center = missionMap?.getCenter();
  const latitude = center?.lat ?? 37.3349;
  const longitude = center?.lng ?? -121.8947;
  store.getState().addMissionWaypoint({ latitudeDeg: latitude, longitudeDeg: longitude });
}

function handleGeneratePattern(): void {
  const center = missionMap?.getCenter();
  if (!center) {
    logMessage('warn', 'Mission map not ready yet');
    return;
  }

  const base = store.getState().missionDraft ?? store.getState().missionPlan;
  const altitude = base?.items?.[0]?.altitudeM ?? DEFAULT_MISSION_ALTITUDE;
  const plan = createMissionDraftSkeleton(base);

  const pattern = buildSquarePattern(center.lat, center.lng, altitude);
  plan.items = pattern.map((point, index) => ({
    seq: index,
    command: 16,
    frame: MissionFrame.GlobalRelativeAlt,
    latitudeDeg: point.latitudeDeg,
    longitudeDeg: point.longitudeDeg,
    altitudeM: point.altitudeM,
    param1: 0,
    param2: 0,
    param3: 0,
    param4: 0,
    autoContinue: true,
    isCurrent: index === 0
  }));

  store.getState().setMissionDraft(plan);
  logMessage('info', 'Generated square survey pattern around map center');
}

function buildSquarePattern(lat: number, lon: number, altitude: number): Array<{
  latitudeDeg: number;
  longitudeDeg: number;
  altitudeM: number;
}> {
  const sizeMeters = 120;
  const latOffset = (sizeMeters / 2) / 111_320;
  const lonOffset = (sizeMeters / 2) / (111_320 * Math.cos((lat * Math.PI) / 180));

  return [
    { latitudeDeg: lat + latOffset, longitudeDeg: lon - lonOffset, altitudeM: altitude },
    { latitudeDeg: lat + latOffset, longitudeDeg: lon + lonOffset, altitudeM: altitude },
    { latitudeDeg: lat - latOffset, longitudeDeg: lon + lonOffset, altitudeM: altitude },
    { latitudeDeg: lat - latOffset, longitudeDeg: lon - lonOffset, altitudeM: altitude }
  ];
}

function renderMissionPlan(plan: MissionPlan | null | undefined): void {
  if (!missionTableBody || !missionEmptyState) {
    return;
  }

  missionTableBody.innerHTML = '';

  if (!plan || plan.items.length === 0) {
    missionEmptyState.classList.remove('hidden');
    return;
  }

  missionEmptyState.classList.add('hidden');
  const fragment = document.createDocumentFragment();
  const selected = store.getState().selectedMissionIndex;

  plan.items.forEach((item, index) => {
    const tr = document.createElement('tr');
    tr.dataset.index = index.toString();
    if (selected === index) {
      tr.dataset.selected = 'true';
    }

    const seqCell = document.createElement('td');
    seqCell.textContent = (index + 1).toString();

    const latCell = document.createElement('td');
    latCell.textContent = formatCoordinate(item.latitudeDeg, 'lat');

    const lonCell = document.createElement('td');
    lonCell.textContent = formatCoordinate(item.longitudeDeg, 'lon');

    const altitudeCell = document.createElement('td');
    const altitudeInput = document.createElement('input');
    altitudeInput.type = 'number';
    altitudeInput.className = 'mission-alt-input';
    altitudeInput.value = item.altitudeM.toFixed(1);
    altitudeInput.addEventListener('change', (event) => {
      const next = parseFloat((event.target as HTMLInputElement).value);
      if (!Number.isFinite(next)) {
        return;
      }
      store.getState().updateMissionItem(index, { altitudeM: next });
    });
    altitudeCell.appendChild(altitudeInput);

    const actionsCell = document.createElement('td');
    const removeButton = document.createElement('button');
    removeButton.textContent = 'Remove';
    removeButton.addEventListener('click', (event) => {
      event.stopPropagation();
      store.getState().removeMissionWaypoint(index);
    });
    actionsCell.appendChild(removeButton);

    tr.append(seqCell, latCell, lonCell, altitudeCell, actionsCell);
    tr.addEventListener('click', () => {
      store.getState().setSelectedMissionIndex(index);
    });

    fragment.appendChild(tr);
  });

  missionTableBody.appendChild(fragment);
}

function renderMissionSync(status: MissionSyncStatus | null | undefined): void {
  if (missionSyncBadge) {
    if (!status) {
      missionSyncBadge.textContent = 'Idle';
      missionSyncBadge.removeAttribute('data-stage');
    } else {
      missionSyncBadge.textContent = status.stage;
      missionSyncBadge.dataset.stage = status.stage.toLowerCase();
    }
  }

  if (missionStatusContainer) {
    if (!status) {
      missionStatusContainer.textContent = 'Mission idle';
    } else {
      const progress = status.total ? `${status.index ?? 0}/${status.total}` : status.index?.toString() ?? '—';
      const detail = status.message ? ` • ${status.message}` : '';
      missionStatusContainer.textContent = `${status.stage}${progress ? ` (${progress})` : ''}${detail}`;
    }
  }
}

function highlightSelectedMissionRow(index: number | null): void {
  if (!missionTableBody) {
    return;
  }

  missionTableBody.querySelectorAll('tr').forEach((row, rowIndex) => {
    if (rowIndex === index) {
      row.dataset.selected = 'true';
    } else {
      row.removeAttribute('data-selected');
    }
  });

  if (index != null) {
    const plan = store.getState().missionDraft ?? store.getState().missionPlan;
    const waypoint = plan?.items[index];
    if (missionMap && missionMapReady && waypoint) {
      missionMap.easeTo({
        center: [waypoint.longitudeDeg, waypoint.latitudeDeg],
        duration: 300
      });
    }
  }
}

function updateMissionMap(plan: MissionPlan | null | undefined): void {
  if (!missionMap || !missionMapReady) {
    return;
  }

  const source = missionMap.getSource(missionSourceId) as maplibregl.GeoJSONSource | undefined;
  const data = createMissionGeoJson(plan ?? null);

  if (source) {
    source.setData(data);
  }

  if (plan && plan.items.length > 0) {
    fitMapToMission(plan);
  }
}

function fitMapToMission(plan: MissionPlan): void {
  if (!missionMap || !missionMapReady || plan.items.length === 0) {
    return;
  }

  const bounds = plan.items.reduce((acc, item) => {
    acc.extend([item.longitudeDeg, item.latitudeDeg]);
    return acc;
  }, new maplibregl.LngLatBounds());

  if (bounds.isEmpty()) {
    missionMap.easeTo({
      center: [plan.items[0].longitudeDeg, plan.items[0].latitudeDeg],
      duration: 0
    });
  } else {
    missionMap.fitBounds(bounds, { padding: 48, maxZoom: 18, duration: 400 });
  }
}

function createMissionGeoJson(plan: MissionPlan | null): GeoJSON.FeatureCollection {
  if (!plan || plan.items.length === 0) {
    return { type: 'FeatureCollection', features: [] };
  }

  const lineFeature: GeoJSON.Feature<GeoJSON.LineString> = {
    type: 'Feature',
    geometry: {
      type: 'LineString',
      coordinates: plan.items.map((item) => [item.longitudeDeg, item.latitudeDeg])
    },
    properties: {}
  };

  const pointFeatures: GeoJSON.Feature<GeoJSON.Point>[] = plan.items.map((item, index) => ({
    type: 'Feature',
    geometry: {
      type: 'Point',
      coordinates: [item.longitudeDeg, item.latitudeDeg]
    },
    properties: {
      seq: item.seq,
      label: `WP${index + 1}`,
      altitude: item.altitudeM
    }
  }));

  return {
    type: 'FeatureCollection',
    features: [lineFeature, ...pointFeatures]
  };
}

function formatCoordinate(value: number, kind: 'lat' | 'lon'): string {
  const hemisphere = kind === 'lat' ? (value >= 0 ? 'N' : 'S') : value >= 0 ? 'E' : 'W';
  return `${Math.abs(value).toFixed(6)}° ${hemisphere}`;
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
    case 'mission_plan': {
      const plan = event.plan as MissionPlan | undefined;
      if (plan) {
        store.getState().setMissionPlan(plan);
        logMessage('info', `Mission plan updated (${plan.items.length} items)`);
      }
      break;
    }
    case 'mission_sync': {
      const status = event.status as MissionSyncStatus | undefined;
      if (status) {
        store.getState().setMissionSync(status);
      }
      break;
    }
    case 'mission_operation': {
      const report = event.report as MissionOperationReport | undefined;
      if (report) {
        const status = String(report.status).toLowerCase();
        const operation = String(report.operation).toLowerCase();
        const detail = report.message ? `: ${report.message}` : '';
        logMessage(status === 'success' ? 'info' : status, `Mission ${operation} ${status}${detail}`);
      }
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
