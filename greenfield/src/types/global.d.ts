interface BackendAPI {
  onRustMessage(callback: (message: string) => void): () => void;
}

declare global {
  interface Window {
    backend: BackendAPI;
  }
}

export {};
