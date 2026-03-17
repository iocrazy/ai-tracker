import React, { useState, useEffect } from 'react';
import { AppSettings } from '../types';
import { Check, Download, Shield, Trash2, Activity, Terminal, Copy, Eye, EyeOff } from 'lucide-react';
import { fetchAlertRules, createAlertRule, updateAlertRule, deleteAlertRule, AlertRule, fetchBackups, createBackup, BackupEntry, fetchDiagnostics, adminRestart, adminClearLogs, DiagnosticComponent, DiagnosticsResult, fetchSetupStatus, SetupStatus, getAuthToken } from '../services/api';

interface SettingsViewProps {
    settings: AppSettings;
    onUpdate: (key: keyof AppSettings, value: any) => void;
}

export const SettingsView: React.FC<SettingsViewProps> = ({ settings, onUpdate }) => {

  const isModern = settings.theme === 'MODERN';

  const effects = [
      { id: 'scanlines', label: 'Scanlines' },
      { id: 'flicker', label: 'Flicker Effect' },
      { id: 'glow', label: 'Glow Effects' },
      { id: 'noise', label: 'Signal Noise' },
      { id: 'rgbShift', label: 'RGB Shift' },
      { id: 'perspectiveGrid', label: '3D Grid' }
  ];

  return (
    <div className="flex flex-col gap-4 sm:gap-8 pt-4 pb-10 px-2 sm:px-0">
       <div className="flex items-center gap-4 sm:gap-6 mb-2">
           <h2 className="text-lg sm:text-2xl font-black text-green-700 uppercase tracking-tighter bg-green-900/10 px-3 sm:px-4 py-1 font-pixel">
               SETTINGS
           </h2>
       </div>

       {/* Theme Selection */}
       <div className={`border-2 p-4 sm:p-8 relative ${isModern ? 'border-green-600 rounded-lg' : 'border-green-600'}`}>
           <h3 className={`absolute -top-4 left-4 px-2 sm:px-4 text-green-500 font-bold tracking-widest text-sm sm:text-lg uppercase ${isModern ? 'bg-[#0d1117]' : 'bg-[#050505]'}`}>
               THEME
           </h3>
           <div className="flex flex-wrap gap-3 sm:gap-4 mt-2">
               {(['PHOSPHOR GREEN', 'AMBER', 'CYAN', 'MODERN'] as const).map((theme) => {
                   const themeKey = theme.replace(' ', '_') as AppSettings['theme'];
                   const isSelected = settings.theme === themeKey;
                   const isModernBtn = theme === 'MODERN';
                   return (
                       <button
                            key={theme}
                            onClick={() => onUpdate('theme', themeKey)}
                            className={`
                                px-4 sm:px-6 py-2 sm:py-3 border-2 font-bold tracking-widest text-sm sm:text-base transition-all uppercase flex-grow min-w-[100px] sm:min-w-[140px]
                                ${isModernBtn ? 'rounded-lg' : ''}
                                ${isSelected
                                    ? isModernBtn
                                        ? 'border-green-400 bg-green-900/30 text-green-300'
                                        : 'border-green-400 bg-green-900/30 text-green-300 shadow-[0_0_20px_rgba(74,222,128,0.3)]'
                                    : 'border-green-900 text-green-800 hover:border-green-600 hover:text-green-500'
                                }
                            `}
                       >
                           {theme}
                       </button>
                   );
               })}
           </div>
       </div>

       {/* Effects List - Hidden for MODERN theme */}
       {!isModern && (
       <div className="border-2 border-green-600 p-4 sm:p-8 relative">
           <h3 className="absolute -top-4 left-4 bg-[#050505] px-2 sm:px-4 text-green-500 font-bold tracking-widest text-sm sm:text-lg uppercase">
               EFFECTS
           </h3>
           <div className="grid grid-cols-1 sm:grid-cols-2 gap-y-4 sm:gap-y-6 gap-x-6 sm:gap-x-12 mt-2">
               {effects.map((effect) => {
                   const isActive = settings[effect.id as keyof AppSettings];

                   return (
                       <button
                           key={effect.id}
                           onClick={() => onUpdate(effect.id as keyof AppSettings, !isActive)}
                           className="flex items-center gap-3 sm:gap-4 group text-left"
                       >
                           {/* Checkbox Visual */}
                           <div className={`
                                w-6 sm:w-8 h-6 sm:h-8 border-2 flex items-center justify-center transition-all flex-shrink-0
                                ${isActive
                                    ? 'bg-green-500 border-green-400 text-black shadow-[0_0_10px_#4ade80]'
                                    : 'border-green-800 bg-black group-hover:border-green-500'
                                }
                           `}>
                               {isActive && <Check className="w-4 sm:w-6 h-4 sm:h-6 stroke-[4]" />}
                           </div>

                           {/* Label */}
                           <div className={`text-base sm:text-xl font-bold tracking-wider transition-colors ${isActive ? 'text-green-300' : 'text-green-700 group-hover:text-green-500'}`}>
                               {effect.label}
                           </div>
                       </button>
                   );
               })}
           </div>
       </div>
       )}

       {/* Setup */}
       <SetupSection isModern={isModern} />

       {/* Alert Rules */}
       <AlertRulesSection isModern={isModern} />

       {/* Security — Passkey */}
       <SecuritySection isModern={isModern} />

       {/* Backups */}
       <BackupSection isModern={isModern} />

       {/* Diagnostics */}
       <DiagnosticsSection isModern={isModern} />

       {/* About */}
       <div className={`border-2 border-green-600 p-4 sm:p-8 relative ${isModern ? 'rounded-lg' : ''}`}>
            <h3 className={`absolute -top-4 left-4 px-2 sm:px-4 text-green-500 font-bold tracking-widest text-sm sm:text-lg uppercase ${isModern ? 'bg-[#0d1117]' : 'bg-[#050505]'}`}>
               ABOUT
            </h3>
            <div className="text-green-600 font-mono text-sm sm:text-lg space-y-2 mt-2 leading-relaxed">
                <p>Agent Tracker Web Console v0.1.0</p>
                <p>Built with React 19 + Tailwind CSS 4.0</p>
                <p>© 2026 HEYGO</p>
            </div>
       </div>
    </div>
  );
};

// Setup sub-component
const SETUP_COMMAND = 'curl -fsSL https://raw.githubusercontent.com/iocrazy/ai-tracker/main/scripts/setup.sh | bash';

const SetupSection: React.FC<{ isModern: boolean }> = ({ isModern }) => {
  const [status, setStatus] = useState<SetupStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [showToken, setShowToken] = useState(false);
  const [copiedCmd, setCopiedCmd] = useState(false);
  const [copiedToken, setCopiedToken] = useState(false);

  const token = getAuthToken() || '';

  const checkStatus = async () => {
    setLoading(true);
    try {
      const s = await fetchSetupStatus();
      setStatus(s);
    } catch {
      setStatus(null);
    }
    setLoading(false);
  };

  useEffect(() => { checkStatus(); }, []);

  const copyCmd = async () => {
    await navigator.clipboard.writeText(SETUP_COMMAND);
    setCopiedCmd(true);
    setTimeout(() => setCopiedCmd(false), 2000);
  };

  const copyToken = async () => {
    await navigator.clipboard.writeText(token);
    setCopiedToken(true);
    setTimeout(() => setCopiedToken(false), 2000);
  };

  const statusDot = (ok: boolean) => {
    const color = ok
      ? 'bg-green-500 shadow-[0_0_6px_#4ade80]'
      : 'bg-yellow-500 shadow-[0_0_6px_#eab308]';
    return <span className={`inline-block w-2.5 h-2.5 rounded-full ${color}`} />;
  };

  const maskedToken = token ? `${token.slice(0, 8)}${'*'.repeat(Math.min(token.length - 8, 24))}` : '(none)';

  return (
    <div className={`border-2 border-green-600 p-4 sm:p-8 relative ${isModern ? 'rounded-lg' : ''}`}>
      <h3 className={`absolute -top-4 left-4 px-2 sm:px-4 text-green-500 font-bold tracking-widest text-sm sm:text-lg uppercase ${isModern ? 'bg-[#0d1117]' : 'bg-[#050505]'}`}>
        <Terminal className="w-4 h-4 inline mr-2" />SETUP
      </h3>
      <div className="mt-2 space-y-4">
        {/* Status indicators */}
        <div className="space-y-2">
          <div className="flex items-center gap-3 font-mono text-xs">
            {statusDot(true)}
            <span className="text-green-400 font-bold w-28 uppercase">Server</span>
            <span className="text-green-600">Running</span>
          </div>
          <div className="flex items-center gap-3 font-mono text-xs">
            {statusDot(status?.claude_hooks_configured ?? false)}
            <span className="text-green-400 font-bold w-28 uppercase">Hooks</span>
            <span className="text-green-600">
              {status === null ? (loading ? 'Checking...' : 'Unknown') : status.claude_hooks_configured ? 'Configured' : 'Not configured'}
            </span>
          </div>
          <div className="flex items-center gap-3 font-mono text-xs">
            {statusDot(!!token)}
            <span className="text-green-400 font-bold w-28 uppercase">Token</span>
            <span className="text-green-600">{token ? 'Set' : 'Missing'}</span>
          </div>
        </div>

        {/* Auth token display */}
        <div className="space-y-1.5">
          <div className="text-green-500 text-xs font-bold tracking-wider uppercase">Auth Token</div>
          <div className="flex items-center gap-2">
            <code className="flex-1 bg-black/40 border border-green-900/40 px-2 py-1.5 rounded font-mono text-xs text-green-400 truncate">
              {showToken ? token : maskedToken}
            </code>
            <button
              onClick={() => setShowToken(!showToken)}
              className="p-1.5 bg-green-900/30 border border-green-700/40 rounded hover:bg-green-900/50 transition-colors"
              title={showToken ? 'Hide token' : 'Show token'}
            >
              {showToken ? <EyeOff className="w-3.5 h-3.5 text-green-500" /> : <Eye className="w-3.5 h-3.5 text-green-500" />}
            </button>
            <button
              onClick={copyToken}
              className="p-1.5 bg-green-900/30 border border-green-700/40 rounded hover:bg-green-900/50 transition-colors"
              title="Copy token"
            >
              {copiedToken ? <Check className="w-3.5 h-3.5 text-green-400" /> : <Copy className="w-3.5 h-3.5 text-green-500" />}
            </button>
          </div>
        </div>

        {/* Setup command */}
        <div className="space-y-1.5">
          <div className="text-green-500 text-xs font-bold tracking-wider uppercase">Setup Command</div>
          <div className="flex items-center gap-2">
            <code className="flex-1 bg-black/40 border border-green-900/40 px-2 py-1.5 rounded font-mono text-[10px] text-green-400 break-all select-all">
              {SETUP_COMMAND}
            </code>
            <button
              onClick={copyCmd}
              className="p-1.5 bg-green-900/30 border border-green-700/40 rounded hover:bg-green-900/50 transition-colors flex-shrink-0"
              title="Copy command"
            >
              {copiedCmd ? <Check className="w-3.5 h-3.5 text-green-400" /> : <Copy className="w-3.5 h-3.5 text-green-500" />}
            </button>
          </div>
        </div>

        {/* Refresh button */}
        <button
          onClick={checkStatus}
          disabled={loading}
          className="px-4 py-2 bg-green-900/30 border border-green-700/40 text-green-500 text-xs font-bold tracking-wider hover:bg-green-900/50 transition-colors disabled:opacity-50"
        >{loading ? 'CHECKING...' : 'REFRESH STATUS'}</button>
      </div>
    </div>
  );
};

// Alert Rules sub-component
const AlertRulesSection: React.FC<{ isModern: boolean }> = ({ isModern }) => {
  const [rules, setRules] = useState<AlertRule[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    fetchAlertRules().then(r => { setRules(r); setLoading(false); });
  }, []);

  const addRule = async (conditionType: string, name: string, threshold: number) => {
    const res = await createAlertRule(name, conditionType, threshold);
    if (res.success) {
      fetchAlertRules().then(r => setRules(r));
    }
  };

  const toggleRule = async (rule: AlertRule) => {
    await updateAlertRule(rule.id, { enabled: rule.enabled === 0 });
    fetchAlertRules().then(r => setRules(r));
  };

  const removeRule = async (id: number) => {
    await deleteAlertRule(id);
    setRules(prev => prev.filter(r => r.id !== id));
  };

  return (
    <div className={`border-2 border-green-600 p-4 sm:p-8 relative ${isModern ? 'rounded-lg' : ''}`}>
      <h3 className={`absolute -top-4 left-4 px-2 sm:px-4 text-green-500 font-bold tracking-widest text-sm sm:text-lg uppercase ${isModern ? 'bg-[#0d1117]' : 'bg-[#050505]'}`}>
        <Shield className="w-4 h-4 inline mr-2" />ALERT RULES
      </h3>
      <div className="mt-2 space-y-3">
        {loading ? (
          <div className="text-green-800 text-sm">Loading...</div>
        ) : (
          <>
            {rules.length === 0 && (
              <div className="text-green-800 text-sm">No alert rules configured. Add one below.</div>
            )}
            {rules.map(rule => (
              <div key={rule.id} className="flex items-center gap-3 py-2 border-b border-green-900/30">
                <button
                  onClick={() => toggleRule(rule)}
                  className={`w-8 h-5 rounded-full transition-colors flex items-center ${rule.enabled ? 'bg-green-600 justify-end' : 'bg-green-900/50 justify-start'}`}
                >
                  <span className={`w-3.5 h-3.5 rounded-full mx-0.5 ${rule.enabled ? 'bg-black' : 'bg-green-700'}`}></span>
                </button>
                <div className="flex-1 min-w-0">
                  <span className="text-green-400 text-sm font-mono">{rule.name}</span>
                  <span className="text-green-800 text-xs ml-2">
                    ({rule.condition_type}{rule.threshold_seconds ? `, ${Math.round(rule.threshold_seconds / 60)}m` : ''})
                  </span>
                </div>
                <span className="text-green-700 text-[10px] tracking-wider">{rule.channels.toUpperCase()}</span>
                <button onClick={() => removeRule(rule.id)} className="text-red-800 hover:text-red-500 p-1">
                  <Trash2 className="w-3.5 h-3.5" />
                </button>
              </div>
            ))}
            <div className="flex gap-2 mt-3">
              <button
                onClick={() => addRule('task_stuck', 'Task stuck >30m', 1800)}
                className="px-3 py-1.5 bg-green-900/30 border border-green-700/40 text-green-500 text-xs tracking-wider hover:bg-green-900/50 transition-colors"
              >+ TASK STUCK</button>
              <button
                onClick={() => addRule('session_idle', 'Session idle >1h', 3600)}
                className="px-3 py-1.5 bg-green-900/30 border border-green-700/40 text-green-500 text-xs tracking-wider hover:bg-green-900/50 transition-colors"
              >+ SESSION IDLE</button>
            </div>
          </>
        )}
      </div>
    </div>
  );
};

// Diagnostics sub-component
const DiagnosticsSection: React.FC<{ isModern: boolean }> = ({ isModern }) => {
  const [result, setResult] = useState<DiagnosticsResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [confirmRestart, setConfirmRestart] = useState(false);

  const runCheck = async () => {
    setLoading(true);
    try {
      const data = await fetchDiagnostics();
      setResult(data);
    } catch {
      setResult(null);
    }
    setLoading(false);
  };

  const handleClearLogs = async () => {
    const res = await adminClearLogs();
    if (res.success) {
      runCheck();
    }
  };

  const handleRestart = async () => {
    if (!confirmRestart) {
      setConfirmRestart(true);
      setTimeout(() => setConfirmRestart(false), 3000);
      return;
    }
    await adminRestart();
    setConfirmRestart(false);
  };

  const statusDot = (status: string) => {
    const color = status === 'ok' ? 'bg-green-500 shadow-[0_0_6px_#4ade80]'
      : status === 'warning' ? 'bg-yellow-500 shadow-[0_0_6px_#eab308]'
      : 'bg-red-500 shadow-[0_0_6px_#ef4444]';
    return <span className={`inline-block w-2.5 h-2.5 rounded-full ${color}`} />;
  };

  const overallColor = !result ? 'text-green-800'
    : result.status === 'healthy' ? 'text-green-400'
    : result.status === 'degraded' ? 'text-yellow-400'
    : 'text-red-400';

  return (
    <div className={`border-2 border-green-600 p-4 sm:p-8 relative ${isModern ? 'rounded-lg' : ''}`}>
      <h3 className={`absolute -top-4 left-4 px-2 sm:px-4 text-green-500 font-bold tracking-widest text-sm sm:text-lg uppercase ${isModern ? 'bg-[#0d1117]' : 'bg-[#050505]'}`}>
        <Activity className="w-4 h-4 inline mr-2" />DIAGNOSTICS
      </h3>
      <div className="mt-2 space-y-3">
        {/* Action buttons */}
        <div className="flex flex-wrap items-center gap-2">
          <button
            onClick={runCheck}
            disabled={loading}
            className="px-4 py-2 bg-green-900/30 border border-green-700/40 text-green-500 text-xs font-bold tracking-wider hover:bg-green-900/50 transition-colors disabled:opacity-50"
          >{loading ? 'CHECKING...' : 'RUN CHECK'}</button>
          <button
            onClick={handleClearLogs}
            className="px-4 py-2 bg-green-900/30 border border-green-700/40 text-green-500 text-xs font-bold tracking-wider hover:bg-green-900/50 transition-colors"
          >CLEAR LOGS</button>
          <button
            onClick={handleRestart}
            className={`px-4 py-2 border text-xs font-bold tracking-wider transition-colors ${
              confirmRestart
                ? 'bg-red-900/40 border-red-600/60 text-red-400 hover:bg-red-900/60'
                : 'bg-green-900/30 border-green-700/40 text-green-500 hover:bg-green-900/50'
            }`}
          >{confirmRestart ? 'CONFIRM RESTART?' : 'RESTART SERVER'}</button>
        </div>

        {/* Results */}
        {!result && !loading && (
          <div className="text-green-800 text-sm">Click RUN CHECK to diagnose system components.</div>
        )}
        {result && (
          <>
            <div className="flex items-center gap-3 text-xs font-mono">
              <span className={`font-bold uppercase ${overallColor}`}>{result.status}</span>
              <span className="text-green-800">{result.response_ms}ms</span>
              <span className="text-green-800 ml-auto">{new Date(result.timestamp).toLocaleTimeString()}</span>
            </div>
            <div className="space-y-1.5">
              {result.components.map(c => (
                <div key={c.name} className="flex items-center gap-3 py-1.5 border-b border-green-900/30 font-mono text-xs">
                  {statusDot(c.status)}
                  <span className="text-green-400 font-bold w-20 uppercase">{c.name}</span>
                  <span className="text-green-600 flex-1 truncate">{c.detail}</span>
                </div>
              ))}
            </div>
          </>
        )}
      </div>
    </div>
  );
};

// Backup sub-component
const BackupSection: React.FC<{ isModern: boolean }> = ({ isModern }) => {
  const [backups, setBackups] = useState<BackupEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [creating, setCreating] = useState(false);

  useEffect(() => {
    fetchBackups().then(b => { setBackups(b); setLoading(false); });
  }, []);

  const doBackup = async () => {
    setCreating(true);
    const res = await createBackup();
    setCreating(false);
    if (res.success) {
      fetchBackups().then(b => setBackups(b));
    }
  };

  const formatSize = (bytes: number) => {
    if (bytes < 1024) return `${bytes}B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)}MB`;
  };

  return (
    <div className={`border-2 border-green-600 p-4 sm:p-8 relative ${isModern ? 'rounded-lg' : ''}`}>
      <h3 className={`absolute -top-4 left-4 px-2 sm:px-4 text-green-500 font-bold tracking-widest text-sm sm:text-lg uppercase ${isModern ? 'bg-[#0d1117]' : 'bg-[#050505]'}`}>
        <Download className="w-4 h-4 inline mr-2" />BACKUPS
      </h3>
      <div className="mt-2 space-y-3">
        <div className="flex items-center gap-3">
          <button
            onClick={doBackup}
            disabled={creating}
            className="px-4 py-2 bg-green-900/30 border border-green-700/40 text-green-500 text-xs font-bold tracking-wider hover:bg-green-900/50 transition-colors disabled:opacity-50"
          >{creating ? 'CREATING...' : 'CREATE BACKUP NOW'}</button>
          <span className="text-green-800 text-xs">Auto-backup runs daily</span>
        </div>
        {loading ? (
          <div className="text-green-800 text-sm">Loading...</div>
        ) : backups.length === 0 ? (
          <div className="text-green-800 text-sm">No backups found.</div>
        ) : (
          <div className="space-y-1">
            {backups.slice(0, 10).map(b => (
              <div key={b.name} className="flex items-center gap-4 py-1.5 border-b border-green-900/30 font-mono text-xs">
                <span className="text-green-500">{b.name}</span>
                <span className="text-green-700">{formatSize(b.size)}</span>
                <span className="text-green-800 ml-auto">{b.created}</span>
              </div>
            ))}
          </div>
        )}
      </div>

    </div>
  );
};

// Security section component for Passkey management
const SecuritySection: React.FC<{ isModern: boolean }> = ({ isModern }) => {
  const [status, setStatus] = useState<'idle' | 'registering' | 'success' | 'error'>('idle');
  const [hasPasskey, setHasPasskey] = useState(false);
  const [message, setMessage] = useState('');

  useEffect(() => {
    import('../services/auth').then(({ checkPasskeyStatus }) => {
      checkPasskeyStatus().then(setHasPasskey);
    });
  }, []);

  const handleRegister = async () => {
    setStatus('registering');
    setMessage('');
    try {
      const { registerPasskey } = await import('../services/auth');
      const success = await registerPasskey();
      if (success) {
        setStatus('success');
        setMessage('Passkey registered successfully');
        setHasPasskey(true);
      } else {
        setStatus('error');
        setMessage('Registration failed or cancelled');
      }
    } catch (err) {
      setStatus('error');
      setMessage('Registration error');
    }
  };

  return (
    <div className={`border-2 p-4 sm:p-8 relative ${isModern ? 'border-green-600 rounded-lg' : 'border-green-600'}`}>
      <h3 className={`absolute -top-4 left-4 px-2 sm:px-4 text-green-500 font-bold tracking-widest text-sm sm:text-lg uppercase ${isModern ? 'bg-[#0d1117]' : 'bg-[#050505]'}`}>
        SECURITY
      </h3>
      <div className="mt-2 space-y-4">
        <div className="flex items-center justify-between">
          <div>
            <div className="text-green-400 font-mono text-sm font-bold">PASSKEY (WebAuthn)</div>
            <div className="text-green-800 font-mono text-xs mt-1">
              {hasPasskey ? 'Passkey registered — biometric login enabled' : 'No passkey registered — using token auth'}
            </div>
          </div>
          <button
            onClick={handleRegister}
            disabled={status === 'registering'}
            className={`px-4 py-2 border-2 font-mono text-sm font-bold tracking-wider transition-all ${
              isModern ? 'rounded-lg' : ''
            } ${
              hasPasskey
                ? 'border-green-800 text-green-700 hover:border-green-600 hover:text-green-500'
                : 'border-green-500 text-green-400 hover:bg-green-500 hover:text-black'
            } disabled:opacity-50 disabled:cursor-not-allowed`}
          >
            {status === 'registering' ? 'REGISTERING...' : hasPasskey ? 'ADD ANOTHER' : 'REGISTER PASSKEY'}
          </button>
        </div>
        {message && (
          <div className={`font-mono text-xs ${status === 'success' ? 'text-green-500' : 'text-red-400'}`}>
            {'>'} {message}
          </div>
        )}
      </div>
    </div>
  );
};
