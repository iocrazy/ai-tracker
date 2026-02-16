import React, { useState, useEffect, useRef, useCallback } from 'react';
import { Search, FolderGit2, Play, ExternalLink, ArrowRight, Command, Monitor, List, Terminal, Settings, BarChart3, X } from 'lucide-react';
import { AppTab, AgentSession } from '../types';
import { ProjectInfo } from '../services/api';

interface CommandPaletteProps {
  isOpen: boolean;
  onClose: () => void;
  projects: ProjectInfo[];
  sessions: AgentSession[];
  activeTab: AppTab;
  onSwitchTab: (tab: AppTab) => void;
  onOpenProject: (project: ProjectInfo) => void;
  onStartSession: (project: ProjectInfo) => void;
}

interface PaletteItem {
  id: string;
  label: string;
  description: string;
  group: string;
  icon: React.ElementType;
  action: () => void;
  badge?: string;
  badgeColor?: string;
}

export const CommandPalette: React.FC<CommandPaletteProps> = ({
  isOpen,
  onClose,
  projects,
  sessions,
  activeTab,
  onSwitchTab,
  onOpenProject,
  onStartSession,
}) => {
  const [query, setQuery] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  // Build items list
  const buildItems = useCallback((): PaletteItem[] => {
    const items: PaletteItem[] = [];
    const q = query.toLowerCase().trim();

    // Tab navigation
    const tabs: { id: AppTab; icon: React.ElementType; label: string }[] = [
      { id: 'WORKSTATIONS', icon: Monitor, label: 'Workstations' },
      { id: 'PROJECTS', icon: FolderGit2, label: 'Projects' },
      { id: 'TIMELINE', icon: List, label: 'Timeline' },
      { id: 'ANALYTICS', icon: BarChart3, label: 'Analytics' },
      { id: 'CONSOLE', icon: Terminal, label: 'Console' },
      { id: 'SETTINGS', icon: Settings, label: 'Settings' },
    ];

    // Filter top-level projects (exclude worktrees)
    const topLevel = projects.filter(p =>
      !p.git_dir.includes('/.worktrees/') && p.git_dir.startsWith('/')
    );

    // Active projects (have running sessions)
    const activeProjects = topLevel.filter(p =>
      sessions.some(s => s.gitDir === p.git_dir)
    );

    // Inactive projects
    const inactiveProjects = topLevel.filter(p =>
      !sessions.some(s => s.gitDir === p.git_dir)
    );

    if (!q) {
      // No query: show active projects first, then navigation
      activeProjects.forEach(p => {
        const name = p.name || p.git_dir.split('/').pop() || '';
        items.push({
          id: `project-${p.git_dir}`,
          label: name,
          description: p.git_dir,
          group: 'ACTIVE PROJECTS',
          icon: FolderGit2,
          action: () => { onOpenProject(p); onClose(); },
          badge: 'ACTIVE',
          badgeColor: 'text-green-400 border-green-600',
        });
      });

      // Navigation
      tabs.filter(t => t.id !== activeTab).forEach(t => {
        items.push({
          id: `tab-${t.id}`,
          label: `Go to ${t.label}`,
          description: `Switch to ${t.label} tab`,
          group: 'NAVIGATION',
          icon: t.icon,
          action: () => { onSwitchTab(t.id); onClose(); },
        });
      });
    } else {
      // With query: fuzzy search across everything
      // Projects
      topLevel.forEach(p => {
        const name = p.name || p.git_dir.split('/').pop() || '';
        if (!name.toLowerCase().includes(q) && !p.git_dir.toLowerCase().includes(q)) return;
        const isActive = sessions.some(s => s.gitDir === p.git_dir);
        items.push({
          id: `project-${p.git_dir}`,
          label: name,
          description: p.git_dir,
          group: 'PROJECTS',
          icon: FolderGit2,
          action: () => { onOpenProject(p); onClose(); },
          badge: isActive ? 'ACTIVE' : undefined,
          badgeColor: isActive ? 'text-green-400 border-green-600' : undefined,
        });
      });

      // Sessions (windows)
      sessions.forEach(s => {
        if (!s.name.toLowerCase().includes(q) && !(s.gitDir || '').toLowerCase().includes(q)) return;
        s.windows.forEach(w => {
          items.push({
            id: `window-${s.id}-${w.id}`,
            label: `${s.name} / ${w.name}`,
            description: `Window ${w.id} — ${w.status}`,
            group: 'SESSIONS',
            icon: Monitor,
            action: () => { onSwitchTab('WORKSTATIONS'); onClose(); },
            badge: w.status,
            badgeColor: w.status === 'BUSY' ? 'text-yellow-500 border-yellow-700' : 'text-green-600 border-green-800',
          });
        });
      });

      // Start inactive projects
      inactiveProjects.forEach(p => {
        const name = p.name || p.git_dir.split('/').pop() || '';
        if (!name.toLowerCase().includes(q) && !p.git_dir.toLowerCase().includes(q)) return;
        items.push({
          id: `start-${p.git_dir}`,
          label: `Start ${name}`,
          description: `Create new session for ${name}`,
          group: 'ACTIONS',
          icon: Play,
          action: () => { onStartSession(p); onClose(); },
        });
      });

      // Tab navigation
      tabs.forEach(t => {
        if (!t.label.toLowerCase().includes(q) && !t.id.toLowerCase().includes(q)) return;
        items.push({
          id: `tab-${t.id}`,
          label: `Go to ${t.label}`,
          description: `Switch to ${t.label} tab`,
          group: 'NAVIGATION',
          icon: t.icon,
          action: () => { onSwitchTab(t.id); onClose(); },
        });
      });
    }

    return items;
  }, [query, projects, sessions, activeTab, onSwitchTab, onOpenProject, onStartSession, onClose]);

  const items = buildItems();

  // Group items
  const groups: { name: string; items: PaletteItem[] }[] = [];
  const seenGroups = new Set<string>();
  items.forEach(item => {
    if (!seenGroups.has(item.group)) {
      seenGroups.add(item.group);
      groups.push({ name: item.group, items: [] });
    }
    groups.find(g => g.name === item.group)!.items.push(item);
  });

  // Reset selection when query changes
  useEffect(() => {
    setSelectedIndex(0);
  }, [query]);

  // Focus input when opened
  useEffect(() => {
    if (isOpen) {
      setQuery('');
      setSelectedIndex(0);
      setTimeout(() => inputRef.current?.focus(), 50);
    }
  }, [isOpen]);

  // Scroll selected item into view
  useEffect(() => {
    if (!listRef.current) return;
    const selected = listRef.current.querySelector('[data-selected="true"]');
    if (selected) {
      selected.scrollIntoView({ block: 'nearest' });
    }
  }, [selectedIndex]);

  // Keyboard navigation
  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setSelectedIndex(i => Math.min(i + 1, items.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setSelectedIndex(i => Math.max(i - 1, 0));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (items[selectedIndex]) {
        items[selectedIndex].action();
      }
    } else if (e.key === 'Escape') {
      e.preventDefault();
      onClose();
    }
  }, [items, selectedIndex, onClose]);

  if (!isOpen) return null;

  let flatIndex = 0;

  return (
    <div
      className="fixed inset-0 z-[100] flex items-start justify-center pt-[15vh] bg-black/80 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="w-full max-w-lg border border-green-700 bg-black shadow-[0_0_40px_rgba(34,197,94,0.3)]"
        onClick={e => e.stopPropagation()}
      >
        {/* Search input */}
        <div className="flex items-center gap-2 px-4 py-3 border-b border-green-900">
          <Search className="w-4 h-4 text-green-600 shrink-0" />
          <input
            ref={inputRef}
            value={query}
            onChange={e => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="Search projects, navigate, actions..."
            className="flex-1 bg-transparent text-green-300 font-mono text-sm outline-none placeholder:text-green-800"
            autoFocus
          />
          <div className="flex items-center gap-1 text-green-800 text-[9px] tracking-widest">
            <kbd className="px-1.5 py-0.5 border border-green-900 bg-green-900/20 font-mono">ESC</kbd>
          </div>
        </div>

        {/* Results */}
        <div ref={listRef} className="max-h-[50vh] overflow-y-auto">
          {items.length === 0 ? (
            <div className="flex flex-col items-center py-8">
              <Search className="w-8 h-8 text-green-900 mb-2" />
              <div className="text-green-600 text-sm font-mono">No results for "{query}"</div>
            </div>
          ) : (
            groups.map(group => (
              <div key={group.name}>
                <div className="px-4 py-1.5 text-green-700 text-[9px] tracking-widest uppercase font-bold bg-green-900/10 border-b border-green-900/30">
                  {group.name}
                </div>
                {group.items.map(item => {
                  const idx = flatIndex++;
                  const isSelected = idx === selectedIndex;
                  return (
                    <div
                      key={item.id}
                      data-selected={isSelected}
                      onClick={() => item.action()}
                      onMouseEnter={() => setSelectedIndex(idx)}
                      className={`flex items-center gap-3 px-4 py-2.5 cursor-pointer transition-all ${
                        isSelected ? 'bg-green-900/30 border-l-2 border-green-400' : 'border-l-2 border-transparent hover:bg-green-900/10'
                      }`}
                    >
                      <item.icon className={`w-4 h-4 shrink-0 ${isSelected ? 'text-green-400' : 'text-green-700'}`} />
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2">
                          <span className={`font-mono text-sm truncate ${isSelected ? 'text-green-300 font-bold' : 'text-green-500'}`}>
                            {item.label}
                          </span>
                          {item.badge && (
                            <span className={`text-[8px] tracking-widest uppercase px-1.5 py-0.5 border shrink-0 ${item.badgeColor || 'text-green-700 border-green-900'}`}>
                              {item.badge}
                            </span>
                          )}
                        </div>
                        <div className="text-green-800 font-mono text-xs truncate">{item.description}</div>
                      </div>
                      <ArrowRight className={`w-3 h-3 shrink-0 ${isSelected ? 'text-green-500' : 'text-green-900'}`} />
                    </div>
                  );
                })}
              </div>
            ))
          )}
        </div>

        {/* Footer hint */}
        <div className="flex items-center justify-between px-4 py-2 border-t border-green-900/50 bg-green-900/5">
          <div className="flex items-center gap-3 text-green-800 text-[9px] tracking-widest">
            <span><kbd className="px-1 py-0.5 border border-green-900/50 bg-green-900/20 font-mono mr-1">↑↓</kbd> navigate</span>
            <span><kbd className="px-1 py-0.5 border border-green-900/50 bg-green-900/20 font-mono mr-1">↵</kbd> select</span>
          </div>
          <div className="flex items-center gap-1 text-green-900 text-[9px] tracking-widest">
            <Command className="w-3 h-3" /><span>K</span>
          </div>
        </div>
      </div>
    </div>
  );
};
