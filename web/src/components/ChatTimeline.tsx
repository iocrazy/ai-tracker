import React, { useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { ToolCallBlock } from './ToolCallBlock';
import type { ToolCallInfo, ToolResultInfo, ToolInteraction, TimelineEntry } from '../services/api';
import type { ChatMessage } from './ChatHistoryModal';

// ============================================================================
// Unified Timeline Types
// ============================================================================

export interface TimelineBlock {
  type: 'text' | 'thinking' | 'tool_call' | 'interaction';
  content?: string;
  toolCall?: ToolCallInfo;
  toolResult?: ToolResultInfo;
  interaction?: ToolInteraction;
}

export interface TimelineItem {
  id: string;
  role: 'user' | 'assistant' | 'system';
  timestamp: string;
  blocks: TimelineBlock[];
}

// ============================================================================
// Adapter: Live chat messages → TimelineItem[]
// ============================================================================

export function fromLiveChatMessages(messages: ChatMessage[]): TimelineItem[] {
  return messages.map((msg, idx) => {
    const blocks: TimelineBlock[] = [];

    // Thinking
    if (msg.thinking) {
      blocks.push({ type: 'thinking', content: msg.thinking });
    }

    // Text
    if (msg.text) {
      blocks.push({ type: 'text', content: msg.text });
    }

    // Tool calls (with result matching from same message or next message)
    if (msg.toolCalls && msg.toolCalls.length > 0) {
      const nextMsg = messages[idx + 1];
      for (const tc of msg.toolCalls) {
        // Check same message first (backend merges tool results into assistant msg),
        // then fall back to next message (legacy/WS push format)
        const result = msg.toolResults?.find(tr => tr.tool_use_id === tc.tool_use_id)
          || nextMsg?.toolResults?.find(tr => tr.tool_use_id === tc.tool_use_id);
        blocks.push({ type: 'tool_call', toolCall: tc, toolResult: result });
      }
    }

    // Interactive questions
    if (msg.interaction) {
      blocks.push({ type: 'interaction', interaction: msg.interaction });
    }

    return {
      id: `live-${idx}`,
      role: (msg.sender === 'USER' ? 'user' : msg.sender === 'SYSTEM' ? 'system' : 'assistant') as TimelineItem['role'],
      timestamp: msg.timestamp,
      blocks,
    };
  }).filter(item => {
    if (item.blocks.length === 0) return false;
    // Skip user items with no visible content (tool-result-only entries are automatic, not real user input)
    if (item.role === 'user' && !item.blocks.some(b => b.type === 'text' || b.type === 'interaction')) {
      return false;
    }
    return true;
  });
}

// ============================================================================
// Adapter: History timeline → TimelineItem[]
// ============================================================================

export function fromHistoryTimeline(entries: TimelineEntry[]): TimelineItem[] {
  const items: TimelineItem[] = [];
  let currentItem: TimelineItem | null = null;

  for (const entry of entries) {
    const role = entry.role as 'user' | 'assistant';
    const timestamp = entry.timestamp?.slice(11, 19) || '';

    // Group consecutive entries from the same role+timestamp into one item
    if (!currentItem || currentItem.role !== role || currentItem.timestamp !== timestamp) {
      if (currentItem) {
        items.push(currentItem);
      }
      currentItem = {
        id: `hist-${items.length}`,
        role,
        timestamp,
        blocks: [],
      };
    }

    switch (entry.entry_type) {
      case 'thinking':
        if (entry.thinking) {
          currentItem.blocks.push({ type: 'thinking', content: entry.thinking });
        }
        break;
      case 'text':
        if (entry.text) {
          currentItem.blocks.push({ type: 'text', content: entry.text });
        }
        break;
      case 'tool_call':
        if (entry.tool_call) {
          // Find matching tool_result in subsequent entries
          const resultEntry = entries.find(
            e => e.entry_type === 'tool_result' &&
                 e.tool_result?.tool_use_id === entry.tool_call?.tool_use_id
          );
          const toolCall: ToolCallInfo = {
            tool_use_id: entry.tool_call.tool_use_id,
            tool_name: entry.tool_call.tool_name,
            args_summary: entry.tool_call.args_summary,
            args_full: entry.tool_call.args_full,
          };
          const toolResult: ToolResultInfo | undefined = resultEntry?.tool_result ? {
            tool_use_id: resultEntry.tool_result.tool_use_id,
            content: resultEntry.tool_result.content,
            is_error: resultEntry.tool_result.is_error,
          } : undefined;
          currentItem.blocks.push({ type: 'tool_call', toolCall, toolResult });
        }
        break;
      case 'tool_result':
        // Handled by tool_call matching above, skip standalone
        break;
    }
  }

  if (currentItem) {
    items.push(currentItem);
  }

  return items.filter(item => {
    if (item.blocks.length === 0) return false;
    if (item.role === 'user' && !item.blocks.some(b => b.type === 'text' || b.type === 'interaction')) {
      return false;
    }
    return true;
  });
}

// ============================================================================
// ChatTimeline Component
// ============================================================================

interface ChatTimelineProps {
  items: TimelineItem[];
  /** For interactive buttons — pass session targeting info */
  onInteractionSelect?: (msgIdx: number, optionIdx: number, multiSelect: boolean, totalOptions: number) => void;
  /** For free-text input on interactive questions (Claude Code's "Type something") */
  onInteractionTextSubmit?: (msgIdx: number, text: string, optionCount: number, multiSelect: boolean) => void;
  sentInteractions?: Set<number>;
}

export const ChatTimeline: React.FC<ChatTimelineProps> = ({
  items,
  onInteractionSelect,
  onInteractionTextSubmit,
  sentInteractions = new Set(),
}) => {
  const [expandedThinking, setExpandedThinking] = useState<Set<string>>(new Set());
  const [customTexts, setCustomTexts] = useState<Record<string, string>>({});

  const toggleThinking = (key: string) => {
    setExpandedThinking(prev => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key); else next.add(key);
      return next;
    });
  };

  if (items.length === 0) {
    return (
      <div className="text-center text-green-900 py-8 italic text-base sm:text-lg">
        NO_DATA_FOUND_IN_ARCHIVE
      </div>
    );
  }

  return (
    <div className="space-y-2 sm:space-y-3">
      {items.map((item, itemIdx) => (
        <div key={item.id} className={`flex gap-2 ${item.role === 'user' ? 'flex-row-reverse' : ''}`}>
          <div className={`
            max-w-[90%] sm:max-w-[85%] p-2 sm:p-3 border leading-snug text-xs sm:text-sm
            ${item.role === 'system'
              ? 'w-full text-center border-none text-green-700 italic text-xs sm:text-sm'
              : item.role === 'user'
                ? 'border-green-600/50 bg-green-900/10 text-green-300 rounded-tl-lg rounded-br-lg rounded-bl-lg'
                : 'border-green-800/50 text-green-400 rounded-tr-lg rounded-br-lg rounded-bl-lg'
            }
          `}>
            {/* Header */}
            {item.role !== 'system' && (
              <div className={`text-[10px] sm:text-xs font-bold mb-1 opacity-70 ${item.role === 'user' ? 'text-right' : 'text-left'}`}>
                {item.role === 'user' ? 'USER' : 'AGENT'} <span className="font-normal mx-1">|</span> {item.timestamp}
              </div>
            )}

            {/* Blocks */}
            {item.blocks.map((block, blockIdx) => {
              const blockKey = `${item.id}-${blockIdx}`;

              switch (block.type) {
                case 'thinking':
                  return (
                    <div key={blockKey} className="mb-1">
                      <button
                        onClick={() => toggleThinking(blockKey)}
                        className="text-[10px] sm:text-xs text-green-700 hover:text-green-500 font-mono transition-colors"
                      >
                        {expandedThinking.has(blockKey) ? '▼' : '▶'} Thinking...
                      </button>
                      {expandedThinking.has(blockKey) && (
                        <pre className="mt-1 p-2 text-[10px] sm:text-xs text-green-800 bg-green-900/10 border border-green-900/30 overflow-x-auto whitespace-pre-wrap break-words max-h-40 overflow-y-auto custom-scrollbar font-mono">
                          {block.content}
                        </pre>
                      )}
                    </div>
                  );

                case 'text':
                  return (
                    <div key={blockKey} className="prose prose-invert prose-green prose-xs sm:prose-sm max-w-none break-words overflow-hidden
                      prose-p:my-0.5 prose-p:leading-snug prose-headings:text-green-400 prose-headings:my-1
                      prose-code:text-green-300 prose-code:bg-green-900/30 prose-code:px-1 prose-code:rounded prose-code:text-xs prose-code:break-all
                      prose-pre:bg-green-900/20 prose-pre:border prose-pre:border-green-800 prose-pre:my-1 prose-pre:p-2 prose-pre:overflow-x-auto prose-pre:max-w-full
                      [&_pre_code]:break-normal [&_pre_code]:whitespace-pre [&_pre_code]:bg-transparent [&_pre_code]:p-0 [&_pre_code]:text-green-500 [&_pre_code]:text-[10px] [&_pre_code]:leading-tight [&_pre_code]:font-mono
                      prose-strong:text-green-300 prose-em:text-green-400
                      prose-ul:my-0.5 prose-ol:my-0.5 prose-li:my-0 prose-li:leading-snug
                      [&_ol]:list-decimal [&_ol]:list-inside [&_ol]:pl-2
                      [&_ul]:list-disc [&_ul]:list-inside [&_ul]:pl-2
                      [&_table]:block [&_table]:overflow-x-auto [&_table]:max-w-full [&_table]:text-xs
                      [&_td]:break-all [&_th]:break-all">
                      <ReactMarkdown remarkPlugins={[remarkGfm]}>{block.content || ''}</ReactMarkdown>
                    </div>
                  );

                case 'tool_call':
                  return block.toolCall ? (
                    <ToolCallBlock
                      key={blockKey}
                      toolCall={block.toolCall}
                      toolResult={block.toolResult}
                    />
                  ) : null;

                case 'interaction':
                  if (!block.interaction) return null;
                  return (
                    <div key={blockKey}>
                      {block.interaction.questions.map((q, qIdx) => (
                        <div key={qIdx} className="mt-2">
                          {q.header && <div className="text-[10px] sm:text-xs text-green-600 font-mono mb-1 uppercase tracking-wider">{q.header}</div>}
                          <div className="text-xs sm:text-sm text-green-400 mb-2">{q.question}</div>
                          {/* Multi-select indicator */}
                          {q.multi_select && (
                            <div className="text-[10px] text-green-700 font-mono mb-1">[MULTI-SELECT]</div>
                          )}
                          <div className="flex flex-col gap-1.5">
                            {q.options.map((opt, optIdx) => (
                              <button
                                key={optIdx}
                                disabled={sentInteractions.has(itemIdx) || !onInteractionSelect}
                                onClick={() => onInteractionSelect?.(itemIdx, optIdx, q.multi_select ?? false, q.options.length)}
                                className={`text-left p-2 border font-mono text-xs sm:text-sm transition-colors ${
                                  sentInteractions.has(itemIdx) || !onInteractionSelect
                                    ? 'border-green-900/50 text-green-800 cursor-default'
                                    : 'border-green-700/50 text-green-400 hover:border-green-500 hover:bg-green-900/20 cursor-pointer'
                                }`}
                              >
                                <span className="text-green-600 mr-2">{optIdx + 1}.</span>
                                <span className="font-bold">{opt.label}</span>
                                {opt.description && <span className="block text-[10px] sm:text-xs text-green-700 mt-0.5 ml-4">{opt.description}</span>}
                              </button>
                            ))}
                          </div>
                          {/* Free text input + Chat about this (matches Claude Code's extra options) */}
                          {!sentInteractions.has(itemIdx) && onInteractionSelect && (
                            <div className="mt-2 flex flex-col gap-1.5">
                              <div className="flex gap-1.5">
                                <input
                                  type="text"
                                  placeholder="Other..."
                                  value={customTexts[`${itemIdx}-${qIdx}`] || ''}
                                  onChange={(e) => setCustomTexts(prev => ({ ...prev, [`${itemIdx}-${qIdx}`]: e.target.value }))}
                                  onKeyDown={(e) => {
                                    const text = customTexts[`${itemIdx}-${qIdx}`]?.trim();
                                    if (e.key === 'Enter' && text) {
                                      onInteractionTextSubmit?.(itemIdx, text, q.options.length, q.multi_select ?? false);
                                    }
                                  }}
                                  className="flex-1 bg-black border border-green-700/50 text-green-400 px-2 py-1.5 text-xs sm:text-sm font-mono placeholder:text-green-900 focus:border-green-500 focus:outline-none"
                                />
                                <button
                                  onClick={() => {
                                    const text = customTexts[`${itemIdx}-${qIdx}`]?.trim();
                                    if (text) onInteractionTextSubmit?.(itemIdx, text, q.options.length, q.multi_select ?? false);
                                  }}
                                  className="px-3 py-1.5 border border-green-700/50 text-green-600 hover:text-green-400 hover:border-green-500 hover:bg-green-900/20 text-xs font-mono transition-colors"
                                >
                                  SEND
                                </button>
                              </div>
                              <button
                                onClick={() => onInteractionSelect?.(itemIdx, q.options.length + 1, q.multi_select ?? false, q.options.length)}
                                className="text-left p-2 border border-dashed border-green-900/50 text-green-700 hover:text-green-500 hover:border-green-700 text-xs font-mono transition-colors"
                              >
                                <span className="text-green-800 mr-2">{q.options.length + 2}.</span>
                                Chat about this
                              </button>
                            </div>
                          )}
                          {sentInteractions.has(itemIdx) && (
                            <div className="text-[10px] text-green-700 mt-1 font-mono">RESPONSE SENT</div>
                          )}
                        </div>
                      ))}
                    </div>
                  );

                default:
                  return null;
              }
            })}
          </div>
        </div>
      ))}
    </div>
  );
};
