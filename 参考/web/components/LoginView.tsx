import React, { useState } from 'react';
import { ShieldCheck, Lock, ChevronRight } from 'lucide-react';

interface LoginViewProps {
  onLogin: (username: string) => void;
}

export const LoginView: React.FC<LoginViewProps> = ({ onLogin }) => {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [bootSequence, setBootSequence] = useState<string[]>([]);
  const [isBooting, setIsBooting] = useState(false);

  const handleLogin = (e: React.FormEvent) => {
    e.preventDefault();
    if (!username.trim()) return;

    setIsBooting(true);
    
    // Fake boot sequence
    const steps = [
      "INITIATING_HANDSHAKE...",
      "VERIFYING_CREDENTIALS...",
      `USER_ID: ${username.toUpperCase()} CONFIRMED`,
      "DECRYPTING_SECURE_CHANNEL...",
      "LOADING_MODULES: [MONITOR, TIMELINE, CONSOLE]",
      "ESTABLISHING_UPLINK...",
      "ACCESS_GRANTED."
    ];

    let delay = 0;
    steps.forEach((step, index) => {
      // Randomize delay slightly for realism
      delay += Math.random() * 300 + 300;
      setTimeout(() => {
        setBootSequence(prev => [...prev, step]);
        if (index === steps.length - 1) {
          setTimeout(() => onLogin(username), 800);
        }
      }, delay);
    });
  };

  return (
    <div className="flex flex-col items-center justify-center min-h-[calc(100vh-100px)] p-4 relative z-20">
      <div className="max-w-md w-full border-2 border-green-800 bg-black/80 p-8 shadow-[0_0_50px_rgba(34,197,94,0.1)] relative overflow-hidden">
        
        {/* Decor */}
        <div className="absolute top-0 left-0 w-full h-1 bg-green-900/50"></div>
        <div className="absolute bottom-0 left-0 w-full h-1 bg-green-900/50"></div>
        
        <div className="text-center mb-10">
            <div className="inline-block p-4 border-2 border-green-500 rounded-full mb-4 shadow-[0_0_15px_rgba(34,197,94,0.5)]">
                <Lock className="w-12 h-12 text-green-500 animate-pulse" />
            </div>
            <h1 className="text-5xl font-black text-green-500 tracking-tighter retro-text-shadow uppercase font-['VT323'] mb-2">
                SYSTEM_ACCESS
            </h1>
            <p className="text-green-800 font-mono text-sm tracking-widest">RESTRICTED AREA // AUTHORIZED PERSONNEL ONLY</p>
        </div>

        {isBooting ? (
            <div className="font-mono text-green-400 h-[240px] border border-green-900/50 bg-black/50 p-4 overflow-hidden flex flex-col justify-end shadow-inner">
                {bootSequence.map((log, i) => (
                    <div key={i} className="mb-1 text-base animate-[fadeIn_0.1s]">> {log}</div>
                ))}
                <div className="animate-pulse text-green-500 font-bold">_</div>
            </div>
        ) : (
            <form onSubmit={handleLogin} className="space-y-6">
                <div>
                    <label className="block text-green-700 text-xs font-bold mb-2 tracking-widest uppercase font-mono">
                        OPERATOR_ID
                    </label>
                    <div className="relative group">
                        <ChevronRight className="absolute left-3 top-3.5 w-5 h-5 text-green-800 group-focus-within:text-green-500 transition-colors" />
                        <input
                            type="text"
                            value={username}
                            onChange={(e) => setUsername(e.target.value)}
                            className="w-full bg-black border-2 border-green-900 text-green-400 pl-10 pr-4 py-3 font-mono text-xl focus:outline-none focus:border-green-500 focus:shadow-[0_0_15px_rgba(34,197,94,0.2)] transition-all placeholder-green-900/30 uppercase"
                            placeholder="ENTER_ID..."
                            autoFocus
                        />
                    </div>
                </div>

                <div>
                    <label className="block text-green-700 text-xs font-bold mb-2 tracking-widest uppercase font-mono">
                        ACCESS_KEY
                    </label>
                    <div className="relative group">
                         <ShieldCheck className="absolute left-3 top-3.5 w-5 h-5 text-green-800 group-focus-within:text-green-500 transition-colors" />
                        <input
                            type="password"
                            value={password}
                            onChange={(e) => setPassword(e.target.value)}
                            className="w-full bg-black border-2 border-green-900 text-green-400 pl-10 pr-4 py-3 font-mono text-xl focus:outline-none focus:border-green-500 focus:shadow-[0_0_15px_rgba(34,197,94,0.2)] transition-all placeholder-green-900/30"
                            placeholder="••••••••"
                        />
                    </div>
                </div>

                <button
                    type="submit"
                    className="w-full bg-green-900/20 border-2 border-green-600 text-green-500 py-4 font-bold tracking-[0.2em] uppercase hover:bg-green-500 hover:text-black hover:shadow-[0_0_20px_rgba(34,197,94,0.6)] transition-all mt-4 text-lg"
                >
                    INITIALIZE_LINK
                </button>
            </form>
        )}
        
        <div className="mt-8 text-center border-t border-green-900/30 pt-4">
             <p className="text-[10px] text-green-900 uppercase font-mono tracking-wider">
                SECURE CONNECTION v4.0.2 // ENCRYPTION: AES-256
             </p>
        </div>
      </div>
    </div>
  );
};