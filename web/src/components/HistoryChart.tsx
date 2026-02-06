import React from 'react';
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
} from 'recharts';
import { SystemMetric } from '../types';

interface HistoryChartProps {
  metrics: SystemMetric[];
}

export const HistoryChart: React.FC<HistoryChartProps> = ({ metrics }) => {
    if (metrics.length === 0) return null;
    
    const dataLength = metrics[0].history.length;
    const data = Array.from({ length: dataLength }).map((_, i) => {
        const point: any = { index: i };
        metrics.forEach(m => {
            point[m.id] = m.history[i];
        });
        return point;
    });

  return (
    <div className="retro-border bg-black/40 p-6 h-full flex flex-col">
      <h3 className="text-green-400 text-2xl font-bold tracking-widest mb-6 retro-text-shadow border-b-2 border-green-900/50 pb-3">
        SYSTEM_TRAJECTORY_PLOT
      </h3>
      <div className="flex-grow w-full h-[200px]">
        <ResponsiveContainer width="100%" height="100%">
          <LineChart data={data}>
            <CartesianGrid strokeDasharray="3 3" stroke="#113311" />
            <XAxis dataKey="index" hide />
            <YAxis stroke="#225533" tick={{fill: '#225533', fontSize: 12, fontFamily: 'VT323'}} />
            <Tooltip 
                contentStyle={{ backgroundColor: '#000', border: '1px solid #4ade80', color: '#4ade80', fontFamily: 'VT323', fontSize: '14px' }}
                itemStyle={{ color: '#4ade80' }}
            />
            {metrics.map((m, i) => (
                <Line 
                    key={m.id}
                    type="step" 
                    dataKey={m.id} 
                    stroke={i === 0 ? '#4ade80' : i === 1 ? '#22d3ee' : '#f472b6'} 
                    strokeWidth={3}
                    dot={false}
                    isAnimationActive={false}
                />
            ))}
          </LineChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
};