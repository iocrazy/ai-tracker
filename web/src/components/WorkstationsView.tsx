import React, { useState, useRef, useEffect } from 'react';
import { AgentSession, AgentWindow } from '../types';
import { Plus, Terminal, Trash2, MessageSquare, XCircle, Pause, Check, Activity, PowerOff, Settings, GripVertical, Pencil, LayoutGrid } from 'lucide-react';
import { ProjectSettings } from './ProjectSettings';
import { tmuxRenameWindow, tmuxRenameSession, tmuxResetLayout } from '../services/tmux';
import {
  DndContext,
  closestCenter,
  PointerSensor,
  useSensor,
  useSensors,
  DragEndEvent,
  DragStartEvent,
  DragOverlay,
} from '@dnd-kit/core';
import {
  SortableContext,
  useSortable,
  horizontalListSortingStrategy,
} from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';

interface WorkstationsViewProps {
  sessions: AgentSession[];
  onRequestAddWindow: (sessionId: string) => void;
  onSelectWindow: (sessionName: string, windowName: string, windowId: string) => void;
  onSwitchToWindow: (sessionName: string, windowName: string, windowId: string) => void;
  onRequestDeleteSession: (sessionId: string, name: string) => void;
  onRequestDeleteWindow: (sessionId: string, windowId: string, name: string) => void;
  onViewHistory: (sessionName: string, windowName: string, windowId: string, claudePane?: string, gitDir?: string) => void;
  onReorderWindow?: (sessionName: string, sourceIndex: number, targetIndex: number) => void;
}

// Config for visual styles based on status
// Icons match tmux status bar: ● (BUSY), ⏸ (PAUSED), ✓ (COMPLETED)
const STATUS_STYLES = {
    IDLE: {
        text: 'text-green-500',
        bg: 'bg-green-800',
        border: 'border-green-700',
        shadow: 'shadow-none',
        barWidth: 'w-[10%]',
        icon: Activity,
        textIcon: ''  // No icon for IDLE
    },
    BUSY: {
        text: 'text-yellow-400',
        bg: 'bg-yellow-500',
        border: 'border-yellow-500',
        shadow: 'shadow-[#eab308]',
        barWidth: 'w-[80%]',
        icon: Activity,
        textIcon: '●'  // Filled circle for BUSY
    },
    PAUSED: {
        text: 'text-orange-400',
        bg: 'bg-orange-500',
        border: 'border-orange-500',
        shadow: 'shadow-[#f97316]',
        barWidth: 'w-[50%]',
        icon: Pause,
        textIcon: '⏸'  // Pause symbol for PAUSED
    },
    COMPLETED: {
        text: 'text-cyan-400',
        bg: 'bg-cyan-500',
        border: 'border-cyan-400',
        shadow: 'shadow-[#06b6d4]',
        barWidth: 'w-[100%]',
        icon: Check,
        textIcon: '✓'  // Check mark for COMPLETED
    },
    OFFLINE: {
        text: 'text-red-600',
        bg: 'bg-red-600',
        border: 'border-red-600',
        shadow: 'shadow-[#dc2626]',
        barWidth: 'w-[0%]',
        icon: PowerOff,
        textIcon: '✗'  // X for OFFLINE
    }
};

// Precomputed radial menu positions to avoid inline style recreation
const RADIAL_EXPANDED = {
  delete: { top: '-5px', left: '-65px' },
  console: { top: '50px', left: '-55px', transitionDelay: '50ms' },
  chat: { top: '70px', left: '-5px', transitionDelay: '100ms' },
} as const;
const RADIAL_COLLAPSED = {
  delete: { top: '5px', left: '5px' },
  console: { top: '5px', left: '5px', transitionDelay: '0ms' },
  chat: { top: '5px', left: '5px', transitionDelay: '0ms' },
} as const;

// Inner content of a window card (shared between sortable and overlay)
const WindowCardContent: React.FC<{
  win: AgentWindow;
  sessionIp: string;
  isExpanded: boolean;
  radialPos: typeof RADIAL_EXPANDED | typeof RADIAL_COLLAPSED;
  style: typeof STATUS_STYLES.IDLE;
  onCardClick?: () => void;
  onContextMenu?: (e: React.MouseEvent) => void;
  onAvatarTap?: (windowId: string, e: React.MouseEvent) => void;
  onDeleteWindow?: () => void;
  onSelectWindow?: () => void;
  onViewHistory?: () => void;
  dragHandleProps?: { listeners: Record<string, any> | undefined; attributes: Record<string, any> };
  isDragging?: boolean;
  renameMode?: boolean;
  onRenameConfirm?: (name: string) => void;
  onRenameCancel?: () => void;
}> = ({ win, sessionIp, isExpanded, radialPos, style, onCardClick, onContextMenu, onAvatarTap, onDeleteWindow, onSelectWindow, onViewHistory, dragHandleProps, isDragging, renameMode, onRenameConfirm, onRenameCancel }) => (
  <div
    className={`relative group cursor-pointer ${isDragging ? 'opacity-50' : ''}`}
    onClick={onCardClick}
    onContextMenu={onContextMenu}
  >
    {/* Drag Handle */}
    {dragHandleProps && (
      <div
        className="absolute top-1 left-1 z-30 p-1 text-green-800 hover:text-green-400 cursor-grab active:cursor-grabbing transition-colors"
        {...dragHandleProps.listeners}
        {...dragHandleProps.attributes}
        onClick={(e) => e.stopPropagation()}
      >
        <GripVertical className="w-4 h-4" />
      </div>
    )}

    {/* Avatar with Radial Menu */}
    <div className="absolute -top-5 right-4 z-20">
      <button
        onClick={(e) => { e.stopPropagation(); onDeleteWindow?.(); }}
        className={`absolute w-11 h-11 rounded-full bg-red-500 border-2 border-red-400 flex items-center justify-center transition-all duration-300 ease-out shadow-[0_0_15px_rgba(239,68,68,0.6)] hover:scale-110 hover:shadow-[0_0_20px_rgba(239,68,68,0.8)] ${
          isExpanded ? 'opacity-100 pointer-events-auto' : 'opacity-0 pointer-events-none scale-50'
        }`}
        style={radialPos.delete}
        title="DELETE"
      >
        <Trash2 className="w-5 h-5 text-black" />
      </button>

      <button
        onClick={(e) => { e.stopPropagation(); onSelectWindow?.(); }}
        className={`absolute w-11 h-11 rounded-full bg-green-500 border-2 border-green-400 flex items-center justify-center transition-all duration-300 ease-out shadow-[0_0_15px_rgba(34,197,94,0.6)] hover:scale-110 hover:shadow-[0_0_20px_rgba(34,197,94,0.8)] ${
          isExpanded ? 'opacity-100 pointer-events-auto' : 'opacity-0 pointer-events-none scale-50'
        }`}
        style={radialPos.console}
        title="CONSOLE"
      >
        <Terminal className="w-5 h-5 text-black" />
      </button>

      <button
        onClick={(e) => { e.stopPropagation(); onViewHistory?.(); }}
        className={`absolute w-11 h-11 rounded-full bg-cyan-500 border-2 border-cyan-400 flex items-center justify-center transition-all duration-300 ease-out shadow-[0_0_15px_rgba(6,182,212,0.6)] hover:scale-110 hover:shadow-[0_0_20px_rgba(6,182,212,0.8)] ${
          isExpanded ? 'opacity-100 pointer-events-auto' : 'opacity-0 pointer-events-none scale-50'
        }`}
        style={radialPos.chat}
        title="CHAT"
      >
        <MessageSquare className="w-5 h-5 text-black" />
      </button>

      <button
        className="relative focus:outline-none"
        onClick={(e) => onAvatarTap?.(win.id, e)}
        title="Click for actions"
      >
        <img
          src={win.avatar}
          alt="Avatar"
          className={`w-12 h-12 sm:w-10 sm:h-10 rounded bg-black border-2 ${isExpanded ? 'border-green-400 scale-110' : style.border} shadow-[0_0_10px_rgba(0,0,0,0.5)] hover:scale-110 hover:border-green-400 active:scale-95 transition-all cursor-pointer`}
        />
        <div className={`absolute bottom-0 right-0 w-3 h-3 rounded-full border border-black ${style.bg} ${win.status === 'BUSY' ? 'animate-pulse' : ''}`}></div>
      </button>
    </div>

    {/* Window Card */}
    <div className={`
      retro-border bg-black/80 p-3 sm:p-4 md:p-5 h-full transition-all duration-300 hover:bg-green-900/10 hover:shadow-[0_0_20px_rgba(34,197,94,0.2)] hover:border-green-400 group-hover:-translate-y-1 flex flex-col justify-between min-h-[120px] sm:min-h-[140px] md:min-h-[160px]
      ${win.status === 'IDLE' ? '!border-green-800/50 !shadow-none' : ''}
      ${win.status === 'COMPLETED' ? '!border-cyan-400 shadow-[0_0_15px_rgba(6,182,212,0.3)]' : ''}
    `}>
      <div>
        <div className="mb-2 sm:mb-4 flex items-center justify-between">
          <span className={`text-[10px] sm:text-xs font-bold tracking-wider border px-1 bg-black/30 ${win.status === 'IDLE' ? 'text-green-600 border-green-800' : 'text-green-600 border-green-900'}`}>
            IP: {sessionIp}
          </span>
        </div>

        <div className="flex items-center gap-2 mb-2 sm:mb-3">
          {renameMode ? (
            <input
              autoFocus
              defaultValue={win.name}
              onKeyDown={e => {
                if (e.key === 'Enter') { const v = (e.target as HTMLInputElement).value.trim(); if (v && v !== win.name) onRenameConfirm?.(v); else onRenameCancel?.(); }
                if (e.key === 'Escape') onRenameCancel?.();
              }}
              onBlur={e => { const v = e.target.value.trim(); if (v && v !== win.name) onRenameConfirm?.(v); else onRenameCancel?.(); }}
              onClick={e => e.stopPropagation()}
              className="text-base sm:text-xl md:text-2xl font-bold retro-text-shadow font-['Share_Tech_Mono'] text-green-400 bg-black/80 border border-green-500 px-1 outline-none w-full"
            />
          ) : (
            <h4 title={win.name} className="text-base sm:text-xl md:text-2xl font-bold retro-text-shadow font-['Share_Tech_Mono'] truncate text-green-400">
              {win.name}
            </h4>
          )}
        </div>

        <div className={`h-1.5 sm:h-2 w-full mb-2 sm:mb-4 overflow-hidden border ${win.status === 'IDLE' ? 'bg-green-900/20 border-green-800/50' : 'bg-green-900/30 border-green-900/50'}`}>
          <div className={`h-full shadow-[0_0_10px_currentColor] transition-all duration-1000 ${style.bg} ${style.barWidth} ${win.status === 'BUSY' ? 'animate-pulse' : ''}`}></div>
        </div>

        <div className="flex justify-between items-center text-xs sm:text-sm font-mono mb-3 sm:mb-6">
          <div className="flex items-center gap-1.5 sm:gap-2">
            {style.textIcon && <span className={`${style.text} text-sm`}>{style.textIcon}</span>}
            <span className={`${style.text} font-bold tracking-wider`}>{win.status}</span>
          </div>
          <span className="text-green-700/80">{win.lastActive}</span>
        </div>

        {win.claudeStatus?.awaiting_resume && (
          <div className="text-xs font-mono mb-2 text-yellow-400 bg-yellow-900/20 px-2 py-1 rounded border border-yellow-800/30">
            ⏸ 等待选择 Resume Session
          </div>
        )}

        {win.claudeStatus && (win.claudeStatus.cost !== null || win.claudeStatus.current_tool !== null || win.claudeStatus.action !== null) && (
          <div className="text-xs font-mono mb-4 space-y-1">
            {(win.claudeStatus.current_tool || win.claudeStatus.action) && (
              <div className="text-yellow-500 truncate flex items-center gap-1" title={win.claudeStatus.current_tool || win.claudeStatus.action || ''}>
                <span className="text-yellow-600">●</span>
                {win.claudeStatus.current_tool || win.claudeStatus.action}
              </div>
            )}
            <div className="flex justify-between text-green-600">
              <span>
                {win.claudeStatus.cost !== null && `$${win.claudeStatus.cost.toFixed(2)}`}
                {win.claudeStatus.context_percent !== null && ` · ${win.claudeStatus.context_percent.toFixed(0)}%`}
              </span>
              <span>{win.claudeStatus.session_duration || ''}</span>
            </div>
          </div>
        )}
      </div>
    </div>
  </div>
);

// Sortable window card wrapper
const SortableWindowCard: React.FC<{
  window: AgentWindow;
  sessionName: string;
  sessionId: string;
  sessionIp: string;
  isExpanded: boolean;
  onCardClick: () => void;
  onContextMenu: (e: React.MouseEvent) => void;
  onAvatarTap: (windowId: string, e: React.MouseEvent) => void;
  onDeleteWindow: () => void;
  onSelectWindow: () => void;
  onViewHistory: () => void;
  renameMode?: boolean;
  onRenameConfirm?: (name: string) => void;
  onRenameCancel?: () => void;
}> = ({ window: win, sessionName, sessionId, sessionIp, isExpanded, onCardClick, onContextMenu, onAvatarTap, onDeleteWindow, onSelectWindow, onViewHistory, renameMode, onRenameConfirm, onRenameCancel }) => {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: win.id });

  const sortableStyle = {
    transform: CSS.Transform.toString(transform),
    transition,
    zIndex: isDragging ? 50 : undefined,
  };

  const statusStyle = STATUS_STYLES[win.status] || STATUS_STYLES.IDLE;
  const radialPos = isExpanded ? RADIAL_EXPANDED : RADIAL_COLLAPSED;

  return (
    <div ref={setNodeRef} style={sortableStyle}>
      <WindowCardContent
        win={win}
        sessionIp={sessionIp}
        isExpanded={isExpanded}
        radialPos={radialPos}
        style={statusStyle}
        onCardClick={onCardClick}
        onContextMenu={onContextMenu}
        onAvatarTap={onAvatarTap}
        onDeleteWindow={onDeleteWindow}
        onSelectWindow={onSelectWindow}
        onViewHistory={onViewHistory}
        dragHandleProps={{ listeners, attributes }}
        isDragging={isDragging}
        renameMode={renameMode}
        onRenameConfirm={onRenameConfirm}
        onRenameCancel={onRenameCancel}
      />
    </div>
  );
};

// Context menu for right-click on window cards
const ContextMenu: React.FC<{
  x: number;
  y: number;
  onRename: () => void;
  onConsole: () => void;
  onHistory: () => void;
  onResetLayout: () => void;
  onDelete: () => void;
  onClose: () => void;
}> = ({ x, y, onRename, onConsole, onHistory, onResetLayout, onDelete, onClose }) => {
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) onClose();
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [onClose]);

  const items = [
    { label: 'Rename', icon: Pencil, action: onRename, color: 'text-green-400' },
    { label: 'Console', icon: Terminal, action: onConsole, color: 'text-green-400' },
    { label: 'History', icon: MessageSquare, action: onHistory, color: 'text-cyan-400' },
    { label: 'Reset Layout', icon: LayoutGrid, action: onResetLayout, color: 'text-yellow-400' },
    { label: 'Delete', icon: Trash2, action: onDelete, color: 'text-red-400' },
  ];

  return (
    <div
      ref={menuRef}
      className="fixed z-[100] bg-black/95 border border-green-700/60 rounded shadow-[0_0_20px_rgba(34,197,94,0.3)] py-1 min-w-[140px]"
      style={{ left: x, top: y }}
    >
      {items.map(item => (
        <button
          key={item.label}
          onClick={() => { item.action(); onClose(); }}
          className="w-full flex items-center gap-2 px-3 py-1.5 text-xs font-mono tracking-wider hover:bg-green-900/30 transition-colors"
        >
          <item.icon className={`w-3.5 h-3.5 ${item.color}`} />
          <span className={item.color}>{item.label.toUpperCase()}</span>
        </button>
      ))}
    </div>
  );
};

// Inline rename input
const InlineRenameInput: React.FC<{
  value: string;
  onConfirm: (name: string) => void;
  onCancel: () => void;
}> = ({ value, onConfirm, onCancel }) => {
  const [text, setText] = useState(value);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.select();
  }, []);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      const trimmed = text.trim();
      if (trimmed && trimmed !== value) onConfirm(trimmed);
      else onCancel();
    }
    if (e.key === 'Escape') onCancel();
  };

  return (
    <input
      ref={inputRef}
      value={text}
      onChange={e => setText(e.target.value)}
      onKeyDown={handleKeyDown}
      onBlur={() => {
        const trimmed = text.trim();
        if (trimmed && trimmed !== value) onConfirm(trimmed);
        else onCancel();
      }}
      autoFocus
      className="text-base sm:text-lg md:text-xl font-black text-green-400 retro-text-shadow-strong font-pixel tracking-wider bg-black/60 px-2 border border-green-500 shadow-[0_0_10px_rgba(34,197,94,0.3)] outline-none focus:border-green-400 w-48"
    />
  );
};

export const WorkstationsView: React.FC<WorkstationsViewProps> = ({
    sessions,
    onRequestAddWindow,
    onSelectWindow,
    onSwitchToWindow,
    onRequestDeleteSession,
    onRequestDeleteWindow,
    onViewHistory,
    onReorderWindow
}) => {
  // Track which card is expanded (for mobile tap to show options)
  const [expandedCard, setExpandedCard] = useState<string | null>(null);

  // Project settings modal
  const [settingsSession, setSettingsSession] = useState<string | null>(null);

  // Active drag state for overlay
  const [activeId, setActiveId] = useState<string | null>(null);

  // Click timer for distinguishing single/double click
  const clickTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Context menu state
  const [contextMenu, setContextMenu] = useState<{
    x: number; y: number;
    sessionName: string; windowName: string; windowId: string; claudePane?: string;
  } | null>(null);

  // Rename state
  const [renamingSession, setRenamingSession] = useState<string | null>(null);
  const [renamingWindow, setRenamingWindow] = useState<{ sessionName: string; windowId: string } | null>(null);

  // Right-click on window card
  const handleWindowContextMenu = (
    e: React.MouseEvent,
    sessionName: string, windowName: string, windowId: string, claudePane?: string
  ) => {
    e.preventDefault();
    setExpandedCard(null);
    setContextMenu({ x: e.clientX, y: e.clientY, sessionName, windowName, windowId, claudePane });
  };

  const handleRenameWindow = async (sessionName: string, windowId: string, newName: string) => {
    // Use window ID (@N) instead of window name to avoid tmux parsing dots as pane separators
    await tmuxRenameWindow(sessionName, windowId, newName);
    setRenamingWindow(null);
  };

  const handleRenameSession = async (sessionName: string, newName: string) => {
    await tmuxRenameSession(sessionName, newName);
    setRenamingSession(null);
  };

  // Pointer sensor with activation distance to distinguish click from drag
  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: {
        distance: 8,
      },
    })
  );

  // Handle card click with single/double click detection
  const handleCardClick = (
    sessionName: string,
    windowName: string,
    windowId: string
  ) => {
    if (clickTimer.current) {
      // Double click - switch to window
      clearTimeout(clickTimer.current);
      clickTimer.current = null;
      onSwitchToWindow(sessionName, windowName, windowId);
    } else {
      // Potential single click - wait to see if it's a double click
      clickTimer.current = setTimeout(() => {
        clickTimer.current = null;
        // Single click - close radial menu if open
        if (expandedCard) {
          setExpandedCard(null);
        }
      }, 250);
    }
  };

  // Handle avatar tap/click - toggle radial menu
  const handleAvatarTap = (windowId: string, e: React.MouseEvent | React.TouchEvent) => {
    e.stopPropagation();
    // Toggle radial menu for this window
    setExpandedCard(expandedCard === windowId ? null : windowId);
  };

  const handleDragStart = (event: DragStartEvent) => {
    setActiveId(event.active.id as string);
    // Close radial menu when dragging starts
    setExpandedCard(null);
  };

  const handleDragEnd = (event: DragEndEvent) => {
    setActiveId(null);
    const { active, over } = event;
    if (!over || active.id === over.id || !onReorderWindow) return;

    // Find which session these windows belong to
    for (const session of sessions) {
      const activeWin = session.windows.find(w => w.id === active.id);
      const overWin = session.windows.find(w => w.id === over.id);
      if (activeWin && overWin) {
        onReorderWindow(session.name, activeWin.windowIndex, overWin.windowIndex);
        break;
      }
    }
  };

  // Find active window for drag overlay
  const activeWindow = activeId
    ? sessions.flatMap(s => s.windows.map(w => ({ ...w, sessionIp: s.ip }))).find(w => w.id === activeId)
    : null;

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragStart={handleDragStart}
      onDragEnd={handleDragEnd}
    >
      <div className="flex flex-col gap-4 sm:gap-8 pb-10">
         {/* Stats Bar */}
         <div className="flex flex-wrap gap-4 sm:gap-8 text-sm sm:text-base md:text-lg font-mono tracking-widest border-b border-green-800/50 pb-4 mb-2 shadow-[0_1px_0_rgba(34,197,94,0.2)]">
              <span className="text-green-600 retro-text-shadow">SESSIONS: <span className="text-green-300 font-bold">{sessions.length}</span></span>
              <span className="text-green-600 retro-text-shadow">WINDOWS: <span className="text-green-300 font-bold">{sessions.reduce((acc, s) => acc + s.windows.length, 0)}</span></span>
         </div>

         {/* Sessions List */}
         <div className="flex flex-col gap-12">
            {sessions.map((session) => (
              <div key={session.id} className="border-t-2 border-green-900/50 pt-6 relative group/session">
                  {/* Session Header */}
                  <div className="flex flex-col sm:flex-row sm:flex-wrap sm:items-center gap-2 sm:gap-4 mb-4 sm:mb-6">
                      <div className="flex items-center gap-2 sm:gap-4">
                          <span className="text-green-700 font-bold tracking-widest uppercase text-xs sm:text-sm">SESSION:</span>
                          {renamingSession === session.name ? (
                            <InlineRenameInput
                              value={session.name}
                              onConfirm={(newName) => handleRenameSession(session.name, newName)}
                              onCancel={() => setRenamingSession(null)}
                            />
                          ) : (
                            <span
                              className="text-base sm:text-lg md:text-xl font-black text-green-400 retro-text-shadow-strong font-pixel tracking-wider bg-black/60 px-2 border border-green-500/30 shadow-[0_0_10px_rgba(34,197,94,0.3)] cursor-pointer hover:border-green-400 transition-colors"
                              onClick={() => setRenamingSession(session.name)}
                              title="Click to rename"
                            >
                              {session.name}
                            </span>
                          )}
                          <span className="text-green-800 text-xs sm:text-sm font-mono">({session.windows.length})</span>
                      </div>
                      <div className="h-px flex-grow bg-gradient-to-r from-green-900/50 to-transparent hidden sm:block"></div>

                      {/* Session Action Buttons */}
                      <div className="flex items-center gap-2">
                          <button
                              onClick={() => setSettingsSession(session.name)}
                              className="flex items-center gap-1 sm:gap-2 px-2 sm:px-3 py-1 border border-green-900/50 text-green-800 hover:bg-green-900/20 hover:text-green-400 hover:border-green-500 transition-all text-[10px] sm:text-xs font-bold tracking-widest uppercase opacity-60 group-hover/session:opacity-100 self-start sm:self-auto"
                          >
                              <Settings className="w-3 sm:w-4 h-3 sm:h-4" />
                              <span className="hidden sm:inline">PROJECT_SETTINGS</span>
                              <span className="sm:hidden">CONFIG</span>
                          </button>
                          <button
                              onClick={() => onRequestDeleteSession(session.id, session.name)}
                              className="flex items-center gap-1 sm:gap-2 px-2 sm:px-3 py-1 border border-red-900/50 text-red-900 hover:bg-red-900/20 hover:text-red-500 hover:border-red-500 transition-all text-[10px] sm:text-xs font-bold tracking-widest uppercase opacity-60 group-hover/session:opacity-100 self-start sm:self-auto"
                          >
                              <XCircle className="w-3 sm:w-4 h-3 sm:h-4" />
                              <span className="hidden sm:inline">TERMINATE_SESSION</span>
                              <span className="sm:hidden">TERMINATE</span>
                          </button>
                      </div>
                  </div>

                  {/* Windows Grid with Sortable Context */}
                  <SortableContext
                    items={session.windows.map(w => w.id)}
                    strategy={horizontalListSortingStrategy}
                  >
                    <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-6 px-2 md:px-6">
                        {session.windows.map((window) => (
                            <SortableWindowCard
                              key={window.id}
                              window={window}
                              sessionName={session.name}
                              sessionId={session.id}
                              sessionIp={session.ip}
                              isExpanded={expandedCard === window.id}
                              onCardClick={() => handleCardClick(session.name, window.name, window.id)}
                              onContextMenu={(e) => handleWindowContextMenu(e, session.name, window.name, window.id, window.claudePane)}
                              onAvatarTap={handleAvatarTap}
                              onDeleteWindow={() => { onRequestDeleteWindow(session.id, window.id, window.name); setExpandedCard(null); }}
                              onSelectWindow={() => { onSelectWindow(session.name, window.name, window.id); setExpandedCard(null); }}
                              onViewHistory={() => { onViewHistory(session.name, window.name, window.id, window.claudePane, session.gitDir); setExpandedCard(null); }}
                              renameMode={renamingWindow?.sessionName === session.name && renamingWindow?.windowId === window.id}
                              onRenameConfirm={(name) => handleRenameWindow(session.name, window.id, name)}
                              onRenameCancel={() => setRenamingWindow(null)}
                            />
                        ))}

                        {/* Add Window Button */}
                        <button
                            onClick={() => onRequestAddWindow(session.id)}
                            className="retro-border border-dashed border-green-900/50 hover:border-green-500 hover:bg-green-900/10 hover:shadow-[0_0_15px_rgba(34,197,94,0.3)] hover:text-green-400 text-green-800 flex flex-col items-center justify-center min-h-[80px] sm:min-h-[120px] md:min-h-[160px] transition-all group relative"
                        >
                            <Plus className="w-8 h-8 sm:w-10 sm:h-10 mb-1 sm:mb-2 group-hover:scale-110 transition-transform" />
                            <span className="font-mono text-xs sm:text-sm tracking-widest">ADD WINDOW</span>
                        </button>
                    </div>
                  </SortableContext>
              </div>
            ))}
         </div>

         {settingsSession && <ProjectSettings sessionName={settingsSession} onClose={() => setSettingsSession(null)} />}

         {/* Context Menu */}
         {contextMenu && (
           <ContextMenu
             x={contextMenu.x}
             y={contextMenu.y}
             onRename={() => setRenamingWindow({ sessionName: contextMenu.sessionName, windowId: contextMenu.windowId })}
             onConsole={() => { onSelectWindow(contextMenu.sessionName, contextMenu.windowName, contextMenu.windowId); }}
             onHistory={() => { const s = sessions.find(s => s.name === contextMenu.sessionName); onViewHistory(contextMenu.sessionName, contextMenu.windowName, contextMenu.windowId, contextMenu.claudePane, s?.gitDir); }}
             onResetLayout={() => { tmuxResetLayout(contextMenu.sessionName, contextMenu.windowId); }}
             onDelete={() => {
               const session = sessions.find(s => s.name === contextMenu.sessionName);
               if (session) onRequestDeleteWindow(session.id, contextMenu.windowId, contextMenu.windowName);
             }}
             onClose={() => setContextMenu(null)}
           />
         )}
      </div>

      {/* Drag Overlay */}
      <DragOverlay>
        {activeWindow && (
          <div className="w-[280px] opacity-90">
            <WindowCardContent
              win={activeWindow}
              sessionIp={activeWindow.sessionIp}
              isExpanded={false}
              radialPos={RADIAL_COLLAPSED}
              style={STATUS_STYLES[activeWindow.status] || STATUS_STYLES.IDLE}
            />
          </div>
        )}
      </DragOverlay>
    </DndContext>
  );
};
