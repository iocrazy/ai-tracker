import React, { useState, useEffect, useMemo } from 'react';
import { X, Layers, Layout, Grid3X3, FolderGit2, ChevronDown, RefreshCw, GitBranch, RotateCcw, FolderOpen, Trash2 } from 'lucide-react';
import { LayoutType, fetchGitBranches, BranchInfo, fetchClosedWindows, ClosedWindow, deleteClosedWindow, resumeClosedWindow, fetchConfig, AgentDef } from '../services/api';

export type WindowType = 'simple' | 'worktree-3pane' | 'worktree-5pane';
export type ModalMode = 'create' | 'resume';

interface AddWindowModalProps {
  sessionName: string;
  gitDir?: string;
  openWindows?: string[];  // List of currently open window names
  onClose: () => void;
  onConfirm: (type: WindowType, branchName: string, baseBranch?: string, agent?: string) => void;
  onResume?: (branchName: string, layout: LayoutType) => void;
}

// Unified resume item: either a worktree or a closed window
interface ResumeItem {
  type: 'worktree' | 'window';
  id: string;              // unique key for React
  displayName: string;     // primary display: worktree dir basename or window name
  branchName?: string;     // git branch (shown as secondary for worktrees)
  workingDir?: string;     // full path
  paneCount?: number;      // original pane count (closed windows only)
  closedWindowId?: number; // DB id for closed window operations
  closedWindow?: ClosedWindow; // original data for resume
}

const WINDOW_TYPES: { type: WindowType; label: string; description: string; icon: React.ElementType; layout: LayoutType }[] = [
  {
    type: 'simple',
    label: 'SIMPLE',
    description: 'Single tmux window',
    icon: Layers,
    layout: 'simple',
  },
  {
    type: 'worktree-3pane',
    label: '3-PANE',
    description: 'Yazi + Lazygit + AI-CLI',
    icon: Layout,
    layout: 'default',
  },
  {
    type: 'worktree-5pane',
    label: '5-PANE',
    description: 'Workspace: Yazi + AI-CLI + Git + BE + FE',
    icon: Grid3X3,
    layout: 'workspace',
  },
];

// Extract basename from a path
const pathBasename = (p: string) => p.split('/').pop() || p;

export const AddWindowModal: React.FC<AddWindowModalProps> = ({
  sessionName,
  gitDir,
  openWindows = [],
  onClose,
  onConfirm,
  onResume,
}) => {
  const [mode, setMode] = useState<ModalMode>('create');
  const [selectedType, setSelectedType] = useState<WindowType>('simple');
  const [baseBranch, setBaseBranch] = useState('');
  const [newBranchName, setNewBranchName] = useState('');
  const [error, setError] = useState('');
  const [branches, setBranches] = useState<BranchInfo[]>([]);
  const [loadingBranches, setLoadingBranches] = useState(false);
  const [showDropdown, setShowDropdown] = useState(false);
  const [closedWindows, setClosedWindows] = useState<ClosedWindow[]>([]);
  const [loadingClosedWindows, setLoadingClosedWindows] = useState(false);
  const [agents, setAgents] = useState<Record<string, AgentDef>>({});
  const [selectedAgent, setSelectedAgent] = useState<string>('');

  // Unified selection state
  const [selectedItem, setSelectedItem] = useState<ResumeItem | null>(null);
  const [resumeLayout, setResumeLayout] = useState<LayoutType>('default');

  const needsBranch = selectedType !== 'simple';

  // Convert branch name to window name (replace : and / with -)
  const branchToWindowName = (branch: string) => branch.replace(/[:/]/g, '-');

  // Build unified resume list
  const resumeItems = useMemo((): ResumeItem[] => {
    const items: ResumeItem[] = [];

    // 1. Closed worktrees (worktree exists on disk but no open tmux window)
    const worktreeWindowNames = new Set<string>();
    for (const b of branches) {
      if (!b.has_worktree) continue;
      const windowName = branchToWindowName(b.name);
      if (openWindows.includes(windowName)) continue;

      worktreeWindowNames.add(windowName);
      const dirName = b.worktree_path ? pathBasename(b.worktree_path) : windowName;
      items.push({
        type: 'worktree',
        id: `wt:${b.name}`,
        displayName: dirName,
        branchName: b.name,
        workingDir: b.worktree_path,
      });
    }

    // 2. Closed windows (from DB, excluding those already covered by worktrees)
    for (const w of closedWindows) {
      if (worktreeWindowNames.has(w.window_name)) continue;
      items.push({
        type: 'window',
        id: `cw:${w.id}`,
        displayName: w.window_name,
        branchName: w.git_branch || undefined,
        workingDir: w.working_dir,
        paneCount: w.pane_count,
        closedWindowId: w.id,
        closedWindow: w,
      });
    }

    return items;
  }, [branches, closedWindows, openWindows]);

  // Fetch branches, closed windows, and config when modal opens
  useEffect(() => {
    if (branches.length === 0) {
      loadBranches();
    }
    loadClosedWindows();
    fetchConfig().then(cfg => {
      setAgents(cfg.agents);
      setSelectedAgent(cfg.defaults.agent);
    }).catch(err => console.error('Failed to load config:', err));
  }, []);

  const loadClosedWindows = async () => {
    setLoadingClosedWindows(true);
    try {
      const windows = await fetchClosedWindows(sessionName);
      setClosedWindows(windows);
    } catch (err) {
      console.error('Failed to load closed windows:', err);
    } finally {
      setLoadingClosedWindows(false);
    }
  };

  const handleDeleteClosedWindow = async (id: number, e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      await deleteClosedWindow(id);
      setClosedWindows(prev => prev.filter(w => w.id !== id));
      if (selectedItem?.closedWindowId === id) {
        setSelectedItem(null);
      }
    } catch (err) {
      console.error('Failed to delete closed window:', err);
    }
  };

  const loadBranches = async () => {
    setLoadingBranches(true);
    try {
      const result = await fetchGitBranches(gitDir);
      if (result.branches_with_status) {
        setBranches(result.branches_with_status);
      } else {
        setBranches(result.branches.map(name => ({ name, has_worktree: false })));
      }
    } catch (err) {
      console.error('Failed to load branches:', err);
    } finally {
      setLoadingBranches(false);
    }
  };

  const loadAll = () => {
    loadBranches();
    loadClosedWindows();
  };

  const handleConfirm = () => {
    if (needsBranch && !newBranchName.trim()) {
      setError('Branch name is required for worktree layouts');
      return;
    }
    onConfirm(selectedType, newBranchName.trim(), baseBranch || undefined, needsBranch ? selectedAgent : undefined);
  };

  const handleResume = async () => {
    if (!selectedItem) {
      setError('Please select an item to resume');
      return;
    }

    if (selectedItem.type === 'worktree') {
      if (onResume && selectedItem.branchName) {
        onResume(selectedItem.branchName, resumeLayout);
      }
    } else {
      // Resume closed window
      const cw = selectedItem.closedWindow;
      if (!cw) return;
      try {
        const result = await resumeClosedWindow(
          sessionName,
          cw.window_name,
          cw.working_dir,
          resumeLayout as 'simple' | 'default' | 'workspace',
          cw.id
        );
        if (result.success) {
          onClose();
        } else {
          setError(result.message);
        }
      } catch (err) {
        setError('Failed to resume window');
      }
    }
  };

  const handleSelectItem = (item: ResumeItem) => {
    setSelectedItem(item);
    setError('');
    // Auto-select layout based on type and pane count
    if (item.type === 'worktree') {
      setResumeLayout('default');
    } else if (item.paneCount && item.paneCount >= 5) {
      setResumeLayout('workspace');
    } else if (item.paneCount && item.paneCount >= 3) {
      setResumeLayout('default');
    } else {
      setResumeLayout('simple');
    }
  };

  const isLoading = loadingBranches || loadingClosedWindows;

  return (
    <div className="fixed inset-0 bg-black/80 backdrop-blur-sm flex items-center justify-center z-50 p-4 overflow-y-auto">
      <div className="bg-black border-2 border-green-500 shadow-[0_0_30px_rgba(34,197,94,0.3)] w-full max-w-xl mx-4 my-auto max-h-[90vh] flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-3 border-b border-green-800 flex-shrink-0">
          <h2 className="text-2xl font-bold text-green-400 tracking-widest font-['VT323']">
            ADD WINDOW
          </h2>
          <button
            onClick={onClose}
            className="text-green-700 hover:text-green-400 transition-colors"
          >
            <X className="w-6 h-6" />
          </button>
        </div>

        {/* Content - scrollable */}
        <div className="p-6 space-y-5 overflow-y-auto flex-1">
          {/* Session Info */}
          <div className="text-base text-green-600">
            Session: <span className="text-green-400 font-bold">{sessionName}</span>
          </div>

          {/* Mode Tabs */}
          <div className="flex border-b border-green-800">
            <button
              onClick={() => { setMode('create'); setError(''); }}
              className={`flex-1 py-2 px-4 text-sm font-bold tracking-widest transition-all ${
                mode === 'create'
                  ? 'text-green-400 border-b-2 border-green-400 bg-green-900/20'
                  : 'text-green-700 hover:text-green-500'
              }`}
            >
              CREATE NEW
            </button>
            <button
              onClick={() => { setMode('resume'); setError(''); }}
              className={`flex-1 py-2 px-4 text-sm font-bold tracking-widest transition-all flex items-center justify-center gap-2 ${
                mode === 'resume'
                  ? 'text-green-400 border-b-2 border-green-400 bg-green-900/20'
                  : 'text-green-700 hover:text-green-500'
              }`}
            >
              <RotateCcw className="w-4 h-4" />
              RESUME
              {resumeItems.length > 0 && (
                <span className="bg-yellow-600 text-black text-xs px-1.5 py-0.5 rounded-full font-bold">
                  {resumeItems.length}
                </span>
              )}
            </button>
          </div>

          {/* CREATE Mode Content */}
          {mode === 'create' && (
            <>
              {/* Window Type Selection */}
              <div className="space-y-2">
                <label className="text-sm text-green-600 tracking-widest font-bold">SELECT LAYOUT:</label>
                <div className="grid grid-cols-1 sm:grid-cols-3 gap-2">
                  {WINDOW_TYPES.map(({ type, label, description, icon: Icon }) => (
                    <button
                      key={type}
                      onClick={() => {
                        setSelectedType(type);
                        setError('');
                      }}
                      className={`
                        flex items-center gap-3 p-3 border transition-all min-h-[60px]
                        sm:flex-col sm:items-center sm:justify-center sm:gap-1 sm:min-h-[100px]
                        ${selectedType === type
                          ? 'border-green-400 bg-green-900/30 text-green-400 shadow-[0_0_15px_rgba(34,197,94,0.3)]'
                          : 'border-green-900 text-green-700 hover:border-green-600 hover:text-green-500'
                        }
                      `}
                    >
                      <Icon className="w-8 h-8 flex-shrink-0 sm:mb-1" />
                      <div className="flex flex-col sm:items-center">
                        <span className="text-sm font-bold tracking-wider">{label}</span>
                        <span className="text-xs mt-0.5 sm:mt-1 opacity-80 sm:text-center leading-tight">{description}</span>
                      </div>
                    </button>
                  ))}
                </div>
              </div>

              {/* Agent Selector (only for non-simple layouts) */}
              {needsBranch && Object.keys(agents).length > 0 && (
                <div className="space-y-2">
                  <label className="text-sm text-green-600 tracking-widest font-bold">AI CLI:</label>
                  <div className="flex gap-2">
                    {Object.entries(agents).map(([name, agent]) => {
                      const isSelected = selectedAgent === name;
                      const color = agent.color || '#22c55e';
                      return (
                        <button
                          key={name}
                          onClick={() => setSelectedAgent(name)}
                          className={`
                            flex items-center gap-2 px-4 py-2 border transition-all
                            ${isSelected
                              ? 'bg-green-900/30'
                              : 'border-green-900 text-green-700 hover:border-green-600 hover:text-green-500'
                            }
                          `}
                          style={isSelected ? { borderColor: color, color, boxShadow: `0 0 15px ${color}33` } : undefined}
                        >
                          <span className="text-lg">{agent.icon || '🤖'}</span>
                          <span className="text-sm font-bold tracking-wider uppercase">{name}</span>
                        </button>
                      );
                    })}
                  </div>
                </div>
              )}
            </>
          )}

          {/* RESUME Mode Content - Unified List */}
          {mode === 'resume' && (
            <div className="space-y-4">
              <div className="flex items-center justify-between">
                <label className="text-sm text-green-600 tracking-widest font-bold">RESUMABLE:</label>
                <button
                  onClick={loadAll}
                  disabled={isLoading}
                  className="px-2 py-1 border border-green-800 text-green-700 hover:border-green-600 hover:text-green-500 transition-all"
                  title="Refresh"
                >
                  <RefreshCw className={`w-4 h-4 ${isLoading ? 'animate-spin' : ''}`} />
                </button>
              </div>

              {isLoading && resumeItems.length === 0 ? (
                <div className="text-green-600 py-4 text-center">Loading...</div>
              ) : resumeItems.length === 0 ? (
                <div className="text-green-700 py-8 text-center border border-green-900 bg-green-900/10">
                  <RotateCcw className="w-8 h-8 mx-auto mb-2 opacity-50" />
                  <p>No closed windows found</p>
                  <p className="text-xs mt-1 opacity-70">All windows are currently open</p>
                </div>
              ) : (
                <div className="space-y-2 max-h-48 overflow-y-auto">
                  {resumeItems.map((item) => {
                    const isSelected = selectedItem?.id === item.id;
                    const isWorktree = item.type === 'worktree';
                    return (
                      <button
                        key={item.id}
                        onClick={() => handleSelectItem(item)}
                        className={`w-full px-4 py-3 text-left font-mono transition-all flex items-center justify-between border group ${
                          isSelected
                            ? isWorktree
                              ? 'border-green-400 bg-green-900/30 text-green-400 shadow-[0_0_10px_rgba(34,197,94,0.3)]'
                              : 'border-cyan-400 bg-cyan-900/30 text-cyan-400 shadow-[0_0_10px_rgba(6,182,212,0.3)]'
                            : 'border-green-900 text-green-500 hover:border-green-700 hover:bg-green-900/10'
                        }`}
                      >
                        <div className="flex flex-col gap-1 min-w-0 flex-1 mr-2">
                          <span className="flex items-center gap-2">
                            {isWorktree ? <GitBranch className="w-4 h-4 flex-shrink-0" /> : <FolderOpen className="w-4 h-4 flex-shrink-0" />}
                            <span className="truncate">{item.displayName}</span>
                          </span>
                          {/* Secondary info: branch name for worktrees (when different from display), path for windows */}
                          {isWorktree && item.branchName && item.displayName !== branchToWindowName(item.branchName) && (
                            <span className="text-xs text-green-700 truncate pl-6">
                              branch: {item.branchName}
                            </span>
                          )}
                          {!isWorktree && item.workingDir && (
                            <span className="text-xs text-green-700 truncate pl-6">
                              {item.workingDir.replace(/^\/Users\/[^/]+/, '~')}
                            </span>
                          )}
                        </div>
                        <div className="flex items-center gap-2 flex-shrink-0">
                          {!isWorktree && item.paneCount && item.paneCount > 1 && (
                            <span className="text-xs text-yellow-600 bg-yellow-900/20 px-1.5 py-0.5 border border-yellow-800/50">
                              {item.paneCount}p
                            </span>
                          )}
                          <span className={`text-xs px-2 py-0.5 border ${
                            isWorktree
                              ? 'text-yellow-600 bg-yellow-900/20 border-yellow-800/50'
                              : 'text-cyan-600 bg-cyan-900/20 border-cyan-800/50'
                          }`}>
                            {isWorktree ? 'worktree' : 'window'}
                          </span>
                          {!isWorktree && item.closedWindowId && (
                            <button
                              onClick={(e) => handleDeleteClosedWindow(item.closedWindowId!, e)}
                              className="opacity-0 group-hover:opacity-100 text-red-600 hover:text-red-400 transition-all p-1"
                              title="Remove from list"
                            >
                              <Trash2 className="w-3 h-3" />
                            </button>
                          )}
                        </div>
                      </button>
                    );
                  })}
                </div>
              )}

              {/* Layout selector (shown when an item is selected) */}
              {selectedItem && (
                <div className="space-y-2 pt-2 border-t border-green-900">
                  <label className="text-sm text-green-600 tracking-widest font-bold">
                    RESUME WITH LAYOUT:
                    {selectedItem.paneCount && selectedItem.paneCount > 1 && (
                      <span className="text-green-700 font-normal ml-2">
                        (originally {selectedItem.paneCount} panes)
                      </span>
                    )}
                  </label>
                  <div className="grid grid-cols-3 gap-2">
                    {([
                      { layout: 'simple' as LayoutType, label: 'SIMPLE', icon: Layers },
                      { layout: 'default' as LayoutType, label: '3-PANE', icon: Layout },
                      { layout: 'workspace' as LayoutType, label: '5-PANE', icon: Grid3X3 },
                    ]).map(({ layout, label, icon: Icon }) => (
                      <button
                        key={layout}
                        onClick={() => setResumeLayout(layout)}
                        className={`flex items-center justify-center gap-2 p-3 border transition-all ${
                          resumeLayout === layout
                            ? selectedItem.type === 'worktree'
                              ? 'border-green-400 bg-green-900/30 text-green-400'
                              : 'border-cyan-400 bg-cyan-900/30 text-cyan-400'
                            : 'border-green-900 text-green-700 hover:border-green-600'
                        }`}
                      >
                        <Icon className="w-5 h-5" />
                        <span className="text-sm font-bold">{label}</span>
                      </button>
                    ))}
                  </div>
                </div>
              )}

              {error && (
                <p className="text-red-500 text-sm">{error}</p>
              )}
            </div>
          )}

          {/* Branch Selection (for worktree types in CREATE mode) */}
          {mode === 'create' && needsBranch && (
            <div className="space-y-5">
              {/* Base Branch Selector */}
              <div className="space-y-2">
                <div className="flex items-center justify-between">
                  <label className="flex items-center gap-2 text-base text-green-600 tracking-widest font-bold">
                    <FolderGit2 className="w-5 h-5" />
                    BASE BRANCH:
                  </label>
                  <button
                    onClick={loadBranches}
                    disabled={loadingBranches}
                    className="px-2 py-1 border border-green-800 text-green-700 hover:border-green-600 hover:text-green-500 transition-all"
                    title="Refresh branches"
                  >
                    <RefreshCw className={`w-4 h-4 ${loadingBranches ? 'animate-spin' : ''}`} />
                  </button>
                </div>
                <div className="relative">
                  <button
                    onClick={() => setShowDropdown(!showDropdown)}
                    className="w-full bg-black border-2 border-green-800 text-green-400 px-4 py-3 font-mono text-base text-left flex items-center justify-between focus:outline-none focus:border-green-400 focus:shadow-[0_0_10px_rgba(34,197,94,0.3)]"
                  >
                    <span className={baseBranch ? 'text-green-400' : 'text-green-700'}>
                      {baseBranch || 'Select base branch...'}
                    </span>
                    <ChevronDown className={`w-5 h-5 transition-transform ${showDropdown ? 'rotate-180' : ''}`} />
                  </button>

                  {showDropdown && (
                    <div className="absolute z-10 w-full mt-1 bg-black border-2 border-green-700 max-h-48 overflow-y-auto">
                      {loadingBranches ? (
                        <div className="px-4 py-3 text-green-600">Loading branches...</div>
                      ) : branches.length === 0 ? (
                        <div className="px-4 py-3 text-green-700">No branches found</div>
                      ) : (
                        branches.map((branch) => (
                          <button
                            key={branch.name}
                            onClick={() => {
                              setBaseBranch(branch.name);
                              setShowDropdown(false);
                            }}
                            className={`w-full px-4 py-2 text-left font-mono hover:bg-green-900/30 transition-colors flex items-center justify-between ${
                              branch.name === baseBranch ? 'bg-green-900/50 text-green-400' : 'text-green-500'
                            } ${branch.name === 'main' || branch.name === 'master' ? 'border-b border-green-900' : ''}`}
                          >
                            <span>
                              {branch.name}
                              {(branch.name === 'main' || branch.name === 'master') && (
                                <span className="ml-2 text-xs text-green-700">(default)</span>
                              )}
                            </span>
                            {branch.has_worktree && (
                              <span className="flex items-center gap-1 text-xs text-yellow-600 bg-yellow-900/20 px-1.5 py-0.5 border border-yellow-800/50">
                                <GitBranch className="w-3 h-3" />
                                worktree
                              </span>
                            )}
                          </button>
                        ))
                      )}
                    </div>
                  )}
                </div>
                <p className="text-xs text-green-800">New branch will be created from this base</p>
              </div>

              {/* New Branch Name Input */}
              <div className="space-y-2">
                <label className="text-base text-green-600 tracking-widest font-bold">
                  NEW BRANCH NAME:
                </label>
                <input
                  type="text"
                  value={newBranchName}
                  onChange={(e) => {
                    setNewBranchName(e.target.value);
                    setError('');
                  }}
                  placeholder="fix:auth-bug or feature:new-feature"
                  className="w-full bg-black border-2 border-green-800 text-green-400 px-4 py-3 font-mono text-base focus:outline-none focus:border-green-400 focus:shadow-[0_0_10px_rgba(34,197,94,0.3)]"
                />
                {error && (
                  <p className="text-red-500 text-sm">{error}</p>
                )}
                <p className="text-xs text-green-800">
                  Window name: <span className="text-green-500 font-mono">{newBranchName ? newBranchName.replace(/[:/]/g, '-') : '...'}</span>
                </p>
              </div>
            </div>
          )}

          {/* Layout Preview (CREATE mode only) */}
          {mode === 'create' && (
          <div className="border border-green-900 p-3 bg-green-900/10">
            <div className="text-xs text-green-700 mb-2 tracking-widest font-bold">LAYOUT PREVIEW:</div>
            {selectedType === 'simple' && (
              <div className="border border-green-700 h-12 flex items-center justify-center text-green-500 text-sm font-bold">
                Single Pane
              </div>
            )}
            {selectedType === 'worktree-3pane' && (
              <div className="flex gap-1 h-16">
                <div className="flex flex-col gap-1 w-1/3">
                  <div className="border border-green-700 flex-1 flex items-center justify-center text-green-500 text-xs font-bold">Yazi</div>
                  <div className="border border-green-700 flex-1 flex items-center justify-center text-green-500 text-xs font-bold">Lazygit</div>
                </div>
                <div className="border border-green-700 flex-1 flex items-center justify-center text-sm font-bold" style={selectedAgent && agents[selectedAgent]?.color ? { color: agents[selectedAgent].color } : { color: '#22c55e' }}>{selectedAgent ? `${agents[selectedAgent]?.icon || '🤖'} ${selectedAgent}` : 'AI-CLI'}</div>
              </div>
            )}
            {selectedType === 'worktree-5pane' && (
              <div className="grid grid-cols-3 grid-rows-2 gap-1 h-16">
                <div className="border border-green-700 flex items-center justify-center text-green-500 text-xs font-bold">Yazi</div>
                <div className="border border-green-700 col-span-2 flex items-center justify-center text-green-500 text-xs font-bold">{selectedAgent ? `${agents[selectedAgent]?.icon || '🤖'} ${selectedAgent}` : 'AI-CLI'}</div>
                <div className="border border-green-700 flex items-center justify-center text-green-500 text-xs font-bold">Git</div>
                <div className="border border-green-700 flex items-center justify-center text-green-500 text-xs font-bold">BE</div>
                <div className="border border-green-700 flex items-center justify-center text-green-500 text-xs font-bold">FE</div>
              </div>
            )}
          </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex justify-end gap-3 px-6 py-3 border-t border-green-800 flex-shrink-0">
          <button
            onClick={onClose}
            className="px-6 py-2 border border-green-800 text-green-600 hover:border-green-500 hover:text-green-400 transition-all tracking-widest text-sm font-bold"
          >
            CANCEL
          </button>
          {mode === 'create' ? (
            <button
              onClick={handleConfirm}
              className="px-6 py-2 bg-green-600 text-black font-bold hover:bg-green-400 transition-all tracking-widest text-sm shadow-[0_0_15px_rgba(34,197,94,0.5)]"
            >
              CREATE
            </button>
          ) : (
            <button
              onClick={handleResume}
              disabled={!selectedItem}
              className={`px-6 py-2 font-bold transition-all tracking-widest text-sm flex items-center gap-2 ${
                selectedItem
                  ? 'bg-yellow-600 text-black hover:bg-yellow-400 shadow-[0_0_15px_rgba(202,138,4,0.5)]'
                  : 'bg-green-900 text-green-700 cursor-not-allowed'
              }`}
            >
              <RotateCcw className="w-4 h-4" />
              RESUME
            </button>
          )}
        </div>
      </div>
    </div>
  );
};

export default AddWindowModal;
