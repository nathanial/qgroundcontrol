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
});
