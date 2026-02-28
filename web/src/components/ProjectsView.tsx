import React, { useState, useEffect, useCallback, useRef } from 'react';
import {
  FolderGit2, ArrowLeft, Search, Plus, Trash2, Eye, EyeOff, Save, Edit3,
  Key, GitBranch, Play, ExternalLink, X, Loader, Globe, Layers, ChevronDown,
  BarChart3, Activity, Clock, Wrench, List, FileText, Check, CheckSquare,
  ChevronRight, ChevronLeft, AlertCircle, Circle, Minus, ArrowUp, Archive, RotateCcw,
} from 'lucide-react';
import { AppTab, AgentSession } from '../types';
import { ProjectTimeline } from './ProjectTimeline';
import { HistoryEntry, HistoryResponse } from '../services/history';
import { fetchProjectHistory } from '../services/projects';
import { tmuxSelectWindow } from '../services/tmux';
import { startWorkspace } from '../services/workspace';
import {
  ProjectInfo, fetchProjects, deleteProject, createNewSession, updateProject,
  createProjectService, createProjectEnvVar as createProjEnvVarApi,
  fetchProjectServices, ProjectService,
  // Global env vars
  GlobalEnvVar, fetchGlobalEnvVars, createGlobalEnvVar, updateGlobalEnvVar, deleteGlobalEnvVar,
  // Project env vars
  ProjectEnvVar, fetchProjectEnvVars, createProjectEnvVar, updateProjectEnvVar, deleteProjectEnvVar,
  // Worktree env vars
  WorktreeEnvVar, fetchWorktreeEnvVars, createWorktreeEnvVar, updateWorktreeEnvVar, deleteWorktreeEnvVar,
  // Effective
  EffectiveEnvVar, fetchEffectiveEnvVars,
  // Worktree slots
  WorktreeSlot, fetchWorktreeSlots, deleteWorktreeSlot,
  // Git info + Statistics
  GitInfoResponse, fetchGitInfo,
  ProjectStatistics, fetchProjectStatistics,
  // Project files
  ProjectFileEntry, fetchProjectFiles,
  // Project todos
  ProjectTodo, fetchProjectTodos, createProjectTodo, updateProjectTodo, deleteProjectTodo, updateProjectTodoStatus,
} from '../services/api';
import { MarkdownText } from './MarkdownText';
import { ConfirmationModal } from './ConfirmationModal';

// Project templates
interface ProjectTemplate {
  id: string;
  name: string;
  services: { name: string; baseValue: number; valueType: string; envKey: string }[];
  envVars: { key: string; value: string }[];
}

const PROJECT_TEMPLATES: ProjectTemplate[] = [
  {
    id: 'nextjs',
    name: 'Next.js',
    services: [
      { name: 'frontend', baseValue: 3000, valueType: 'port', envKey: 'PORT' },
      { name: 'api', baseValue: 3001, valueType: 'port', envKey: 'API_PORT' },
    ],
    envVars: [
      { key: 'NODE_ENV', value: 'development' },
    ],
  },
  {
    id: 'rust-react',
    name: 'Rust + React (Vite)',
    services: [
      { name: 'frontend', baseValue: 5173, valueType: 'port', envKey: 'FRONTEND_PORT' },
      { name: 'backend', baseValue: 8080, valueType: 'port', envKey: 'BACKEND_PORT' },
    ],
    envVars: [
      { key: 'RUST_LOG', value: 'info' },
    ],
  },
  {
    id: 'fullstack-supabase',
    name: 'Full Stack (Supabase)',
    services: [
      { name: 'frontend', baseValue: 5173, valueType: 'port', envKey: 'FRONTEND_PORT' },
      { name: 'backend', baseValue: 8080, valueType: 'port', envKey: 'BACKEND_PORT' },
      { name: 'supabase', baseValue: 54321, valueType: 'port', envKey: 'SUPABASE_PORT' },
      { name: 'redis', baseValue: 6379, valueType: 'port', envKey: 'REDIS_PORT' },
    ],
    envVars: [
      { key: 'SUPABASE_URL', value: 'http://127.0.0.1:54321' },
    ],
  },
];

interface ProjectsViewProps {
  sessions: AgentSession[];
  onSwitchTab: (tab: AppTab) => void;
}

type EnvScope = 'effective' | 'global' | 'project' | 'worktree';
type DetailTab = 'overview' | 'todos' | 'timeline' | 'env-vars' | 'worktrees' | 'statistics' | 'docs';

export const ProjectsView: React.FC<ProjectsViewProps> = ({ sessions, onSwitchTab }) => {
  // Project list state
  const [projects, setProjects] = useState<ProjectInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [searchQuery, setSearchQuery] = useState('');
  const searchRef = useRef<HTMLInputElement>(null);

  // Detail view state
  const [selectedProject, setSelectedProject] = useState<ProjectInfo | null>(null);
  const [detailTab, setDetailTab] = useState<DetailTab>('overview');
  const [envScope, setEnvScope] = useState<EnvScope>('effective');

  // Add project modal
  const [showAddModal, setShowAddModal] = useState(false);
  const [addPath, setAddPath] = useState('');
  const [addName, setAddName] = useState('');
  const [addTemplate, setAddTemplate] = useState('');

  // Session creation
  const [creatingSession, setCreatingSession] = useState<string | null>(null);

  // Env vars state
  const [globalVars, setGlobalVars] = useState<GlobalEnvVar[]>([]);
  const [projectVars, setProjectVars] = useState<ProjectEnvVar[]>([]);
  const [worktreeVars, setWorktreeVars] = useState<WorktreeEnvVar[]>([]);
  const [effectiveVars, setEffectiveVars] = useState<EffectiveEnvVar[]>([]);
  const [worktreeSlots, setWorktreeSlots] = useState<WorktreeSlot[]>([]);
  const [selectedSlot, setSelectedSlot] = useState(0);
  const [envLoading, setEnvLoading] = useState(false);

  // Add var state
  const [newVarKey, setNewVarKey] = useState('');
  const [newVarValue, setNewVarValue] = useState('');
  const [newVarSecret, setNewVarSecret] = useState(false);
  const varKeyRef = useRef<HTMLInputElement>(null);

  // Edit var state
  const [editingVarId, setEditingVarId] = useState<number | null>(null);
  const [editVarKey, setEditVarKey] = useState('');
  const [editVarValue, setEditVarValue] = useState('');
  const [editVarSecret, setEditVarSecret] = useState(false);
  const [revealedSecrets, setRevealedSecrets] = useState<Set<number>>(new Set());
  const [flashVarId, setFlashVarId] = useState<number | null>(null);

  // Git info + Statistics state
  const [gitInfo, setGitInfo] = useState<GitInfoResponse | null>(null);
  const [gitLoading, setGitLoading] = useState(false);
  const [statistics, setStatistics] = useState<ProjectStatistics | null>(null);
  const [statsLoading, setStatsLoading] = useState(false);
  const [statsRange, setStatsRange] = useState('24h');

  // Activity feed state
  const [recentActivity, setRecentActivity] = useState<HistoryEntry[]>([]);
  const [activityLoading, setActivityLoading] = useState(false);

  // Project services (for worktree computed ports)
  const [projectServices, setProjectServices] = useState<ProjectService[]>([]);

  // DOCS tab state
  const [projectFiles, setProjectFiles] = useState<ProjectFileEntry[]>([]);
  const [filesLoading, setFilesLoading] = useState(false);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);

  // Todos state
  const [projectTodos, setProjectTodos] = useState<ProjectTodo[]>([]);
  const [todosLoading, setTodosLoading] = useState(false);
  const [newTodoTitle, setNewTodoTitle] = useState('');
  const [editingTodoId, setEditingTodoId] = useState<number | null>(null);
  const [editTodoTitle, setEditTodoTitle] = useState('');
  const [editTodoDesc, setEditTodoDesc] = useState('');
  const [expandedTodoId, setExpandedTodoId] = useState<number | null>(null);
  const [showAddInput, setShowAddInput] = useState(false);

  // Archive filter
  const [showArchived, setShowArchived] = useState(false);

  // Delete confirmation
  const [deleteConfirmProject, setDeleteConfirmProject] = useState<ProjectInfo | null>(null);

  // Inline editing state (for OVERVIEW info card)
  const [editingField, setEditingField] = useState<string | null>(null);
  const [editFieldValue, setEditFieldValue] = useState('');

  // Fetch projects
  const loadProjects = useCallback(async () => {
    const p = await fetchProjects();
    setProjects(p);
    setLoading(false);
  }, []);

  useEffect(() => { loadProjects(); }, [loadProjects]);

  // Get session name for a project (from its sessions)
  const getSessionName = useCallback((project: ProjectInfo) => {
    return project.last_session || project.name;
  }, []);

  // Load env vars when scope or project changes
  const loadEnvVars = useCallback(async () => {
    if (!selectedProject) return;
    setEnvLoading(true);
    const sessionName = getSessionName(selectedProject);

    if (envScope === 'global') {
      setGlobalVars(await fetchGlobalEnvVars());
    } else if (envScope === 'project') {
      setProjectVars(await fetchProjectEnvVars(sessionName));
    } else if (envScope === 'worktree') {
      setWorktreeVars(await fetchWorktreeEnvVars(sessionName, selectedSlot));
    } else if (envScope === 'effective') {
      setEffectiveVars(await fetchEffectiveEnvVars(sessionName, selectedSlot));
    }

    setEnvLoading(false);
  }, [selectedProject, envScope, selectedSlot, getSessionName]);

  useEffect(() => {
    if (selectedProject && detailTab === 'env-vars') loadEnvVars();
  }, [selectedProject, detailTab, envScope, selectedSlot, loadEnvVars]);

  // Load worktree slots when entering detail view or worktrees tab
  const loadWorktreeSlots = useCallback(async () => {
    if (!selectedProject) return;
    const sessionName = getSessionName(selectedProject);
    setWorktreeSlots(await fetchWorktreeSlots(sessionName));
  }, [selectedProject, getSessionName]);

  useEffect(() => {
    if (selectedProject) loadWorktreeSlots();
  }, [selectedProject, loadWorktreeSlots]);

  // Load git info when overview tab is selected
  useEffect(() => {
    if (selectedProject && detailTab === 'overview') {
      setGitLoading(true);
      fetchGitInfo(selectedProject.git_dir).then(info => {
        setGitInfo(info);
        setGitLoading(false);
      });
    }
  }, [selectedProject, detailTab]);

  // Load statistics when statistics tab is selected
  useEffect(() => {
    if (selectedProject && detailTab === 'statistics') {
      setStatsLoading(true);
      const sessionName = getSessionName(selectedProject);
      fetchProjectStatistics(sessionName, statsRange).then(stats => {
        setStatistics(stats);
        setStatsLoading(false);
      });
    }
  }, [selectedProject, detailTab, statsRange, getSessionName]);

  // Load recent activity for overview tab
  useEffect(() => {
    if (selectedProject && detailTab === 'overview') {
      setActivityLoading(true);
      fetchProjectHistory({ project: selectedProject.git_dir, per_page: 10 })
        .then((res: HistoryResponse) => {
          const entries = res.groups.flatMap(g => g.records);
          setRecentActivity(entries.slice(0, 10));
          setActivityLoading(false);
        })
        .catch(() => setActivityLoading(false));
    }
  }, [selectedProject, detailTab]);

  // Load project files when DOCS tab is selected
  useEffect(() => {
    if (selectedProject && detailTab === 'docs') {
      setFilesLoading(true);
      fetchProjectFiles(selectedProject.git_dir).then(files => {
        setProjectFiles(files);
        // Auto-select first existing file
        const firstExisting = files.find(f => f.exists);
        setSelectedFile(firstExisting?.name || null);
        setFilesLoading(false);
      });
    }
  }, [selectedProject, detailTab]);

  // Load todos when TODOS tab is selected
  const loadTodos = useCallback(async () => {
    if (!selectedProject) return;
    setTodosLoading(true);
    const todos = await fetchProjectTodos(selectedProject.git_dir);
    setProjectTodos(todos);
    setTodosLoading(false);
  }, [selectedProject]);

  useEffect(() => {
    if (selectedProject && detailTab === 'todos') loadTodos();
  }, [selectedProject, detailTab, loadTodos]);

  // Load project services for worktree port display
  useEffect(() => {
    if (selectedProject && (detailTab === 'worktrees' || detailTab === 'overview')) {
      const sessionName = getSessionName(selectedProject);
      fetchProjectServices(sessionName).then(setProjectServices);
    }
  }, [selectedProject, detailTab, getSessionName]);

  // Check if a project has active sessions
  const isProjectActive = useCallback((project: ProjectInfo) => {
    return sessions.some(s => s.gitDir === project.git_dir);
  }, [sessions]);

  const getProjectSessionCount = useCallback((project: ProjectInfo) => {
    return sessions.filter(s => s.gitDir === project.git_dir).length;
  }, [sessions]);

  const getProjectWindowCount = useCallback((project: ProjectInfo) => {
    return sessions
      .filter(s => s.gitDir === project.git_dir)
      .reduce((sum, s) => sum + s.windows.length, 0);
  }, [sessions]);

  // Derive physical worktrees for selected project from projects list
  const physicalWorktrees = selectedProject
    ? projects.filter(p => p.git_dir.startsWith(selectedProject.git_dir + '/.worktrees/'))
    : [];

  // Get worktree count for any project
  const getWorktreeCount = useCallback((project: ProjectInfo) => {
    return projects.filter(p => p.git_dir.startsWith(project.git_dir + '/.worktrees/')).length;
  }, [projects]);

  // Filter projects by search
  // Filter out worktree paths and invalid entries (like "..")
  const topLevelProjects = projects.filter(p =>
    !p.git_dir.includes('/.worktrees/') && p.git_dir.startsWith('/')
  );
  const archivedCount = topLevelProjects.filter(p => p.status === 'archived').length;
  const filteredProjects = topLevelProjects
    .filter(p => {
      // Hide archived unless toggled on
      if (!showArchived && p.status === 'archived') return false;
      if (!searchQuery) return true;
      const q = searchQuery.toLowerCase();
      return p.name.toLowerCase().includes(q) || p.git_dir.toLowerCase().includes(q);
    });

  // Flash helper
  const flashVar = (id: number) => {
    setFlashVarId(id);
    setTimeout(() => setFlashVarId(null), 600);
  };

  const toggleReveal = (id: number) => {
    setRevealedSecrets(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  };

  // Var CRUD handlers
  const handleAddVar = async () => {
    if (!newVarKey.trim() || !selectedProject) return;
    const sessionName = getSessionName(selectedProject);
    let result;

    if (envScope === 'global') {
      result = await createGlobalEnvVar(newVarKey.trim(), newVarValue, newVarSecret);
    } else if (envScope === 'project') {
      result = await createProjectEnvVar(sessionName, newVarKey.trim(), newVarValue, newVarSecret);
    } else if (envScope === 'worktree') {
      result = await createWorktreeEnvVar(sessionName, selectedSlot, newVarKey.trim(), newVarValue, newVarSecret);
    }

    setNewVarKey(''); setNewVarValue(''); setNewVarSecret(false);
    await loadEnvVars();
    if (result?.id) flashVar(result.id);
    varKeyRef.current?.focus();
  };

  const handleUpdateVar = async (id: number) => {
    const updates = { key: editVarKey, value: editVarValue, is_secret: editVarSecret };
    if (envScope === 'global') {
      await updateGlobalEnvVar(id, updates);
    } else if (envScope === 'project') {
      await updateProjectEnvVar(id, updates);
    } else if (envScope === 'worktree') {
      await updateWorktreeEnvVar(id, updates);
    }
    setEditingVarId(null);
    await loadEnvVars();
    flashVar(id);
  };

  const handleDeleteVar = async (id: number) => {
    if (envScope === 'global') {
      await deleteGlobalEnvVar(id);
    } else if (envScope === 'project') {
      await deleteProjectEnvVar(id);
    } else if (envScope === 'worktree') {
      await deleteWorktreeEnvVar(id);
    }
    await loadEnvVars();
  };

  const startEditVar = (v: { id: number; key: string; value: string; is_secret: number }) => {
    setEditingVarId(v.id);
    setEditVarKey(v.key);
    setEditVarValue(v.value);
    setEditVarSecret(!!v.is_secret);
  };

  const editKeyHandler = (e: React.KeyboardEvent, saveFn: () => void, cancelFn: () => void) => {
    if (e.key === 'Enter') saveFn();
    if (e.key === 'Escape') { e.stopPropagation(); cancelFn(); }
  };

  // Open project: select its first (main) tmux window
  const handleOpenProject = useCallback((project: ProjectInfo) => {
    const matchingSessions = sessions.filter(s => s.gitDir === project.git_dir);
    if (matchingSessions.length > 0) {
      const session = matchingSessions[0];
      const firstWindow = session.windows[0];
      if (firstWindow) {
        tmuxSelectWindow(session.name, firstWindow.name, firstWindow.id).catch(() => {});
      }
    }
  }, [sessions]);

  // Session creation — start with 3-pane layout (yazi | lazygit | claude) on main branch
  const handleStartSession = async (project: ProjectInfo) => {
    setCreatingSession(project.git_dir);
    try {
      await startWorkspace({
        git_dir: project.git_dir,
        branch: 'main',
        layout: 'default',
      });
    } finally {
      setCreatingSession(null);
    }
  };

  // Add project
  const handleAddProject = async () => {
    if (!addPath.trim()) return;
    const name = addName.trim() || addPath.split('/').filter(Boolean).pop() || 'project';
    const result = await createNewSession(name, addPath.trim());
    const sessionName = result?.session_name || name;

    // Apply template if selected
    if (addTemplate) {
      const template = PROJECT_TEMPLATES.find(t => t.id === addTemplate);
      if (template) {
        await Promise.all([
          ...template.services.map(s =>
            createProjectService(sessionName, s.name, s.baseValue, s.valueType, s.envKey)
          ),
          ...template.envVars.map(v =>
            createProjEnvVarApi(sessionName, v.key, v.value)
          ),
        ]);
      }
    }

    setShowAddModal(false);
    setAddPath(''); setAddName(''); setAddTemplate('');
    await loadProjects();
  };

  // Delete project
  const handleDeleteProject = (project: ProjectInfo) => {
    setDeleteConfirmProject(project);
  };

  const confirmDeleteProject = async () => {
    if (!deleteConfirmProject) return;
    await deleteProject(deleteConfirmProject.git_dir);
    setDeleteConfirmProject(null);
    setSelectedProject(null);
    await loadProjects();
  };

  // Format time ago
  const timeAgo = (isoString: string | null) => {
    if (!isoString) return 'Never';
    const diff = Date.now() - new Date(isoString).getTime();
    const mins = Math.floor(diff / 60000);
    if (mins < 1) return 'Just now';
    if (mins < 60) return `${mins}m ago`;
    const hours = Math.floor(mins / 60);
    if (hours < 24) return `${hours}h ago`;
    const days = Math.floor(hours / 24);
    return `${days}d ago`;
  };

  // Keyboard shortcuts
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (showAddModal) {
        if (e.key === 'Escape') setShowAddModal(false);
        return;
      }
      if (selectedProject) {
        if (e.key === 'Escape') {
          if (editingVarId !== null) { setEditingVarId(null); return; }
          setSelectedProject(null);
        }
        return;
      }
      if (e.key === '/' && !e.metaKey && !e.ctrlKey) {
        e.preventDefault();
        searchRef.current?.focus();
      }
    };
    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, [selectedProject, showAddModal, editingVarId]);

  // =====================================================
  // RENDER: Project List
  // =====================================================
  if (!selectedProject) {
    return (
      <div className="space-y-4">
        {/* Header */}
        <div className="flex items-center justify-between flex-wrap gap-2">
          <div className="flex items-center gap-3">
            <FolderGit2 className="w-5 h-5 text-green-500" />
            <span className="text-green-400 font-bold tracking-widest uppercase text-sm font-pixel">PROJECTS</span>
            {!loading && (
              <span className="text-green-700 text-xs font-mono">({topLevelProjects.length})</span>
            )}
          </div>
          <div className="flex items-center gap-2">
            <div className="relative">
              <Search className="absolute left-2 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-green-700" />
              <input
                ref={searchRef}
                value={searchQuery}
                onChange={e => setSearchQuery(e.target.value)}
                placeholder="Search... (/)"
                className="pl-7 pr-2 py-1.5 bg-black/60 border border-green-900 text-green-300 text-sm font-mono focus:border-green-500 outline-none placeholder:text-green-900 w-48"
              />
            </div>
            {archivedCount > 0 && (
              <button
                onClick={() => setShowArchived(!showArchived)}
                className={`flex items-center gap-1.5 px-3 py-1.5 border text-xs font-bold tracking-widest uppercase transition-all ${
                  showArchived
                    ? 'border-green-600 text-green-400 bg-green-900/30'
                    : 'border-green-900 text-green-800 hover:border-green-700 hover:text-green-600'
                }`}
              >
                <Archive className="w-3.5 h-3.5" /> {archivedCount}
              </button>
            )}
            <button
              onClick={() => setShowAddModal(true)}
              className="flex items-center gap-1.5 px-3 py-1.5 border border-green-700 text-green-500 hover:bg-green-900/30 hover:border-green-500 text-xs font-bold tracking-widest uppercase transition-all"
            >
              <Plus className="w-3.5 h-3.5" /> ADD
            </button>
          </div>
        </div>

        {/* Project Cards */}
        {loading ? (
          <div className="flex items-center justify-center py-16">
            <Loader className="w-5 h-5 text-green-700 animate-spin mr-2" />
            <span className="text-green-700 text-sm font-mono tracking-widest">LOADING PROJECTS...</span>
          </div>
        ) : filteredProjects.length === 0 ? (
          <div className="flex flex-col items-center py-16 retro-border">
            <FolderGit2 className="w-10 h-10 text-green-900 mb-3" />
            <div className="text-green-600 text-sm font-mono mb-1">
              {searchQuery ? 'No matching projects' : 'No projects registered yet'}
            </div>
            <div className="text-green-800 text-xs font-mono">
              {searchQuery ? 'Try a different search term' : 'Projects are auto-discovered from tmux sessions'}
            </div>
          </div>
        ) : (
          <div className="space-y-2">
            {filteredProjects.map(project => {
              const active = isProjectActive(project);
              const sessionCount = getProjectSessionCount(project);
              const windowCount = getProjectWindowCount(project);
              const wtCount = getWorktreeCount(project);
              return (
                <div
                  key={project.git_dir}
                  className={`retro-border bg-black/40 hover:bg-green-900/10 transition-all cursor-pointer ${project.status === 'archived' ? 'opacity-60' : ''}`}
                  onClick={() => setSelectedProject(project)}
                >
                  <div className="px-4 py-3 flex items-center justify-between flex-wrap gap-2">
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2 mb-1">
                        <span className="text-green-300 font-bold font-mono text-sm tracking-wider truncate">{project.name || project.git_dir.split('/').pop()}</span>
                        <span className={`text-[9px] tracking-widest uppercase px-1.5 py-0.5 border shrink-0 ${
                          project.status === 'archived'
                            ? 'text-green-900 border-green-900/40 bg-green-900/10'
                            : active
                            ? 'text-green-400 border-green-600 bg-green-900/30'
                            : 'text-green-800 border-green-900/50'
                        }`}>
                          {project.status === 'archived' ? 'ARCHIVED' : active ? 'ACTIVE' : 'INACTIVE'}
                        </span>
                      </div>
                      <div className="text-green-700 font-mono text-xs truncate">{project.git_dir}</div>
                      {project.tech_stack && (
                        <div className="flex items-center gap-1.5 mt-1 flex-wrap">
                          {project.tech_stack.split(/[|,+]/).map(t => t.trim()).filter(Boolean).map(tech => (
                            <span key={tech} className="text-[9px] tracking-wider px-1.5 py-0.5 border border-blue-900/60 text-blue-500/80 bg-blue-900/15">{tech}</span>
                          ))}
                        </div>
                      )}
                      <div className="flex items-center gap-3 mt-1.5 text-green-800 text-[10px] font-mono tracking-wider">
                        <span>Last active: {timeAgo(project.last_active_at)}</span>
                        {sessionCount > 0 && <span>{sessionCount} session{sessionCount > 1 ? 's' : ''}</span>}
                        {windowCount > 0 && <span>{windowCount} window{windowCount > 1 ? 's' : ''}</span>}
                        {project.history_count > 0 && <span>{project.history_count} tasks</span>}
                        {wtCount > 0 && <span>{wtCount} worktree{wtCount > 1 ? 's' : ''}</span>}
                        {project.todos_count > 0 && (
                          <span className="text-yellow-600">
                            {project.todos_count} todo{project.todos_count > 1 ? 's' : ''}
                          </span>
                        )}
                      </div>
                    </div>
                    <div className="flex items-center gap-2 shrink-0">
                      {project.status === 'archived' ? (
                        <button
                          onClick={e => { e.stopPropagation(); updateProject(project.git_dir, { status: 'active' }).then(loadProjects); }}
                          className="flex items-center gap-1 px-3 py-1.5 border border-green-900 text-green-700 hover:bg-green-900/30 hover:border-green-600 hover:text-green-500 text-xs font-bold tracking-widest uppercase transition-all"
                        >
                          <RotateCcw className="w-3 h-3" /> RESTORE
                        </button>
                      ) : (
                        <>
                          <button
                            onClick={e => { e.stopPropagation(); updateProject(project.git_dir, { status: 'archived' }).then(loadProjects); }}
                            className="flex items-center gap-1 px-2 py-1.5 border border-transparent text-green-900 hover:border-green-900 hover:text-green-700 text-xs tracking-widest uppercase transition-all"
                            title="Archive project"
                          >
                            <Archive className="w-3 h-3" />
                          </button>
                          {active ? (
                            <button
                              onClick={e => { e.stopPropagation(); handleOpenProject(project); }}
                              className="flex items-center gap-1 px-3 py-1.5 border border-green-700 text-green-500 hover:bg-green-900/30 hover:border-green-500 text-xs font-bold tracking-widest uppercase transition-all"
                            >
                              <ExternalLink className="w-3 h-3" /> OPEN
                            </button>
                          ) : (
                            <button
                              onClick={e => { e.stopPropagation(); handleStartSession(project); }}
                              disabled={creatingSession === project.git_dir}
                              className="flex items-center gap-1 px-3 py-1.5 border border-green-700 text-green-500 hover:bg-green-900/30 hover:border-green-500 text-xs font-bold tracking-widest uppercase transition-all disabled:opacity-50"
                            >
                              {creatingSession === project.git_dir ? (
                                <Loader className="w-3 h-3 animate-spin" />
                              ) : (
                                <Play className="w-3 h-3" />
                              )}
                              START
                            </button>
                          )}
                        </>
                      )}
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        )}

        {/* Add Project Modal */}
        {showAddModal && (
          <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/80 backdrop-blur-sm" onClick={() => setShowAddModal(false)}>
            <div className="retro-border bg-black shadow-[0_0_30px_rgba(34,197,94,0.3)] w-full max-w-md" onClick={e => e.stopPropagation()}>
              <div className="flex items-center justify-between px-4 py-3 border-b border-green-900">
                <span className="text-green-400 font-bold tracking-widest uppercase text-sm font-pixel">ADD PROJECT</span>
                <button onClick={() => setShowAddModal(false)} className="text-green-700 hover:text-green-400 transition-colors">
                  <X className="w-5 h-5" />
                </button>
              </div>
              <div className="p-4 space-y-3">
                <div>
                  <label className="block text-green-700 text-[10px] tracking-widest uppercase mb-1">GIT DIRECTORY PATH</label>
                  <input
                    value={addPath}
                    onChange={e => setAddPath(e.target.value)}
                    placeholder="/path/to/project"
                    autoFocus
                    onKeyDown={e => e.key === 'Enter' && handleAddProject()}
                    className="w-full bg-black/60 border border-green-900 text-green-300 px-3 py-2 text-sm font-mono focus:border-green-500 outline-none placeholder:text-green-900"
                  />
                </div>
                <div>
                  <label className="block text-green-700 text-[10px] tracking-widest uppercase mb-1">PROJECT NAME (OPTIONAL)</label>
                  <input
                    value={addName}
                    onChange={e => setAddName(e.target.value)}
                    placeholder="my-project"
                    onKeyDown={e => e.key === 'Enter' && handleAddProject()}
                    className="w-full bg-black/60 border border-green-900 text-green-300 px-3 py-2 text-sm font-mono focus:border-green-500 outline-none placeholder:text-green-900"
                  />
                </div>
                <div>
                  <label className="block text-green-700 text-[10px] tracking-widest uppercase mb-1">TEMPLATE (OPTIONAL)</label>
                  <div className="relative">
                    <select
                      value={addTemplate}
                      onChange={e => setAddTemplate(e.target.value)}
                      className="w-full bg-black/60 border border-green-900 text-green-300 px-3 py-2 text-sm font-mono focus:border-green-500 outline-none appearance-none pr-8"
                    >
                      <option value="">No template</option>
                      {PROJECT_TEMPLATES.map(t => (
                        <option key={t.id} value={t.id}>{t.name}</option>
                      ))}
                    </select>
                    <ChevronDown className="absolute right-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-green-700 pointer-events-none" />
                  </div>
                  {addTemplate && (() => {
                    const tmpl = PROJECT_TEMPLATES.find(t => t.id === addTemplate);
                    if (!tmpl) return null;
                    return (
                      <div className="mt-1.5 text-green-800 text-[10px] font-mono tracking-wider">
                        {tmpl.services.length} service{tmpl.services.length !== 1 ? 's' : ''}, {tmpl.envVars.length} env var{tmpl.envVars.length !== 1 ? 's' : ''} will be added
                      </div>
                    );
                  })()}
                </div>
                <button
                  onClick={handleAddProject}
                  className="w-full flex items-center justify-center gap-2 px-4 py-2 border border-green-700 text-green-500 hover:bg-green-900/30 hover:border-green-500 text-xs font-bold tracking-widest uppercase transition-all"
                >
                  <Plus className="w-3.5 h-3.5" /> CREATE SESSION & REGISTER
                </button>
              </div>
            </div>
          </div>
        )}
      </div>
    );
  }

  // =====================================================
  // RENDER: Project Detail
  // =====================================================
  const currentSessionName = getSessionName(selectedProject);
  const active = isProjectActive(selectedProject);

  const scopeButtons: { id: EnvScope; label: string; icon: React.ElementType }[] = [
    { id: 'effective', label: 'EFFECTIVE', icon: Layers },
    { id: 'global', label: 'GLOBAL', icon: Globe },
    { id: 'project', label: 'PROJECT', icon: FolderGit2 },
    { id: 'worktree', label: 'WORKTREE', icon: GitBranch },
  ];

  // Get current vars based on scope
  const getCurrentVars = (): Array<{ id: number; key: string; value: string; is_secret: number }> => {
    if (envScope === 'global') return globalVars;
    if (envScope === 'project') return projectVars;
    if (envScope === 'worktree') return worktreeVars;
    return [];
  };

  return (
    <div className="space-y-4">
      {/* Breadcrumb + Back */}
      <div className="flex items-center justify-between flex-wrap gap-2">
        <div className="flex items-center gap-2">
          <button
            onClick={() => setSelectedProject(null)}
            className="flex items-center gap-1 text-green-700 hover:text-green-400 transition-colors text-xs font-bold tracking-widest uppercase"
          >
            <ArrowLeft className="w-4 h-4" /> PROJECTS
          </button>
          <span className="text-green-900">/</span>
          <span className="text-green-300 font-bold font-mono text-sm tracking-wider">{selectedProject.name || selectedProject.git_dir.split('/').pop()}</span>
          <span className={`text-[9px] tracking-widest uppercase px-1.5 py-0.5 border ${
            active ? 'text-green-400 border-green-600 bg-green-900/30' : 'text-green-800 border-green-900/50'
          }`}>
            {active ? 'ACTIVE' : 'INACTIVE'}
          </span>
        </div>
        <div className="flex items-center gap-2">
          {active && (
            <button
              onClick={() => handleOpenProject(selectedProject)}
              className="flex items-center gap-1 px-3 py-1.5 border border-green-700 text-green-500 hover:bg-green-900/30 hover:border-green-500 text-xs font-bold tracking-widest uppercase transition-all"
            >
              <ExternalLink className="w-3 h-3" /> OPEN
            </button>
          )}
          <button
            onClick={() => handleDeleteProject(selectedProject)}
            className="flex items-center gap-1 px-3 py-1.5 border border-red-900 text-red-700 hover:bg-red-900/20 hover:border-red-700 text-xs font-bold tracking-widest uppercase transition-all"
          >
            <Trash2 className="w-3 h-3" /> DELETE
          </button>
        </div>
      </div>

      {/* Project Info */}
      <div className="retro-border bg-black/40 px-4 py-3">
        <div className="text-green-700 font-mono text-xs truncate">{selectedProject.git_dir}</div>
        <div className="flex items-center gap-4 mt-1.5 text-green-800 text-[10px] font-mono tracking-wider">
          <span>Last active: {timeAgo(selectedProject.last_active_at)}</span>
          <span>Session: {currentSessionName || 'N/A'}</span>
          {selectedProject.history_count > 0 && <span>{selectedProject.history_count} tasks</span>}
          {selectedProject.notes_count > 0 && <span>{selectedProject.notes_count} notes</span>}
        </div>
      </div>

      {/* Sub-tabs */}
      <div className="border-b border-green-900">
        <div className="flex">
          {([
            { id: 'overview' as DetailTab, label: 'OVERVIEW', icon: Activity },
            { id: 'todos' as DetailTab, label: `TODOS${selectedProject.todos_count ? ` (${selectedProject.todos_count})` : ''}`, icon: CheckSquare },
            { id: 'timeline' as DetailTab, label: `TIMELINE${selectedProject.history_count > 0 ? ` (${selectedProject.history_count})` : ''}`, icon: List },
            { id: 'docs' as DetailTab, label: 'DOCS', icon: FileText },
            { id: 'env-vars' as DetailTab, label: 'ENV VARS', icon: Key },
            { id: 'worktrees' as DetailTab, label: `WORKTREES${physicalWorktrees.length + worktreeSlots.length > 0 ? ` (${physicalWorktrees.length + worktreeSlots.length})` : ''}`, icon: GitBranch },
            { id: 'statistics' as DetailTab, label: 'STATISTICS', icon: BarChart3 },
          ]).map(tab => (
            <button
              key={tab.id}
              onClick={() => setDetailTab(tab.id)}
              className={`flex items-center gap-2 px-4 py-2 text-xs font-bold tracking-widest uppercase transition-all
                ${detailTab === tab.id
                  ? 'text-green-300 border-b-2 border-green-400 bg-green-900/20'
                  : 'text-green-700 hover:text-green-500'}`}
            >
              <tab.icon className="w-4 h-4" />
              {tab.label}
            </button>
          ))}
        </div>
      </div>

      {/* OVERVIEW Tab */}
      {detailTab === 'overview' && (
        <div className="space-y-4">
          {/* Project Info Card */}
          <div className="retro-border bg-black/40 px-4 py-3 space-y-2.5">
            {/* Description */}
            <div className="flex items-start gap-2">
              <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold w-[80px] shrink-0 pt-0.5">DESC</span>
              {editingField === 'description' ? (
                <input
                  autoFocus
                  value={editFieldValue}
                  onChange={e => setEditFieldValue(e.target.value)}
                  onKeyDown={e => {
                    if (e.key === 'Enter') {
                      updateProject(selectedProject.git_dir, { description: editFieldValue });
                      setSelectedProject({ ...selectedProject, description: editFieldValue });
                      setEditingField(null);
                    }
                    if (e.key === 'Escape') { e.stopPropagation(); setEditingField(null); }
                  }}
                  onBlur={() => {
                    updateProject(selectedProject.git_dir, { description: editFieldValue });
                    setSelectedProject({ ...selectedProject, description: editFieldValue });
                    setEditingField(null);
                  }}
                  className="flex-1 bg-black/60 border border-green-700 text-green-300 px-2 py-0.5 text-sm font-mono focus:border-green-500 outline-none"
                />
              ) : (
                <span
                  className="flex-1 text-green-400 font-mono text-sm cursor-pointer hover:text-green-300 transition-colors"
                  onClick={() => { setEditingField('description'); setEditFieldValue(selectedProject.description || ''); }}
                >
                  {selectedProject.description || <span className="text-green-800 italic">Click to add description...</span>}
                </span>
              )}
            </div>
            {/* Tech Stack */}
            <div className="flex items-start gap-2">
              <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold w-[80px] shrink-0 pt-0.5">STACK</span>
              {editingField === 'tech_stack' ? (
                <input
                  autoFocus
                  value={editFieldValue}
                  onChange={e => setEditFieldValue(e.target.value)}
                  onKeyDown={e => {
                    if (e.key === 'Enter') {
                      updateProject(selectedProject.git_dir, { tech_stack: editFieldValue });
                      setSelectedProject({ ...selectedProject, tech_stack: editFieldValue });
                      setEditingField(null);
                    }
                    if (e.key === 'Escape') { e.stopPropagation(); setEditingField(null); }
                  }}
                  onBlur={() => {
                    updateProject(selectedProject.git_dir, { tech_stack: editFieldValue });
                    setSelectedProject({ ...selectedProject, tech_stack: editFieldValue });
                    setEditingField(null);
                  }}
                  className="flex-1 bg-black/60 border border-green-700 text-green-300 px-2 py-0.5 text-sm font-mono focus:border-green-500 outline-none"
                  placeholder="e.g. React + Vite | Rust + Axum"
                />
              ) : (
                <span
                  className="flex-1 text-green-400 font-mono text-sm cursor-pointer hover:text-green-300 transition-colors"
                  onClick={() => { setEditingField('tech_stack'); setEditFieldValue(selectedProject.tech_stack || ''); }}
                >
                  {selectedProject.tech_stack || <span className="text-green-800 italic">Click to add tech stack...</span>}
                </span>
              )}
            </div>
            {/* Tags */}
            <div className="flex items-start gap-2">
              <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold w-[80px] shrink-0 pt-0.5">TAGS</span>
              {editingField === 'tags' ? (
                <input
                  autoFocus
                  value={editFieldValue}
                  onChange={e => setEditFieldValue(e.target.value)}
                  onKeyDown={e => {
                    if (e.key === 'Enter') {
                      updateProject(selectedProject.git_dir, { tags: editFieldValue });
                      setSelectedProject({ ...selectedProject, tags: editFieldValue });
                      setEditingField(null);
                    }
                    if (e.key === 'Escape') { e.stopPropagation(); setEditingField(null); }
                  }}
                  onBlur={() => {
                    updateProject(selectedProject.git_dir, { tags: editFieldValue });
                    setSelectedProject({ ...selectedProject, tags: editFieldValue });
                    setEditingField(null);
                  }}
                  className="flex-1 bg-black/60 border border-green-700 text-green-300 px-2 py-0.5 text-sm font-mono focus:border-green-500 outline-none"
                  placeholder="e.g. web, fullstack, private"
                />
              ) : (
                <div
                  className="flex-1 flex items-center gap-1.5 flex-wrap cursor-pointer"
                  onClick={() => { setEditingField('tags'); setEditFieldValue(selectedProject.tags || ''); }}
                >
                  {selectedProject.tags ? (
                    selectedProject.tags.split(',').map(t => t.trim()).filter(Boolean).map(tag => (
                      <span key={tag} className="text-[10px] tracking-widest uppercase px-1.5 py-0.5 border border-green-800 text-green-500 bg-green-900/20">{tag}</span>
                    ))
                  ) : (
                    <span className="text-green-800 italic text-sm font-mono">Click to add tags...</span>
                  )}
                </div>
              )}
            </div>
            {/* Status */}
            <div className="flex items-center gap-2">
              <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold w-[80px] shrink-0">STATUS</span>
              <span className={`text-[10px] tracking-widest uppercase px-1.5 py-0.5 border font-bold ${
                selectedProject.status === 'active' || !selectedProject.status
                  ? 'text-green-400 border-green-600 bg-green-900/30'
                  : selectedProject.status === 'archived'
                  ? 'text-green-800 border-green-900/50'
                  : 'text-yellow-600 border-yellow-800 bg-yellow-900/20'
              }`}>
                {(selectedProject.status || 'active').toUpperCase()}
              </span>
            </div>
          </div>

          {gitLoading ? (
            <div className="flex items-center justify-center py-8">
              <Loader className="w-5 h-5 text-green-700 animate-spin mr-2" />
              <span className="text-green-700 text-sm font-mono tracking-widest">LOADING GIT INFO...</span>
            </div>
          ) : gitInfo ? (
            <>
              {/* Current Branch + Status */}
              <div className="retro-border bg-black/40 px-4 py-3">
                <div className="flex items-center gap-2 mb-2">
                  <GitBranch className="w-4 h-4 text-green-500" />
                  <span className="text-green-400 font-bold font-mono text-sm">{gitInfo.current_branch}</span>
                  <span className="text-green-800 text-[10px] tracking-widest uppercase">CURRENT BRANCH</span>
                </div>
                <div className="flex items-center gap-4 text-[10px] font-mono tracking-wider">
                  {gitInfo.status.is_clean ? (
                    <span className="text-green-600">CLEAN</span>
                  ) : (
                    <>
                      {gitInfo.status.modified > 0 && <span className="text-yellow-600">{gitInfo.status.modified} modified</span>}
                      {gitInfo.status.staged > 0 && <span className="text-green-500">{gitInfo.status.staged} staged</span>}
                      {gitInfo.status.untracked > 0 && <span className="text-green-700">{gitInfo.status.untracked} untracked</span>}
                      {gitInfo.status.conflicts > 0 && <span className="text-red-500">{gitInfo.status.conflicts} conflicts</span>}
                    </>
                  )}
                </div>
              </div>

              {/* Branches */}
              {gitInfo.branches.length > 0 && (
                <div className="border border-green-900/50">
                  <div className="flex items-center px-3 py-2 border-b border-green-900/50 bg-green-900/10">
                    <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold flex-1">BRANCH</span>
                    <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold w-[100px] text-center">AHEAD/BEHIND</span>
                    <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold flex-[2] text-right">LAST COMMIT</span>
                  </div>
                  {gitInfo.branches.map(b => (
                    <div key={b.name} className={`flex items-center px-3 py-2 border-b border-green-900/30 hover:bg-green-900/5 ${b.is_current ? 'bg-green-900/10' : ''}`}>
                      <div className="flex items-center gap-2 flex-1 min-w-0">
                        <GitBranch className={`w-3 h-3 shrink-0 ${b.is_current ? 'text-green-400' : 'text-green-800'}`} />
                        <span className={`font-mono text-sm truncate ${b.is_current ? 'text-green-300 font-bold' : 'text-green-600'}`}>{b.name}</span>
                      </div>
                      <div className="w-[100px] text-center flex items-center justify-center gap-1.5">
                        {(b.ahead > 0 || b.behind > 0) ? (
                          <>
                            {b.ahead > 0 && <span className="text-green-500 text-[10px] font-mono">+{b.ahead}</span>}
                            {b.behind > 0 && <span className="text-red-600 text-[10px] font-mono">-{b.behind}</span>}
                          </>
                        ) : (
                          <span className="text-green-800 text-[10px] font-mono">--</span>
                        )}
                      </div>
                      <div className="flex-[2] text-right">
                        <span className="text-green-700 font-mono text-xs truncate">{b.message}</span>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </>
          ) : (
            <div className="flex flex-col items-center py-8 retro-border bg-black/40">
              <GitBranch className="w-8 h-8 text-green-900 mb-2" />
              <div className="text-green-600 text-sm font-mono mb-1">Git info unavailable</div>
              <div className="text-green-800 text-xs font-mono">Could not read git repository information.</div>
            </div>
          )}

          {/* Quick Info */}
          {selectedProject && (
            <div className="retro-border bg-black/40 px-4 py-3">
              <div className="flex items-center gap-2 mb-2">
                <Activity className="w-4 h-4 text-green-600" />
                <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold">QUICK INFO</span>
              </div>
              <div className="grid grid-cols-2 md:grid-cols-4 gap-3 text-xs font-mono">
                <div>
                  <div className="text-green-800 text-[9px] tracking-wider uppercase">Sessions</div>
                  <div className="text-green-400">{sessions.filter(s => s.gitDir === selectedProject.git_dir).length} active</div>
                </div>
                <div>
                  <div className="text-green-800 text-[9px] tracking-wider uppercase">Worktrees</div>
                  <div className="text-green-400">{worktreeSlots.length} slots</div>
                </div>
                <div>
                  <div className="text-green-800 text-[9px] tracking-wider uppercase">Total Tasks</div>
                  <div className="text-green-400">{selectedProject.history_count}</div>
                </div>
                <div>
                  <div className="text-green-800 text-[9px] tracking-wider uppercase">Last Active</div>
                  <div className="text-green-400">{timeAgo(selectedProject.last_active_at)}</div>
                </div>
              </div>
            </div>
          )}

          {/* Recent Activity */}
          <div className="border border-green-900/50">
            <div className="flex items-center px-3 py-2 border-b border-green-900/50 bg-green-900/10">
              <Clock className="w-3.5 h-3.5 text-green-700 mr-2" />
              <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold">RECENT ACTIVITY</span>
            </div>
            {activityLoading ? (
              <div className="flex items-center justify-center py-6">
                <Loader className="w-4 h-4 text-green-700 animate-spin mr-2" />
                <span className="text-green-700 text-xs font-mono">Loading...</span>
              </div>
            ) : recentActivity.length > 0 ? (
              recentActivity.map(entry => (
                <div key={entry.id} className="flex items-start gap-3 px-3 py-2.5 border-b border-green-900/20 hover:bg-green-900/5">
                  <div className="mt-0.5 w-5 h-5 rounded-full bg-green-900/30 flex items-center justify-center shrink-0">
                    <span className="text-green-500 text-[10px]">&#10003;</span>
                  </div>
                  <div className="flex-1 min-w-0">
                    <div className="text-green-400 font-mono text-sm truncate">{entry.summary}</div>
                    <div className="flex items-center gap-3 mt-0.5 text-[10px] font-mono tracking-wider text-green-700">
                      <span>{entry.window}</span>
                      <span>{Math.floor(entry.duration_seconds / 60)}m {Math.floor(entry.duration_seconds % 60)}s</span>
                    </div>
                  </div>
                  <div className="text-green-800 text-[10px] font-mono shrink-0">{timeAgo(entry.started_at)}</div>
                </div>
              ))
            ) : (
              <div className="flex flex-col items-center py-6">
                <Clock className="w-6 h-6 text-green-900 mb-1" />
                <div className="text-green-700 text-xs font-mono">No recent activity</div>
              </div>
            )}
          </div>
        </div>
      )}

      {/* TODOS Tab */}
      {detailTab === 'todos' && selectedProject && (
        <div>
          {todosLoading ? (
            <div className="flex items-center justify-center py-8">
              <Loader className="w-5 h-5 text-green-500 animate-spin" />
            </div>
          ) : (
            (() => {
              const todoItems = projectTodos.filter(t => t.status === 'todo');
              const inProgressItems = projectTodos.filter(t => t.status === 'in_progress');
              const doneItems = projectTodos.filter(t => t.status === 'done');
              const urgentCount = projectTodos.filter(t => t.priority >= 2).length;

              const nextStatus = (s: string) => s === 'todo' ? 'in_progress' : s === 'in_progress' ? 'done' : 'done';
              const prevStatus = (s: string) => s === 'done' ? 'in_progress' : s === 'in_progress' ? 'todo' : 'todo';

              const handleStatusChange = async (id: number, status: string) => {
                await updateProjectTodoStatus(id, status);
                loadTodos();
                loadProjects();
              };

              const handleCreateTodo = async () => {
                if (!newTodoTitle.trim()) return;
                await createProjectTodo(selectedProject.git_dir, newTodoTitle.trim());
                setNewTodoTitle('');
                setShowAddInput(false);
                loadTodos();
                loadProjects();
              };

              const handleDeleteTodo = async (id: number) => {
                await deleteProjectTodo(id);
                loadTodos();
                loadProjects();
              };

              const handleSaveEdit = async (id: number) => {
                await updateProjectTodo(id, { title: editTodoTitle, description: editTodoDesc });
                setEditingTodoId(null);
                loadTodos();
              };

              const handlePriorityChange = async (id: number, priority: number) => {
                await updateProjectTodo(id, { priority });
                loadTodos();
              };

              const priorityBorderColor = (p: number) =>
                p >= 2 ? 'border-l-red-500' : p === 1 ? 'border-l-yellow-500' : 'border-l-green-900/50';

              const PriorityIcon = ({ priority }: { priority: number }) => {
                if (priority >= 2) return <span className="flex items-center gap-0.5 text-red-400"><ArrowUp className="w-3 h-3" /><ArrowUp className="w-3 h-3 -ml-2" /></span>;
                if (priority === 1) return <ArrowUp className="w-3 h-3 text-yellow-400" />;
                return <Minus className="w-3 h-3 text-green-800" />;
              };

              const renderCard = (todo: ProjectTodo, isDone = false) => (
                <div
                  key={todo.id}
                  className={`group bg-black/40 border border-green-900/40 border-l-2 ${priorityBorderColor(todo.priority)} p-2.5 hover:border-green-700/60 hover:bg-black/50 transition-all`}
                >
                  {editingTodoId === todo.id ? (
                    <div className="space-y-2">
                      <input
                        value={editTodoTitle}
                        onChange={e => setEditTodoTitle(e.target.value)}
                        onKeyDown={e => { if (e.key === 'Enter') handleSaveEdit(todo.id); if (e.key === 'Escape') setEditingTodoId(null); }}
                        autoFocus
                        className="w-full bg-black/60 border border-green-700 text-green-300 px-2 py-1 text-sm font-mono focus:border-green-400 outline-none"
                      />
                      <textarea
                        value={editTodoDesc}
                        onChange={e => setEditTodoDesc(e.target.value)}
                        onKeyDown={e => { if (e.key === 'Escape') setEditingTodoId(null); }}
                        placeholder="Description (optional)"
                        rows={2}
                        className="w-full bg-black/60 border border-green-900 text-green-400 px-2 py-1 text-xs font-mono focus:border-green-700 outline-none resize-none placeholder:text-green-900"
                      />
                      <div className="flex gap-1.5">
                        <button
                          onClick={() => handleSaveEdit(todo.id)}
                          className="flex items-center gap-1 px-2 py-0.5 border border-green-700 text-green-500 hover:bg-green-900/30 text-[10px] font-bold tracking-widest uppercase"
                        >
                          <Save className="w-3 h-3" /> SAVE
                        </button>
                        <button
                          onClick={() => setEditingTodoId(null)}
                          className="flex items-center gap-1 px-2 py-0.5 border border-green-900 text-green-700 hover:text-green-500 text-[10px] font-bold tracking-widest uppercase"
                        >
                          <X className="w-3 h-3" /> CANCEL
                        </button>
                      </div>
                    </div>
                  ) : (
                    <>
                      {/* Row 1: ID + hover menu */}
                      <div className="flex items-center justify-between mb-1">
                        <span className="text-green-900 text-[10px] font-mono">#{todo.id}</span>
                        <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                          <button
                            onClick={() => { setEditingTodoId(todo.id); setEditTodoTitle(todo.title); setEditTodoDesc(todo.description); }}
                            className="text-green-800 hover:text-green-400 transition-colors p-0.5"
                            title="Edit"
                          >
                            <Edit3 className="w-3 h-3" />
                          </button>
                          <button
                            onClick={() => handleDeleteTodo(todo.id)}
                            className="text-red-900 hover:text-red-500 transition-colors p-0.5"
                            title="Delete"
                          >
                            <Trash2 className="w-3 h-3" />
                          </button>
                        </div>
                      </div>
                      {/* Row 2: Title */}
                      <div className={`text-sm font-mono leading-tight truncate ${isDone ? 'line-through opacity-60 text-green-600' : 'text-green-300'}`}>
                        {todo.title}
                      </div>
                      {/* Row 3: Description preview */}
                      {todo.description && (
                        <div
                          className="text-green-700 text-xs font-mono leading-snug mt-1 cursor-pointer hover:text-green-600"
                          style={{ display: '-webkit-box', WebkitLineClamp: expandedTodoId === todo.id ? 999 : 2, WebkitBoxOrient: 'vertical', overflow: 'hidden' }}
                          onClick={() => setExpandedTodoId(expandedTodoId === todo.id ? null : todo.id)}
                        >
                          {todo.description}
                        </div>
                      )}
                      {/* Row 4: Priority icon + status arrows */}
                      <div className="flex items-center justify-between mt-1.5">
                        <button
                          onClick={() => handlePriorityChange(todo.id, (todo.priority + 1) % 3)}
                          className="flex items-center gap-1 hover:opacity-80 transition-opacity"
                          title="Click to cycle priority"
                        >
                          <PriorityIcon priority={todo.priority} />
                          <span className={`text-[9px] font-mono tracking-wider uppercase ${todo.priority >= 2 ? 'text-red-400' : todo.priority === 1 ? 'text-yellow-400' : 'text-green-800'}`}>
                            {todo.priority >= 2 ? 'URGENT' : todo.priority === 1 ? 'HIGH' : ''}
                          </span>
                        </button>
                        <div className="flex items-center gap-0.5">
                          {todo.status !== 'todo' && (
                            <button
                              onClick={() => handleStatusChange(todo.id, prevStatus(todo.status))}
                              className="text-green-800 hover:text-green-400 transition-colors p-0.5"
                              title="Move back"
                            >
                              <ChevronLeft className="w-3.5 h-3.5" />
                            </button>
                          )}
                          {todo.status !== 'done' && (
                            <button
                              onClick={() => handleStatusChange(todo.id, nextStatus(todo.status))}
                              className="text-green-800 hover:text-green-400 transition-colors p-0.5"
                              title="Move forward"
                            >
                              <ChevronRight className="w-3.5 h-3.5" />
                            </button>
                          )}
                        </div>
                      </div>
                    </>
                  )}
                </div>
              );

              // Empty state component
              const EmptyColumn = ({ message }: { message: string }) => (
                <div className="border border-dashed border-green-900/40 rounded px-3 py-6 flex items-center justify-center">
                  <span className="text-green-900/60 text-xs font-mono text-center">{message}</span>
                </div>
              );

              return (
                <>
                  {/* Stats bar */}
                  <div className="text-[10px] font-mono tracking-wider text-green-800 px-1 pb-2 flex items-center gap-2">
                    <span>{projectTodos.length} total</span>
                    <span className="text-green-900">·</span>
                    <span>{inProgressItems.length} in progress</span>
                    <span className="text-green-900">·</span>
                    <span>{doneItems.length} done</span>
                    {urgentCount > 0 && (
                      <>
                        <span className="text-green-900">·</span>
                        <span className="text-red-500">{urgentCount} urgent</span>
                      </>
                    )}
                  </div>

                  {/* Kanban columns */}
                  <div className="flex h-[calc(100vh-280px)] min-h-[400px] border border-green-900/30">
                    {/* TODO Column */}
                    <div className="flex-1 min-w-0 flex flex-col border-r border-green-900/50">
                      <div className="sticky top-0 z-10 bg-green-900/20 px-3 py-2 border-b border-green-900/50">
                        <div className="flex items-center justify-between">
                          <div className="flex items-center gap-2 text-[10px] font-bold tracking-[0.2em] uppercase text-green-400">
                            <span className="w-2 h-2 rounded-full bg-green-400 shrink-0" />
                            TODO
                            <span className="text-green-700 font-normal">({todoItems.length})</span>
                          </div>
                          <button
                            onClick={() => { setShowAddInput(!showAddInput); if (!showAddInput) setNewTodoTitle(''); }}
                            className="text-green-700 hover:text-green-400 transition-colors p-0.5"
                            title="Add todo"
                          >
                            <Plus className="w-3.5 h-3.5" />
                          </button>
                        </div>
                        {showAddInput && (
                          <div className="mt-2">
                            <input
                              value={newTodoTitle}
                              onChange={e => setNewTodoTitle(e.target.value)}
                              onKeyDown={e => { if (e.key === 'Enter') handleCreateTodo(); if (e.key === 'Escape') { setShowAddInput(false); setNewTodoTitle(''); } }}
                              autoFocus
                              placeholder="New todo title..."
                              className="w-full bg-black/60 border border-green-700 text-green-300 px-2 py-1.5 text-xs font-mono focus:border-green-400 outline-none placeholder:text-green-900"
                            />
                          </div>
                        )}
                      </div>
                      <div className="flex-1 overflow-y-auto p-2 space-y-2">
                        {todoItems.length > 0 ? todoItems.map(t => renderCard(t)) : (
                          <EmptyColumn message="Add your first todo above" />
                        )}
                      </div>
                    </div>

                    {/* IN PROGRESS Column */}
                    <div className="flex-1 min-w-0 flex flex-col border-r border-green-900/50">
                      <div className="sticky top-0 z-10 bg-yellow-900/15 px-3 py-2 border-b border-green-900/50">
                        <div className="flex items-center gap-2 text-[10px] font-bold tracking-[0.2em] uppercase text-yellow-400">
                          <span className="w-2 h-2 rounded-full bg-yellow-400 shrink-0" />
                          IN PROGRESS
                          <span className="text-yellow-700 font-normal">({inProgressItems.length})</span>
                        </div>
                      </div>
                      <div className="flex-1 overflow-y-auto p-2 space-y-2">
                        {inProgressItems.length > 0 ? inProgressItems.map(t => renderCard(t)) : (
                          <EmptyColumn message="Move items here when you start working" />
                        )}
                      </div>
                    </div>

                    {/* DONE Column */}
                    <div className="flex-1 min-w-0 flex flex-col">
                      <div className="sticky top-0 z-10 bg-emerald-900/15 px-3 py-2 border-b border-green-900/50">
                        <div className="flex items-center gap-2 text-[10px] font-bold tracking-[0.2em] uppercase text-emerald-400">
                          <span className="w-2 h-2 rounded-full bg-emerald-500 shrink-0" />
                          DONE
                          <span className="text-emerald-700 font-normal">({doneItems.length})</span>
                        </div>
                      </div>
                      <div className="flex-1 overflow-y-auto p-2 space-y-2">
                        {doneItems.length > 0 ? doneItems.map(t => renderCard(t, true)) : (
                          <EmptyColumn message="Completed items will appear here" />
                        )}
                      </div>
                    </div>
                  </div>
                </>
              );
            })()
          )}
        </div>
      )}

      {/* TIMELINE Tab */}
      {detailTab === 'timeline' && selectedProject && (
        <div className="h-[calc(100vh-280px)] min-h-[400px]">
          <ProjectTimeline
            gitDir={selectedProject.git_dir}
            projectName={selectedProject.name}
            isActive={detailTab === 'timeline'}
          />
        </div>
      )}

      {/* DOCS Tab */}
      {detailTab === 'docs' && (
        <div className="space-y-3">
          {filesLoading ? (
            <div className="flex items-center justify-center py-8">
              <Loader className="w-5 h-5 text-green-700 animate-spin mr-2" />
              <span className="text-green-700 text-sm font-mono tracking-widest">LOADING FILES...</span>
            </div>
          ) : (
            <>
              {/* File selector buttons */}
              <div className="flex items-center gap-1.5 flex-wrap">
                {projectFiles.map(file => (
                  <button
                    key={file.name}
                    onClick={() => file.exists && setSelectedFile(file.name)}
                    disabled={!file.exists}
                    className={`flex items-center gap-1.5 px-3 py-1.5 text-[10px] font-bold tracking-widest uppercase transition-all border
                      ${selectedFile === file.name
                        ? 'text-green-300 border-green-500 bg-green-900/30'
                        : file.exists
                        ? 'text-green-600 border-green-900 hover:border-green-700 hover:text-green-400'
                        : 'text-green-900 border-green-900/50 cursor-not-allowed'}`}
                  >
                    <FileText className="w-3 h-3" />
                    {file.name}{!file.exists && ' (missing)'}
                  </button>
                ))}
              </div>

              {/* File content viewer */}
              {(() => {
                const file = projectFiles.find(f => f.name === selectedFile);
                if (!file || !file.exists) {
                  return (
                    <div className="flex flex-col items-center py-12 retro-border bg-black/40">
                      <FileText className="w-8 h-8 text-green-900 mb-2" />
                      <div className="text-green-600 text-sm font-mono mb-1">No file selected</div>
                      <div className="text-green-800 text-xs font-mono">Select a file above to view its contents.</div>
                    </div>
                  );
                }
                return (
                  <div className="border border-green-900/50">
                    {/* File header */}
                    <div className="flex items-center gap-2 px-3 py-2 border-b border-green-900/50 bg-green-900/10">
                      <FileText className="w-3.5 h-3.5 text-green-600" />
                      <span className="text-green-400 font-mono text-sm font-bold">{file.name}</span>
                    </div>
                    <div className="px-3 py-1.5 border-b border-green-900/30 bg-black/20">
                      <span className="text-green-800 font-mono text-[10px] break-all">{file.path}</span>
                    </div>
                    {/* File content */}
                    <div className="p-4 max-h-[60vh] overflow-auto">
                      {file.name.endsWith('.md') ? (
                        <div className="prose-green">
                          <MarkdownText content={file.content} />
                        </div>
                      ) : (
                        <pre className="text-green-400 font-mono text-sm whitespace-pre-wrap break-words">{file.content}</pre>
                      )}
                    </div>
                  </div>
                );
              })()}
            </>
          )}
        </div>
      )}

      {/* ENV VARS Tab */}
      {detailTab === 'env-vars' && (
        <div className="space-y-3">
          {/* Scope selector */}
          <div className="flex items-center gap-1 flex-wrap">
            {scopeButtons.map(btn => (
              <button
                key={btn.id}
                onClick={() => { setEnvScope(btn.id); setEditingVarId(null); }}
                className={`flex items-center gap-1.5 px-3 py-1.5 text-[10px] font-bold tracking-widest uppercase transition-all border
                  ${envScope === btn.id
                    ? 'text-green-300 border-green-500 bg-green-900/30'
                    : 'text-green-700 border-green-900 hover:border-green-700 hover:text-green-500'}`}
              >
                <btn.icon className="w-3 h-3" />
                {btn.label}
              </button>
            ))}
            {envScope === 'worktree' && (
              <div className="relative ml-2">
                <select
                  value={selectedSlot}
                  onChange={e => setSelectedSlot(Number(e.target.value))}
                  className="bg-black/60 border border-green-900 text-green-300 px-2 py-1.5 text-xs font-mono focus:border-green-500 outline-none appearance-none pr-6"
                >
                  <option value={0}>Slot 0 (main)</option>
                  {worktreeSlots.map(s => (
                    <option key={s.slot} value={s.slot}>Slot {s.slot} ({s.branch})</option>
                  ))}
                </select>
                <ChevronDown className="absolute right-1.5 top-1/2 -translate-y-1/2 w-3 h-3 text-green-700 pointer-events-none" />
              </div>
            )}
            {envScope === 'effective' && (
              <div className="relative ml-2">
                <select
                  value={selectedSlot}
                  onChange={e => setSelectedSlot(Number(e.target.value))}
                  className="bg-black/60 border border-green-900 text-green-300 px-2 py-1.5 text-xs font-mono focus:border-green-500 outline-none appearance-none pr-6"
                >
                  <option value={0}>Slot 0 (main)</option>
                  {worktreeSlots.map(s => (
                    <option key={s.slot} value={s.slot}>Slot {s.slot} ({s.branch})</option>
                  ))}
                </select>
                <ChevronDown className="absolute right-1.5 top-1/2 -translate-y-1/2 w-3 h-3 text-green-700 pointer-events-none" />
              </div>
            )}
          </div>

          {/* Env Vars Table */}
          {envLoading ? (
            <div className="flex items-center justify-center py-8">
              <Loader className="w-5 h-5 text-green-700 animate-spin mr-2" />
              <span className="text-green-700 text-sm font-mono tracking-widest">LOADING...</span>
            </div>
          ) : envScope === 'effective' ? (
            /* Effective: read-only table */
            <div className="border border-green-900/50">
              <div className="flex items-center px-3 py-2 border-b border-green-900/50 bg-green-900/10">
                <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold flex-1 min-w-[140px]">NAME</span>
                <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold flex-[2] min-w-0">VALUE</span>
                <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold w-[80px] text-right">SOURCE</span>
              </div>
              {effectiveVars.length === 0 ? (
                <div className="flex flex-col items-center py-8">
                  <Layers className="w-8 h-8 text-green-900 mb-2" />
                  <div className="text-green-600 text-sm font-mono mb-1">No effective variables</div>
                  <div className="text-green-800 text-xs font-mono">Add variables at any scope to see them merged here.</div>
                </div>
              ) : effectiveVars.map((v, i) => (
                <div key={v.key} className={`flex items-center px-3 py-2 ${i < effectiveVars.length - 1 ? 'border-b border-green-900/30' : ''} hover:bg-green-900/5`}>
                  <span className="text-green-400 font-mono text-sm font-bold flex-1 min-w-[140px] truncate">{v.key}</span>
                  <span className="flex-[2] min-w-0 flex items-center gap-2">
                    <span className="text-green-300 font-mono text-sm truncate">
                      {v.is_secret ? '••••••••' : v.value}
                    </span>
                    {!!v.is_secret && (
                      <span className="text-green-800 text-[9px] tracking-widest uppercase border border-green-900/50 px-1.5 py-0.5 shrink-0">SECRET</span>
                    )}
                  </span>
                  <span className={`w-[80px] text-right text-[9px] tracking-widest uppercase font-mono ${
                    v.source === 'global' ? 'text-blue-500' :
                    v.source === 'project' ? 'text-green-500' :
                    'text-yellow-600'
                  }`}>{v.source}</span>
                </div>
              ))}
            </div>
          ) : (
            /* CRUD table for global/project/worktree */
            <div className="border border-green-900/50">
              <div className="flex items-center px-3 py-2 border-b border-green-900/50 bg-green-900/10">
                <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold flex-1 min-w-[140px]">NAME</span>
                <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold flex-[2] min-w-0">VALUE</span>
                <span className="w-[90px]" />
              </div>

              {getCurrentVars().length === 0 ? (
                <div className="flex flex-col items-center py-8">
                  <Key className="w-8 h-8 text-green-900 mb-2" />
                  <div className="text-green-600 text-sm font-mono mb-1">No {envScope} variables yet</div>
                  <div className="text-green-800 text-xs font-mono">Add variables using the form below.</div>
                </div>
              ) : getCurrentVars().map(v => (
                <div
                  key={v.id}
                  className={`flex items-center px-3 py-2 border-b border-green-900/30 transition-all duration-500 ${
                    flashVarId === v.id ? 'bg-green-900/30' :
                    editingVarId === v.id ? 'bg-green-900/10' : 'hover:bg-green-900/5'
                  }`}
                >
                  {editingVarId === v.id ? (
                    <>
                      <input value={editVarKey} onChange={e => setEditVarKey(e.target.value)}
                        onKeyDown={e => editKeyHandler(e, () => handleUpdateVar(v.id), () => setEditingVarId(null))}
                        className="flex-1 min-w-[120px] bg-black/60 border border-green-700 text-green-300 px-2 py-1 text-sm font-mono focus:border-green-500 outline-none" />
                      <input value={editVarValue} onChange={e => setEditVarValue(e.target.value)}
                        onKeyDown={e => editKeyHandler(e, () => handleUpdateVar(v.id), () => setEditingVarId(null))}
                        className="flex-[2] min-w-0 mx-2 bg-black/60 border border-green-700 text-green-300 px-2 py-1 text-sm font-mono focus:border-green-500 outline-none" />
                      <label className="flex items-center gap-1 text-green-700 text-[10px] tracking-widest cursor-pointer mr-2">
                        <input type="checkbox" checked={editVarSecret} onChange={e => setEditVarSecret(e.target.checked)} className="accent-green-500" />
                        SECRET
                      </label>
                      <button onClick={() => handleUpdateVar(v.id)} className="text-green-500 hover:text-green-300 flex items-center gap-1 mr-1" title="Save">
                        <Save className="w-4 h-4" /><span className="text-[10px] tracking-widest uppercase">Save</span>
                      </button>
                      <button onClick={() => setEditingVarId(null)} className="text-green-700 hover:text-green-500 flex items-center gap-1" title="Cancel">
                        <X className="w-4 h-4" /><span className="text-[10px] tracking-widest uppercase">Cancel</span>
                      </button>
                    </>
                  ) : (
                    <>
                      <span className="text-green-400 font-mono text-sm font-bold flex-1 min-w-[140px] truncate">{v.key}</span>
                      <span className="flex-[2] min-w-0 flex items-center gap-2">
                        <span className="text-green-300 font-mono text-sm truncate">
                          {v.is_secret && !revealedSecrets.has(v.id) ? '••••••••' : v.value}
                        </span>
                        {!!v.is_secret && !revealedSecrets.has(v.id) && (
                          <span className="text-green-800 text-[9px] tracking-widest uppercase border border-green-900/50 px-1.5 py-0.5 shrink-0">SECRET</span>
                        )}
                      </span>
                      <div className="w-[90px] flex items-center justify-end gap-1.5">
                        {!!v.is_secret && (
                          <button onClick={() => toggleReveal(v.id)} className="text-green-700 hover:text-green-500" title={revealedSecrets.has(v.id) ? 'Hide' : 'Reveal'}>
                            {revealedSecrets.has(v.id) ? <EyeOff className="w-3.5 h-3.5" /> : <Eye className="w-3.5 h-3.5" />}
                          </button>
                        )}
                        <button onClick={() => startEditVar(v)} className="text-green-700 hover:text-green-500" title="Edit">
                          <Edit3 className="w-3.5 h-3.5" />
                        </button>
                        <button onClick={() => handleDeleteVar(v.id)} className="text-red-900 hover:text-red-500" title="Delete">
                          <Trash2 className="w-3.5 h-3.5" />
                        </button>
                      </div>
                    </>
                  )}
                </div>
              ))}

              {/* Add row */}
              <div className="flex flex-wrap items-center gap-2 px-3 py-2 bg-black/30">
                <input ref={varKeyRef} value={newVarKey} onChange={e => setNewVarKey(e.target.value)}
                  placeholder="VARIABLE_NAME" onKeyDown={e => e.key === 'Enter' && handleAddVar()}
                  className="flex-1 min-w-[120px] bg-black/60 border border-green-900 text-green-300 px-2 py-1.5 text-sm font-mono focus:border-green-500 outline-none placeholder:text-green-900" />
                <input value={newVarValue} onChange={e => setNewVarValue(e.target.value)}
                  placeholder="value" onKeyDown={e => e.key === 'Enter' && handleAddVar()}
                  className="flex-[2] min-w-[120px] bg-black/60 border border-green-900 text-green-300 px-2 py-1.5 text-sm font-mono focus:border-green-500 outline-none placeholder:text-green-900" />
                <label className="flex items-center gap-1.5 text-green-700 text-[10px] tracking-widest uppercase cursor-pointer shrink-0">
                  <input type="checkbox" checked={newVarSecret} onChange={e => setNewVarSecret(e.target.checked)}
                    className="accent-green-500" />
                  SECRET
                </label>
                <button onClick={handleAddVar}
                  className="flex items-center gap-1 px-3 py-1.5 border border-green-700 text-green-500 hover:bg-green-900/30 hover:border-green-500 text-xs font-bold tracking-widest uppercase transition-all shrink-0">
                  <Plus className="w-3 h-3" /> ADD
                </button>
              </div>
            </div>
          )}
        </div>
      )}

      {/* WORKTREES Tab */}
      {detailTab === 'worktrees' && (
        <div className="space-y-2">
          {/* Registered worktree slots */}
          {worktreeSlots.map(s => (
            <div key={s.id} className="retro-border bg-black/40 hover:bg-green-900/10 transition-all px-4 py-3">
              <div className="flex items-center justify-between mb-1">
                <div className="flex items-center gap-2">
                  <GitBranch className="w-3.5 h-3.5 text-green-600" />
                  <span className="text-green-300 font-mono text-sm font-bold">{s.branch}</span>
                  <span className="text-[9px] tracking-widest uppercase px-1.5 py-0.5 border text-blue-500 border-blue-800 bg-blue-900/20">SLOT {s.slot}</span>
                </div>
                <button
                  onClick={() => { deleteWorktreeSlot(s.id).then(() => loadWorktreeSlots()); }}
                  className="text-red-900 hover:text-red-500" title="Delete slot"
                >
                  <Trash2 className="w-3.5 h-3.5" />
                </button>
              </div>
              {/* Computed ports from services */}
              {projectServices.length > 0 && (
                <div className="flex items-center gap-3 mt-1.5 mb-1 flex-wrap">
                  {projectServices.map(svc => (
                    <span key={svc.id} className="text-[10px] font-mono tracking-wider">
                      <span className="text-green-700">{svc.env_key}</span>{' '}
                      <span className="text-green-400">{svc.base_value + s.slot}</span>
                    </span>
                  ))}
                </div>
              )}
              <div className="text-green-700 font-mono text-xs truncate">{s.worktree_path || '--'}</div>
            </div>
          ))}
          {/* Physical worktrees detected from projects */}
          {physicalWorktrees.map(wt => {
            const branchName = wt.git_dir.split('/.worktrees/').pop() || wt.name;
            const relativePath = '.worktrees/' + branchName;
            return (
              <div key={wt.git_dir} className="retro-border bg-black/40 hover:bg-green-900/10 transition-all px-4 py-3">
                <div className="flex items-center justify-between mb-1">
                  <div className="flex items-center gap-2">
                    <GitBranch className="w-3.5 h-3.5 text-green-600" />
                    <span className="text-green-300 font-mono text-sm font-bold">{branchName}</span>
                  </div>
                </div>
                <div className="text-green-700 font-mono text-xs truncate mb-1.5">{relativePath}</div>
                <div className="flex items-center gap-3 text-green-800 text-[10px] font-mono tracking-wider">
                  <span>Last active: {timeAgo(wt.last_active_at)}</span>
                  {wt.history_count > 0 && <span>{wt.history_count} tasks</span>}
                </div>
              </div>
            );
          })}
          {worktreeSlots.length === 0 && physicalWorktrees.length === 0 && (
            <div className="flex flex-col items-center py-8 retro-border bg-black/40">
              <GitBranch className="w-8 h-8 text-green-900 mb-2" />
              <div className="text-green-600 text-sm font-mono mb-1">No worktrees found</div>
              <div className="text-green-800 text-xs font-mono">Worktrees are created when starting isolated workspaces.</div>
            </div>
          )}
        </div>
      )}

      {/* STATISTICS Tab */}
      {detailTab === 'statistics' && (
        <div className="space-y-4">
          {/* Time range selector */}
          <div className="flex items-center gap-1">
            {['24h', '7d', '30d', 'all'].map(r => (
              <button
                key={r}
                onClick={() => setStatsRange(r)}
                className={`px-3 py-1.5 text-[10px] font-bold tracking-widest uppercase transition-all border
                  ${statsRange === r
                    ? 'text-green-300 border-green-500 bg-green-900/30'
                    : 'text-green-700 border-green-900 hover:border-green-700 hover:text-green-500'}`}
              >
                {r.toUpperCase()}
              </button>
            ))}
          </div>

          {statsLoading ? (
            <div className="flex items-center justify-center py-8">
              <Loader className="w-5 h-5 text-green-700 animate-spin mr-2" />
              <span className="text-green-700 text-sm font-mono tracking-widest">LOADING STATISTICS...</span>
            </div>
          ) : statistics ? (
            <>
              {/* Stats Cards */}
              <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
                {/* Tasks */}
                <div className="retro-border bg-black/40 px-4 py-3">
                  <div className="text-green-700 text-[10px] tracking-widest uppercase font-bold mb-2">TASKS</div>
                  <div className="text-green-300 font-mono text-2xl font-bold">{statistics.tasks.total}</div>
                  <div className="flex items-center gap-2 mt-1 text-[10px] font-mono tracking-wider flex-wrap">
                    <span className="text-green-500">{statistics.tasks.completed} done</span>
                    {statistics.tasks.in_progress > 0 && <span className="text-yellow-600">{statistics.tasks.in_progress} active</span>}
                    {statistics.tasks.failed > 0 && <span className="text-red-500">{statistics.tasks.failed} failed</span>}
                  </div>
                  {statistics.tasks.total > 0 && (
                    <div className="mt-2 h-1.5 bg-green-900/30 overflow-hidden">
                      <div
                        className="h-full bg-green-500 transition-all"
                        style={{ width: `${statistics.tasks.completion_rate}%` }}
                      />
                    </div>
                  )}
                </div>

                {/* Agent Time */}
                <div className="retro-border bg-black/40 px-4 py-3">
                  <div className="text-green-700 text-[10px] tracking-widest uppercase font-bold mb-2">AGENT TIME</div>
                  <div className="text-green-300 font-mono text-2xl font-bold">
                    {statistics.agent_time.total_seconds >= 3600
                      ? `${(statistics.agent_time.total_seconds / 3600).toFixed(1)}h`
                      : `${Math.floor(statistics.agent_time.total_seconds / 60)}m`}
                  </div>
                  <div className="flex items-center gap-2 mt-1 text-[10px] font-mono tracking-wider flex-wrap">
                    <span className="text-green-500">
                      {statistics.agent_time.busy_seconds >= 3600
                        ? `${(statistics.agent_time.busy_seconds / 3600).toFixed(1)}h`
                        : `${Math.floor(statistics.agent_time.busy_seconds / 60)}m`} busy
                    </span>
                    <span className="text-green-700">
                      {statistics.agent_time.idle_seconds >= 3600
                        ? `${(statistics.agent_time.idle_seconds / 3600).toFixed(1)}h`
                        : `${Math.floor(statistics.agent_time.idle_seconds / 60)}m`} idle
                    </span>
                  </div>
                </div>

                {/* Completion Rate */}
                <div className="retro-border bg-black/40 px-4 py-3">
                  <div className="text-green-700 text-[10px] tracking-widest uppercase font-bold mb-2">COMPLETION</div>
                  <div className="text-green-300 font-mono text-2xl font-bold">{statistics.tasks.completion_rate.toFixed(0)}%</div>
                  <div className="text-green-700 text-[10px] font-mono tracking-wider mt-1">
                    {statistics.tasks.completed} of {statistics.tasks.total} tasks
                  </div>
                </div>

                {/* Activity */}
                <div className="retro-border bg-black/40 px-4 py-3">
                  <div className="text-green-700 text-[10px] tracking-widest uppercase font-bold mb-2">ACTIVITY</div>
                  <div className="text-green-300 font-mono text-2xl font-bold">
                    {statistics.activity.reduce((sum, a) => sum + a.count, 0)}
                  </div>
                  <div className="text-green-700 text-[10px] font-mono tracking-wider mt-1">
                    events in {statsRange}
                  </div>
                </div>
              </div>

              {/* Top Tools */}
              {statistics.top_tools.length > 0 && (
                <div className="retro-border bg-black/40 px-4 py-3">
                  <div className="flex items-center gap-2 mb-3">
                    <Wrench className="w-4 h-4 text-green-600" />
                    <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold">TOP TOOLS</span>
                  </div>
                  <div className="space-y-2">
                    {statistics.top_tools.map(t => {
                      const maxCount = statistics.top_tools[0]?.count || 1;
                      return (
                        <div key={t.tool} className="flex items-center gap-3">
                          <span className="text-green-600 font-mono text-xs w-[140px] truncate">{t.tool}</span>
                          <div className="flex-1 h-3 bg-green-900/20 overflow-hidden">
                            <div
                              className="h-full bg-green-700/60 transition-all"
                              style={{ width: `${(t.count / maxCount) * 100}%` }}
                            />
                          </div>
                          <span className="text-green-500 font-mono text-xs w-[40px] text-right">{t.count}</span>
                        </div>
                      );
                    })}
                  </div>
                </div>
              )}

              {/* Hourly Activity */}
              {statistics.activity.length > 0 && (
                <div className="retro-border bg-black/40 px-4 py-3">
                  <div className="flex items-center gap-2 mb-3">
                    <Activity className="w-4 h-4 text-green-600" />
                    <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold">ACTIVITY TIMELINE</span>
                  </div>
                  <div className="flex items-end gap-[2px] h-[80px]">
                    {statistics.activity.map((a, i) => {
                      const maxCount = Math.max(...statistics.activity.map(x => x.count), 1);
                      const height = maxCount > 0 ? (a.count / maxCount) * 100 : 0;
                      return (
                        <div key={i} className="flex-1 flex flex-col items-center justify-end h-full" title={`${a.hour}: ${a.count} events`}>
                          <div
                            className="w-full bg-green-700/50 hover:bg-green-500/60 transition-all min-h-[1px]"
                            style={{ height: `${Math.max(height, 2)}%` }}
                          />
                        </div>
                      );
                    })}
                  </div>
                  <div className="flex justify-between mt-1">
                    <span className="text-green-900 text-[8px] font-mono">{statistics.activity[0]?.hour}</span>
                    <span className="text-green-900 text-[8px] font-mono">{statistics.activity[statistics.activity.length - 1]?.hour}</span>
                  </div>
                </div>
              )}

              {/* Empty state */}
              {statistics.tasks.total === 0 && statistics.activity.length === 0 && (
                <div className="flex flex-col items-center py-8 retro-border bg-black/40">
                  <BarChart3 className="w-8 h-8 text-green-900 mb-2" />
                  <div className="text-green-600 text-sm font-mono mb-1">No statistics available</div>
                  <div className="text-green-800 text-xs font-mono">Run some tasks to see statistics here.</div>
                </div>
              )}
            </>
          ) : (
            <div className="flex flex-col items-center py-8 retro-border bg-black/40">
              <BarChart3 className="w-8 h-8 text-green-900 mb-2" />
              <div className="text-green-600 text-sm font-mono mb-1">Statistics unavailable</div>
              <div className="text-green-800 text-xs font-mono">Could not load project statistics.</div>
            </div>
          )}
        </div>
      )}

      {/* Delete project confirmation */}
      <ConfirmationModal
        isOpen={!!deleteConfirmProject}
        onClose={() => setDeleteConfirmProject(null)}
        onConfirm={confirmDeleteProject}
        title="DELETE_PROJECT"
        message={`Permanently delete "${deleteConfirmProject?.name || deleteConfirmProject?.git_dir.split('/').pop()}" and ALL associated data? Tasks, history, notes, goals, env vars, todos will be removed from the database. This cannot be undone.`}
        confirmLabel="DELETE"
      />
    </div>
  );
};
