import React from 'react';
import { AgentSession, AgentWindow } from '../types';
import { Plus, Terminal, Trash2, History, XCircle, Pause, Check, Activity, PowerOff } from 'lucide-react';

interface WorkstationsViewProps {
  sessions: AgentSession[];
  onRequestAddWindow: (sessionId: string) => void;
  onSelectWindow: (sessionName: string, windowName: string) => void;
  onRequestDeleteSession: (sessionId: string, name: string) => void;
  onRequestDeleteWindow: (sessionId: string, windowId: string, name: string) => void;
  onViewHistory: (sessionName: string, windowName: string) => void;
}

// Config for visual styles based on status
const STATUS_STYLES = {
    IDLE: {
        text: 'text-green-700',
        bg: 'bg-green-900',
        border: 'border-green-800',
        shadow: 'shadow-none',
        barWidth: 'w-[10%]',
        icon: Activity
    },
    BUSY: {
        text: 'text-yellow-400',
        bg: 'bg-yellow-500',
        border: 'border-yellow-500',
        shadow: 'shadow-[#eab308]',
        barWidth: 'w-[80%]',
        icon: Activity
    },
    PAUSED: {
        text: 'text-orange-400',
        bg: 'bg-orange-500',
        border: 'border-orange-500',
        shadow: 'shadow-[#f97316]',
        barWidth: 'w-[50%]',
        icon: Pause
    },
    COMPLETED: {
        text: 'text-cyan-400',
        bg: 'bg-cyan-500',
        border: 'border-cyan-400',
        shadow: 'shadow-[#06b6d4]',
        barWidth: 'w-[100%]',
        icon: Check
    },
    OFFLINE: {
        text: 'text-red-600',
        bg: 'bg-red-600',
        border: 'border-red-600',
        shadow: 'shadow-[#dc2626]',
        barWidth: 'w-[0%]',
        icon: PowerOff
    }
};

export const WorkstationsView: React.FC<WorkstationsViewProps> = ({ 
    sessions, 
    onRequestAddWindow, 
    onSelectWindow,
    onRequestDeleteSession,
    onRequestDeleteWindow,
    onViewHistory
}) => {
  return (
    <div className="flex flex-col gap-8 pb-10">
       {/* Stats Bar */}
       <div className="flex gap-8 text-base md:text-lg font-mono tracking-widest border-b border-green-800/50 pb-4 mb-2 shadow-[0_1px_0_rgba(34,197,94,0.2)]">
            <span className="text-green-600 retro-text-shadow">SESSIONS: <span className="text-green-300 font-bold">{sessions.length}</span></span>
            <span className="text-green-600 retro-text-shadow">TOTAL WINDOWS: <span className="text-green-300 font-bold">{sessions.reduce((acc, s) => acc + s.windows.length, 0)}</span></span>
       </div>

       {/* Sessions List */}
       <div className="flex flex-col gap-12">
          {sessions.map((session) => (
            <div key={session.id} className="border-t-2 border-green-900/50 pt-6 relative group/session">
                {/* Session Header */}
                <div className="flex items-center gap-4 mb-6">
                    <span className="text-green-700 font-bold tracking-widest uppercase text-sm">SESSION:</span>
                    <span className="text-2xl md:text-3xl font-black text-green-400 retro-text-shadow-strong font-['VT323'] tracking-wider bg-black/60 px-2 border border-green-500/30 shadow-[0_0_10px_rgba(34,197,94,0.3)]">
                        {session.name}
                    </span>
                    <span className="text-green-800 text-sm font-mono">({session.windows.length} windows)</span>
                    <div className="h-px flex-grow bg-gradient-to-r from-green-900/50 to-transparent"></div>
                    
                    {/* Terminate Session Button */}
                    <button 
                        onClick={() => onRequestDeleteSession(session.id, session.name)}
                        className="flex items-center gap-2 px-3 py-1 border border-red-900/50 text-red-900 hover:bg-red-900/20 hover:text-red-500 hover:border-red-500 transition-all text-xs font-bold tracking-widest uppercase opacity-60 group-hover/session:opacity-100"
                    >
                        <XCircle className="w-4 h-4" />
                        TERMINATE_SESSION
                    </button>
                </div>

                {/* Windows Grid */}
                <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-6 px-2 md:px-6">
                    {session.windows.map((window) => {
                        const style = STATUS_STYLES[window.status] || STATUS_STYLES.IDLE;
                        const StatusIcon = style.icon;

                        return (
                            <div 
                                key={window.id} 
                                className="relative group cursor-pointer"
                            >
                                {/* Avatar - Positioned on top edge overlapping */}
                                <div className="absolute -top-5 right-4 z-20">
                                    <div className="relative">
                                        <img 
                                            src={window.avatar} 
                                            alt="Avatar" 
                                            className={`w-10 h-10 rounded bg-black border-2 ${style.border} shadow-[0_0_10px_rgba(0,0,0,0.5)] group-hover:scale-110 transition-transform`}
                                        />
                                        <div className={`absolute bottom-0 right-0 w-3 h-3 rounded-full border border-black ${style.bg} ${window.status === 'BUSY' ? 'animate-pulse' : ''}`}></div>
                                    </div>
                                </div>

                                {/* Close Window Button (Top Left) */}
                                <button
                                    onClick={(e) => {
                                        e.stopPropagation();
                                        onRequestDeleteWindow(session.id, window.id, window.name);
                                    }}
                                    className="absolute -top-3 -left-3 z-30 bg-black border border-red-900 text-red-800 p-1.5 rounded-full hover:bg-red-900 hover:text-white hover:border-red-500 opacity-0 group-hover:opacity-100 transition-all shadow-lg"
                                    title="Close Window"
                                >
                                    <Trash2 className="w-4 h-4" />
                                </button>

                                {/* Window Card */}
                                <div className={`
                                    retro-border bg-black/80 p-5 h-full transition-all duration-300 hover:bg-green-900/10 hover:shadow-[0_0_20px_rgba(34,197,94,0.2)] hover:border-green-400 group-hover:-translate-y-1 flex flex-col justify-between
                                    ${window.status === 'IDLE' ? '!border-green-900/30 !shadow-none' : ''}
                                    ${window.status === 'COMPLETED' ? '!border-cyan-400 shadow-[0_0_15px_rgba(6,182,212,0.3)]' : ''}
                                `}>
                                    
                                    <div>
                                        {/* Header / IP */}
                                        <div className="mb-4 flex items-center justify-between">
                                            <span className={`text-xs font-bold tracking-wider border px-1 bg-black/30 ${window.status === 'IDLE' ? 'text-green-800 border-green-900' : 'text-green-600 border-green-900'}`}>
                                                IP: {session.ip}
                                            </span>
                                        </div>

                                        {/* Window Name (Project/Process) */}
                                        <h4 className={`text-2xl font-bold retro-text-shadow mb-3 font-['Share_Tech_Mono'] truncate ${style.text}`}>
                                            {window.name}
                                        </h4>

                                        {/* Status Bar */}
                                        <div className={`h-2 w-full mb-4 overflow-hidden border ${window.status === 'IDLE' ? 'bg-green-900/10 border-green-900/30' : 'bg-green-900/30 border-green-900/50'}`}>
                                            <div className={`h-full shadow-[0_0_10px_currentColor] transition-all duration-1000 ${style.bg} ${style.barWidth} ${window.status === 'BUSY' ? 'animate-pulse' : ''}`}></div>
                                        </div>

                                        {/* Status Text */}
                                        <div className="flex justify-between items-center text-sm font-mono mb-6">
                                            <div className="flex items-center gap-2">
                                                <div className={`flex items-center justify-center w-5 h-5 rounded-full border ${style.border} ${style.shadow.replace('inset', '')} ${window.status === 'IDLE' ? 'bg-transparent' : 'bg-black/50'}`}>
                                                    <StatusIcon className={`w-3 h-3 ${style.text}`} />
                                                </div>
                                                <span className={`${style.text} font-bold tracking-wider`}>
                                                    {window.status}
                                                </span>
                                            </div>
                                            <span className="text-green-700/80">{window.lastActive}</span>
                                        </div>
                                    </div>

                                    {/* Hover Action Overlay */}
                                    <div className="absolute inset-0 flex flex-col items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity bg-black/80 backdrop-blur-[2px] gap-3 z-10">
                                        <button 
                                            onClick={() => onSelectWindow(session.name, window.name)}
                                            className="flex items-center gap-2 text-green-900 bg-green-500 font-bold tracking-widest border border-green-400 px-4 py-2 hover:bg-green-400 hover:text-black shadow-[0_0_15px_rgba(34,197,94,0.5)] w-[180px] justify-center"
                                        >
                                            <Terminal className="w-4 h-4" />
                                            OPEN CONSOLE
                                        </button>
                                        
                                        <button 
                                            onClick={(e) => {
                                                e.stopPropagation();
                                                onViewHistory(session.name, window.name);
                                            }}
                                            className="flex items-center gap-2 text-green-400 font-bold tracking-widest border border-green-600 px-4 py-2 bg-black hover:border-green-400 hover:text-green-300 w-[180px] justify-center"
                                        >
                                            <History className="w-4 h-4" />
                                            VIEW HISTORY
                                        </button>
                                    </div>
                                </div>
                            </div>
                        );
                    })}

                    {/* Add Window Button */}
                    <button 
                        onClick={() => onRequestAddWindow(session.id)}
                        className="retro-border border-dashed border-green-900/50 hover:border-green-500 hover:bg-green-900/10 hover:shadow-[0_0_15px_rgba(34,197,94,0.3)] hover:text-green-400 text-green-800 flex flex-col items-center justify-center min-h-[160px] transition-all group relative"
                    >
                        <Plus className="w-10 h-10 mb-2 group-hover:scale-110 transition-transform" />
                        <span className="font-mono text-sm tracking-widest">ADD WINDOW</span>
                    </button>
                </div>
            </div>
          ))}
       </div>
    </div>
  );
};