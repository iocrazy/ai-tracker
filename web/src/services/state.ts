// State management, WebSocket, and stream APIs

import { API_BASE, WS_BASE, authFetch, getAuthToken } from './auth';
import type { ChatMessageEvent } from './claude';
import type { TmuxWindowInfo } from './tmux';

// Backend types (from tracker-server)
export interface BackendTask {
  session_id: string;
  session: string;
  window_id: string;
  window: string;
  pane: string;
  status: 'in_progress' | 'awaiting_input' | 'completed';
  summary: string;
  completion_note: string;
  started_at: string | null;
  completed_at: string | null;
  duration_seconds: number;
  acknowledged: boolean;
  archived: boolean;
}

export interface BackendNote {
  id: string;
  scope: 'window' | 'session' | 'all';
  session_id: string;
  session: string;
  window_id: string;
  window: string;
  pane: string;
  summary: string;
  completed: boolean;
  archived: boolean;
}

export interface BackendGoal {
  id: string;
  summary: string;
  completed: boolean;
}

export interface BackendHistoryRecord {
  id: number;
  session_id: string;
  session: string;
  window_id: string;
  window: string;
  pane: string;
  summary: string;
  completion_note: string;
  started_at: string | null;
  completed_at: string | null;
  duration_seconds: number;
}

export interface BackendState {
  kind: string;
  tasks: BackendTask[];
  archived_tasks: BackendTask[];
  notes: BackendNote[];
  archived: BackendNote[];
  goals: BackendGoal[];
  history: BackendHistoryRecord[];
  message: string;
  changed?: string[];  // Tables that changed (for selective re-rendering)
}

// Fetch current state
export async function fetchState(): Promise<BackendState> {
  const response = await authFetch(`${API_BASE}/state`);
  if (!response.ok) {
    throw new Error(`Failed to fetch state: ${response.status}`);
  }
  return response.json();
}

// Send a command
export async function sendCommand(command: string, params: Record<string, string> = {}): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/command`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ command, ...params }),
  });
  return response.json();
}

// Realtime message from WebSocket (state + tmux windows)
export interface RealtimeMessage {
  state: BackendState;
  tmux_windows: TmuxWindowInfo[];
}

// Stream chunk from real-time pane output
export interface StreamChunk {
  pane_id: string;
  target: string;
  raw: string;
  text: string;
  timestamp: string;
}

// Stream message from WebSocket
export interface StreamMessage {
  kind: 'stream';
  chunk: StreamChunk;
}

// Hook event types (from backend WebSocket broadcasts)
export interface HookChatMessage {
  type: 'chat_message';
  claude_session_id: string;
  git_dir: string;
  session_name: string;
  window_id: string;
  role: string;
  content: string;
  agent_type?: string;
  timestamp: string;
}

export interface HookToolEvent {
  type: 'tool_event';
  claude_session_id: string;
  git_dir: string;
  tool_name: string;
  tool_use_id: string;
  timestamp: string;
}

export interface HookSessionUpdate {
  type: 'hook_session_update';
  claude_session_id: string;
  git_dir: string;
  session_name: string;
  window_id: string;
  event: string;
}

// WebSocket callbacks
export type ConnectionStatus = 'connected' | 'reconnecting' | 'offline';

interface WebSocketCallbacks {
  onStateUpdate: (msg: RealtimeMessage) => void;
  onStreamChunk?: (chunk: StreamChunk) => void;
  onChatMessage?: (event: ChatMessageEvent) => void;
  onHookChatMessage?: (msg: HookChatMessage) => void;
  onHookToolEvent?: (msg: HookToolEvent) => void;
  onHookSessionUpdate?: (msg: HookSessionUpdate) => void;
  onConnectionChange?: (status: ConnectionStatus, retryCount?: number) => void;
}

// Reconnection state (module-level so it persists across calls)
let _retryCount = 0;
let _reconnectTimer: ReturnType<typeof setTimeout> | null = null;
let _currentWs: WebSocket | null = null;
let _intentionalClose = false;  // Suppress auto-reconnect on explicit disconnect
let _lastConnectTime = 0;  // Debounce rapid-fire connect calls
const BASE_DELAY = 1000;
const MAX_DELAY = 30000;
const CONNECT_DEBOUNCE_MS = 3000;  // Min 3s between connect attempts

export function connectWebSocket(
  callbacksOrOnMessage: WebSocketCallbacks | ((msg: RealtimeMessage) => void)
): WebSocket {
  // Debounce: if we just connected within 3s, return existing WS
  const now = Date.now();
  if (_currentWs && (now - _lastConnectTime) < CONNECT_DEBOUNCE_MS
      && _currentWs.readyState <= WebSocket.OPEN) {
    console.log('[WS] Debounced — already connected/connecting within 3s');
    return _currentWs;
  }
  _lastConnectTime = now;

  // Clear any pending reconnect timer
  if (_reconnectTimer) {
    clearTimeout(_reconnectTimer);
    _reconnectTimer = null;
  }

  // Close any existing WS to prevent duplicate connections
  if (_currentWs && _currentWs.readyState <= WebSocket.OPEN) {
    _intentionalClose = true;
    _currentWs.close();
  }

  const token = getAuthToken();
  const wsUrl = token ? `${WS_BASE}?token=${encodeURIComponent(token)}` : WS_BASE;
  const ws = new WebSocket(wsUrl);
  _currentWs = ws;

  // Support both old and new callback formats
  const callbacks: WebSocketCallbacks = typeof callbacksOrOnMessage === 'function'
    ? { onStateUpdate: callbacksOrOnMessage }
    : callbacksOrOnMessage;

  let pingInterval: ReturnType<typeof setInterval> | null = null;

  ws.onopen = () => {
    console.log('[WS] Connected to tracker-server');
    _retryCount = 0;
    callbacks.onConnectionChange?.('connected');
    // Client-side heartbeat: send ping every 25s to keep connection alive through proxies
    pingInterval = setInterval(() => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send('ping');
      }
    }, 25000);
  };

  ws.onmessage = (event) => {
    try {
      const data = JSON.parse(event.data);

      // Handle stream messages
      if (data.kind === 'stream' && callbacks.onStreamChunk) {
        callbacks.onStreamChunk(data.chunk as StreamChunk);
        return;
      }

      // Handle chat message events
      if (data.kind === 'chat' && callbacks.onChatMessage) {
        callbacks.onChatMessage(data as ChatMessageEvent);
        return;
      }

      // Handle hook chat messages
      if (data.type === 'chat_message' && callbacks.onHookChatMessage) {
        callbacks.onHookChatMessage(data as HookChatMessage);
        return;
      }

      // Handle hook tool events
      if (data.type === 'tool_event' && callbacks.onHookToolEvent) {
        callbacks.onHookToolEvent(data as HookToolEvent);
        return;
      }

      // Handle hook session updates (start/end)
      if (data.type === 'hook_session_update' && callbacks.onHookSessionUpdate) {
        callbacks.onHookSessionUpdate(data as HookSessionUpdate);
        return;
      }

      // Handle state messages (new format: state + tmux_windows)
      if (data.state && data.tmux_windows) {
        callbacks.onStateUpdate(data as RealtimeMessage);
      } else if (data.kind === 'state') {
        // Legacy format compatibility
        callbacks.onStateUpdate({
          state: data as BackendState,
          tmux_windows: [],
        });
      }
    } catch (e) {
      console.error('[WS] Failed to parse message:', e);
    }
  };

  ws.onclose = () => {
    if (pingInterval) { clearInterval(pingInterval); pingInterval = null; }

    // Skip auto-reconnect if this was an intentional close (e.g., disconnect or replacement)
    if (_intentionalClose) {
      _intentionalClose = false;
      return;
    }

    // Only auto-reconnect if this is still the current WS (avoid ghost reconnects)
    if (_currentWs !== ws) return;

    _retryCount++;
    const delay = Math.min(BASE_DELAY * Math.pow(2, _retryCount - 1), MAX_DELAY) + Math.random() * 1000;
    console.log(`[WS] Disconnected, reconnecting in ${(delay / 1000).toFixed(1)}s (attempt #${_retryCount})...`);
    callbacks.onConnectionChange?.('reconnecting', _retryCount);
    _reconnectTimer = setTimeout(() => connectWebSocket(callbacks), delay);
  };

  ws.onerror = (error) => {
    console.error('[WS] Error:', error);
  };

  return ws;
}

/** Disconnect WebSocket without auto-reconnect */
export function disconnectWebSocket(): void {
  if (_reconnectTimer) {
    clearTimeout(_reconnectTimer);
    _reconnectTimer = null;
  }
  if (_currentWs) {
    _intentionalClose = true;
    _currentWs.close();
    _currentWs = null;
  }
}

/** Get current WebSocket instance (for readyState checks) */
export function getCurrentWs(): WebSocket | null {
  return _currentWs;
}

/** Subscribe to real-time updates for a specific JSONL session file */
export function subscribeChatFile(sessionFile: string): void {
  if (_currentWs && _currentWs.readyState === WebSocket.OPEN) {
    _currentWs.send(JSON.stringify({ type: 'subscribe_chat', session_file: sessionFile }));
  }
}

/** Unsubscribe from a specific JSONL session file */
export function unsubscribeChatFile(sessionFile: string): void {
  if (_currentWs && _currentWs.readyState === WebSocket.OPEN) {
    _currentWs.send(JSON.stringify({ type: 'unsubscribe_chat', session_file: sessionFile }));
  }
}

// Health check API (no auth required)
export async function fetchHealth(): Promise<{ status: string; checks: Record<string, any>; response_ms: number } | null> {
  try {
    const res = await fetch(`${API_BASE}/health`);
    if (!res.ok) return null;
    return res.json();
  } catch {
    return null;
  }
}

// Stream control APIs
export async function startStream(
  session: string,
  window: string,
  pane: string
): Promise<{ success: boolean; target: string; message: string }> {
  const response = await authFetch(`${API_BASE}/stream/start`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session, window, pane }),
  });
  return response.json();
}

export async function stopStream(pane: string): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/stream/stop`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ pane }),
  });
  return response.json();
}

export interface StreamEntry {
  pane_id: string;
  target: string;
}

export async function listStreams(): Promise<StreamEntry[]> {
  const response = await authFetch(`${API_BASE}/stream/list`);
  const data = await response.json();
  return data.streams || [];
}

// Log viewer API
export interface LogEntry {
  timestamp: string;
  level: string;
  module: string;
  message: string;
}

export async function fetchLogs(params: { limit?: number; level?: string; search?: string } = {}): Promise<{ entries: LogEntry[]; total: number }> {
  const searchParams = new URLSearchParams();
  if (params.limit) searchParams.set('limit', String(params.limit));
  if (params.level) searchParams.set('level', params.level);
  if (params.search) searchParams.set('search', params.search);

  const response = await authFetch(`${API_BASE}/logs?${searchParams}`);
  if (!response.ok) return { entries: [], total: 0 };
  return response.json();
}

// Notification API
export interface NotificationEntry {
  id: number;
  type: string;
  session_name: string | null;
  message: string;
  read: number;
  created_at: string;
}

export async function fetchNotifications(unreadOnly = false, limit = 50): Promise<NotificationEntry[]> {
  const params = new URLSearchParams();
  if (unreadOnly) params.set('unread_only', 'true');
  params.set('limit', String(limit));
  const res = await authFetch(`${API_BASE}/notifications?${params}`);
  if (!res.ok) return [];
  const data = await res.json();
  return data.notifications || [];
}

export async function fetchUnreadCount(): Promise<number> {
  const res = await authFetch(`${API_BASE}/notifications/count`);
  if (!res.ok) return 0;
  const data = await res.json();
  return data.count || 0;
}

export async function markNotificationRead(id: number): Promise<void> {
  await authFetch(`${API_BASE}/notifications/${id}/read`, { method: 'POST' });
}

export async function markAllNotificationsRead(): Promise<void> {
  await authFetch(`${API_BASE}/notifications/read-all`, { method: 'POST' });
}

// Alert Rules API
export interface AlertRule {
  id: number;
  name: string;
  condition_type: string;
  threshold_seconds: number | null;
  enabled: number;
  channels: string;
  created_at: string;
}

export async function fetchAlertRules(): Promise<AlertRule[]> {
  const res = await authFetch(`${API_BASE}/alert-rules`);
  if (!res.ok) return [];
  const data = await res.json();
  return data.rules || [];
}

export async function createAlertRule(name: string, conditionType: string, thresholdSeconds?: number, channels = 'web'): Promise<{ success: boolean; id?: number }> {
  const res = await authFetch(`${API_BASE}/alert-rules`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name, condition_type: conditionType, threshold_seconds: thresholdSeconds, channels }),
  });
  return res.json();
}

export async function updateAlertRule(id: number, updates: { enabled?: boolean; threshold_seconds?: number; channels?: string }): Promise<void> {
  await authFetch(`${API_BASE}/alert-rules/${id}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(updates),
  });
}

export async function deleteAlertRule(id: number): Promise<void> {
  await authFetch(`${API_BASE}/alert-rules/${id}`, { method: 'DELETE' });
}

// Backup API
export interface BackupEntry {
  name: string;
  path: string;
  size: number;
  created: string;
}

export async function createBackup(): Promise<{ success: boolean; path?: string }> {
  const res = await authFetch(`${API_BASE}/backup`, { method: 'POST' });
  return res.json();
}

export async function fetchBackups(): Promise<BackupEntry[]> {
  const res = await authFetch(`${API_BASE}/backup/list`);
  if (!res.ok) return [];
  const data = await res.json();
  return data.backups || [];
}

// ============================================================================
// Channel management (API key → tmux session routing)
// ============================================================================

export interface ApiChannel {
  id: number;
  name: string;
  api_key: string;
  session_name: string;
  window_name: string;
  created_at: string;
}

export async function fetchChannels(): Promise<ApiChannel[]> {
  const res = await authFetch(`${API_BASE}/channels`);
  if (!res.ok) return [];
  const data = await res.json();
  return data.channels || [];
}

export async function createChannel(name: string, session_name: string, window_name: string): Promise<{ success: boolean; channel?: ApiChannel }> {
  const res = await authFetch(`${API_BASE}/channels`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name, session_name, window_name }),
  });
  return res.json();
}

export async function deleteChannel(id: number): Promise<{ success: boolean }> {
  const res = await authFetch(`${API_BASE}/channels/${id}`, { method: 'DELETE' });
  return res.json();
}
