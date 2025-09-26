import { describe, expect, it, beforeAll, vi } from 'vitest';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import fs from 'node:fs';

type StatusMessage = {
  kind: string;
  message: string;
};

type VehicleStatus = {
  vehicleId: string;
  vehicleType: string;
  armingState: string;
  heartbeatMillis: number;
  flightMode?: {
    label: string;
    baseMode: number;
    customMode: number;
  } | null;
  battery?: {
    voltageV: number;
    currentA?: number | null;
    remainingPercent?: number | null;
  } | null;
  gps?: {
    fixType: string;
    satellitesVisible: number;
    latitudeDeg?: number | null;
    longitudeDeg?: number | null;
  } | null;
};

type DeviceDescriptor = {
  id: string;
  label: string;
  transport: string;
};

type ConnectionStatus = {
  phase: string;
  message?: string;
  device?: DeviceDescriptor | null;
};

type ParameterValue = {
  name: string;
  value: number;
};

type MissionItem = {
  seq: number;
  command: number;
  frame: string;
  latitudeDeg: number;
  longitudeDeg: number;
  altitudeM: number;
  param1: number;
  param2: number;
  param3: number;
  param4: number;
  autoContinue: boolean;
  isCurrent: boolean;
};

type MissionPlan = {
  planId: string;
  revision: number;
  items: MissionItem[];
  lastModifiedMillis: number;
  notes?: string | null;
};

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const projectRoot = path.resolve(__dirname, '..');

function resolveNativeModule(): string {
  const rustCoreDir = path.join(projectRoot, 'rust-core');
  const defaultModule = path.join(rustCoreDir, 'index.node');

  if (fs.existsSync(defaultModule)) {
    return defaultModule;
  }

  const entries = fs.readdirSync(rustCoreDir);
  const candidate = entries.find((entry) => entry.startsWith('index.') && entry.endsWith('.node'));
  if (!candidate) {
    throw new Error('Could not locate compiled napi module');
  }

  return path.join(rustCoreDir, candidate);
}

type RustModule = {
  healthCheck(): StatusMessage;
  runDiagnostics(timeoutMs?: number): Promise<StatusMessage>;
  bootstrapVehicleStatus(): VehicleStatus;
  registerEventSink(callback: (event: unknown) => void): void;
  listDevices(): DeviceDescriptor[];
  connectMavlink(options?: Record<string, unknown>): Promise<ConnectionStatus>;
  disconnectMavlink(): Promise<void>;
  fetchParameters(timeoutMs?: number): Promise<ParameterValue[]>;
  currentConnectionStatus(): ConnectionStatus;
  currentMissionPlan(): MissionPlan;
  cachedMissionPlan(): MissionPlan;
  downloadMission(timeoutMs?: number): Promise<MissionPlan>;
  uploadMission(plan: MissionPlan, timeoutMs?: number): Promise<MissionPlan>;
};

const rust = require(resolveNativeModule()) as RustModule;

beforeAll(() => {
  rust.registerEventSink(vi.fn());
});

describe('rust-core napi surface', () => {
  it('returns a health check message', () => {
    const status = rust.healthCheck();
    expect(status.kind).toBe('health');
    expect(status.message).toContain('rust-core');
  });

  it('produces an async diagnostic response', async () => {
    const result = await rust.runDiagnostics(25);
    expect(result.kind).toBe('diagnostic');
    expect(result.message).toContain('25');
  });

  it('fetches a bootstrap vehicle snapshot', () => {
    const vehicle = rust.bootstrapVehicleStatus();
    expect(vehicle.vehicleId).toBeDefined();
    expect(vehicle.vehicleType).not.toHaveLength(0);
    expect(vehicle.flightMode?.label).toBeDefined();
  });

  it('supports simulated link connectivity', async () => {
    const devices = rust.listDevices();
    expect(devices.length).toBeGreaterThan(0);

    const status = await rust.connectMavlink({ forceSimulated: true });
    expect(status.phase).toBeDefined();

    const params = await rust.fetchParameters(1_000);
    expect(params.length).toBeGreaterThan(0);

    await rust.disconnectMavlink();
    const idleStatus = rust.currentConnectionStatus();
    expect(idleStatus.phase).toBeDefined();
  });

  it('round-trips mission upload/download for simulated link', async () => {
    const status = await rust.connectMavlink({ forceSimulated: true });
    expect(status.phase).toBeDefined();

    const baseline = rust.currentMissionPlan();
    const altitude = baseline.items[0]?.altitudeM ?? 60;

    const nextPlan: MissionPlan = {
      planId: baseline.planId,
      revision: baseline.revision,
      lastModifiedMillis: Date.now(),
      notes: baseline.notes ?? undefined,
      items: [
        {
          seq: 0,
          command: 16,
          frame: 'GlobalRelativeAlt',
          latitudeDeg: 37.3349,
          longitudeDeg: -121.8946,
          altitudeM: altitude,
          param1: 0,
          param2: 0,
          param3: 0,
          param4: 0,
          autoContinue: true,
          isCurrent: true
        },
        {
          seq: 1,
          command: 16,
          frame: 'GlobalRelativeAlt',
          latitudeDeg: 37.3354,
          longitudeDeg: -121.8938,
          altitudeM: altitude,
          param1: 0,
          param2: 0,
          param3: 0,
          param4: 0,
          autoContinue: true,
          isCurrent: false
        },
        {
          seq: 2,
          command: 16,
          frame: 'GlobalRelativeAlt',
          latitudeDeg: 37.3359,
          longitudeDeg: -121.8949,
          altitudeM: altitude,
          param1: 0,
          param2: 0,
          param3: 0,
          param4: 0,
          autoContinue: true,
          isCurrent: false
        }
      ]
    };

    const uploaded = await rust.uploadMission(nextPlan, 10_000);
    expect(uploaded.items.length).toBe(nextPlan.items.length);
    expect(uploaded.revision).toBeGreaterThanOrEqual(baseline.revision);

    const downloaded = await rust.downloadMission(10_000);
    expect(downloaded.items.length).toBe(nextPlan.items.length);
    expect(downloaded.items[0].latitudeDeg).toBeCloseTo(nextPlan.items[0].latitudeDeg, 5);

    await rust.disconnectMavlink();
  });
});
