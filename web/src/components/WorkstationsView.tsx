import React, { useState, useRef } from 'react';
import { AgentSession, AgentWindow } from '../types';
import { Plus, Terminal, Trash2, History, MessageSquare, XCircle, Pause, Check, Activity, PowerOff, ExternalLink } from 'lucide-react';

interface WorkstationsViewProps {
  sessions: AgentSession[];
  onRequestAddWindow: (sessionId: string) => void;
  onSelectWindow: (sessionName: string, windowName: string, windowId: string) => void;
  onSwitchToWindow: (sessionName: string, windowName: string, windowId: string) => void;
  onRequestDeleteSession: (sessionId: string, name: string) => void;
  onRequestDeleteWindow: (sessionId: string, windowId: string, name: string) => void;
  onViewHistory: (sessionName: string, windowName: string, windowId: string, claudePane?: string) => void;
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

export const WorkstationsView: React.FC<WorkstationsViewProps> = ({
    sessions,
    onRequestAddWindow,
    onSelectWindow,
    onSwitchToWindow,
    onRequestDeleteSession,
    onRequestDeleteWindow,
    onViewHistory
}) => {
  // Track which card is expanded (for mobile tap to show options)
  const [expandedCard, setExpandedCard] = useState<string | null>(null);

  // Click timer for distinguishing single/double click
  const clickTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

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

  return (
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
                        <span className="text-base sm:text-lg md:text-xl font-black text-green-400 retro-text-shadow-strong font-pixel tracking-wider bg-black/60 px-2 border border-green-500/30 shadow-[0_0_10px_rgba(34,197,94,0.3)]">
                            {session.name}
                        </span>
                        <span className="text-green-800 text-xs sm:text-sm font-mono">({session.windows.length})</span>
                    </div>
                    <div className="h-px flex-grow bg-gradient-to-r from-green-900/50 to-transparent hidden sm:block"></div>

                    {/* Terminate Session Button - on mobile, show at left to avoid avatar overlap */}
                    <button
                        onClick={() => onRequestDeleteSession(session.id, session.name)}
                        className="flex items-center gap-1 sm:gap-2 px-2 sm:px-3 py-1 border border-red-900/50 text-red-900 hover:bg-red-900/20 hover:text-red-500 hover:border-red-500 transition-all text-[10px] sm:text-xs font-bold tracking-widest uppercase opacity-60 group-hover/session:opacity-100 self-start sm:self-auto"
                    >
                        <XCircle className="w-3 sm:w-4 h-3 sm:h-4" />
                        <span className="hidden sm:inline">TERMINATE_SESSION</span>
                        <span className="sm:hidden">TERMINATE</span>
                    </button>
                </div>

                {/* Windows Grid */}
                <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-6 px-2 md:px-6">
                    {session.windows.map((window) => {
                        const style = STATUS_STYLES[window.status] || STATUS_STYLES.IDLE;
                        const StatusIcon = style.icon;

                        const isExpanded = expandedCard === window.id;

                        return (
                            <div
                                key={window.id}
                                className="relative group cursor-pointer"
                                onClick={() => handleCardClick(session.name, window.name, window.id)}
                            >
                                {/* Avatar with Radial Menu */}
                                <div className="absolute -top-5 right-4 z-20">
                                    {/* Radial Menu Buttons - 180° to 270° arc (left to bottom) */}
                                    {/* DELETE - 180° (9点钟, 正左) */}
                                    <button
                                        onClick={(e) => {
                                            e.stopPropagation();
                                            onRequestDeleteWindow(session.id, window.id, window.name);
                                            setExpandedCard(null);
                                        }}
                                        className={`absolute w-11 h-11 rounded-full bg-red-500 border-2 border-red-400 flex items-center justify-center transition-all duration-300 ease-out shadow-[0_0_15px_rgba(239,68,68,0.6)] hover:scale-110 hover:shadow-[0_0_20px_rgba(239,68,68,0.8)] ${
                                            isExpanded
                                                ? 'opacity-100 pointer-events-auto'
                                                : 'opacity-0 pointer-events-none scale-50'
                                        }`}
                                        style={{
                                            top: isExpanded ? '-5px' : '5px',
                                            left: isExpanded ? '-65px' : '5px',
                                        }}
                                        title="DELETE"
                                    >
                                        <Trash2 className="w-5 h-5 text-black" />
                                    </button>

                                    {/* CONSOLE - 225° (7点30分, 左下) */}
                                    <button
                                        onClick={(e) => {
                                            e.stopPropagation();
                                            onSelectWindow(session.name, window.name, window.id);
                                            setExpandedCard(null);
                                        }}
                                        className={`absolute w-11 h-11 rounded-full bg-green-500 border-2 border-green-400 flex items-center justify-center transition-all duration-300 ease-out shadow-[0_0_15px_rgba(34,197,94,0.6)] hover:scale-110 hover:shadow-[0_0_20px_rgba(34,197,94,0.8)] ${
                                            isExpanded
                                                ? 'opacity-100 pointer-events-auto'
                                                : 'opacity-0 pointer-events-none scale-50'
                                        }`}
                                        style={{
                                            top: isExpanded ? '50px' : '5px',
                                            left: isExpanded ? '-55px' : '5px',
                                            transitionDelay: isExpanded ? '50ms' : '0ms',
                                        }}
                                        title="CONSOLE"
                                    >
                                        <Terminal className="w-5 h-5 text-black" />
                                    </button>

                                    {/* CHAT - 270° (6点钟, 正下) */}
                                    <button
                                        onClick={(e) => {
                                            e.stopPropagation();
                                            onViewHistory(session.name, window.name, window.id, window.claudePane);
                                            setExpandedCard(null);
                                        }}
                                        className={`absolute w-11 h-11 rounded-full bg-cyan-500 border-2 border-cyan-400 flex items-center justify-center transition-all duration-300 ease-out shadow-[0_0_15px_rgba(6,182,212,0.6)] hover:scale-110 hover:shadow-[0_0_20px_rgba(6,182,212,0.8)] ${
                                            isExpanded
                                                ? 'opacity-100 pointer-events-auto'
                                                : 'opacity-0 pointer-events-none scale-50'
                                        }`}
                                        style={{
                                            top: isExpanded ? '70px' : '5px',
                                            left: isExpanded ? '-5px' : '5px',
                                            transitionDelay: isExpanded ? '100ms' : '0ms',
                                        }}
                                        title="CHAT"
                                    >
                                        <MessageSquare className="w-5 h-5 text-black" />
                                    </button>

                                    {/* Avatar Button - Click to toggle menu */}
                                    <button
                                        className="relative focus:outline-none"
                                        onClick={(e) => handleAvatarTap(window.id, e)}
                                        title="Click for actions"
                                    >
                                        <img
                                            src={window.avatar}
                                            alt="Avatar"
                                            className={`w-12 h-12 sm:w-10 sm:h-10 rounded bg-black border-2 ${isExpanded ? 'border-green-400 scale-110' : style.border} shadow-[0_0_10px_rgba(0,0,0,0.5)] hover:scale-110 hover:border-green-400 active:scale-95 transition-all cursor-pointer`}
                                        />
                                        <div className={`absolute bottom-0 right-0 w-3 h-3 rounded-full border border-black ${style.bg} ${window.status === 'BUSY' ? 'animate-pulse' : ''}`}></div>
                                    </button>
                                </div>


                                {/* Window Card */}
                                <div className={`
                                    retro-border bg-black/80 p-3 sm:p-4 md:p-5 h-full transition-all duration-300 hover:bg-green-900/10 hover:shadow-[0_0_20px_rgba(34,197,94,0.2)] hover:border-green-400 group-hover:-translate-y-1 flex flex-col justify-between min-h-[120px] sm:min-h-[140px] md:min-h-[160px]
                                    ${window.status === 'IDLE' ? '!border-green-800/50 !shadow-none' : ''}
                                    ${window.status === 'COMPLETED' ? '!border-cyan-400 shadow-[0_0_15px_rgba(6,182,212,0.3)]' : ''}
                                `}>

                                    <div>
                                        {/* Header / IP */}
                                        <div className="mb-2 sm:mb-4 flex items-center justify-between">
                                            <span className={`text-[10px] sm:text-xs font-bold tracking-wider border px-1 bg-black/30 ${window.status === 'IDLE' ? 'text-green-600 border-green-800' : 'text-green-600 border-green-900'}`}>
                                                IP: {session.ip}
                                            </span>
                                        </div>

                                        {/* Window Name */}
                                        <div className="flex items-center gap-2 mb-2 sm:mb-3">
                                            <h4
                                                title={window.name}
                                                className="text-base sm:text-xl md:text-2xl font-bold retro-text-shadow font-['Share_Tech_Mono'] truncate text-green-400"
                                            >
                                                {window.name}
                                            </h4>
                                        </div>

                                        {/* Status Bar */}
                                        <div className={`h-1.5 sm:h-2 w-full mb-2 sm:mb-4 overflow-hidden border ${window.status === 'IDLE' ? 'bg-green-900/20 border-green-800/50' : 'bg-green-900/30 border-green-900/50'}`}>
                                            <div className={`h-full shadow-[0_0_10px_currentColor] transition-all duration-1000 ${style.bg} ${style.barWidth} ${window.status === 'BUSY' ? 'animate-pulse' : ''}`}></div>
                                        </div>

                                        {/* Status Text */}
                                        <div className="flex justify-between items-center text-xs sm:text-sm font-mono mb-3 sm:mb-6">
                                            <div className="flex items-center gap-1.5 sm:gap-2">
                                                {style.textIcon && (
                                                    <span className={`${style.text} text-sm`}>{style.textIcon}</span>
                                                )}
                                                <span className={`${style.text} font-bold tracking-wider`}>
                                                    {window.status}
                                                </span>
                                            </div>
                                            <span className="text-green-700/80">{window.lastActive}</span>
                                        </div>

                                        {/* Claude Status (only shown when available) */}
                                        {window.claudeStatus && (window.claudeStatus.cost !== null || window.claudeStatus.current_tool !== null || window.claudeStatus.action !== null) && (
                                            <div className="text-xs font-mono mb-4 space-y-1">
                                                {/* Show current tool (e.g., Bash command) or action status */}
                                                {(window.claudeStatus.current_tool || window.claudeStatus.action) && (
                                                    <div className="text-yellow-500 truncate flex items-center gap-1" title={window.claudeStatus.current_tool || window.claudeStatus.action || ''}>
                                                        <span className="text-yellow-600">●</span>
                                                        {window.claudeStatus.current_tool || window.claudeStatus.action}
                                                    </div>
                                                )}
                                                <div className="flex justify-between text-green-600">
                                                    <span>
                                                        {window.claudeStatus.cost !== null && `$${window.claudeStatus.cost.toFixed(2)}`}
                                                        {window.claudeStatus.context_percent !== null && ` · ${window.claudeStatus.context_percent.toFixed(0)}%`}
                                                    </span>
                                                    <span>{window.claudeStatus.session_duration || ''}</span>
                                                </div>
                                            </div>
                                        )}
                                    </div>

                                </div>
                            </div>
                        );
                    })}

                    {/* Add Window Button */}
                    <button
                        onClick={() => onRequestAddWindow(session.id)}
                        className="retro-border border-dashed border-green-900/50 hover:border-green-500 hover:bg-green-900/10 hover:shadow-[0_0_15px_rgba(34,197,94,0.3)] hover:text-green-400 text-green-800 flex flex-col items-center justify-center min-h-[80px] sm:min-h-[120px] md:min-h-[160px] transition-all group relative"
                    >
                        <Plus className="w-8 h-8 sm:w-10 sm:h-10 mb-1 sm:mb-2 group-hover:scale-110 transition-transform" />
                        <span className="font-mono text-xs sm:text-sm tracking-widest">ADD WINDOW</span>
                    </button>
                </div>
            </div>
          ))}
       </div>
    </div>
  );
};