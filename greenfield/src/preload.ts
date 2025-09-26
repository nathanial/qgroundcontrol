import { contextBridge, ipcRenderer, IpcRendererEvent } from 'electron';

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
  }
});
