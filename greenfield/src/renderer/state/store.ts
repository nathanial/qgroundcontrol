import { createStore } from 'zustand/vanilla';
import { subscribeWithSelector } from 'zustand/middleware';
import type {
  ConnectionStatus,
  DeviceDescriptor,
  MissionItem,
  MissionPlan,
  MissionSyncStatus,
  ParameterValue,
  VehicleStatus
} from '../../../rust-core/index';
import { MissionFrame } from '../../../rust-core/index';

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
  missionPlan: MissionPlan | null;
  missionDraft: MissionPlan | null;
  missionSync: MissionSyncStatus | null;
  selectedMissionIndex: number | null;
  setDevices(devices: DeviceDescriptor[]): void;
  upsertDevice(device: DeviceDescriptor): void;
  removeDevice(deviceId: string): void;
  setConnectionStatus(status: ConnectionStatus | null): void;
  setVehicleStatus(status: VehicleStatus | null): void;
  setParameterValues(values: ParameterValue[], timestamp: number): void;
  setParameterProgress(received: number, expected?: number): void;
  appendLog(entry: LogEntry): void;
  resetLogs(): void;
  setMissionPlan(plan: MissionPlan): void;
  setMissionDraft(plan: MissionPlan): void;
  updateMissionItem(index: number, updates: Partial<MissionItem>): void;
  addMissionWaypoint(
    coordinate: { latitudeDeg: number; longitudeDeg: number },
    altitude?: number
  ): void;
  removeMissionWaypoint(index: number): void;
  setMissionSync(status: MissionSyncStatus | null): void;
  setSelectedMissionIndex(index: number | null): void;
};

const DEVICE_LIMIT = 128;
const LOG_LIMIT = 200;

const sortDevices = (devices: DeviceDescriptor[]): DeviceDescriptor[] =>
  devices.slice().sort((a, b) => a.id.localeCompare(b.id));

const sortParameters = (parameters: ParameterValue[]): ParameterValue[] =>
  parameters.slice().sort((a, b) => a.name.localeCompare(b.name));

const cloneMissionItem = (item: MissionItem): MissionItem => ({
  ...item
});

export const cloneMissionPlan = (plan: MissionPlan): MissionPlan => ({
  ...plan,
  items: plan.items.map(cloneMissionItem),
  home: plan.home ? { ...plan.home } : undefined
});

const normalizeMissionPlanDraft = (plan: MissionPlan): MissionPlan => {
  const normalized = cloneMissionPlan(plan);
  normalized.items = normalized.items.map((item, index) => ({
    ...item,
    seq: index,
    isCurrent: index === 0,
    autoContinue: item.autoContinue ?? true
  }));
  normalized.lastModifiedMillis = Date.now();
  return normalized;
};

export const createMissionDraftSkeleton = (base?: MissionPlan | null): MissionPlan => {
  if (base) {
    return normalizeMissionPlanDraft(cloneMissionPlan(base));
  }

  return normalizeMissionPlanDraft({
    planId: 'mission-draft',
    revision: 0,
    items: [],
    home: undefined,
    lastModifiedMillis: Date.now(),
    notes: undefined
  } as MissionPlan);
};

const DEFAULT_ALTITUDE = 50;

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
  missionPlan: null,
  missionDraft: null,
  missionSync: null,
  selectedMissionIndex: null,
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
  },
  setMissionPlan(plan) {
    set(() => {
      const normalized = normalizeMissionPlanDraft(plan);
      return {
        missionPlan: normalized,
        missionDraft: normalizeMissionPlanDraft(normalized),
        selectedMissionIndex: normalized.items.length > 0 ? 0 : null
      };
    });
  },
  setMissionDraft(plan) {
    set(() => ({ missionDraft: normalizeMissionPlanDraft(plan) }));
  },
  updateMissionItem(index, updates) {
    set((state) => {
      if (!state.missionDraft || !state.missionDraft.items[index]) {
        return {};
      }
      const draft = normalizeMissionPlanDraft(state.missionDraft);
      draft.items[index] = {
        ...draft.items[index],
        ...updates
      };
      return { missionDraft: normalizeMissionPlanDraft(draft) };
    });
  },
  addMissionWaypoint(coordinate, altitude) {
    set((state) => {
      const draft = state.missionDraft
        ? normalizeMissionPlanDraft(state.missionDraft)
        : createMissionDraftSkeleton(state.missionPlan);
      const nextAltitude =
        altitude ?? draft.items[draft.items.length - 1]?.altitudeM ?? DEFAULT_ALTITUDE;
      const newItem: MissionItem = {
        seq: draft.items.length,
        command: 16,
        frame: MissionFrame.GlobalRelativeAlt,
        latitudeDeg: coordinate.latitudeDeg,
        longitudeDeg: coordinate.longitudeDeg,
        altitudeM: nextAltitude,
        param1: 0,
        param2: 0,
        param3: 0,
        param4: 0,
        autoContinue: true,
        isCurrent: draft.items.length === 0
      };
      draft.items = [...draft.items, newItem];
      const normalized = normalizeMissionPlanDraft(draft);
      return {
        missionDraft: normalized,
        selectedMissionIndex: normalized.items.length - 1
      };
    });
  },
  removeMissionWaypoint(index) {
    set((state) => {
      if (!state.missionDraft || index < 0 || index >= state.missionDraft.items.length) {
        return {};
      }
      const draft = normalizeMissionPlanDraft(state.missionDraft);
      draft.items = draft.items.filter((_, idx) => idx !== index);
      const normalized = normalizeMissionPlanDraft(draft);
      const nextIndex = normalized.items.length === 0 ? null : Math.min(index, normalized.items.length - 1);
      return {
        missionDraft: normalized,
        selectedMissionIndex: nextIndex
      };
    });
  },
  setMissionSync(status) {
    set(() => ({ missionSync: status ?? null }));
  },
  setSelectedMissionIndex(index) {
    set(() => ({ selectedMissionIndex: index }));
  }
})));
