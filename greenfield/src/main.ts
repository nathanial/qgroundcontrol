import { app, BrowserWindow } from 'electron';
import fs from 'node:fs';
import path from 'node:path';

const RUST_CHANNEL = 'rust-message';
const pendingRustMessages: string[] = [];
let mainWindow: BrowserWindow | null = null;

interface RustCoreModule {
  healthCheck(): { kind: string; message: string };
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
  if (!rustCore) {
    forwardRustJSONObject({ level: 'error', message: 'rust-core module unavailable' });
    return;
  }

  try {
    const status = rustCore.healthCheck();
    forwardRustJSONObject({ level: 'info', ...status });
  } catch (error) {
    forwardRustJSONObject({ level: 'error', message: `health_check failed: ${(error as Error).message}` });
  }

  try {
    const version = rustCore.version();
    forwardRustJSONObject({ level: 'debug', kind: 'version', message: `rust-core v${version}` });
  } catch (error) {
    forwardRustJSONObject({ level: 'error', message: `version lookup failed: ${(error as Error).message}` });
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
