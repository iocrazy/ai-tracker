import React, { useState } from 'react';
import { GoogleGenAI } from '@google/genai';
import { SystemMetric, LogEntry } from '../types';

interface AIAnalystProps {
  metrics: SystemMetric[];
  logs: LogEntry[];
}

export const AIAnalyst: React.FC<AIAnalystProps> = ({ metrics, logs }) => {
  const [analysis, setAnalysis] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const analyzeSystem = async () => {
    setLoading(true);
    setError(null);
    setAnalysis(null);

    try {
      if (!process.env.API_KEY) {
        throw new Error("API_KEY_MISSING: CONNECT TO MAINFRAME");
      }

      const ai = new GoogleGenAI({ apiKey: process.env.API_KEY });
      
      const metricsText = metrics.map(m => `${m.label}: ${m.value.toFixed(1)}${m.unit} [${m.status}]`).join('\n');
      const logsText = logs.slice(-5).map(l => `[${l.timestamp}] ${l.level}: ${l.message}`).join('\n');
      
      const prompt = `
        You are the Central Mainframe AI of a retro-futuristic industrial facility (Year 199X).
        Analyze the following system telemetry.
        
        CURRENT METRICS:
        ${metricsText}
        
        RECENT LOGS:
        ${logsText}
        
        Output a status report. 
        Style: Robotic, curt, slightly ominous but helpful. Use uppercase for key terms.
        Format:
        STATUS: [One word status]
        RISK: [Percentage]%
        RECOMMENDATION: [Brief tactical advice]
        ANALYSIS: [2-3 sentences max]
      `;

      const response = await ai.models.generateContent({
        model: 'gemini-3-flash-preview',
        contents: prompt,
      });

      setAnalysis(response.text || "NO DATA RETURNED FROM AI CORE.");
    } catch (err: any) {
        console.error(err);
        setError(err.message || "UPLINK FAILURE");
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="retro-border bg-black/40 p-6 h-full flex flex-col">
       <div className="flex justify-between items-center mb-6 border-b-2 border-green-900/50 pb-3">
            <h3 className="text-cyan-400 text-2xl font-bold tracking-widest retro-text-shadow">
                AI_DIAGNOSTIC_CORE
            </h3>
            <div className="flex gap-3">
                 <div className={`w-4 h-4 rounded-full ${loading ? 'bg-yellow-400 animate-ping' : error ? 'bg-red-500' : 'bg-cyan-500'}`}></div>
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
            ) : error ? (
                <div className="text-red-500 font-bold border-2 border-red-500 p-4 text-lg">
                    ERROR: {error}
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