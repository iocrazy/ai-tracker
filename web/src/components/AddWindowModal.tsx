import React, { useState, useEffect } from 'react';
import { X, Layers, Layout, Grid3X3, FolderGit2, ChevronDown, RefreshCw, GitBranch, RotateCcw, FolderOpen, Trash2 } from 'lucide-react';
import { LayoutType, fetchGitBranches, BranchInfo, fetchClosedWindows, ClosedWindow, deleteClosedWindow, resumeClosedWindow } from '../services/api';

export type WindowType = 'simple' | 'worktree-3pane' | 'worktree-5pane';
export type ModalMode = 'create' | 'resume';

interface AddWindowModalProps {
  sessionName: string;
  gitDir?: string;
  openWindows?: string[];  // List of currently open window names
  onClose: () => void;
  onConfirm: (type: WindowType, branchName: string, baseBranch?: string) => void;
  onResume?: (branchName: string, layout: LayoutType) => void;
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
    description: 'Workspace: Yazi + Claude + Git + Backend + Frontend',
    icon: Grid3X3,
    layout: 'workspace',
  },
];

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
  const [baseBranch, setBaseBranch] = useState('');  // Base branch to create from
  const [newBranchName, setNewBranchName] = useState('');  // New branch/window name
  const [error, setError] = useState('');
  const [branches, setBranches] = useState<BranchInfo[]>([]);
  const [loadingBranches, setLoadingBranches] = useState(false);
  const [showDropdown, setShowDropdown] = useState(false);
  const [selectedResumeBranch, setSelectedResumeBranch] = useState('');
  const [resumeLayout, setResumeLayout] = useState<LayoutType>('default');
  // Closed windows (for resume without worktree)
  const [closedWindows, setClosedWindows] = useState<ClosedWindow[]>([]);
  const [loadingClosedWindows, setLoadingClosedWindows] = useState(false);
  const [selectedClosedWindow, setSelectedClosedWindow] = useState<ClosedWindow | null>(null);
  const [resumeType, setResumeType] = useState<'worktree' | 'window'>('worktree');
  const [closedWindowLayout, setClosedWindowLayout] = useState<'simple' | 'default' | 'workspace'>('simple');

  const needsBranch = selectedType !== 'simple';

  // Convert branch name to window name (replace : and / with -)
  const branchToWindowName = (branch: string) => branch.replace(/[:/]/g, '-');

  // Filter branches that have worktree but are not currently open
  // Note: openWindows contains window names (sanitized), branches contain git branch names
  const closedWorktrees = branches.filter(b =>
    b.has_worktree && !openWindows.includes(branchToWindowName(b.name))
  );

  // Fetch branches and closed windows when modal opens
  useEffect(() => {
    if (branches.length === 0) {
      loadBranches();
    }
    loadClosedWindows();
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
      if (selectedClosedWindow?.id === id) {
        setSelectedClosedWindow(null);
      }
    } catch (err) {
      console.error('Failed to delete closed window:', err);
    }
  };

  const loadBranches = async () => {
    setLoadingBranches(true);
    try {
      const result = await fetchGitBranches(gitDir);
      // Use branches_with_status if available, otherwise fall back to branches
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

  const handleConfirm = () => {
    if (needsBranch && !newBranchName.trim()) {
      setError('Branch name is required for worktree layouts');
      return;
    }
    // Pass the new branch name (which becomes window name) and base branch
    onConfirm(selectedType, newBranchName.trim(), baseBranch || undefined);
  };

  const handleResume = async () => {
    if (resumeType === 'worktree') {
      if (!selectedResumeBranch) {
        setError('Please select a worktree to resume');
        return;
      }
      if (onResume) {
        onResume(selectedResumeBranch, resumeLayout);
      }
    } else {
      // Resume closed window with optional layout
      if (!selectedClosedWindow) {
        setError('Please select a window to resume');
        return;
      }
      try {
        const result = await resumeClosedWindow(
          sessionName,
          selectedClosedWindow.window_name,
          selectedClosedWindow.working_dir,
          closedWindowLayout,
          selectedClosedWindow.id
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
              {(closedWorktrees.length > 0 || closedWindows.length > 0) && (
                <span className="bg-yellow-600 text-black text-xs px-1.5 py-0.5 rounded-full font-bold">
                  {closedWorktrees.length + closedWindows.length}
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
            </>
          )}

          {/* RESUME Mode Content */}
          {mode === 'resume' && (
            <div className="space-y-4">
              {/* Closed Windows Section */}
              {closedWindows.length > 0 && (
                <>
                  <div className="flex items-center justify-between">
                    <label className="text-sm text-green-600 tracking-widest font-bold">CLOSED WINDOWS:</label>
                    <button
                      onClick={loadClosedWindows}
                      disabled={loadingClosedWindows}
                      className="px-2 py-1 border border-green-800 text-green-700 hover:border-green-600 hover:text-green-500 transition-all"
                      title="Refresh"
                    >
                      <RefreshCw className={`w-4 h-4 ${loadingClosedWindows ? 'animate-spin' : ''}`} />
                    </button>
                  </div>
                  <div className="space-y-2 max-h-32 overflow-y-auto">
                    {closedWindows.map((win) => (
                      <button
                        key={win.id}
                        onClick={() => {
                          setSelectedClosedWindow(win);
                          setResumeType('window');
                          setSelectedResumeBranch('');
                          // Auto-select layout based on original pane count
                          if (win.pane_count >= 5) {
                            setClosedWindowLayout('workspace');
                          } else if (win.pane_count >= 3) {
                            setClosedWindowLayout('default');
                          } else {
                            setClosedWindowLayout('simple');
                          }
                        }}
                        className={`w-full px-4 py-3 text-left font-mono transition-all flex items-center justify-between border group ${
                          selectedClosedWindow?.id === win.id
                            ? 'border-cyan-400 bg-cyan-900/30 text-cyan-400 shadow-[0_0_10px_rgba(6,182,212,0.3)]'
                            : 'border-green-900 text-green-500 hover:border-green-700 hover:bg-green-900/10'
                        }`}
                      >
                        <div className="flex flex-col gap-1">
                          <span className="flex items-center gap-2">
                            <FolderOpen className="w-4 h-4" />
                            {win.window_name}
                          </span>
                          {win.working_dir && (
                            <span className="text-xs text-green-700 truncate max-w-[200px]">
                              {win.working_dir.replace(/^\/Users\/[^/]+/, '~')}
                            </span>
                          )}
                        </div>
                        <div className="flex items-center gap-2">
                          {win.pane_count > 1 && (
                            <span className="text-xs text-yellow-600 bg-yellow-900/20 px-1.5 py-0.5 border border-yellow-800/50">
                              {win.pane_count}p
                            </span>
                          )}
                          <span className="text-xs text-cyan-600 bg-cyan-900/20 px-2 py-0.5 border border-cyan-800/50">
                            window
                          </span>
                          <button
                            onClick={(e) => handleDeleteClosedWindow(win.id, e)}
                            className="opacity-0 group-hover:opacity-100 text-red-600 hover:text-red-400 transition-all p-1"
                            title="Remove from list"
                          >
                            <Trash2 className="w-3 h-3" />
                          </button>
                        </div>
                      </button>
                    ))}
                  </div>
                </>
              )}

              {/* Closed Worktrees Section */}
              <div className="flex items-center justify-between">
                <label className="text-sm text-green-600 tracking-widest font-bold">CLOSED WORKTREES:</label>
                <button
                  onClick={loadBranches}
                  disabled={loadingBranches}
                  className="px-2 py-1 border border-green-800 text-green-700 hover:border-green-600 hover:text-green-500 transition-all"
                  title="Refresh"
                >
                  <RefreshCw className={`w-4 h-4 ${loadingBranches ? 'animate-spin' : ''}`} />
                </button>
              </div>

              {loadingBranches ? (
                <div className="text-green-600 py-4 text-center">Loading...</div>
              ) : closedWorktrees.length === 0 && closedWindows.length === 0 ? (
                <div className="text-green-700 py-8 text-center border border-green-900 bg-green-900/10">
                  <RotateCcw className="w-8 h-8 mx-auto mb-2 opacity-50" />
                  <p>No closed windows found</p>
                  <p className="text-xs mt-1 opacity-70">All windows are currently open</p>
                </div>
              ) : closedWorktrees.length === 0 ? (
                <div className="text-green-700 py-4 text-center text-sm opacity-70">
                  No closed worktrees
                </div>
              ) : (
                <div className="space-y-2 max-h-32 overflow-y-auto">
                  {closedWorktrees.map((branch) => (
                    <button
                      key={branch.name}
                      onClick={() => { setSelectedResumeBranch(branch.name); setResumeType('worktree'); setSelectedClosedWindow(null); }}
                      className={`w-full px-4 py-3 text-left font-mono transition-all flex items-center justify-between border ${
                        selectedResumeBranch === branch.name
                          ? 'border-green-400 bg-green-900/30 text-green-400 shadow-[0_0_10px_rgba(34,197,94,0.3)]'
                          : 'border-green-900 text-green-500 hover:border-green-700 hover:bg-green-900/10'
                      }`}
                    >
                      <span className="flex items-center gap-2">
                        <GitBranch className="w-4 h-4" />
                        {branch.name}
                      </span>
                      <span className="text-xs text-yellow-600 bg-yellow-900/20 px-2 py-0.5 border border-yellow-800/50">
                        worktree
                      </span>
                    </button>
                  ))}
                </div>
              )}

              {/* Layout selector for worktree resume */}
              {selectedResumeBranch && resumeType === 'worktree' && (
                <div className="space-y-2 pt-2 border-t border-green-900">
                  <label className="text-sm text-green-600 tracking-widest font-bold">RESUME WITH LAYOUT:</label>
                  <div className="grid grid-cols-2 gap-2">
                    <button
                      onClick={() => setResumeLayout('default')}
                      className={`flex items-center justify-center gap-2 p-3 border transition-all ${
                        resumeLayout === 'default'
                          ? 'border-green-400 bg-green-900/30 text-green-400'
                          : 'border-green-900 text-green-700 hover:border-green-600'
                      }`}
                    >
                      <Layout className="w-5 h-5" />
                      <span className="text-sm font-bold">3-PANE</span>
                    </button>
                    <button
                      onClick={() => setResumeLayout('workspace')}
                      className={`flex items-center justify-center gap-2 p-3 border transition-all ${
                        resumeLayout === 'workspace'
                          ? 'border-green-400 bg-green-900/30 text-green-400'
                          : 'border-green-900 text-green-700 hover:border-green-600'
                      }`}
                    >
                      <Grid3X3 className="w-5 h-5" />
                      <span className="text-sm font-bold">5-PANE</span>
                    </button>
                  </div>
                </div>
              )}

              {/* Layout selector for closed window resume (when original had multiple panes) */}
              {selectedClosedWindow && resumeType === 'window' && selectedClosedWindow.pane_count >= 3 && (
                <div className="space-y-2 pt-2 border-t border-green-900">
                  <label className="text-sm text-green-600 tracking-widest font-bold">
                    RESUME WITH LAYOUT:
                    <span className="text-green-700 font-normal ml-2">
                      (originally {selectedClosedWindow.pane_count} panes)
                    </span>
                  </label>
                  <div className="grid grid-cols-3 gap-2">
                    <button
                      onClick={() => setClosedWindowLayout('simple')}
                      className={`flex items-center justify-center gap-2 p-3 border transition-all ${
                        closedWindowLayout === 'simple'
                          ? 'border-cyan-400 bg-cyan-900/30 text-cyan-400'
                          : 'border-green-900 text-green-700 hover:border-green-600'
                      }`}
                    >
                      <Layers className="w-5 h-5" />
                      <span className="text-sm font-bold">SIMPLE</span>
                    </button>
                    <button
                      onClick={() => setClosedWindowLayout('default')}
                      className={`flex items-center justify-center gap-2 p-3 border transition-all ${
                        closedWindowLayout === 'default'
                          ? 'border-cyan-400 bg-cyan-900/30 text-cyan-400'
                          : 'border-green-900 text-green-700 hover:border-green-600'
                      }`}
                    >
                      <Layout className="w-5 h-5" />
                      <span className="text-sm font-bold">3-PANE</span>
                    </button>
                    <button
                      onClick={() => setClosedWindowLayout('workspace')}
                      className={`flex items-center justify-center gap-2 p-3 border transition-all ${
                        closedWindowLayout === 'workspace'
                          ? 'border-cyan-400 bg-cyan-900/30 text-cyan-400'
                          : 'border-green-900 text-green-700 hover:border-green-600'
                      }`}
                    >
                      <Grid3X3 className="w-5 h-5" />
                      <span className="text-sm font-bold">5-PANE</span>
                    </button>
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
                <div className="border border-green-700 flex-1 flex items-center justify-center text-green-500 text-sm font-bold">AI-CLI</div>
              </div>
            )}
            {selectedType === 'worktree-5pane' && (
              <div className="grid grid-cols-3 grid-rows-2 gap-1 h-16">
                <div className="border border-green-700 flex items-center justify-center text-green-500 text-xs font-bold">Yazi</div>
                <div className="border border-green-700 col-span-2 flex items-center justify-center text-green-500 text-xs font-bold">Claude</div>
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
              disabled={!selectedResumeBranch && !selectedClosedWindow}
              className={`px-6 py-2 font-bold transition-all tracking-widest text-sm flex items-center gap-2 ${
                selectedResumeBranch || selectedClosedWindow
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
