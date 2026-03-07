// Setup status API (unauthenticated)

export interface SetupStatus {
  server_running: boolean;
  claude_hooks_configured: boolean;
  setup_complete: boolean;
}

export async function fetchSetupStatus(): Promise<SetupStatus> {
  const res = await fetch('/api/setup/status');
  if (!res.ok) {
    return { server_running: true, claude_hooks_configured: false, setup_complete: false };
  }
  return res.json();
}
