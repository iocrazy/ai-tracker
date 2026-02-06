import React, { useEffect, useRef } from 'react';
import { X, AlertTriangle } from 'lucide-react';

interface ConfirmationModalProps {
  isOpen: boolean;
  onClose: () => void;
  onConfirm: () => void;
  title: string;
  message: string;
}

export const ConfirmationModal: React.FC<ConfirmationModalProps> = ({ isOpen, onClose, onConfirm, title, message }) => {
    const confirmRef = useRef<HTMLButtonElement>(null);

    useEffect(() => {
        if(isOpen) {
             // Focus the confirm button for quick access
             setTimeout(() => confirmRef.current?.focus(), 50);
        }
    }, [isOpen]);

    if (!isOpen) return null;

    return (
    <div className="fixed inset-0 z-[70] flex items-center justify-center p-4 bg-black/90 backdrop-blur-sm animate-[fadeIn_0.2s_ease-out]">
      <div className="w-full max-w-md retro-border bg-black shadow-[0_0_50px_rgba(239,68,68,0.3)] relative border-red-500/50">
        
        {/* Header */}
        <div className="flex items-center justify-between p-4 border-b border-red-900 bg-red-900/10">
          <div className="flex items-center gap-3">
            <AlertTriangle className="w-6 h-6 text-red-500 animate-pulse" />
            <h3 className="text-xl font-bold text-red-500 tracking-widest uppercase font-mono">{title}</h3>
          </div>
          <button 
            onClick={onClose}
            className="text-red-800 hover:text-red-400 transition-colors p-1"
          >
            <X className="w-6 h-6" />
          </button>
        </div>

        {/* Content */}
        <div className="p-6">
            <p className="text-red-300 font-mono text-lg leading-relaxed mb-8 border-l-2 border-red-900 pl-4">
                {message}
            </p>

          <div className="flex justify-end gap-4">
            <button
              onClick={onClose}
              className="px-4 py-2 text-red-800 hover:text-red-500 font-mono tracking-widest uppercase text-sm border border-transparent hover:border-red-900 transition-all"
            >
              CANCEL
            </button>
            <button
              ref={confirmRef}
              onClick={() => { onConfirm(); onClose(); }}
              className="bg-red-900/20 text-red-500 border border-red-500 px-6 py-2 font-bold tracking-widest uppercase hover:bg-red-500 hover:text-black hover:shadow-[0_0_15px_rgba(239,68,68,0.6)] transition-all"
            >
              CONFIRM_DELETION
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};