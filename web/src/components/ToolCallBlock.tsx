import React, { useState } from 'react';

// Tool call info from backend
export interface ToolCallInfo {
  tool_use_id: string;
  tool_name: string;
  args_summary: string;
  args_full?: string;
}

// Tool result info from backend
export interface ToolResultInfo {
  tool_use_id: string;
  content: string;
  is_error: boolean;
}

// Tool icons by name
const TOOL_ICONS: Record<string, string> = {
  Bash: '⌘',
  Read: '📄',
  Write: '✏️',
  Edit: '✏️',
  Grep: '🔍',
  Glob: '📁',
  Task: '🔀',
  WebSearch: '🌐',
  WebFetch: '🌐',
  NotebookEdit: '📓',
};

interface ToolCallBlockProps {
  toolCall: ToolCallInfo;
  toolResult?: ToolResultInfo;
}

export const ToolCallBlock: React.FC<ToolCallBlockProps> = ({ toolCall, toolResult }) => {
  const [expanded, setExpanded] = useState(false);

  const icon = TOOL_ICONS[toolCall.tool_name] || '🔧';
  const isError = toolResult?.is_error ?? false;
  const borderColor = isError ? 'border-red-700/50' : 'border-cyan-800/50';
  const headerColor = isError ? 'text-red-400' : 'text-cyan-500';
  const statusDot = toolResult
    ? (isError ? '🔴' : '🟢')
    : '⏳';

  // Parse Edit tool args for diff display
  const editDiff = toolCall.tool_name === 'Edit' && toolCall.args_full
    ? parseEditArgs(toolCall.args_full)
    : null;

  return (
    <div className={`my-1 border ${borderColor} bg-black/30 font-mono text-xs`}>
      {/* Header - always visible */}
      <button
        onClick={() => setExpanded(!expanded)}
        className={`w-full text-left px-2 py-1 flex items-center gap-1.5 hover:bg-green-900/10 transition-colors ${headerColor}`}
      >
        <span className="text-[10px] opacity-70">{expanded ? '▼' : '▶'}</span>
        <span>{icon}</span>
        <span className="font-bold">{toolCall.tool_name}</span>
        <span className="text-green-700 truncate flex-1 ml-1">{toolCall.args_summary}</span>
        <span className="text-[10px] flex-shrink-0">{statusDot}</span>
      </button>

      {/* Expanded details */}
      {expanded && (
        <div className="px-2 pb-2 space-y-1 border-t border-green-900/30">
          {/* Edit tool: diff view */}
          {editDiff ? (
            <EditDiffView diff={editDiff} />
          ) : toolCall.args_full ? (
            <div className="mt-1">
              <div className="text-[10px] text-green-700 uppercase tracking-wider mb-0.5">ARGS</div>
              <pre className="p-1.5 text-[10px] text-green-600 bg-green-900/10 border border-green-900/30 overflow-x-auto whitespace-pre-wrap break-all max-h-32 overflow-y-auto custom-scrollbar">
                {formatArgs(toolCall.args_full)}
              </pre>
            </div>
          ) : null}

          {/* Result */}
          {toolResult && (
            <div>
              <div className={`text-[10px] uppercase tracking-wider mb-0.5 ${isError ? 'text-red-600' : 'text-green-700'}`}>
                {isError ? 'ERROR' : 'RESULT'}
              </div>
              <pre className={`p-1.5 text-[10px] bg-green-900/10 border overflow-x-auto whitespace-pre-wrap break-all max-h-40 overflow-y-auto custom-scrollbar ${
                isError ? 'text-red-400 border-red-900/30' : 'text-green-600 border-green-900/30'
              }`}>
                {toolResult.content || '(empty)'}
              </pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
};

// ============================================================================
// Edit diff rendering
// ============================================================================

interface EditDiffData {
  file_path: string;
  old_string: string;
  new_string: string;
  replace_all?: boolean;
}

function parseEditArgs(argsStr: string): EditDiffData | null {
  try {
    const parsed = JSON.parse(argsStr);
    if (parsed.file_path && typeof parsed.old_string === 'string' && typeof parsed.new_string === 'string') {
      return {
        file_path: parsed.file_path,
        old_string: parsed.old_string,
        new_string: parsed.new_string,
        replace_all: parsed.replace_all,
      };
    }
  } catch { /* ignore */ }
  return null;
}

const EditDiffView: React.FC<{ diff: EditDiffData }> = ({ diff }) => {
  const oldLines = diff.old_string.split('\n');
  const newLines = diff.new_string.split('\n');

  return (
    <div className="mt-1 space-y-1">
      {diff.replace_all && (
        <div className="text-[10px] text-yellow-600">replace_all</div>
      )}
      <div className="border border-green-900/30 bg-black/20 overflow-x-auto max-h-60 overflow-y-auto custom-scrollbar">
        {/* Removed lines */}
        {oldLines.map((line, i) => (
          <div key={`old-${i}`} className="px-2 py-px text-[10px] bg-red-900/20 text-red-400 whitespace-pre-wrap break-all">
            <span className="inline-block w-3 text-red-600 select-none">-</span>{line}
          </div>
        ))}
        {/* Added lines */}
        {newLines.map((line, i) => (
          <div key={`new-${i}`} className="px-2 py-px text-[10px] bg-green-900/20 text-green-400 whitespace-pre-wrap break-all">
            <span className="inline-block w-3 text-green-600 select-none">+</span>{line}
          </div>
        ))}
      </div>
    </div>
  );
};

// Try to pretty-print JSON args, fall back to raw string
function formatArgs(argsStr: string): string {
  try {
    const parsed = JSON.parse(argsStr);
    return JSON.stringify(parsed, null, 2);
  } catch {
    return argsStr;
  }
}
