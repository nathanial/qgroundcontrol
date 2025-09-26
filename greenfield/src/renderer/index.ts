type RustEnvelope = {
  level?: string;
  message?: string;
  [key: string]: unknown;
};

const statusBadge = document.getElementById('status-badge');
const logContainer = document.getElementById('rust-log');

if (statusBadge) {
  statusBadge.textContent = 'Awaiting core signal…';
}

window.backend.onRustMessage((payload: string) => {
  let entryText = payload;
  let level: string | undefined;

  try {
    const envelope = JSON.parse(payload) as RustEnvelope;
    if (typeof envelope.message === 'string') {
      entryText = envelope.message;
    }
    if (typeof envelope.level === 'string') {
      level = envelope.level;
    }
  } catch {
    // Ignore JSON parse failures and use the raw payload
  }

  if (statusBadge) {
    statusBadge.textContent = 'Connected to rust-core';
  }

  if (logContainer) {
    const entry = document.createElement('div');
    entry.className = 'log-entry';
    entry.dataset.level = level ?? 'info';
    entry.textContent = entryText;
    logContainer.prepend(entry);
  }
});
