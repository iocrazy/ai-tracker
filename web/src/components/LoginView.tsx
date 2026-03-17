import React, { useEffect, useState } from 'react';
import { Fingerprint, KeyRound, ArrowRight, ShieldCheck, ShieldX } from 'lucide-react';
import { checkPasskeyStatus, loginWithPasskey } from '../services/auth';

interface LoginViewProps {
  onTokenSubmit: (token: string) => void;
  onPasskeyLogin: () => void;
  error?: string;
}

export const LoginView: React.FC<LoginViewProps> = ({ onTokenSubmit, onPasskeyLogin, error }) => {
  const [token, setToken] = useState('');
  const [hasPasskey, setHasPasskey] = useState(false);
  const [showTokenInput, setShowTokenInput] = useState(false);
  const [passkeyError, setPasskeyError] = useState('');
  const [isAuthenticating, setIsAuthenticating] = useState(false);
  useEffect(() => {
    checkPasskeyStatus().then(has => {
      setHasPasskey(has);
      if (!has) setShowTokenInput(true);
    });
  }, []);

  const handleTokenSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!token.trim()) return;
    onTokenSubmit(token.trim());
  };

  const handlePasskeyLogin = async () => {
    setPasskeyError('');
    setIsAuthenticating(true);
    try {
      const success = await loginWithPasskey();
      if (success) {
        onPasskeyLogin();
      } else {
        setPasskeyError('Authentication failed or cancelled');
      }
    } catch (err: any) {
      console.error('Passkey error:', err);
      setPasskeyError(err?.message || 'Passkey authentication error');
    } finally {
      setIsAuthenticating(false);
    }
  };

  return (
    <div className="flex items-center justify-center min-h-[100dvh] p-4" style={{ background: 'linear-gradient(135deg, #0a0a0f 0%, #0d1117 50%, #0a0f1a 100%)' }}>

      {/* Ambient glow */}
      <div className="fixed inset-0 pointer-events-none overflow-hidden">
        <div className="absolute top-[-20%] left-[-10%] w-[60%] h-[60%] rounded-full opacity-[0.03]" style={{ background: 'radial-gradient(circle, #3b82f6, transparent 70%)' }} />
        <div className="absolute bottom-[-20%] right-[-10%] w-[50%] h-[50%] rounded-full opacity-[0.04]" style={{ background: 'radial-gradient(circle, #8b5cf6, transparent 70%)' }} />
      </div>

      {/* Card */}
      <div className="relative max-w-[380px] w-full z-10">

        {/* Logo / Brand */}
        <div className="text-center mb-8">
          <div className="inline-flex items-center justify-center w-16 h-16 rounded-2xl mb-5 border border-white/[0.08] bg-white/[0.03]" style={{ boxShadow: '0 0 40px rgba(99, 102, 241, 0.08), inset 0 1px 0 rgba(255,255,255,0.05)' }}>
            <ShieldCheck className="w-8 h-8 text-indigo-400" />
          </div>
          <h1 className="text-[22px] font-semibold text-white/90 tracking-[-0.02em] mb-1.5">
            Agent Tracker
          </h1>
          <p className="text-white/30 text-[13px]">
            Sign in to continue
          </p>
        </div>

        {/* Main card */}
        <div className="rounded-2xl border border-white/[0.06] p-6 backdrop-blur-xl" style={{ background: 'linear-gradient(180deg, rgba(255,255,255,0.03) 0%, rgba(255,255,255,0.01) 100%)', boxShadow: '0 20px 60px rgba(0,0,0,0.3), inset 0 1px 0 rgba(255,255,255,0.04)' }}>

          <div className="space-y-4">
            {/* Passkey button */}
            {hasPasskey && (
              <button
                onClick={handlePasskeyLogin}
                disabled={isAuthenticating}
                className="w-full group relative overflow-hidden rounded-xl py-3.5 px-4 font-medium text-[15px] transition-all duration-200 disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2.5 text-white border border-white/[0.08] hover:border-white/[0.15]"
                style={{ background: 'linear-gradient(180deg, rgba(99, 102, 241, 0.15) 0%, rgba(99, 102, 241, 0.08) 100%)' }}
              >
                <Fingerprint className="w-[18px] h-[18px] text-indigo-400" />
                <span>{isAuthenticating ? 'Authenticating...' : 'Sign in with Passkey'}</span>
                {!isAuthenticating && (
                  <ArrowRight className="w-4 h-4 text-white/40 group-hover:text-white/60 transition-colors ml-auto" />
                )}
              </button>
            )}

            {passkeyError && (
              <div className="rounded-lg bg-red-500/[0.08] border border-red-500/[0.15] px-3.5 py-3 flex items-center gap-3">
                <ShieldX className="w-5 h-5 text-red-400 flex-shrink-0" />
                <span className="text-red-400/90 text-[13px]">{passkeyError}</span>
              </div>
            )}

            {/* Divider */}
            {hasPasskey && !showTokenInput && (
              <div className="relative py-1">
                <div className="absolute inset-0 flex items-center">
                  <div className="w-full border-t border-white/[0.06]" />
                </div>
                <div className="relative flex justify-center">
                  <button
                    onClick={() => setShowTokenInput(true)}
                    className="bg-[#0d1117] px-3 text-white/20 hover:text-white/40 text-[12px] transition-colors flex items-center gap-1.5"
                  >
                    <KeyRound className="w-3 h-3" />
                    or use token
                  </button>
                </div>
              </div>
            )}

            {/* Token input */}
            {showTokenInput && (
              <form onSubmit={handleTokenSubmit} className="space-y-3">
                {hasPasskey && (
                  <div className="flex items-center gap-2 py-1">
                    <div className="flex-1 h-px bg-white/[0.06]" />
                    <span className="text-white/20 text-[11px] uppercase tracking-wider">Token</span>
                    <div className="flex-1 h-px bg-white/[0.06]" />
                  </div>
                )}

                <div>
                  <input
                    type="text"
                    value={token}
                    onChange={(e) => setToken(e.target.value)}
                    className="w-full rounded-xl border border-white/[0.08] bg-white/[0.03] text-white/90 px-4 py-3 text-[14px] placeholder-white/20 focus:outline-none focus:border-indigo-500/40 focus:ring-1 focus:ring-indigo-500/20 transition-all"
                    placeholder="Paste auth token..."
                    autoFocus={!hasPasskey}
                    spellCheck={false}
                    autoComplete="off"
                  />
                </div>

                {error && (
                  <div className="rounded-lg bg-red-500/[0.08] border border-red-500/[0.15] px-3.5 py-3 flex items-center gap-3">
                    <ShieldX className="w-5 h-5 text-red-400 flex-shrink-0" />
                    <span className="text-red-400/90 text-[13px]">{error}</span>
                  </div>
                )}

                <button
                  type="submit"
                  disabled={!token.trim()}
                  className="w-full rounded-xl py-3 px-4 font-medium text-[14px] text-white/80 border border-white/[0.08] hover:border-white/[0.15] bg-white/[0.04] hover:bg-white/[0.06] transition-all disabled:opacity-30 disabled:cursor-not-allowed flex items-center justify-center gap-2"
                >
                  Continue
                  <ArrowRight className="w-4 h-4" />
                </button>
              </form>
            )}
          </div>
        </div>

        {/* Footer */}
        <div className="mt-5 text-center">
          <p className="text-white/15 text-[11px] tracking-wide">
            Protected by WebAuthn
          </p>
        </div>
      </div>
    </div>
  );
};
