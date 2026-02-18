import React, { useState } from 'react';
import { KeyRound, Loader2 } from 'lucide-react';
import { verifyToken, setAuthToken } from './shared/services/auth';

interface LoginViewProps {
  onLogin: () => void;
}

export const LoginView: React.FC<LoginViewProps> = ({ onLogin }) => {
  const [token, setToken] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!token.trim()) return;

    setLoading(true);
    setError('');

    const valid = await verifyToken(token.trim());
    if (valid) {
      await setAuthToken(token.trim());
      onLogin();
    } else {
      setError('Invalid token or server unreachable');
    }
    setLoading(false);
  };

  return (
    <div className="flex flex-col items-center justify-center h-full p-6 bg-neutral-900">
      <KeyRound className="w-8 h-8 text-neutral-400 mb-4" />
      <h2 className="text-sm font-medium text-neutral-200 mb-4">Connect to Agent Tracker</h2>
      <form onSubmit={handleSubmit} className="w-full space-y-3">
        <input
          type="password"
          value={token}
          onChange={e => setToken(e.target.value)}
          placeholder="Auth token"
          className="w-full px-3 py-2 bg-neutral-800 border border-neutral-700 rounded text-sm text-neutral-200 placeholder:text-neutral-500 outline-none focus:border-blue-500"
          autoFocus
        />
        {error && <p className="text-xs text-red-400">{error}</p>}
        <button
          type="submit"
          disabled={loading || !token.trim()}
          className="w-full py-2 bg-blue-600 hover:bg-blue-500 disabled:opacity-50 rounded text-sm text-white font-medium flex items-center justify-center gap-2"
        >
          {loading ? <Loader2 className="w-4 h-4 animate-spin" /> : null}
          Connect
        </button>
      </form>
      <p className="text-[10px] text-neutral-600 mt-4">localhost:3099</p>
    </div>
  );
};
