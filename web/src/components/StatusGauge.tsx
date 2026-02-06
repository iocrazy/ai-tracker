import React from 'react';
import { SystemMetric } from '../types';

interface StatusGaugeProps {
  metric: SystemMetric;
}

export const StatusGauge: React.FC<StatusGaugeProps> = ({ metric }) => {
  // Create 15 segments for the bar
  const segments = 15;
  const activeSegments = Math.round((metric.value / metric.max) * segments);
  
  // Determine colors based on status
  const getColor = (s: string) => {
    if (s === 'CRITICAL') return 'bg-red-500 shadow-[0_0_8px_rgba(239,68,68,0.8)]';
    if (s === 'WARNING') return 'bg-yellow-500 shadow-[0_0_8px_rgba(234,179,8,0.8)]';
    return 'bg-green-500 shadow-[0_0_8px_rgba(34,197,94,0.8)]';
  };

  const statusColor = getColor(metric.status);
  const textColor = metric.status === 'CRITICAL' ? 'text-red-500' : metric.status === 'WARNING' ? 'text-yellow-500' : 'text-green-400';

  return (
    <div className="retro-border bg-black/40 p-5 flex flex-col justify-between h-full relative overflow-hidden group min-h-[180px]">
      
      {/* Header Row: Label + LED */}
      <div className="flex justify-between items-start mb-4">
        <span className="text-sm md:text-base font-bold tracking-widest text-green-600 uppercase font-mono">{metric.label}</span>
        {/* Status LED - Larger */}
        <div className="flex gap-2">
             <div className={`w-3 h-3 ${metric.status === 'NORMAL' ? 'bg-green-500 shadow-[0_0_5px_#22c55e]' : 'bg-green-900'} rounded-full`}></div>
             <div className={`w-3 h-3 ${metric.status === 'WARNING' ? 'bg-yellow-500 shadow-[0_0_5px_#eab308]' : 'bg-yellow-900'} rounded-full`}></div>
             <div className={`w-3 h-3 ${metric.status === 'CRITICAL' ? 'bg-red-500 shadow-[0_0_5px_#ef4444]' : 'bg-red-900'} rounded-full`}></div>
        </div>
      </div>

      {/* Value Display - Larger */}
      <div className={`text-4xl md:text-5xl font-bold font-mono ${textColor} retro-text-shadow leading-none mb-6`}>
        {metric.value.toFixed(1)}
        <span className="text-lg ml-2 opacity-60 text-green-700 font-sans">{metric.unit}</span>
      </div>

      {/* Segmented Bar Visual - Taller */}
      <div className="flex gap-[3px] h-6 w-full mt-auto items-end">
        {[...Array(segments)].map((_, i) => (
          <div 
            key={i}
            className={`flex-1 transition-all duration-300 ${
                i < activeSegments 
                ? statusColor 
                : 'bg-green-900/20'
            }`}
            style={{
                height: i < activeSegments ? '100%' : '50%'
            }}
          />
        ))}
      </div>
      
      {/* Decorative corner bits */}
      <div className="absolute top-0 left-0 w-3 h-3 border-t-2 border-l-2 border-green-500/50"></div>
      <div className="absolute top-0 right-0 w-3 h-3 border-t-2 border-r-2 border-green-500/50"></div>
      <div className="absolute bottom-0 left-0 w-3 h-3 border-b-2 border-l-2 border-green-500/50"></div>
      <div className="absolute bottom-0 right-0 w-3 h-3 border-b-2 border-r-2 border-green-500/50"></div>
    </div>
  );
};