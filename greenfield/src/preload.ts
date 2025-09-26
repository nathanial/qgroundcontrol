import { contextBridge, ipcRenderer, IpcRendererEvent } from 'electron';
import type { StatusMessage, VehicleStatus } from '../rust-core/index';

const RUST_CHANNEL = 'rust-message';

type RustMessageCallback = (message: string) => void;

contextBridge.exposeInMainWorld('backend', {
  onRustMessage(callback: RustMessageCallback): () => void {
    const listener = (_event: IpcRendererEvent, message: string) => {
      callback(message);
    };

    ipcRenderer.on(RUST_CHANNEL, listener);

    return () => {
      ipcRenderer.removeListener(RUST_CHANNEL, listener);
    };
  },
  invokeDiagnostics(timeoutMs?: number): Promise<StatusMessage> {
    return ipcRenderer.invoke('rust:runDiagnostics', timeoutMs ?? null);
  },
  simulateFailure(): Promise<{ level: string; kind: string; message: string }> {
    return ipcRenderer.invoke('rust:simulateFailure');
  },
  fetchVehicleStatus(): Promise<VehicleStatus> {
    return ipcRenderer.invoke('rust:bootstrapStatus');
  }
});
