// API service for tracker-server

const API_BASE = '/api';
// Use relative WebSocket URL - works with both dev proxy (port 5173) and production (port 3099)
const WS_BASE = `${window.location.protocol === 'https:' ? 'wss:' : 'ws:'}//${window.location.host}/ws`;

// ============================================================================
// Auth Token Management
// ============================================================================

const AUTH_TOKEN_KEY = 'agent-tracker-auth-token';

export function getAuthToken(): string | null {
  return localStorage.getItem(AUTH_TOKEN_KEY);
}

export function setAuthToken(token: string): void {
  localStorage.setItem(AUTH_TOKEN_KEY, token);
}

export function clearAuthToken(): void {
  localStorage.removeItem(AUTH_TOKEN_KEY);
}

/** Authenticated fetch wrapper — injects Bearer token, handles 401 */
export async function authFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  const token = getAuthToken();
  const headers = new Headers(init?.headers);
  if (token) {
    headers.set('Authorization', `Bearer ${token}`);
  }
  const response = await fetch(input, { ...init, headers });
  if (response.status === 401) {
    clearAuthToken();
    window.location.reload();
  }
  return response;
}

/** Verify a token against the server */
export async function verifyToken(token: string): Promise<boolean> {
  try {
    const response = await fetch(`${API_BASE}/auth/verify`, {
      headers: { 'Authorization': `Bearer ${token}` },
    });
    return response.ok;
  } catch {
    return false;
  }
}

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

// WebSocket connection for real-time updates
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

// WebSocket callbacks
interface WebSocketCallbacks {
  onStateUpdate: (msg: RealtimeMessage) => void;
  onStreamChunk?: (chunk: StreamChunk) => void;
  onChatMessage?: (event: ChatMessageEvent) => void;
}

export function connectWebSocket(
  callbacksOrOnMessage: WebSocketCallbacks | ((msg: RealtimeMessage) => void)
): WebSocket {
  const token = getAuthToken();
  const wsUrl = token ? `${WS_BASE}?token=${encodeURIComponent(token)}` : WS_BASE;
  const ws = new WebSocket(wsUrl);

  // Support both old and new callback formats
  const callbacks: WebSocketCallbacks = typeof callbacksOrOnMessage === 'function'
    ? { onStateUpdate: callbacksOrOnMessage }
    : callbacksOrOnMessage;

  ws.onopen = () => {
    console.log('[WS] Connected to tracker-server');
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
    console.log('[WS] Disconnected, reconnecting in 3s...');
    setTimeout(() => connectWebSocket(callbacks), 3000);
  };

  ws.onerror = (error) => {
    console.error('[WS] Error:', error);
  };

  return ws;
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

// Tmux window info (from /api/tmux/windows)
export interface TmuxWindowInfo {
  session_id: string;
  session_name: string;
  window_id: string;
  window_name: string;
  pane_count: number;
  active: boolean;
  git_dir?: string;  // Git directory for the session
}

// Fetch all tmux windows with full details
export async function fetchTmuxWindows(): Promise<TmuxWindowInfo[]> {
  const response = await authFetch(`${API_BASE}/tmux/windows`);
  if (!response.ok) {
    throw new Error(`Failed to fetch tmux windows: ${response.status}`);
  }
  const data = await response.json();
  return data.windows || [];
}

// Execute tmux send-keys command
// suffixKey: key to send after the text (e.g., "Enter", "C-m", "C-s", or empty for none)
export async function tmuxSendKeys(
  session: string,
  window: string,
  pane: string,
  keys: string,
  suffixKey: string = 'Enter'
): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/tmux/send-keys`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session, window, pane, keys, suffix_key: suffixKey }),
  });
  return response.json();
}

// Send image(s) to a tmux pane (saves to temp file, sends path via send-keys)
export async function sendImage(
  session: string,
  windowId: string,
  pane: string,
  imageBase64: string,
  message?: string
): Promise<{ success: boolean; message: string; image_path?: string }> {
  const response = await authFetch(`${API_BASE}/tmux/send-image`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      session,
      window_id: windowId,
      pane,
      image_base64: imageBase64,
      message,
    }),
  });
  return response.json();
}

// Send multiple images to a tmux pane
export async function sendImages(
  session: string,
  windowId: string,
  pane: string,
  imagesBase64: string[],
  message?: string
): Promise<{ success: boolean; message: string; image_paths?: string[] }> {
  const response = await authFetch(`${API_BASE}/tmux/send-image`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      session,
      window_id: windowId,
      pane,
      images: imagesBase64,
      message,
    }),
  });
  return response.json();
}

// Execute arbitrary tmux command (parse and route to appropriate API)
export async function executeTmuxCommand(command: string): Promise<{ success: boolean; message: string }> {
  // Parse tmux send-keys command with quoted keys
  // Format: tmux send-keys -t session:window "keys" [suffix_key]
  // Or: tmux send-keys -t session:window.pane "keys" [suffix_key]
  // suffix_key can be: C-m, C-s, Enter, etc.
  const sendKeysMatch = command.match(/^tmux\s+send-keys\s+-t\s+([^:]+):([^.\s"]+)(?:\.([^\s"]+))?\s+"([^"]+)"\s*(C-[a-z]|Enter)?$/);
  if (sendKeysMatch) {
    const [, session, window, pane = '', keys, suffixKey = ''] = sendKeysMatch;
    return tmuxSendKeys(session, window, pane, keys, suffixKey);
  }

  // Parse tmux send-keys without quotes
  const sendKeysMatch2 = command.match(/^tmux\s+send-keys\s+-t\s+([^:]+):([^.\s]+)(?:\.([^\s]+))?\s+([^\s].+?)\s*(C-[a-z]|Enter)?$/);
  if (sendKeysMatch2) {
    const [, session, window, pane = '', keys, suffixKey = ''] = sendKeysMatch2;
    return tmuxSendKeys(session, window, pane, keys, suffixKey);
  }

  return { success: false, message: `Unknown command format: ${command}` };
}

// Kill a tmux session
export async function tmuxKillSession(session: string): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/tmux/kill-session`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session }),
  });
  if (!response.ok) {
    return { success: false, message: `HTTP ${response.status}` };
  }
  return response.json();
}

// Kill a tmux window
export async function tmuxKillWindow(session: string, window: string): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/tmux/kill-window`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session, window }),
  });
  if (!response.ok) {
    return { success: false, message: `HTTP ${response.status}` };
  }
  return response.json();
}

// Create a new tmux window
export async function tmuxNewWindow(session: string, name: string): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/tmux/new-window`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session, name }),
  });
  if (!response.ok) {
    return { success: false, message: `HTTP ${response.status}` };
  }
  return response.json();
}

// Closed window info
export interface ClosedWindow {
  id: number;
  session_name: string;
  window_name: string;
  working_dir: string;
  git_branch: string;
  pane_count: number;
  closed_at: string | null;
}

// Get closed windows for a session (for resume without worktree)
export async function fetchClosedWindows(sessionName: string): Promise<ClosedWindow[]> {
  const response = await authFetch(`${API_BASE}/tmux/closed-windows/${encodeURIComponent(sessionName)}`);
  if (!response.ok) {
    return [];
  }
  return response.json();
}

// Delete a closed window record
export async function deleteClosedWindow(id: number): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/tmux/closed-windows`, {
    method: 'DELETE',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ id }),
  });
  if (!response.ok) {
    return { success: false, message: `HTTP ${response.status}` };
  }
  return response.json();
}

// Resume a closed window with optional layout
export async function resumeClosedWindow(
  session: string,
  windowName: string,
  workingDir: string,
  layout?: 'simple' | 'default' | 'workspace',
  closedWindowId?: number
): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/tmux/resume-window`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      session,
      window_name: windowName,
      working_dir: workingDir,
      layout: layout || 'simple',
      closed_window_id: closedWindowId,
    }),
  });
  if (!response.ok) {
    return { success: false, message: `HTTP ${response.status}` };
  }
  return response.json();
}

// Select (switch to) a tmux window
// windowId is optional - use it for precise targeting when windows have duplicate names
export async function tmuxSelectWindow(session: string, window: string, windowId?: string): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/tmux/select-window`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session, window, window_id: windowId }),
  });
  if (!response.ok) {
    return { success: false, message: `HTTP ${response.status}` };
  }
  return response.json();
}

// History API Types
export interface ConversationMessage {
  role: string;
  content: string;
  created_at: string;
}

export interface ToolUsageRecord {
  id: number;
  tool_name: string;
  tool_args: string;
  result_summary: string;
  success: boolean;
  timestamp: string;
}

export interface GitCommitRecord {
  id: number;
  commit_hash: string;
  commit_message: string;
  files_changed: number;
  timestamp: string;
}

export interface HistoryDetailStats {
  message_count: number;
  tool_count: number;
  commit_count: number;
  duration_seconds: number;
  tools_used: string[];
}

// Timeline entry from transcript parser (for rich history view)
export interface TimelineToolCallDetail {
  tool_use_id: string;
  tool_name: string;
  args_summary: string;
  args_full?: string;
}

export interface TimelineToolResultDetail {
  tool_use_id: string;
  content: string;
  is_error: boolean;
}

export interface TimelineEntry {
  entry_type: 'text' | 'thinking' | 'tool_call' | 'tool_result';
  timestamp?: string;
  role: 'user' | 'assistant';
  text?: string;
  thinking?: string;
  tool_call?: TimelineToolCallDetail;
  tool_result?: TimelineToolResultDetail;
}

export interface HistoryDetail {
  id: number;
  session: string;
  window: string;
  summary: string;
  completion_note: string;
  started_at: string;
  ended_at: string;
  transcript_path: string;
  resume_command: string;
  messages: ConversationMessage[];
  tool_usage?: ToolUsageRecord[];
  commits?: GitCommitRecord[];
  stats?: HistoryDetailStats;
  timeline?: TimelineEntry[];
}

// Project info (from /api/projects)
export interface ProjectInfo {
  git_dir: string;
  name: string;
  last_session: string;
  last_window: string;
  last_active_at: string | null;
  notes_count: number;
  goals_count: number;
  history_count: number;
}

// Fetch registered projects
export async function fetchProjects(): Promise<ProjectInfo[]> {
  const response = await authFetch(`${API_BASE}/projects`);
  if (!response.ok) return [];
  return response.json();
}

// Fetch project-specific history
export async function fetchProjectHistory(params: HistoryQueryParams = {}): Promise<HistoryResponse> {
  const searchParams = new URLSearchParams();
  if (params.project) searchParams.set('project', params.project);
  if (params.range) searchParams.set('range', params.range);
  if (params.search) searchParams.set('search', params.search);
  if (params.page) searchParams.set('page', String(params.page));
  if (params.per_page) searchParams.set('per_page', String(params.per_page));

  const response = await authFetch(`${API_BASE}/projects/history?${searchParams}`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return response.json();
}

// History query params
export interface HistoryQueryParams {
  range?: 'today' | 'yesterday' | '7days' | '30days' | 'all';
  start_date?: string;
  end_date?: string;
  search?: string;
  page?: number;
  per_page?: number;
  project?: string;
}

export interface HistoryEntry {
  id: number;
  session: string;
  window: string;
  summary: string;
  completion_note: string;
  duration_seconds: number;
  started_at: string;
  ended_at: string;
  message_count: number;
  file_path?: string;  // Session JSONL file path (for session-based entries)
  project?: string;    // Project name (for project-filtered entries)
}

export interface HistoryGroup {
  label: string;
  records: HistoryEntry[];
}

export interface HistoryResponse {
  groups: HistoryGroup[];
  total: number;
}

// Fetch history with filtering
export async function fetchHistory(params: HistoryQueryParams = {}): Promise<HistoryResponse> {
  const searchParams = new URLSearchParams();
  if (params.range) searchParams.set('range', params.range);
  if (params.start_date) searchParams.set('start_date', params.start_date);
  if (params.end_date) searchParams.set('end_date', params.end_date);
  if (params.search) searchParams.set('search', params.search);
  if (params.page) searchParams.set('page', String(params.page));
  if (params.per_page) searchParams.set('per_page', String(params.per_page));

  const response = await authFetch(`${API_BASE}/history?${searchParams}`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return response.json();
}

// Fetch history detail
export async function fetchHistoryDetail(id: number): Promise<HistoryDetail> {
  const response = await authFetch(`${API_BASE}/history/${id}`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return response.json();
}

// Export history as JSON or CSV
export async function exportHistory(
  params: HistoryQueryParams,
  format: 'json' | 'csv' = 'json'
): Promise<Blob> {
  const searchParams = new URLSearchParams();
  if (params.range) searchParams.set('range', params.range);
  if (params.start_date) searchParams.set('start_date', params.start_date);
  if (params.end_date) searchParams.set('end_date', params.end_date);
  if (params.search) searchParams.set('search', params.search);
  searchParams.set('format', format);

  const response = await authFetch(`${API_BASE}/history/export?${searchParams}`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return response.blob();
}

// Fetch sessions (from Claude JSONL files, scanned by background indexer)
export async function fetchSessions(params: HistoryQueryParams = {}): Promise<HistoryResponse> {
  const searchParams = new URLSearchParams();
  if (params.range) searchParams.set('range', params.range);
  if (params.search) searchParams.set('search', params.search);
  if (params.page) searchParams.set('page', String(params.page));
  if (params.per_page) searchParams.set('per_page', String(params.per_page));

  const response = await authFetch(`${API_BASE}/sessions?${searchParams}`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return response.json();
}

// Fetch session detail (parsed on demand from JSONL file)
export async function fetchSessionDetail(filePath: string): Promise<HistoryDetail> {
  const searchParams = new URLSearchParams({ file_path: filePath });
  const response = await authFetch(`${API_BASE}/sessions/detail?${searchParams}`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return response.json();
}

// Claude messages API
export interface InteractiveOption {
  label: string;
  description: string;
}

export interface InteractiveQuestion {
  question: string;
  header: string;
  options: InteractiveOption[];
  multi_select: boolean;
}

export interface ToolInteraction {
  tool_name: string;
  questions: InteractiveQuestion[];
}

export interface ToolCallInfo {
  tool_use_id: string;
  tool_name: string;
  args_summary: string;
  args_full?: string;
}

export interface ToolResultInfo {
  tool_use_id: string;
  content: string;
  is_error: boolean;
}

export interface ClaudeMessage {
  role: string;  // "user" or "assistant"
  timestamp: string;
  text: string;
  thinking?: string;
  interaction?: ToolInteraction;
  tool_calls?: ToolCallInfo[];
  tool_results?: ToolResultInfo[];
}

// Chat message event from WebSocket (real-time JSONL push)
export interface ChatMessageEvent {
  kind: 'chat';
  session_file: string;
  messages: ClaudeMessage[];
}

export interface ClaudeMessagesResponse {
  success: boolean;
  messages: ClaudeMessage[];
  session_file: string;
}

export async function fetchClaudeMessages(
  count: number = 1,
  options?: { project?: string; session?: string; window?: string; pane?: string }
): Promise<ClaudeMessagesResponse> {
  const params = new URLSearchParams({ count: String(count) });
  if (options?.project) {
    params.append('project', options.project);
  }
  if (options?.session) {
    params.append('session', options.session);
  }
  if (options?.window) {
    params.append('window', options.window);
  }
  if (options?.pane) {
    params.append('pane', options.pane);
  }
  // Add cache-busting timestamp to prevent browser caching
  params.append('_t', String(Date.now()));
  const response = await authFetch(`${API_BASE}/claude/messages?${params}`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return response.json();
}

// Claude status API
export interface ClaudeStatus {
  agent_type: 'claude' | 'opencode' | null;  // Detected AI agent type
  action: string | null;
  current_tool: string | null;
  model: string | null;
  context_percent: number | null;
  tokens: number | null;
  cost: number | null;
  session_duration: string | null;
  pane: string | null;  // Detected pane where Claude runs
}

export interface ClaudeStatusResponse {
  success: boolean;
  status: ClaudeStatus;
}

export async function fetchClaudeStatus(
  session: string,
  window: string
): Promise<ClaudeStatusResponse> {
  const params = new URLSearchParams({ session, window });
  const response = await authFetch(`${API_BASE}/tmux/claude-status?${params}`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return response.json();
}

// Tmux capture API
export interface TmuxCaptureResponse {
  content: string;
  cursor_x: number;
  cursor_y: number;
}

export async function fetchTmuxCapture(
  session: string,
  window: string,
  pane: string = '',
  lines?: number
): Promise<TmuxCaptureResponse> {
  const params = new URLSearchParams({ session, window, pane });
  if (lines) {
    params.append('lines', String(lines));
  }
  const response = await authFetch(`${API_BASE}/tmux/capture?${params}`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return response.json();
}

// ============================================
// Workspace APIs (worktree-based windows)
// ============================================

export type LayoutType = 'simple' | 'default' | 'single_service' | 'fullstack' | 'workspace';

export interface StartWorkspaceRequest {
  git_dir: string;
  branch: string;
  base_branch?: string;  // Base branch to create new branch from
  session?: string;
  agent?: string;
  layout?: LayoutType;
  fullstack_mode?: boolean;
  port_base?: number;
  frontend_cmd?: string;
  backend_cmd?: string;
  auto_open_browser?: boolean;
}

export interface StartWorkspaceResponse {
  success: boolean;
  session_name: string;
  worktree_path: string;
  message: string;
  port?: number;
  frontend_port?: number;
  backend_port?: number;
  browser_url?: string;
}

export interface DestroyWorkspaceRequest {
  git_dir: string;
  branch: string;
  session?: string;
  force?: boolean;
  kill_ports?: boolean;
  delete_branch?: boolean;
}

export interface ResumeWorkspaceRequest {
  git_dir: string;
  branch: string;
  session?: string;
  agent?: string;
  layout?: LayoutType;
}

// Start a new workspace with worktree
export async function startWorkspace(req: StartWorkspaceRequest): Promise<StartWorkspaceResponse> {
  const response = await authFetch(`${API_BASE}/workspace/start`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
  return response.json();
}

// Destroy a workspace (delete worktree + tmux window)
export async function destroyWorkspace(req: DestroyWorkspaceRequest): Promise<{ success: boolean; message: string }> {
  const response = await authFetch(`${API_BASE}/workspace/destroy`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
  return response.json();
}

// Resume a workspace (reopen existing worktree)
export async function resumeWorkspace(req: ResumeWorkspaceRequest): Promise<StartWorkspaceResponse> {
  const response = await authFetch(`${API_BASE}/workspace/resume`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  });
  return response.json();
}

// Close a window (just close tmux window, keep worktree)
export async function closeWindow(session: string, window: string): Promise<{ success: boolean; message: string }> {
  // Just kill the tmux window without destroying worktree
  return tmuxKillWindow(session, window);
}

// ============================================
// Git APIs
// ============================================

export interface BranchInfo {
  name: string;
  has_worktree: boolean;
  worktree_path?: string;
}

export interface GitBranchesResponse {
  branches: string[];
  local: string[];
  remote: string[];
  branches_with_status?: BranchInfo[];
}

// Config types (from /api/config)
export interface AgentDef {
  command: string;
  color?: string;
  icon?: string;
}

export interface ConfigDefaults {
  layout: string;
  agent: string;
}

export interface ConfigResponse {
  agents: Record<string, AgentDef>;
  defaults: ConfigDefaults;
}

// Fetch server config (agents, defaults)
export async function fetchConfig(): Promise<ConfigResponse> {
  const response = await authFetch(`${API_BASE}/config`);
  if (!response.ok) {
    throw new Error(`Failed to fetch config: ${response.status}`);
  }
  return response.json();
}

// Fetch git branches for a repository
export async function fetchGitBranches(gitDir?: string): Promise<GitBranchesResponse> {
  const params = gitDir ? `?git_dir=${encodeURIComponent(gitDir)}` : '';
  const response = await authFetch(`${API_BASE}/git/branches${params}`);
  if (!response.ok) {
    throw new Error(`Failed to fetch branches: ${response.status}`);
  }
  return response.json();
}

// ============================================================================
// Project Environment & Worktree Isolation
// ============================================================================

export interface ProjectEnvVar {
  id: number;
  session_name: string;
  key: string;
  value: string;
  is_secret: number;
  sort_order: number;
  created_at: string;
  updated_at: string;
}

export interface ProjectService {
  id: number;
  session_name: string;
  service_name: string;
  base_value: number;
  value_type: string;
  env_key: string;
  sort_order: number;
}

export interface WorktreeSlot {
  id: number;
  session_name: string;
  slot: number;
  branch: string;
  worktree_path: string | null;
  created_at: string;
}

// Project Env Vars
export async function fetchProjectEnvVars(sessionName: string): Promise<ProjectEnvVar[]> {
  const res = await authFetch(`${API_BASE}/project/env-vars?session_name=${encodeURIComponent(sessionName)}`);
  return res.ok ? res.json() : [];
}

export async function createProjectEnvVar(sessionName: string, key: string, value: string, isSecret = false) {
  return authFetch(`${API_BASE}/project/env-vars`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session_name: sessionName, key, value, is_secret: isSecret }),
  }).then(r => r.json());
}

export async function updateProjectEnvVar(id: number, updates: { key?: string; value?: string; is_secret?: boolean; sort_order?: number }) {
  return authFetch(`${API_BASE}/project/env-vars/${id}`, {
    method: 'PUT', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(updates),
  }).then(r => r.json());
}

export async function deleteProjectEnvVar(id: number) {
  return authFetch(`${API_BASE}/project/env-vars/${id}`, { method: 'DELETE' }).then(r => r.json());
}

// Project Services
export async function fetchProjectServices(sessionName: string): Promise<ProjectService[]> {
  const res = await authFetch(`${API_BASE}/project/services?session_name=${encodeURIComponent(sessionName)}`);
  return res.ok ? res.json() : [];
}

export async function createProjectService(sessionName: string, serviceName: string, baseValue: number, valueType: string, envKey: string) {
  return authFetch(`${API_BASE}/project/services`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session_name: sessionName, service_name: serviceName, base_value: baseValue, value_type: valueType, env_key: envKey }),
  }).then(r => r.json());
}

export async function updateProjectService(id: number, updates: { service_name?: string; base_value?: number; value_type?: string; env_key?: string; sort_order?: number }) {
  return authFetch(`${API_BASE}/project/services/${id}`, {
    method: 'PUT', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(updates),
  }).then(r => r.json());
}

export async function deleteProjectService(id: number) {
  return authFetch(`${API_BASE}/project/services/${id}`, { method: 'DELETE' }).then(r => r.json());
}

// Worktree Slots
export async function fetchWorktreeSlots(sessionName: string): Promise<WorktreeSlot[]> {
  const res = await authFetch(`${API_BASE}/project/worktree-slots?session_name=${encodeURIComponent(sessionName)}`);
  return res.ok ? res.json() : [];
}

export async function createWorktreeSlot(sessionName: string, branch: string, worktreePath?: string) {
  return authFetch(`${API_BASE}/project/worktree-slots`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session_name: sessionName, branch, worktree_path: worktreePath }),
  }).then(r => r.json());
}

export async function deleteWorktreeSlot(id: number) {
  return authFetch(`${API_BASE}/project/worktree-slots/${id}`, { method: 'DELETE' }).then(r => r.json());
}

// Helper: Format duration
export function formatDuration(seconds: number): string {
  if (seconds < 60) return `${Math.floor(seconds)}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  const hours = Math.floor(seconds / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  return `${hours}h${mins}m`;
}

// Helper: Format time from ISO string
export function formatTime(isoString: string | null): string {
  if (!isoString) return '--:--';
  const date = new Date(isoString);
  return date.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', hour12: false });
}
