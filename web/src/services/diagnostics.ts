// Diagnostics & Admin API

import { API_BASE, authFetch } from './auth';

export interface DiagnosticComponent {
  name: string;
  status: 'ok' | 'warning' | 'error';
  detail: string;
}

export interface DiagnosticsResult {
  status: 'healthy' | 'degraded' | 'unhealthy';
  components: DiagnosticComponent[];
  timestamp: string;
  response_ms: number;
}

export async function fetchDiagnostics(): Promise<DiagnosticsResult> {
  const res = await authFetch(`${API_BASE}/diagnostics`);
  return res.json();
}

export async function adminRestart(): Promise<{ success: boolean; message: string }> {
  const res = await authFetch(`${API_BASE}/admin/restart`, { method: 'POST' });
  return res.json();
}

export async function adminClearLogs(): Promise<{ success: boolean; before_bytes?: number; after_bytes?: number; error?: string }> {
  const res = await authFetch(`${API_BASE}/admin/clear-logs`, { method: 'POST' });
  return res.json();
}
