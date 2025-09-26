import type { StatusMessage, VehicleStatus } from '../../rust-core/index';

export {};

declare global {
  interface Window {
    backend: {
      onRustMessage: (callback: (payload: string) => void) => () => void;
      invokeDiagnostics: (timeoutMs?: number) => Promise<StatusMessage>;
      simulateFailure: () => Promise<{ level: string; kind: string; message: string }>;
      fetchVehicleStatus: () => Promise<VehicleStatus>;
    };
  }
}
