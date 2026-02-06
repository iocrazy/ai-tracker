import React, { useState, useEffect, useRef } from 'react';
import { X, Terminal } from 'lucide-react';

interface InputModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSubmit: (value: string) => void;
  title: string;
  placeholder?: string;
}

export const InputModal: React.FC<InputModalProps> = ({ isOpen, onClose, onSubmit, title, placeholder }) => {
  const [value, setValue] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (isOpen) {
      setValue('');
      // Slight delay to ensure render before focus
      setTimeout(() => inputRef.current?.focus(), 50);
    }
  }, [isOpen]);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (value.trim()) {
      onSubmit(value.trim());
      onClose();
    }
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center p-4 bg-black/90 backdrop-blur-sm animate-[fadeIn_0.2s_ease-out] overflow-y-auto">
      <div className="w-full max-w-md retro-border bg-black shadow-[0_0_50px_rgba(34,197,94,0.3)] relative my-auto max-h-[90vh] flex flex-col">
        
        {/* Header */}
        <div className="flex items-center justify-between p-4 border-b border-green-800 bg-green-900/20">
          <div className="flex items-center gap-3">
            <Terminal className="w-5 h-5 text-green-400" />
            <h3 className="text-xl font-bold text-green-400 tracking-widest uppercase font-mono">{title}</h3>
          </div>
          <button 
            onClick={onClose}
            className="text-green-800 hover:text-green-400 transition-colors p-1"
          >
            <X className="w-6 h-6" />
          </button>
        </div>

        {/* Form */}
        <form onSubmit={handleSubmit} className="p-6">
          <div className="mb-6">
            <label className="block text-green-700 text-sm font-bold mb-2 tracking-widest font-mono">
              INPUT_PARAMETER:
            </label>
            <input
              ref={inputRef}
              type="text"
              value={value}
              onChange={(e) => setValue(e.target.value)}
              placeholder={placeholder}
              className="w-full bg-black border-2 border-green-800 p-3 text-green-300 placeholder-green-900 focus:outline-none focus:border-green-400 focus:shadow-[0_0_15px_rgba(34,197,94,0.3)] font-mono text-lg"
            />
          </div>

          <div className="flex justify-end gap-4">
            <button
              type="button"
              onClick={onClose}
              className="px-4 py-2 text-green-800 hover:text-green-500 font-mono tracking-widest uppercase text-sm"
            >
              CANCEL
            </button>
            <button
              type="submit"
              disabled={!value.trim()}
              className="bg-green-600 text-black px-6 py-2 font-bold tracking-widest uppercase hover:bg-green-400 hover:shadow-[0_0_15px_rgba(34,197,94,0.6)] disabled:opacity-50 disabled:cursor-not-allowed transition-all"
            >
              CONFIRM
            </button>
          </div>
        </form>
      </div>
    </div>
  );
};