import type {
  StatusMessage,
  VehicleStatus,
  DeviceDescriptor,
  ConnectionStatus,
  ParameterValue,
  ConnectOptions
} from 'rust-core';

interface BackendAPI {
  onRustMessage(callback: (message: string) => void): () => void;
  invokeDiagnostics(timeoutMs?: number): Promise<StatusMessage>;
  simulateFailure(): Promise<{ level: string; kind: string; message: string }>;
  fetchVehicleStatus(): Promise<VehicleStatus>;
  listDevices(): Promise<DeviceDescriptor[]>;
  connect(options?: Partial<ConnectOptions>): Promise<ConnectionStatus>;
  disconnect(): Promise<void>;
  fetchParameters(timeoutMs?: number): Promise<ParameterValue[]>;
  getConnectionStatus(): Promise<ConnectionStatus>;
  getCachedParameters(): Promise<ParameterValue[]>;
}

declare global {
  interface Window {
    backend: BackendAPI;
  }
}

export {};
