import { contextBridge, ipcRenderer, IpcRendererEvent } from 'electron';
import type {
  StatusMessage,
  VehicleStatus,
  DeviceDescriptor,
  ConnectionStatus,
  ParameterValue,
  ConnectOptions,
  MissionPlan
} from '../rust-core/index';

const RUST_CHANNEL = 'rust-message';

type RustMessageCallback = (message: string) => void;

type ConnectPayload = Partial<ConnectOptions>;

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
  },
  listDevices(): Promise<DeviceDescriptor[]> {
    return ipcRenderer.invoke('rust:listDevices');
  },
  connect(options?: ConnectPayload): Promise<ConnectionStatus> {
    return ipcRenderer.invoke('rust:connect', options ?? {});
  },
  disconnect(): Promise<void> {
    return ipcRenderer.invoke('rust:disconnect');
  },
  fetchParameters(timeoutMs?: number): Promise<ParameterValue[]> {
    return ipcRenderer.invoke('rust:fetchParameters', timeoutMs ?? null);
  },
  getConnectionStatus(): Promise<ConnectionStatus> {
    return ipcRenderer.invoke('rust:getConnectionStatus');
  },
  getCachedParameters(): Promise<ParameterValue[]> {
    return ipcRenderer.invoke('rust:getCachedParameters');
  },
  getMissionPlan(): Promise<MissionPlan> {
    return ipcRenderer.invoke('rust:getMissionPlan');
  },
  getCachedMissionPlan(): Promise<MissionPlan> {
    return ipcRenderer.invoke('rust:getCachedMissionPlan');
  },
  downloadMission(timeoutMs?: number): Promise<MissionPlan> {
    return ipcRenderer.invoke('rust:downloadMission', timeoutMs ?? undefined);
  },
  uploadMission(plan: MissionPlan, timeoutMs?: number): Promise<MissionPlan> {
    return ipcRenderer.invoke('rust:uploadMission', plan, timeoutMs ?? undefined);
  }
});
