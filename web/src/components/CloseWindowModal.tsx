import React, { useState } from 'react';
import { X, Trash2, Power, AlertTriangle } from 'lucide-react';

export type CloseAction = 'close' | 'destroy';

interface CloseWindowModalProps {
  sessionName: string;
  windowName: string;
  hasWorktree?: boolean;
  onClose: () => void;
  onConfirm: (action: CloseAction, deleteBranch: boolean) => void;
}

export const CloseWindowModal: React.FC<CloseWindowModalProps> = ({
  sessionName,
  windowName,
  hasWorktree = false,
  onClose,
  onConfirm,
}) => {
  const [selectedAction, setSelectedAction] = useState<CloseAction>('close');
  const [deleteBranch, setDeleteBranch] = useState(false);

  const handleConfirm = () => {
    onConfirm(selectedAction, deleteBranch);
  };

  return (
    <div className="fixed inset-0 bg-black/80 backdrop-blur-sm flex items-center justify-center z-50 p-4 overflow-y-auto">
      <div className="bg-black border-2 border-green-500 shadow-[0_0_30px_rgba(34,197,94,0.3)] w-full max-w-md mx-4 my-auto max-h-[90vh] flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between px-4 sm:px-6 py-3 border-b border-green-800 flex-shrink-0">
          <h2 className="text-xl font-bold text-green-400 tracking-widest font-['VT323']">
            CLOSE WINDOW
          </h2>
          <button
            onClick={onClose}
            className="text-green-700 hover:text-green-400 transition-colors"
          >
            <X className="w-6 h-6" />
          </button>
        </div>

        {/* Content - scrollable */}
        <div className="p-4 sm:p-6 space-y-4 overflow-y-auto flex-1">
          {/* Window Info */}
          <div className="text-sm text-green-600">
            <span className="text-green-400 font-bold">{sessionName}</span>
            <span className="mx-2">:</span>
            <span className="text-green-400 font-bold">{windowName}</span>
          </div>

          {/* Action Selection */}
          <div className="space-y-3">
            {/* Close (Temporary) */}
            <button
              onClick={() => setSelectedAction('close')}
              className={`
                w-full flex items-start gap-4 p-4 border transition-all text-left
                ${selectedAction === 'close'
                  ? 'border-green-400 bg-green-900/30 shadow-[0_0_15px_rgba(34,197,94,0.3)]'
                  : 'border-green-900 hover:border-green-600'
                }
              `}
            >
              <Power className={`w-6 h-6 mt-0.5 ${selectedAction === 'close' ? 'text-green-400' : 'text-green-700'}`} />
              <div>
                <div className={`font-bold tracking-wider ${selectedAction === 'close' ? 'text-green-400' : 'text-green-600'}`}>
                  CLOSE
                </div>
                <div className="text-xs text-green-800 mt-1">
                  Close tmux window only. Worktree remains on disk.
                  <br />
                  Can be reopened with RESUME.
                </div>
              </div>
            </button>

            {/* Destroy (Permanent) */}
            <button
              onClick={() => setSelectedAction('destroy')}
              className={`
                w-full flex items-start gap-4 p-4 border transition-all text-left
                ${selectedAction === 'destroy'
                  ? 'border-red-500 bg-red-900/20 shadow-[0_0_15px_rgba(239,68,68,0.3)]'
                  : 'border-red-900/50 hover:border-red-700'
                }
              `}
            >
              <Trash2 className={`w-6 h-6 mt-0.5 ${selectedAction === 'destroy' ? 'text-red-400' : 'text-red-800'}`} />
              <div>
                <div className={`font-bold tracking-wider ${selectedAction === 'destroy' ? 'text-red-400' : 'text-red-700'}`}>
                  DESTROY
                </div>
                <div className="text-xs text-red-900 mt-1">
                  Permanently delete tmux window and git worktree.
                  <br />
                  This action cannot be undone.
                </div>
              </div>
            </button>
          </div>

          {/* Delete Branch Option (only for destroy) */}
          {selectedAction === 'destroy' && hasWorktree && (
            <div className="border border-red-900/50 p-4 bg-red-900/10">
              <label className="flex items-center gap-3 cursor-pointer">
                <input
                  type="checkbox"
                  checked={deleteBranch}
                  onChange={(e) => setDeleteBranch(e.target.checked)}
                  className="w-4 h-4 accent-red-500"
                />
                <span className="text-red-600 text-sm">
                  Also delete git branch
                </span>
              </label>
              {deleteBranch && (
                <div className="flex items-center gap-2 mt-3 text-xs text-red-500">
                  <AlertTriangle className="w-4 h-4" />
                  Branch will be permanently deleted from repository
                </div>
              )}
            </div>
          )}

          {/* Warning for destroy */}
          {selectedAction === 'destroy' && (
            <div className="flex items-center gap-3 p-3 border border-yellow-800 bg-yellow-900/10">
              <AlertTriangle className="w-5 h-5 text-yellow-500 flex-shrink-0" />
              <span className="text-yellow-600 text-sm">
                All unsaved changes in the worktree will be lost
              </span>
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex justify-end gap-3 px-4 sm:px-6 py-3 border-t border-green-800 flex-shrink-0">
          <button
            onClick={onClose}
            className="px-6 py-2 border border-green-800 text-green-700 hover:border-green-500 hover:text-green-400 transition-all tracking-widest text-sm"
          >
            CANCEL
          </button>
          <button
            onClick={handleConfirm}
            className={`
              px-6 py-2 font-bold transition-all tracking-widest text-sm
              ${selectedAction === 'destroy'
                ? 'bg-red-600 text-white hover:bg-red-500 shadow-[0_0_15px_rgba(239,68,68,0.5)]'
                : 'bg-green-600 text-black hover:bg-green-400 shadow-[0_0_15px_rgba(34,197,94,0.5)]'
              }
            `}
          >
            {selectedAction === 'destroy' ? 'DESTROY' : 'CLOSE'}
          </button>
        </div>
      </div>
    </div>
  );
};

export default CloseWindowModal;
