// History and session APIs

import { API_BASE, authFetch } from './auth';

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

// History query params
export interface HistoryQueryParams {
  range?: 'today' | 'yesterday' | '7days' | '30days' | 'all';
  start_date?: string;
  end_date?: string;
  search?: string;
  page?: number;
  per_page?: number;
  project?: string;
  group_by?: 'window';
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

// ============================================================================
// Grouped history types (session:window grouped view)
// ============================================================================

export interface WindowGroupEntry {
  group_key: string;
  session: string;
  window: string;
  entry_ids: number[];
  task_count: number;
  total_messages: number;
  total_duration: number;
  first_started: string;
  last_ended: string;
  summaries: string[];
}

export interface WindowGroupDateGroup {
  label: string;
  records: WindowGroupEntry[];
}

export interface WindowGroupResponse {
  groups: WindowGroupDateGroup[];
  total: number;
  grouped: boolean;
}

export interface TaskSegment {
  history_id: number;
  summary: string;
  started_at: string;
  ended_at: string;
  message_start_index: number;
  message_count: number;
}

export interface GroupedMessage {
  role: string;
  content: string;
  created_at: string;
  history_id: number;
}

export interface GroupedToolUsage {
  id: number;
  tool_name: string;
  tool_args: string;
  result_summary: string;
  success: boolean;
  timestamp: string;
  history_id: number;
}

export interface GroupedCommit {
  id: number;
  commit_hash: string;
  commit_message: string;
  files_changed: number;
  timestamp: string;
  history_id: number;
}

export interface GroupedDetailResponse {
  segments: TaskSegment[];
  messages: GroupedMessage[];
  tool_usage: GroupedToolUsage[];
  commits: GroupedCommit[];
}

// Fetch grouped detail (merged messages from multiple history entries)
export async function fetchGroupedDetail(project: string, ids: number[]): Promise<GroupedDetailResponse> {
  const searchParams = new URLSearchParams();
  searchParams.set('project', project);
  searchParams.set('ids', ids.join(','));

  const response = await authFetch(`${API_BASE}/projects/history/grouped-detail?${searchParams}`);
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
