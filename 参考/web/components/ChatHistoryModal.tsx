import React, { useEffect, useRef } from 'react';
import { X, MessageSquare, ChevronDown, ChevronUp } from 'lucide-react';

export interface ChatMessage {
  sender: 'USER' | 'AGENT' | 'SYSTEM';
  text: string;
  timestamp: string;
}

interface ChatHistoryModalProps {
  isOpen: boolean;
  onClose: () => void;
  title: string;
  subtitle?: string;
  messages: ChatMessage[];
}

export const ChatHistoryModal: React.FC<ChatHistoryModalProps> = ({ isOpen, onClose, title, subtitle, messages }) => {
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!isOpen) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      switch (e.key) {
        case 'Escape':
        case 'h': // VIM back
          onClose();
          break;
        case 'j': // Scroll Down
        case 'ArrowDown':
           if (scrollRef.current) scrollRef.current.scrollBy({ top: 50, behavior: 'smooth' });
           break;
        case 'k': // Scroll Up
        case 'ArrowUp':
           if (scrollRef.current) scrollRef.current.scrollBy({ top: -50, behavior: 'smooth' });
           break;
        case 'J': // Fast Scroll Down
           if (scrollRef.current) scrollRef.current.scrollBy({ top: 200, behavior: 'smooth' });
           break;
        case 'K': // Fast Scroll Up
           if (scrollRef.current) scrollRef.current.scrollBy({ top: -200, behavior: 'smooth' });
           break;
      }
    };
    
    // Focus the modal content to capture keys if needed, 
    // but window listener is safer for modals
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/80 backdrop-blur-sm animate-[fadeIn_0.2s_ease-out]">
      <div 
        className="w-full max-w-3xl max-h-[80vh] flex flex-col retro-border bg-black shadow-[0_0_50px_rgba(34,197,94,0.3)] relative"
      >
        {/* Header */}
        <div className="flex items-center justify-between p-4 border-b border-green-800 bg-green-900/20">
            <div className="flex items-center gap-3">
                <MessageSquare className="w-5 h-5 text-green-400" />
                <div>
                    <h3 className="text-xl font-bold text-green-400 tracking-widest uppercase font-mono">{title}</h3>
                    {subtitle && <p className="text-xs text-green-700 font-mono tracking-wider">{subtitle}</p>}
                </div>
            </div>
            <button 
                onClick={onClose}
                className="text-green-800 hover:text-green-400 transition-colors p-1"
                title="Close [ESC / h]"
            >
                <X className="w-6 h-6" />
            </button>
        </div>

        {/* Content */}
        <div ref={scrollRef} className="flex-grow overflow-y-auto p-6 space-y-4 custom-scrollbar font-mono scroll-smooth">
            {messages.length === 0 ? (
                <div className="text-center text-green-900 py-10 italic">NO_DATA_FOUND_IN_ARCHIVE</div>
            ) : (
                messages.map((msg, idx) => (
                    <div key={idx} className={`flex gap-4 ${msg.sender === 'USER' ? 'flex-row-reverse' : ''}`}>
                        <div className={`
                            max-w-[80%] p-3 border leading-relaxed
                            ${msg.sender === 'SYSTEM' 
                                ? 'w-full text-center border-none text-green-800 italic text-sm' 
                                : msg.sender === 'USER' 
                                    ? 'border-green-600/50 bg-green-900/10 text-green-300 rounded-tl-lg rounded-br-lg rounded-bl-lg' 
                                    : 'border-green-800/50 text-green-400 rounded-tr-lg rounded-br-lg rounded-bl-lg'
                            }
                        `}>
                            {msg.sender !== 'SYSTEM' && (
                                <div className={`text-xs font-bold mb-1 opacity-70 ${msg.sender === 'USER' ? 'text-right' : 'text-left'}`}>
                                    {msg.sender} <span className="font-normal mx-1">|</span> {msg.timestamp}
                                </div>
                            )}
                            {msg.text}
                        </div>
                    </div>
                ))
            )}
        </div>

        {/* Footer */}
        <div className="p-2 border-t border-green-900/50 bg-black flex justify-between items-center px-4">
            <span className="text-[10px] text-green-900 uppercase tracking-widest">SCROLL: [J/K] • CLOSE: [ESC/h]</span>
            <span className="text-[10px] text-green-900 uppercase tracking-[0.3em]">END_OF_TRANSCRIPT</span>
        </div>
      </div>
    </div>
  );
};