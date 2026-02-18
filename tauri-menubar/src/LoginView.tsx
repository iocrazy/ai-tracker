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
    <div className="flex flex-col items-center justify-center h-full p-6 rounded-[10px] overflow-hidden">
      <KeyRound className="w-7 h-7 text-gray-400 mb-3" />
      <h2 className="text-[13px] font-medium text-gray-700 mb-4">Connect to Agent Tracker</h2>
      <form onSubmit={handleSubmit} className="w-full space-y-3">
        <input
          type="password"
          value={token}
          onChange={e => setToken(e.target.value)}
          placeholder="Auth token"
          className="w-full px-3 py-1.5 bg-white/60 border border-black/10 rounded-md text-[12px] text-gray-800 placeholder:text-gray-400 outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500/30"
          autoFocus
        />
        {error && <p className="text-[11px] text-red-500">{error}</p>}
        <button
          type="submit"
          disabled={loading || !token.trim()}
          className="w-full py-1.5 bg-blue-500 hover:bg-blue-600 disabled:opacity-50 rounded-md text-[12px] text-white font-medium flex items-center justify-center gap-2"
        >
          {loading ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : null}
          Connect
        </button>
      </form>
      <p className="text-[10px] text-gray-400 mt-3">localhost:3099</p>
    </div>
  );
};
