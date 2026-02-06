import React, { useState } from 'react';
import { SystemMetric, LogEntry } from '../types';

interface AIAnalystProps {
  metrics: SystemMetric[];
  logs: LogEntry[];
}

export const AIAnalyst: React.FC<AIAnalystProps> = ({ metrics, logs }) => {
  const [analysis, setAnalysis] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const analyzeSystem = async () => {
    setLoading(true);
    setAnalysis(null);

    // Simulate analysis delay
    await new Promise(resolve => setTimeout(resolve, 1500));

    // Generate mock analysis based on metrics
    const criticalMetrics = metrics.filter(m => m.status === 'CRITICAL');
    const warningMetrics = metrics.filter(m => m.status === 'WARNING');

    let status = 'NOMINAL';
    let risk = 15;

    if (criticalMetrics.length > 0) {
      status = 'CRITICAL';
      risk = 85;
    } else if (warningMetrics.length > 0) {
      status = 'CAUTION';
      risk = 45;
    }

    const analysisText = `STATUS: ${status}
RISK: ${risk}%
RECOMMENDATION: ${criticalMetrics.length > 0 ? 'IMMEDIATE ATTENTION REQUIRED' : warningMetrics.length > 0 ? 'MONITOR CLOSELY' : 'CONTINUE NORMAL OPERATIONS'}
ANALYSIS: System telemetry indicates ${metrics.length} active monitoring points. ${logs.length} log entries processed. All subsystems ${status === 'NOMINAL' ? 'operating within parameters' : 'require attention'}.`;

    setAnalysis(analysisText);
    setLoading(false);
  };

  return (
    <div className="retro-border bg-black/40 p-6 h-full flex flex-col">
       <div className="flex justify-between items-center mb-6 border-b-2 border-green-900/50 pb-3">
            <h3 className="text-cyan-400 text-2xl font-bold tracking-widest retro-text-shadow">
                AI_DIAGNOSTIC_CORE
            </h3>
            <div className="flex gap-3">
                 <div className={`w-4 h-4 rounded-full ${loading ? 'bg-yellow-400 animate-ping' : 'bg-cyan-500'}`}></div>
            </div>
       </div>

       <div className="flex-grow flex flex-col min-h-[150px]">
            {loading ? (
                <div className="flex-grow flex items-center justify-center text-cyan-500 animate-pulse text-xl">
                    PROCESSING_NEURAL_NET...
                </div>
            ) : analysis ? (
                <div className="text-green-300 font-['Share_Tech_Mono'] whitespace-pre-wrap leading-relaxed text-lg md:text-xl">
                    {analysis}
                </div>
            ) : (
                <div className="text-green-700/50 text-center mt-6 text-lg">
                    [AWAITING_MANUAL_TRIGGER]
                </div>
            )}
       </div>

       <button
        onClick={analyzeSystem}
        disabled={loading}
        className={`mt-6 w-full py-4 text-xl font-bold uppercase tracking-widest transition-all
            ${loading
                ? 'bg-gray-800 text-gray-500 cursor-not-allowed border-gray-600'
                : 'bg-cyan-900/30 text-cyan-400 border-2 border-cyan-500 hover:bg-cyan-500 hover:text-black hover:shadow-[0_0_20px_rgba(34,211,238,0.8)]'
            }
        `}
       >
        {loading ? 'CALCULATING...' : 'INITIATE_ANALYSIS_SEQUENCE'}
       </button>
    </div>
  );
};
