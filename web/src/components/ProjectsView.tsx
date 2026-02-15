import React, { useState, useEffect, useCallback, useRef } from 'react';
import {
  FolderGit2, ArrowLeft, Search, Plus, Trash2, Eye, EyeOff, Save, Edit3,
  Key, GitBranch, Play, ExternalLink, X, Loader, Globe, Layers, ChevronDown,
} from 'lucide-react';
import { AppTab, AgentSession } from '../types';
import {
  ProjectInfo, fetchProjects, deleteProject, createNewSession,
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
} from '../services/api';

interface ProjectsViewProps {
  sessions: AgentSession[];
  onSwitchTab: (tab: AppTab) => void;
}

type EnvScope = 'effective' | 'global' | 'project' | 'worktree';
type DetailTab = 'env-vars' | 'worktrees';

export const ProjectsView: React.FC<ProjectsViewProps> = ({ sessions, onSwitchTab }) => {
  // Project list state
  const [projects, setProjects] = useState<ProjectInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [searchQuery, setSearchQuery] = useState('');
  const searchRef = useRef<HTMLInputElement>(null);

  // Detail view state
  const [selectedProject, setSelectedProject] = useState<ProjectInfo | null>(null);
  const [detailTab, setDetailTab] = useState<DetailTab>('env-vars');
  const [envScope, setEnvScope] = useState<EnvScope>('effective');

  // Add project modal
  const [showAddModal, setShowAddModal] = useState(false);
  const [addPath, setAddPath] = useState('');
  const [addName, setAddName] = useState('');

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

  // Filter projects by search
  // Filter out worktree paths (they belong to parent projects, not standalone)
  const topLevelProjects = projects.filter(p => !p.git_dir.includes('/.worktrees/'));
  const filteredProjects = topLevelProjects
    .filter(p => {
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

  // Session creation
  const handleStartSession = async (project: ProjectInfo) => {
    setCreatingSession(project.git_dir);
    try {
      await createNewSession(project.name, project.git_dir);
    } finally {
      setCreatingSession(null);
    }
  };

  // Add project
  const handleAddProject = async () => {
    if (!addPath.trim()) return;
    const name = addName.trim() || addPath.split('/').filter(Boolean).pop() || 'project';
    // Use the createNewSession to register
    // Actually, just register via a lightweight approach — the session create also registers
    // For now, use the session API to at least register the project
    await createNewSession(name, addPath.trim());
    setShowAddModal(false);
    setAddPath(''); setAddName('');
    await loadProjects();
  };

  // Delete project
  const handleDeleteProject = async (project: ProjectInfo) => {
    await deleteProject(project.git_dir);
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
              return (
                <div
                  key={project.git_dir}
                  className="retro-border bg-black/40 hover:bg-green-900/10 transition-all cursor-pointer"
                  onClick={() => setSelectedProject(project)}
                >
                  <div className="px-4 py-3 flex items-center justify-between flex-wrap gap-2">
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2 mb-1">
                        <span className="text-green-300 font-bold font-mono text-sm tracking-wider truncate">{project.name || project.git_dir.split('/').pop()}</span>
                        <span className={`text-[9px] tracking-widest uppercase px-1.5 py-0.5 border shrink-0 ${
                          active
                            ? 'text-green-400 border-green-600 bg-green-900/30'
                            : 'text-green-800 border-green-900/50'
                        }`}>
                          {active ? 'ACTIVE' : 'INACTIVE'}
                        </span>
                      </div>
                      <div className="text-green-700 font-mono text-xs truncate">{project.git_dir}</div>
                      <div className="flex items-center gap-3 mt-1.5 text-green-800 text-[10px] font-mono tracking-wider">
                        <span>Last active: {timeAgo(project.last_active_at)}</span>
                        {sessionCount > 0 && <span>{sessionCount} session{sessionCount > 1 ? 's' : ''}</span>}
                        {windowCount > 0 && <span>{windowCount} window{windowCount > 1 ? 's' : ''}</span>}
                        {project.history_count > 0 && <span>{project.history_count} tasks</span>}
                      </div>
                    </div>
                    <div className="flex items-center gap-2 shrink-0">
                      {active ? (
                        <button
                          onClick={e => { e.stopPropagation(); onSwitchTab('WORKSTATIONS'); }}
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
              onClick={() => onSwitchTab('WORKSTATIONS')}
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
            { id: 'env-vars' as DetailTab, label: 'ENV VARS', icon: Key },
            { id: 'worktrees' as DetailTab, label: 'WORKTREES', icon: GitBranch },
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
        <div className="border border-green-900/50">
          <div className="flex items-center px-3 py-2 border-b border-green-900/50 bg-green-900/10">
            <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold w-[60px] shrink-0">SLOT</span>
            <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold flex-1 min-w-0">BRANCH</span>
            <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold flex-[2] min-w-0">PATH</span>
            <span className="w-[40px]" />
          </div>
          {worktreeSlots.length === 0 ? (
            <div className="flex flex-col items-center py-8">
              <GitBranch className="w-8 h-8 text-green-900 mb-2" />
              <div className="text-green-600 text-sm font-mono mb-1">No worktree slots allocated</div>
              <div className="text-green-800 text-xs font-mono">Worktrees are created when starting isolated workspaces.</div>
            </div>
          ) : worktreeSlots.map(s => (
            <div key={s.id} className="flex items-center px-3 py-2 border-b border-green-900/30 hover:bg-green-900/5">
              <span className="text-green-400 font-mono text-sm font-bold w-[60px] shrink-0">{s.slot}</span>
              <span className="text-green-300 font-mono text-sm flex-1 min-w-0 truncate">{s.branch}</span>
              <span className="text-green-700 font-mono text-xs flex-[2] min-w-0 truncate">{s.worktree_path || '--'}</span>
              <div className="w-[40px] flex items-center justify-end">
                <button
                  onClick={() => { deleteWorktreeSlot(s.id).then(() => loadWorktreeSlots()); }}
                  className="text-red-900 hover:text-red-500" title="Delete slot"
                >
                  <Trash2 className="w-3.5 h-3.5" />
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
};
