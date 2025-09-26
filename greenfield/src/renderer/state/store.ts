import { createStore } from 'zustand/vanilla';
import { subscribeWithSelector } from 'zustand/middleware';
import type {
  ConnectionStatus,
  DeviceDescriptor,
  ParameterValue,
  VehicleStatus
} from '../../../rust-core/index';

export type LogEntry = {
  id: string;
  level: string;
  message: string;
  timestamp: number;
};

export type ParameterProgressState = {
  received: number;
  expected?: number;
  lastBatchCount: number;
  lastUpdated?: number;
};

export type AppState = {
  devices: DeviceDescriptor[];
  connectionStatus: ConnectionStatus | null;
  vehicleStatus: VehicleStatus | null;
  parameterValues: ParameterValue[];
  parameterProgress: ParameterProgressState;
  logs: LogEntry[];
  setDevices(devices: DeviceDescriptor[]): void;
  upsertDevice(device: DeviceDescriptor): void;
  removeDevice(deviceId: string): void;
  setConnectionStatus(status: ConnectionStatus | null): void;
  setVehicleStatus(status: VehicleStatus | null): void;
  setParameterValues(values: ParameterValue[], timestamp: number): void;
  setParameterProgress(received: number, expected?: number): void;
  appendLog(entry: LogEntry): void;
  resetLogs(): void;
};

const DEVICE_LIMIT = 128;
const LOG_LIMIT = 200;

const sortDevices = (devices: DeviceDescriptor[]): DeviceDescriptor[] =>
  devices.slice().sort((a, b) => a.id.localeCompare(b.id));

const sortParameters = (parameters: ParameterValue[]): ParameterValue[] =>
  parameters.slice().sort((a, b) => a.name.localeCompare(b.name));

export const appStore = createStore(subscribeWithSelector<AppState>((set) => ({
  devices: [],
  connectionStatus: null,
  vehicleStatus: null,
  parameterValues: [],
  parameterProgress: {
    received: 0,
    expected: undefined,
    lastBatchCount: 0,
    lastUpdated: undefined
  },
  logs: [],
  setDevices(devices) {
    set(() => ({ devices: sortDevices(devices).slice(0, DEVICE_LIMIT) }));
  },
  upsertDevice(device) {
    set((state) => {
      const filtered = state.devices.filter((existing) => existing.id !== device.id);
      filtered.push(device);
      return { devices: sortDevices(filtered).slice(0, DEVICE_LIMIT) };
    });
  },
  removeDevice(deviceId) {
    set((state) => ({
      devices: state.devices.filter((device) => device.id !== deviceId)
    }));
  },
  setConnectionStatus(status) {
    set(() => ({ connectionStatus: status }));
  },
  setVehicleStatus(status) {
    set(() => ({ vehicleStatus: status }));
  },
  setParameterValues(values, timestamp) {
    set((state) => ({
      parameterValues: sortParameters(values),
      parameterProgress: {
        received: values.length,
        expected: state.parameterProgress.expected,
        lastBatchCount: values.length,
        lastUpdated: timestamp
      }
    }));
  },
  setParameterProgress(received, expected) {
    set((state) => ({
      parameterProgress: {
        received,
        expected: expected ?? state.parameterProgress.expected,
        lastBatchCount: state.parameterProgress.lastBatchCount,
        lastUpdated: state.parameterProgress.lastUpdated
      }
    }));
  },
  appendLog(entry) {
    set((state) => {
      const next = [entry, ...state.logs];
      if (next.length > LOG_LIMIT) {
        next.length = LOG_LIMIT;
      }
      return { logs: next };
    });
  },
  resetLogs() {
    set(() => ({ logs: [] }));
  }
})));
