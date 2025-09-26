import { app, BrowserWindow, ipcMain } from 'electron';
import fs from 'node:fs';
import path from 'node:path';
import type { StatusMessage, VehicleStatus } from '../rust-core/index';

const RUST_CHANNEL = 'rust-message';
const pendingRustMessages: string[] = [];
let mainWindow: BrowserWindow | null = null;

interface RustCoreModule {
  healthCheck(): StatusMessage;
  runDiagnostics(timeoutMs?: number): Promise<StatusMessage>;
  bootstrapVehicleStatus(): VehicleStatus;
  simulateFailure(): void;
  describeStatusChannel(): string;
  version(): string;
}

function resolveRustModulePath(): string {
  const rustCoreDir = path.join(__dirname, '..', 'rust-core');
  const defaultPath = path.join(rustCoreDir, 'index.node');

  if (fs.existsSync(defaultPath)) {
    return defaultPath;
  }

  try {
    const entries = fs.readdirSync(rustCoreDir);
    const candidate = entries.find((entry) => entry.startsWith('index.') && entry.endsWith('.node'));

    if (candidate) {
      return path.join(rustCoreDir, candidate);
    }
  } catch (error) {
    forwardRustJSONObject({
      level: 'warn',
      message: `Unable to inspect rust-core directory for napi module: ${(error as Error).message}`
    });
  }

  return defaultPath;
}

const rustModulePath = resolveRustModulePath();
let rustCore: RustCoreModule | null = null;

try {
  // eslint-disable-next-line @typescript-eslint/no-var-requires, global-require
  rustCore = require(rustModulePath) as RustCoreModule;
} catch (error) {
  forwardRustJSONObject({
    level: 'error',
    message: `Failed to load napi module at ${rustModulePath}: ${(error as Error).message}`
  });
}

setupIpcHandlers();

function ensureRustCore(): RustCoreModule {
  if (!rustCore) {
    throw new Error('rust-core module unavailable');
  }

  return rustCore;
}

function normalizeRustError(error: unknown, fallback: string) {
  if (error && typeof error === 'object') {
    const err = error as { message?: string; code?: string };
    return {
      level: 'error',
      kind: err.code ?? 'RustCoreError',
      message: err.message ?? fallback
    };
  }

  return { level: 'error', kind: 'RustCoreError', message: fallback };
}

function setupIpcHandlers(): void {
  ipcMain.handle('rust:runDiagnostics', async (_event, timeoutMs?: number) => {
    const core = ensureRustCore();

    try {
      const result = await core.runDiagnostics(timeoutMs);
      forwardRustJSONObject({ level: 'info', ...result });
      return result;
    } catch (error) {
      const message = normalizeRustError(error, 'runDiagnostics failed');
      forwardRustJSONObject(message);
      throw error;
    }
  });

  ipcMain.handle('rust:simulateFailure', async () => {
    const core = ensureRustCore();

    try {
      core.simulateFailure();
      const message = { level: 'warn', kind: 'diagnostic', message: 'simulateFailure completed without error' };
      forwardRustJSONObject(message);
      return message;
    } catch (error) {
      const message = normalizeRustError(error, 'simulateFailure failed');
      forwardRustJSONObject(message);
      throw error;
    }
  });

  ipcMain.handle('rust:bootstrapStatus', async () => {
    const core = ensureRustCore();

    const vehicle = core.bootstrapVehicleStatus();
    forwardRustJSONObject({ level: 'info', kind: 'vehicle-bootstrap', vehicle });
    return vehicle;
  });
}

function createWindow(): void {
  mainWindow = new BrowserWindow({
    width: 1024,
    height: 768,
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
      preload: path.join(__dirname, 'preload.js')
    }
  });

  mainWindow.on('closed', () => {
    mainWindow = null;
  });

  mainWindow.webContents.once('did-finish-load', () => {
    while (pendingRustMessages.length > 0) {
      const message = pendingRustMessages.shift();
      if (message && !mainWindow?.webContents.isDestroyed()) {
        mainWindow?.webContents.send(RUST_CHANNEL, message);
      }
    }
  });

  void mainWindow.loadFile(path.join(__dirname, 'renderer/index.html'));
}

function forwardRustMessage(message: string): void {
  const webContents = mainWindow?.webContents;
  if (webContents && !webContents.isDestroyed() && !webContents.isLoadingMainFrame()) {
    webContents.send(RUST_CHANNEL, message);
  } else {
    pendingRustMessages.push(message);
  }
}

function forwardRustJSONObject(payload: Record<string, unknown>): void {
  forwardRustMessage(JSON.stringify(payload));
}

function emitRustStatus(): void {
  try {
    const core = ensureRustCore();
    const status = core.healthCheck();
    forwardRustJSONObject({ level: 'info', ...status });

    const version = core.version();
    forwardRustJSONObject({ level: 'debug', kind: 'version', message: `rust-core v${version}` });

    const channelDescription = core.describeStatusChannel();
    forwardRustJSONObject({ level: 'debug', kind: 'status-channel', message: channelDescription });

    const vehicle = core.bootstrapVehicleStatus();
    forwardRustJSONObject({ level: 'info', kind: 'vehicle-bootstrap', vehicle });
  } catch (error) {
    const message = normalizeRustError(error, 'health check failed');
    forwardRustJSONObject(message);
  }
}

app.whenReady().then(() => {
  createWindow();
  emitRustStatus();

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createWindow();
      emitRustStatus();
    }
  });
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') {
    app.quit();
  }
});
