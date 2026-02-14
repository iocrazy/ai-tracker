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
    // Full-screen modal overlay
    <div className="fixed inset-0 bg-black/80 z-50 flex items-center justify-center p-4" onClick={onClose}>
      <div className="bg-[#0a0f0a] border-2 border-green-600 w-full max-w-4xl max-h-[85vh] flex flex-col" onClick={e => e.stopPropagation()}>
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
        <div className="flex border-b border-green-900">
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

        {/* Content */}
        <div className="flex-1 overflow-y-auto p-4">
          {activeTab === 'variables' && (
            <div className="space-y-3">
              {/* Add new variable */}
              <div className="flex flex-wrap gap-2 items-end border border-green-900/50 p-3 bg-black/30">
                <div className="flex-1 min-w-[120px]">
                  <label className="text-green-700 text-[10px] tracking-widest uppercase block mb-1">KEY</label>
                  <input value={newVarKey} onChange={e => setNewVarKey(e.target.value)}
                    placeholder="VARIABLE_NAME" onKeyDown={e => e.key === 'Enter' && handleAddVar()}
                    className="w-full bg-black/60 border border-green-900 text-green-300 px-2 py-1.5 text-sm font-mono focus:border-green-500 outline-none" />
                </div>
                <div className="flex-1 min-w-[120px]">
                  <label className="text-green-700 text-[10px] tracking-widest uppercase block mb-1">VALUE</label>
                  <input value={newVarValue} onChange={e => setNewVarValue(e.target.value)}
                    placeholder="value" onKeyDown={e => e.key === 'Enter' && handleAddVar()}
                    className="w-full bg-black/60 border border-green-900 text-green-300 px-2 py-1.5 text-sm font-mono focus:border-green-500 outline-none" />
                </div>
                <label className="flex items-center gap-1.5 text-green-700 text-[10px] tracking-widest uppercase cursor-pointer">
                  <input type="checkbox" checked={newVarSecret} onChange={e => setNewVarSecret(e.target.checked)}
                    className="accent-green-500" />
                  SECRET
                </label>
                <button onClick={handleAddVar}
                  className="flex items-center gap-1 px-3 py-1.5 border border-green-700 text-green-500 hover:bg-green-900/30 hover:border-green-500 text-xs font-bold tracking-widest uppercase transition-all">
                  <Plus className="w-3 h-3" /> ADD
                </button>
              </div>

              {/* Variable list */}
              {vars.map(v => (
                <div key={v.id} className="flex items-center gap-2 border border-green-900/30 px-3 py-2 bg-black/20 hover:border-green-800 transition-colors">
                  {editingVarId === v.id ? (
                    <>
                      <input value={editVarKey} onChange={e => setEditVarKey(e.target.value)}
                        className="flex-1 bg-black/60 border border-green-700 text-green-300 px-2 py-1 text-sm font-mono focus:border-green-500 outline-none min-w-0" />
                      <input value={editVarValue} onChange={e => setEditVarValue(e.target.value)}
                        className="flex-1 bg-black/60 border border-green-700 text-green-300 px-2 py-1 text-sm font-mono focus:border-green-500 outline-none min-w-0" />
                      <label className="flex items-center gap-1 text-green-700 text-[10px] tracking-widest cursor-pointer">
                        <input type="checkbox" checked={editVarSecret} onChange={e => setEditVarSecret(e.target.checked)} className="accent-green-500" />
                        S
                      </label>
                      <button onClick={() => handleUpdateVar(v.id)} className="text-green-500 hover:text-green-300"><Save className="w-4 h-4" /></button>
                      <button onClick={() => setEditingVarId(null)} className="text-green-700 hover:text-green-500"><X className="w-4 h-4" /></button>
                    </>
                  ) : (
                    <>
                      <span className="text-green-500 font-mono text-sm font-bold min-w-[140px] truncate">{v.key}</span>
                      <span className="text-green-700 mx-1">=</span>
                      <span className="flex-1 text-green-300 font-mono text-sm truncate">
                        {v.is_secret && !revealedSecrets.has(v.id) ? '••••••••' : v.value}
                      </span>
                      {!!v.is_secret && (
                        <button onClick={() => toggleReveal(v.id)} className="text-green-700 hover:text-green-500">
                          {revealedSecrets.has(v.id) ? <EyeOff className="w-3.5 h-3.5" /> : <Eye className="w-3.5 h-3.5" />}
                        </button>
                      )}
                      <button onClick={() => startEditVar(v)} className="text-green-700 hover:text-green-500"><Edit3 className="w-3.5 h-3.5" /></button>
                      <button onClick={() => handleDeleteVar(v.id)} className="text-red-900 hover:text-red-500"><Trash2 className="w-3.5 h-3.5" /></button>
                    </>
                  )}
                </div>
              ))}
              {vars.length === 0 && (
                <div className="text-green-800 text-sm font-mono text-center py-8 tracking-widest">NO VARIABLES DEFINED</div>
              )}
            </div>
          )}

          {activeTab === 'services' && (
            <div className="space-y-3">
              {/* Add new service */}
              <div className="flex flex-wrap gap-2 items-end border border-green-900/50 p-3 bg-black/30">
                <div className="min-w-[100px] flex-1">
                  <label className="text-green-700 text-[10px] tracking-widest uppercase block mb-1">SERVICE</label>
                  <input value={newSvcName} onChange={e => setNewSvcName(e.target.value)}
                    placeholder="frontend" onKeyDown={e => e.key === 'Enter' && handleAddService()}
                    className="w-full bg-black/60 border border-green-900 text-green-300 px-2 py-1.5 text-sm font-mono focus:border-green-500 outline-none" />
                </div>
                <div className="w-[80px]">
                  <label className="text-green-700 text-[10px] tracking-widest uppercase block mb-1">BASE</label>
                  <input value={newSvcBase} onChange={e => setNewSvcBase(e.target.value)}
                    placeholder="5175" type="number" onKeyDown={e => e.key === 'Enter' && handleAddService()}
                    className="w-full bg-black/60 border border-green-900 text-green-300 px-2 py-1.5 text-sm font-mono focus:border-green-500 outline-none" />
                </div>
                <div className="w-[100px]">
                  <label className="text-green-700 text-[10px] tracking-widest uppercase block mb-1">TYPE</label>
                  <select value={newSvcType} onChange={e => setNewSvcType(e.target.value)}
                    className="w-full bg-black/60 border border-green-900 text-green-300 px-2 py-1.5 text-sm font-mono focus:border-green-500 outline-none">
                    <option value="port">port</option>
                    <option value="db_index">db_index</option>
                  </select>
                </div>
                <div className="min-w-[120px] flex-1">
                  <label className="text-green-700 text-[10px] tracking-widest uppercase block mb-1">ENV KEY</label>
                  <input value={newSvcEnvKey} onChange={e => setNewSvcEnvKey(e.target.value)}
                    placeholder="FRONTEND_PORT" onKeyDown={e => e.key === 'Enter' && handleAddService()}
                    className="w-full bg-black/60 border border-green-900 text-green-300 px-2 py-1.5 text-sm font-mono focus:border-green-500 outline-none" />
                </div>
                <button onClick={handleAddService}
                  className="flex items-center gap-1 px-3 py-1.5 border border-green-700 text-green-500 hover:bg-green-900/30 hover:border-green-500 text-xs font-bold tracking-widest uppercase transition-all">
                  <Plus className="w-3 h-3" /> ADD
                </button>
              </div>

              {/* Service list */}
              {services.map(s => (
                <div key={s.id} className="flex items-center gap-3 border border-green-900/30 px-3 py-2 bg-black/20 hover:border-green-800 transition-colors">
                  {editingSvcId === s.id ? (
                    <>
                      <input value={editSvcName} onChange={e => setEditSvcName(e.target.value)}
                        className="flex-1 bg-black/60 border border-green-700 text-green-300 px-2 py-1 text-sm font-mono focus:border-green-500 outline-none min-w-0" />
                      <input value={editSvcBase} onChange={e => setEditSvcBase(e.target.value)} type="number"
                        className="w-[70px] bg-black/60 border border-green-700 text-green-300 px-2 py-1 text-sm font-mono focus:border-green-500 outline-none" />
                      <select value={editSvcType} onChange={e => setEditSvcType(e.target.value)}
                        className="w-[90px] bg-black/60 border border-green-700 text-green-300 px-2 py-1 text-sm font-mono focus:border-green-500 outline-none">
                        <option value="port">port</option>
                        <option value="db_index">db_index</option>
                      </select>
                      <input value={editSvcEnvKey} onChange={e => setEditSvcEnvKey(e.target.value)}
                        className="flex-1 bg-black/60 border border-green-700 text-green-300 px-2 py-1 text-sm font-mono focus:border-green-500 outline-none min-w-0" />
                      <button onClick={() => handleUpdateService(s.id)} className="text-green-500 hover:text-green-300"><Save className="w-4 h-4" /></button>
                      <button onClick={() => setEditingSvcId(null)} className="text-green-700 hover:text-green-500"><X className="w-4 h-4" /></button>
                    </>
                  ) : (
                    <>
                      <span className="text-green-400 font-mono text-sm font-bold min-w-[100px]">{s.service_name}</span>
                      <span className="text-green-600 font-mono text-sm">{s.base_value}</span>
                      <span className="text-green-800 font-mono text-[10px] tracking-widest uppercase border border-green-900/50 px-1.5 py-0.5">{s.value_type}</span>
                      <span className="flex-1 text-green-500 font-mono text-sm">{s.env_key}</span>
                      <button onClick={() => startEditService(s)} className="text-green-700 hover:text-green-500"><Edit3 className="w-3.5 h-3.5" /></button>
                      <button onClick={() => handleDeleteService(s.id)} className="text-red-900 hover:text-red-500"><Trash2 className="w-3.5 h-3.5" /></button>
                    </>
                  )}
                </div>
              ))}
              {services.length === 0 && (
                <div className="text-green-800 text-sm font-mono text-center py-8 tracking-widest">NO SERVICES DEFINED</div>
              )}
            </div>
          )}

          {activeTab === 'worktrees' && (
            <div className="space-y-3">
              {slots.map(slot => (
                <div key={slot.id} className="border border-green-900/30 px-4 py-3 bg-black/20 hover:border-green-800 transition-colors">
                  <div className="flex items-center justify-between mb-2">
                    <div className="flex items-center gap-3">
                      <span className="text-green-600 font-mono text-[10px] tracking-widest uppercase bg-green-900/30 px-2 py-0.5 border border-green-900/50">
                        SLOT {slot.slot}
                      </span>
                      <span className="text-green-300 font-mono text-sm font-bold">{slot.branch}</span>
                    </div>
                    <button onClick={() => handleDeleteSlot(slot.id)} className="text-red-900 hover:text-red-500 text-[10px] tracking-widest uppercase font-bold flex items-center gap-1">
                      <Trash2 className="w-3 h-3" /> FREE
                    </button>
                  </div>
                  {slot.worktree_path && (
                    <div className="text-green-800 font-mono text-xs truncate mb-1">{slot.worktree_path}</div>
                  )}
                  {/* Calculated ports */}
                  <div className="flex flex-wrap gap-2 mt-1">
                    {services.map(svc => (
                      <span key={svc.id} className="text-green-600 font-mono text-xs">
                        {svc.env_key}=<span className="text-green-400">{svc.base_value + slot.slot}</span>
                      </span>
                    ))}
                  </div>
                </div>
              ))}
              {slots.length === 0 && (
                <div className="text-green-800 text-sm font-mono text-center py-8 tracking-widest">NO WORKTREE SLOTS ALLOCATED</div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
