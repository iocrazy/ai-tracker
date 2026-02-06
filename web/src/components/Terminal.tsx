import React, { useEffect, useRef } from 'react';
import { LogEntry } from '../types';

interface TerminalProps {
  logs: LogEntry[];
}

export const Terminal: React.FC<TerminalProps> = ({ logs }) => {
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [logs]);

  return (
    <div className="retro-border bg-black/60 p-6 h-full flex flex-col font-mono text-base md:text-lg">
      <h3 className="text-green-400 text-2xl font-bold tracking-widest mb-4 retro-text-shadow flex justify-between items-center border-b border-green-900/50 pb-2">
        <span>{'>>'} SYSTEM_LOG</span>
        <span className="text-xs md:text-sm bg-green-900/50 px-3 py-1 rounded text-green-300 animate-pulse">LIVE</span>
      </h3>
      <div className="flex-grow overflow-y-auto space-y-2 pr-2 font-['Share_Tech_Mono']">
        {logs.map((log) => (
          <div key={log.id} className="flex gap-3 hover:bg-green-900/20 py-1">
            <span className="text-green-700 select-none text-sm md:text-base">[{log.timestamp}]</span>
            <span className={`font-bold ${
                log.level === 'ERROR' ? 'text-red-500' :
                log.level === 'WARN' ? 'text-yellow-500' :
                log.level === 'SYS' ? 'text-cyan-400' :
                'text-green-400'
            }`}>
              {log.level}
            </span>
            <span className="text-green-300 opacity-90 break-all">{log.message}</span>
          </div>
        ))}
        <div ref={bottomRef} />
      </div>
      <div className="mt-4 border-t border-green-900/50 pt-3 flex items-center text-green-500 animate-pulse">
        <span className="text-xl">{'>'}</span>
        <span className="ml-2 w-4 h-5 bg-green-500 inline-block"></span>
      </div>
    </div>
  );
};