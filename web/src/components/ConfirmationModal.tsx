import React, { useEffect, useRef, useState } from 'react';
import { X, AlertTriangle } from 'lucide-react';

interface ConfirmationModalProps {
  isOpen: boolean;
  onClose: () => void;
  onConfirm: () => void;
  title: string;
  message: string;
  confirmLabel?: string;
  confirmText?: string; // If set, user must type this to enable confirm button
}

export const ConfirmationModal: React.FC<ConfirmationModalProps> = ({ isOpen, onClose, onConfirm, title, message, confirmLabel = 'CONFIRM', confirmText }) => {
    const confirmRef = useRef<HTMLButtonElement>(null);
    const inputRef = useRef<HTMLInputElement>(null);
    const [typedText, setTypedText] = useState('');

    useEffect(() => {
        if (isOpen) {
            setTypedText('');
            setTimeout(() => {
                if (confirmText) {
                    inputRef.current?.focus();
                } else {
                    confirmRef.current?.focus();
                }
            }, 50);
        }
    }, [isOpen, confirmText]);

    if (!isOpen) return null;

    const canConfirm = !confirmText || typedText === confirmText;

    return (
    <div className="fixed inset-0 z-[70] flex items-center justify-center p-4 bg-black/90 backdrop-blur-sm animate-[fadeIn_0.2s_ease-out] overflow-y-auto">
      <div className="w-full max-w-md retro-border bg-black shadow-[0_0_50px_rgba(239,68,68,0.3)] relative border-red-500/50 my-auto max-h-[90vh] flex flex-col">

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
            <p className="text-red-300 font-mono text-lg leading-relaxed mb-6 border-l-2 border-red-900 pl-4">
                {message}
            </p>

          {confirmText && (
            <div className="mb-6">
              <label className="block text-red-500/70 font-mono text-xs tracking-widest uppercase mb-2">
                Type <span className="text-red-400 font-bold">{confirmText}</span> to confirm
              </label>
              <input
                ref={inputRef}
                type="text"
                value={typedText}
                onChange={e => setTypedText(e.target.value)}
                onKeyDown={e => { if (e.key === 'Enter' && canConfirm) { onConfirm(); onClose(); } }}
                placeholder={confirmText}
                className="w-full bg-black/60 border border-red-900 text-red-300 px-3 py-2 font-mono text-sm focus:border-red-500 outline-none placeholder:text-red-900/50"
                autoComplete="off"
                spellCheck={false}
              />
            </div>
          )}

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
              disabled={!canConfirm}
              className={`border px-6 py-2 font-bold tracking-widest uppercase transition-all ${
                canConfirm
                  ? 'bg-red-900/20 text-red-500 border-red-500 hover:bg-red-500 hover:text-black hover:shadow-[0_0_15px_rgba(239,68,68,0.6)]'
                  : 'bg-red-900/5 text-red-900 border-red-900/30 cursor-not-allowed'
              }`}
            >
              {confirmLabel}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
