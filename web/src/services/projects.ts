// Projects, env vars, services, and worktree slot APIs

import { API_BASE, authFetch } from './auth';
import type { HistoryQueryParams, HistoryResponse } from './history';

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
  description: string;
  status: string;
  tags: string;
  created_at: string;
  tech_stack: string;
  todos_count: number;
}

// Project todo item
export interface ProjectTodo {
  id: number;
  git_dir: string;
  title: string;
  description: string;
  status: 'todo' | 'in_progress' | 'done';
  priority: number;
  sort_order: number;
  created_at: string;
  updated_at: string;
}

// Fetch registered projects
export async function fetchProjects(): Promise<ProjectInfo[]> {
  const response = await authFetch(`${API_BASE}/projects`);
  if (!response.ok) return [];
  return response.json();
}

// Fetch project-specific history (flat or grouped by session:window)
export async function fetchProjectHistory(params: HistoryQueryParams = {}): Promise<any> {
  const searchParams = new URLSearchParams();
  if (params.project) searchParams.set('project', params.project);
  if (params.range) searchParams.set('range', params.range);
  if (params.search) searchParams.set('search', params.search);
  if (params.page) searchParams.set('page', String(params.page));
  if (params.per_page) searchParams.set('per_page', String(params.per_page));
  if (params.group_by) searchParams.set('group_by', params.group_by);

  const response = await authFetch(`${API_BASE}/projects/history?${searchParams}`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return response.json();
}

// Project Environment Variables
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
export interface ProjectService {
  id: number;
  session_name: string;
  service_name: string;
  base_value: number;
  value_type: string;
  env_key: string;
  sort_order: number;
}

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
export interface WorktreeSlot {
  id: number;
  session_name: string;
  slot: number;
  branch: string;
  worktree_path: string | null;
  created_at: string;
}

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

// Global Env Vars
export interface GlobalEnvVar {
  id: number;
  key: string;
  value: string;
  is_secret: number;
  sort_order: number;
  created_at: string;
  updated_at: string;
}

export async function fetchGlobalEnvVars(): Promise<GlobalEnvVar[]> {
  const res = await authFetch(`${API_BASE}/global/env-vars`);
  return res.ok ? res.json() : [];
}

export async function createGlobalEnvVar(key: string, value: string, isSecret = false) {
  return authFetch(`${API_BASE}/global/env-vars`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ key, value, is_secret: isSecret }),
  }).then(r => r.json());
}

export async function updateGlobalEnvVar(id: number, updates: { key?: string; value?: string; is_secret?: boolean; sort_order?: number }) {
  return authFetch(`${API_BASE}/global/env-vars/${id}`, {
    method: 'PUT', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(updates),
  }).then(r => r.json());
}

export async function deleteGlobalEnvVar(id: number) {
  return authFetch(`${API_BASE}/global/env-vars/${id}`, { method: 'DELETE' }).then(r => r.json());
}

// Worktree Env Vars
export interface WorktreeEnvVar {
  id: number;
  session_name: string;
  slot: number;
  key: string;
  value: string;
  is_secret: number;
  sort_order: number;
  created_at: string;
  updated_at: string;
}

export async function fetchWorktreeEnvVars(sessionName: string, slot: number): Promise<WorktreeEnvVar[]> {
  const res = await authFetch(`${API_BASE}/project/worktree-env-vars?session_name=${encodeURIComponent(sessionName)}&slot=${slot}`);
  return res.ok ? res.json() : [];
}

export async function createWorktreeEnvVar(sessionName: string, slot: number, key: string, value: string, isSecret = false) {
  return authFetch(`${API_BASE}/project/worktree-env-vars`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ session_name: sessionName, slot, key, value, is_secret: isSecret }),
  }).then(r => r.json());
}

export async function updateWorktreeEnvVar(id: number, updates: { key?: string; value?: string; is_secret?: boolean; sort_order?: number }) {
  return authFetch(`${API_BASE}/project/worktree-env-vars/${id}`, {
    method: 'PUT', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(updates),
  }).then(r => r.json());
}

export async function deleteWorktreeEnvVar(id: number) {
  return authFetch(`${API_BASE}/project/worktree-env-vars/${id}`, { method: 'DELETE' }).then(r => r.json());
}

// Effective (merged) Env Vars
export interface EffectiveEnvVar {
  key: string;
  value: string;
  is_secret: number;
  source: string;
}

export async function fetchEffectiveEnvVars(sessionName: string, slot: number): Promise<EffectiveEnvVar[]> {
  const res = await authFetch(`${API_BASE}/project/effective-env-vars?session_name=${encodeURIComponent(sessionName)}&slot=${slot}`);
  return res.ok ? res.json() : [];
}

// Session creation
export async function createNewSession(projectName: string, gitDir: string, sessionName?: string) {
  return authFetch(`${API_BASE}/sessions/create`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ project_name: projectName, git_dir: gitDir, session_name: sessionName }),
  }).then(r => r.json());
}

// Delete project
export async function deleteProject(gitDir: string) {
  return authFetch(`${API_BASE}/projects/${encodeURIComponent(gitDir)}`, { method: 'DELETE' }).then(r => r.json());
}

// Project files (CLAUDE.md, MEMORY.md, etc.)
export interface ProjectFileEntry {
  name: string;
  path: string;
  content: string;
  exists: boolean;
}

export async function fetchProjectFiles(gitDir: string): Promise<ProjectFileEntry[]> {
  const res = await authFetch(`${API_BASE}/projects/files?git_dir=${encodeURIComponent(gitDir)}`);
  if (!res.ok) return [];
  const data = await res.json();
  return data.files || [];
}

// Update project metadata
export async function updateProject(gitDir: string, updates: { description?: string; status?: string; tags?: string; tech_stack?: string }) {
  return authFetch(`${API_BASE}/projects/${encodeURIComponent(gitDir)}`, {
    method: 'PUT', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(updates),
  }).then(r => r.json());
}

// Git info
export interface GitBranchInfo {
  name: string;
  is_current: boolean;
  last_commit: string;
  message: string;
  ahead: number;
  behind: number;
}

export interface GitStatus {
  modified: number;
  untracked: number;
  staged: number;
  conflicts: number;
  is_clean: boolean;
}

export interface GitInfoResponse {
  current_branch: string;
  branches: GitBranchInfo[];
  status: GitStatus;
}

export async function fetchGitInfo(gitDir: string): Promise<GitInfoResponse | null> {
  const res = await authFetch(`${API_BASE}/projects/git-info?git_dir=${encodeURIComponent(gitDir)}`);
  if (!res.ok) return null;
  const data = await res.json();
  if (data.error) return null;
  return data;
}

// Project statistics
export interface TaskStats {
  completed: number;
  in_progress: number;
  failed: number;
  total: number;
  completion_rate: number;
}

export interface AgentTimeStats {
  total_seconds: number;
  busy_seconds: number;
  idle_seconds: number;
}

export interface ToolUsage {
  tool: string;
  count: number;
}

export interface HourlyActivity {
  hour: string;
  count: number;
}

export interface ProjectStatistics {
  tasks: TaskStats;
  agent_time: AgentTimeStats;
  top_tools: ToolUsage[];
  activity: HourlyActivity[];
}

export async function fetchProjectStatistics(sessionName: string, range = '24h'): Promise<ProjectStatistics> {
  const res = await authFetch(`${API_BASE}/projects/statistics?session_name=${encodeURIComponent(sessionName)}&range=${range}`);
  if (!res.ok) {
    return {
      tasks: { completed: 0, in_progress: 0, failed: 0, total: 0, completion_rate: 0 },
      agent_time: { total_seconds: 0, busy_seconds: 0, idle_seconds: 0 },
      top_tools: [],
      activity: [],
    };
  }
  return res.json();
}

// Project Todos
export async function fetchProjectTodos(gitDir: string): Promise<ProjectTodo[]> {
  const res = await authFetch(`${API_BASE}/projects/todos?git_dir=${encodeURIComponent(gitDir)}`);
  return res.ok ? res.json() : [];
}

export async function createProjectTodo(gitDir: string, title: string, description = '') {
  return authFetch(`${API_BASE}/projects/todos`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ git_dir: gitDir, title, description }),
  }).then(r => r.json());
}

export async function updateProjectTodo(id: number, updates: Partial<Pick<ProjectTodo, 'title' | 'description' | 'status' | 'priority' | 'sort_order'>>) {
  return authFetch(`${API_BASE}/projects/todos/${id}`, {
    method: 'PUT', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(updates),
  }).then(r => r.json());
}

export async function deleteProjectTodo(id: number) {
  return authFetch(`${API_BASE}/projects/todos/${id}`, { method: 'DELETE' }).then(r => r.json());
}

export async function updateProjectTodoStatus(id: number, status: string) {
  return authFetch(`${API_BASE}/projects/todos/${id}/status`, {
    method: 'PUT', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ status }),
  }).then(r => r.json());
}

export interface TodoHistoryEntry {
  id: number;
  summary: string;
  completion_note: string;
  started_at: string | null;
  completed_at: string | null;
  duration_seconds: number;
}

export async function fetchTodoHistory(todoId: number): Promise<TodoHistoryEntry[]> {
  const res = await authFetch(`${API_BASE}/projects/todos/${todoId}/history`);
  if (!res.ok) return [];
  const data = await res.json();
  return data.history || [];
}
