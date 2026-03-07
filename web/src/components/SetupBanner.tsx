import React, { useState, useEffect } from 'react';
import { X, Copy, Check, Terminal } from 'lucide-react';
import { fetchSetupStatus, SetupStatus } from '../services/api';

const DISMISS_KEY = 'at-setup-banner-dismissed';
const SETUP_COMMAND = 'curl -fsSL https://raw.githubusercontent.com/iocrazy/ai-tracker/main/scripts/setup.sh | bash';

export const SetupBanner: React.FC = () => {
  const [status, setStatus] = useState<SetupStatus | null>(null);
  const [dismissed, setDismissed] = useState(() => localStorage.getItem(DISMISS_KEY) === '1');
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (dismissed) return;
    fetchSetupStatus().then(setStatus).catch(() => {});
  }, [dismissed]);

  // Re-check periodically to auto-hide when setup completes
  useEffect(() => {
    if (dismissed || !status || status.setup_complete) return;
    const interval = setInterval(() => {
      fetchSetupStatus().then(setStatus).catch(() => {});
    }, 30000);
    return () => clearInterval(interval);
  }, [dismissed, status]);

  const handleCopy = async () => {
    await navigator.clipboard.writeText(SETUP_COMMAND);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const handleDismiss = () => {
    setDismissed(true);
    localStorage.setItem(DISMISS_KEY, '1');
  };

  // Don't show if dismissed, loading, or already configured
  if (dismissed || !status || status.setup_complete) return null;

  return (
    <div className="mx-2 md:mx-4 mb-2 px-3 py-2 rounded border border-yellow-700/60 bg-yellow-900/20 text-yellow-300 text-xs font-mono">
      <div className="flex items-start gap-2">
        <Terminal className="w-4 h-4 flex-shrink-0 mt-0.5 text-yellow-500" />
        <div className="flex-1 min-w-0">
          <div className="font-bold tracking-wider mb-1">SETUP REQUIRED</div>
          <div className="text-yellow-400/80 text-[11px] mb-2">
            Claude Code hooks are not configured. Run this command to complete setup:
          </div>
          <div className="flex items-center gap-2">
            <code className="flex-1 bg-black/40 px-2 py-1 rounded text-yellow-300 text-[10px] break-all select-all">
              {SETUP_COMMAND}
            </code>
            <button
              onClick={handleCopy}
              className="flex-shrink-0 p-1.5 rounded bg-yellow-900/40 hover:bg-yellow-900/60 transition-colors"
              title="Copy command"
            >
              {copied ? <Check className="w-3.5 h-3.5 text-green-400" /> : <Copy className="w-3.5 h-3.5" />}
            </button>
          </div>
        </div>
        <button
          onClick={handleDismiss}
          className="flex-shrink-0 p-1 hover:bg-yellow-900/40 rounded transition-colors"
          title="Dismiss"
        >
          <X className="w-3.5 h-3.5 text-yellow-600" />
        </button>
      </div>
    </div>
  );
};
