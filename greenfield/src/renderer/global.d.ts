export {};

declare global {
  interface Window {
    backend: {
      onRustMessage: (callback: (payload: string) => void) => () => void;
    };
  }
}
