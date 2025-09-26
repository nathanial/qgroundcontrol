import { describe, expect, it } from 'vitest';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import fs from 'node:fs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const projectRoot = path.resolve(__dirname, '..');

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

const rust = require(resolveNativeModule()) as {
  healthCheck(): StatusMessage;
  runDiagnostics(timeoutMs?: number): Promise<StatusMessage>;
  bootstrapVehicleStatus(): VehicleStatus;
};

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
});
