import React, { useState, useEffect, useCallback } from 'react';
import { X, Plus, Trash2, Eye, EyeOff, Save, Edit3, Server, Key, GitBranch } from 'lucide-react';
import {
  ProjectEnvVar, ProjectService, WorktreeSlot,
  fetchProjectEnvVars, createProjectEnvVar, updateProjectEnvVar, deleteProjectEnvVar,
  fetchProjectServices, createProjectService, updateProjectService, deleteProjectService,
  fetchWorktreeSlots, deleteWorktreeSlot,
} from '../services/api';

interface ProjectSettingsProps {
  sessionName: string;
  onClose: () => void;
}

type SettingsTab = 'variables' | 'services' | 'worktrees';

const tabDescriptions: Record<SettingsTab, string> = {
  variables: 'Key-value pairs available to all worktrees via .worktree.env',
  services: 'Port and resource definitions. Each worktree gets base_value + slot offset.',
  worktrees: 'Active worktree slot allocations and their computed port assignments.',
};

export const ProjectSettings: React.FC<ProjectSettingsProps> = ({ sessionName, onClose }) => {
  const [activeTab, setActiveTab] = useState<SettingsTab>('variables');

  // Variables state
  const [vars, setVars] = useState<ProjectEnvVar[]>([]);
  const [newVarKey, setNewVarKey] = useState('');
  const [newVarValue, setNewVarValue] = useState('');
  const [newVarSecret, setNewVarSecret] = useState(false);
  const [editingVarId, setEditingVarId] = useState<number | null>(null);
  const [editVarKey, setEditVarKey] = useState('');
  const [editVarValue, setEditVarValue] = useState('');
  const [editVarSecret, setEditVarSecret] = useState(false);
  const [revealedSecrets, setRevealedSecrets] = useState<Set<number>>(new Set());

  // Services state
  const [services, setServices] = useState<ProjectService[]>([]);
  const [newSvcName, setNewSvcName] = useState('');
  const [newSvcBase, setNewSvcBase] = useState('');
  const [newSvcType, setNewSvcType] = useState('port');
  const [newSvcEnvKey, setNewSvcEnvKey] = useState('');
  const [editingSvcId, setEditingSvcId] = useState<number | null>(null);
  const [editSvcName, setEditSvcName] = useState('');
  const [editSvcBase, setEditSvcBase] = useState('');
  const [editSvcType, setEditSvcType] = useState('');
  const [editSvcEnvKey, setEditSvcEnvKey] = useState('');

  // Worktrees state
  const [slots, setSlots] = useState<WorktreeSlot[]>([]);

  const loadAll = useCallback(async () => {
    const [v, s, w] = await Promise.all([
      fetchProjectEnvVars(sessionName),
      fetchProjectServices(sessionName),
      fetchWorktreeSlots(sessionName),
    ]);
    setVars(v);
    setServices(s);
    setSlots(w);
  }, [sessionName]);

  useEffect(() => { loadAll(); }, [loadAll]);

  // Escape key to close
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, [onClose]);

  // ---- Variable handlers ----
  const handleAddVar = async () => {
    if (!newVarKey.trim()) return;
    await createProjectEnvVar(sessionName, newVarKey.trim(), newVarValue, newVarSecret);
    setNewVarKey(''); setNewVarValue(''); setNewVarSecret(false);
    loadAll();
  };

  const handleUpdateVar = async (id: number) => {
    await updateProjectEnvVar(id, { key: editVarKey, value: editVarValue, is_secret: editVarSecret });
    setEditingVarId(null);
    loadAll();
  };

  const handleDeleteVar = async (id: number) => {
    await deleteProjectEnvVar(id);
    loadAll();
  };

  const startEditVar = (v: ProjectEnvVar) => {
    setEditingVarId(v.id);
    setEditVarKey(v.key);
    setEditVarValue(v.value);
    setEditVarSecret(!!v.is_secret);
  };

  const toggleReveal = (id: number) => {
    setRevealedSecrets(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  };

  // ---- Service handlers ----
  const handleAddService = async () => {
    if (!newSvcName.trim() || !newSvcBase || !newSvcEnvKey.trim()) return;
    await createProjectService(sessionName, newSvcName.trim(), parseInt(newSvcBase), newSvcType, newSvcEnvKey.trim());
    setNewSvcName(''); setNewSvcBase(''); setNewSvcType('port'); setNewSvcEnvKey('');
    loadAll();
  };

  const handleUpdateService = async (id: number) => {
    await updateProjectService(id, {
      service_name: editSvcName, base_value: parseInt(editSvcBase),
      value_type: editSvcType, env_key: editSvcEnvKey,
    });
    setEditingSvcId(null);
    loadAll();
  };

  const handleDeleteService = async (id: number) => {
    await deleteProjectService(id);
    loadAll();
  };

  const startEditService = (s: ProjectService) => {
    setEditingSvcId(s.id);
    setEditSvcName(s.service_name);
    setEditSvcBase(String(s.base_value));
    setEditSvcType(s.value_type);
    setEditSvcEnvKey(s.env_key);
  };

  // ---- Worktree slot handlers ----
  const handleDeleteSlot = async (id: number) => {
    await deleteWorktreeSlot(id);
    loadAll();
  };

  const tabs: { id: SettingsTab; label: string; icon: React.ElementType }[] = [
    { id: 'variables', label: 'VARIABLES', icon: Key },
    { id: 'services', label: 'SERVICES', icon: Server },
    { id: 'worktrees', label: 'WORKTREES', icon: GitBranch },
  ];

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/80 backdrop-blur-sm animate-[fadeIn_0.2s_ease-out]"
      onClick={onClose}
    >
      <div
        className="retro-border bg-black shadow-[0_0_30px_rgba(34,197,94,0.3)] w-full max-w-4xl max-h-[85vh] flex flex-col"
        onClick={e => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-green-900">
          <div className="flex items-center gap-3">
            <span className="text-green-500 font-bold tracking-widest uppercase text-sm font-pixel">PROJECT:</span>
            <span className="text-green-300 font-bold tracking-wider text-lg font-pixel">{sessionName}</span>
          </div>
          <button onClick={onClose} className="text-green-700 hover:text-green-400 transition-colors">
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Tab Bar */}
        <div className="border-b border-green-900">
          <div className="flex">
            {tabs.map(tab => (
              <button
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                className={`flex items-center gap-2 px-4 py-2 text-xs font-bold tracking-widest uppercase transition-all
                  ${activeTab === tab.id
                    ? 'text-green-300 border-b-2 border-green-400 bg-green-900/20'
                    : 'text-green-700 hover:text-green-500'}`}
              >
                <tab.icon className="w-4 h-4" />
                {tab.label}
              </button>
            ))}
          </div>
          <div className="px-4 py-2 text-green-700 text-xs font-mono">
            {tabDescriptions[activeTab]}
          </div>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto p-4">
          {activeTab === 'variables' && renderVariablesTab()}
          {activeTab === 'services' && renderServicesTab()}
          {activeTab === 'worktrees' && renderWorktreesTab()}
        </div>
      </div>
    </div>
  );

  function renderVariablesTab() {
    return (
      <div className="flex flex-col">
        {vars.length > 0 ? (
          <div className="border border-green-900/50">
            {/* Column headers */}
            <div className="flex items-center px-3 py-2 border-b border-green-900/50 bg-green-900/10">
              <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold flex-1 min-w-[140px]">NAME</span>
              <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold flex-[2] min-w-0">VALUE</span>
              <span className="w-[90px]" />
            </div>

            {/* Variable rows */}
            {vars.map(v => (
              <div
                key={v.id}
                className={`flex items-center px-3 py-2 border-b border-green-900/30 transition-colors ${
                  editingVarId === v.id ? 'bg-green-900/10' : 'hover:bg-green-900/5'
                }`}
              >
                {editingVarId === v.id ? (
                  <>
                    <input value={editVarKey} onChange={e => setEditVarKey(e.target.value)}
                      className="flex-1 min-w-[120px] bg-black/60 border border-green-700 text-green-300 px-2 py-1 text-sm font-mono focus:border-green-500 outline-none" />
                    <input value={editVarValue} onChange={e => setEditVarValue(e.target.value)}
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

            {/* Add row at bottom of table */}
            <div className="flex flex-wrap items-center gap-2 px-3 py-2 bg-black/30">
              <input value={newVarKey} onChange={e => setNewVarKey(e.target.value)}
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
        ) : (
          <div className="flex flex-col items-center">
            {/* Empty state */}
            <div className="flex flex-col items-center py-10">
              <Key className="w-10 h-10 text-green-900 mb-3" />
              <div className="text-green-600 text-sm font-mono mb-1">No environment variables yet</div>
              <div className="text-green-800 text-xs font-mono">Add variables that will be shared across all worktrees.</div>
            </div>
            {/* Add form */}
            <div className="w-full border border-green-900/50">
              <div className="flex flex-wrap items-center gap-2 px-3 py-2 bg-black/30">
                <input value={newVarKey} onChange={e => setNewVarKey(e.target.value)}
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
          </div>
        )}
      </div>
    );
  }

  function renderServicesTab() {
    return (
      <div className="flex flex-col">
        {services.length > 0 ? (
          <div className="border border-green-900/50">
            {/* Column headers */}
            <div className="flex items-center px-3 py-2 border-b border-green-900/50 bg-green-900/10">
              <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold w-[130px] shrink-0">SERVICE</span>
              <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold w-[70px] shrink-0">BASE</span>
              <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold w-[80px] shrink-0">TYPE</span>
              <span className="text-green-700 text-[10px] tracking-widest uppercase font-bold flex-1 min-w-0">ENV KEY</span>
              <span className="w-[60px]" />
            </div>

            {/* Service rows */}
            {services.map(s => (
              <div
                key={s.id}
                className={`flex items-center px-3 py-2 border-b border-green-900/30 transition-colors ${
                  editingSvcId === s.id ? 'bg-green-900/10' : 'hover:bg-green-900/5'
                }`}
              >
                {editingSvcId === s.id ? (
                  <>
                    <input value={editSvcName} onChange={e => setEditSvcName(e.target.value)}
                      className="w-[120px] shrink-0 mr-2 bg-black/60 border border-green-700 text-green-300 px-2 py-1 text-sm font-mono focus:border-green-500 outline-none" />
                    <input value={editSvcBase} onChange={e => setEditSvcBase(e.target.value)} type="number"
                      className="w-[60px] shrink-0 mr-2 bg-black/60 border border-green-700 text-green-300 px-2 py-1 text-sm font-mono focus:border-green-500 outline-none" />
                    <select value={editSvcType} onChange={e => setEditSvcType(e.target.value)}
                      className="w-[80px] shrink-0 mr-2 bg-black/60 border border-green-700 text-green-300 px-2 py-1 text-sm font-mono focus:border-green-500 outline-none">
                      <option value="port">port</option>
                      <option value="db_index">db_index</option>
                    </select>
                    <input value={editSvcEnvKey} onChange={e => setEditSvcEnvKey(e.target.value)}
                      className="flex-1 min-w-0 mr-2 bg-black/60 border border-green-700 text-green-300 px-2 py-1 text-sm font-mono focus:border-green-500 outline-none" />
                    <button onClick={() => handleUpdateService(s.id)} className="text-green-500 hover:text-green-300 flex items-center gap-1 mr-1" title="Save">
                      <Save className="w-4 h-4" /><span className="text-[10px] tracking-widest uppercase">Save</span>
                    </button>
                    <button onClick={() => setEditingSvcId(null)} className="text-green-700 hover:text-green-500 flex items-center gap-1" title="Cancel">
                      <X className="w-4 h-4" /><span className="text-[10px] tracking-widest uppercase">Cancel</span>
                    </button>
                  </>
                ) : (
                  <>
                    <span className="text-green-400 font-mono text-sm font-bold w-[130px] shrink-0 truncate">{s.service_name}</span>
                    <span className="text-green-300 font-mono text-sm w-[70px] shrink-0">{s.base_value}</span>
                    <span className="w-[80px] shrink-0">
                      <span className="text-green-800 font-mono text-[10px] tracking-widest uppercase border border-green-900/50 px-1.5 py-0.5">{s.value_type}</span>
                    </span>
                    <span className="text-green-500 font-mono text-sm flex-1 min-w-0 truncate">{s.env_key}</span>
                    <div className="w-[60px] flex items-center justify-end gap-1.5">
                      <button onClick={() => startEditService(s)} className="text-green-700 hover:text-green-500" title="Edit">
                        <Edit3 className="w-3.5 h-3.5" />
                      </button>
                      <button onClick={() => handleDeleteService(s.id)} className="text-red-900 hover:text-red-500" title="Delete">
                        <Trash2 className="w-3.5 h-3.5" />
                      </button>
                    </div>
                  </>
                )}
              </div>
            ))}

            {/* Add row at bottom of table */}
            <div className="flex flex-wrap items-center gap-2 px-3 py-2 bg-black/30">
              <input value={newSvcName} onChange={e => setNewSvcName(e.target.value)}
                placeholder="service_name" onKeyDown={e => e.key === 'Enter' && handleAddService()}
                className="w-[120px] shrink-0 bg-black/60 border border-green-900 text-green-300 px-2 py-1.5 text-sm font-mono focus:border-green-500 outline-none placeholder:text-green-900" />
              <input value={newSvcBase} onChange={e => setNewSvcBase(e.target.value)}
                placeholder="5175" type="number" onKeyDown={e => e.key === 'Enter' && handleAddService()}
                className="w-[70px] shrink-0 bg-black/60 border border-green-900 text-green-300 px-2 py-1.5 text-sm font-mono focus:border-green-500 outline-none placeholder:text-green-900" />
              <select value={newSvcType} onChange={e => setNewSvcType(e.target.value)}
                className="w-[80px] shrink-0 bg-black/60 border border-green-900 text-green-300 px-2 py-1.5 text-sm font-mono focus:border-green-500 outline-none">
                <option value="port">port</option>
                <option value="db_index">db_index</option>
              </select>
              <input value={newSvcEnvKey} onChange={e => setNewSvcEnvKey(e.target.value)}
                placeholder="ENV_KEY" onKeyDown={e => e.key === 'Enter' && handleAddService()}
                className="flex-1 min-w-[100px] bg-black/60 border border-green-900 text-green-300 px-2 py-1.5 text-sm font-mono focus:border-green-500 outline-none placeholder:text-green-900" />
              <button onClick={handleAddService}
                className="flex items-center gap-1 px-3 py-1.5 border border-green-700 text-green-500 hover:bg-green-900/30 hover:border-green-500 text-xs font-bold tracking-widest uppercase transition-all shrink-0">
                <Plus className="w-3 h-3" /> ADD
              </button>
            </div>
          </div>
        ) : (
          <div className="flex flex-col items-center">
            {/* Empty state */}
            <div className="flex flex-col items-center py-10">
              <Server className="w-10 h-10 text-green-900 mb-3" />
              <div className="text-green-600 text-sm font-mono mb-1">No services defined yet</div>
              <div className="text-green-800 text-xs font-mono">Define port and resource allocations for worktree isolation.</div>
            </div>
            {/* Add form */}
            <div className="w-full border border-green-900/50">
              <div className="flex flex-wrap items-center gap-2 px-3 py-2 bg-black/30">
                <input value={newSvcName} onChange={e => setNewSvcName(e.target.value)}
                  placeholder="service_name" onKeyDown={e => e.key === 'Enter' && handleAddService()}
                  className="w-[120px] shrink-0 bg-black/60 border border-green-900 text-green-300 px-2 py-1.5 text-sm font-mono focus:border-green-500 outline-none placeholder:text-green-900" />
                <input value={newSvcBase} onChange={e => setNewSvcBase(e.target.value)}
                  placeholder="5175" type="number" onKeyDown={e => e.key === 'Enter' && handleAddService()}
                  className="w-[70px] shrink-0 bg-black/60 border border-green-900 text-green-300 px-2 py-1.5 text-sm font-mono focus:border-green-500 outline-none placeholder:text-green-900" />
                <select value={newSvcType} onChange={e => setNewSvcType(e.target.value)}
                  className="w-[80px] shrink-0 bg-black/60 border border-green-900 text-green-300 px-2 py-1.5 text-sm font-mono focus:border-green-500 outline-none">
                  <option value="port">port</option>
                  <option value="db_index">db_index</option>
                </select>
                <input value={newSvcEnvKey} onChange={e => setNewSvcEnvKey(e.target.value)}
                  placeholder="ENV_KEY" onKeyDown={e => e.key === 'Enter' && handleAddService()}
                  className="flex-1 min-w-[100px] bg-black/60 border border-green-900 text-green-300 px-2 py-1.5 text-sm font-mono focus:border-green-500 outline-none placeholder:text-green-900" />
                <button onClick={handleAddService}
                  className="flex items-center gap-1 px-3 py-1.5 border border-green-700 text-green-500 hover:bg-green-900/30 hover:border-green-500 text-xs font-bold tracking-widest uppercase transition-all shrink-0">
                  <Plus className="w-3 h-3" /> ADD
                </button>
              </div>
            </div>
          </div>
        )}
      </div>
    );
  }

  function renderWorktreesTab() {
    if (slots.length === 0) {
      return (
        <div className="flex flex-col items-center py-10">
          <GitBranch className="w-10 h-10 text-green-900 mb-3" />
          <div className="text-green-600 text-sm font-mono mb-1">No worktree slots allocated</div>
          <div className="text-green-800 text-xs font-mono">Slots are auto-assigned when a workspace starts.</div>
        </div>
      );
    }

    return (
      <div className="space-y-3">
        {slots.map(slot => (
          <div key={slot.id} className="border border-green-900/50 hover:border-green-800 transition-colors">
            {/* Slot header */}
            <div className="flex items-center justify-between px-4 py-2 border-b border-green-900/30 bg-green-900/5">
              <div className="flex items-center gap-3">
                <span className="text-green-600 font-mono text-[10px] tracking-widest uppercase bg-green-900/30 px-2 py-0.5 border border-green-900/50">
                  SLOT #{slot.slot}
                </span>
                <span className="text-green-300 font-mono text-sm font-bold">{slot.branch}</span>
              </div>
              <button onClick={() => handleDeleteSlot(slot.id)} className="text-red-900 hover:text-red-500 text-[10px] tracking-widest uppercase font-bold flex items-center gap-1">
                <Trash2 className="w-3 h-3" /> FREE
              </button>
            </div>

            {/* Port assignments */}
            <div className="px-4 py-2">
              {services.length > 0 ? (
                <div className="space-y-1">
                  {services.map(svc => (
                    <div key={svc.id} className="flex items-center font-mono text-xs">
                      <span className="text-green-600 w-[180px]">{svc.env_key}</span>
                      <span className="text-green-800 mr-1">=</span>
                      <span className="text-green-400">{svc.base_value + slot.slot}</span>
                    </div>
                  ))}
                </div>
              ) : (
                <div className="text-green-800 text-xs font-mono">Define services to see port assignments</div>
              )}
              {slot.worktree_path && (
                <div className="text-green-900 font-mono text-[11px] truncate mt-2 pt-2 border-t border-green-900/20">{slot.worktree_path}</div>
              )}
            </div>
          </div>
        ))}
      </div>
    );
  }
};
