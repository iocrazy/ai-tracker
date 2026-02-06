import React from 'react';
import { AppSettings } from '../types';
import { Check } from 'lucide-react';

interface SettingsViewProps {
    settings: AppSettings;
    onUpdate: (key: keyof AppSettings, value: any) => void;
}

export const SettingsView: React.FC<SettingsViewProps> = ({ settings, onUpdate }) => {
  
  const effects = [
      { id: 'scanlines', label: 'Scanlines' },
      { id: 'flicker', label: 'Flicker Effect' },
      { id: 'glow', label: 'Glow Effects' },
      { id: 'noise', label: 'Signal Noise' },
      { id: 'rgbShift', label: 'RGB Shift' },
      { id: 'perspectiveGrid', label: '3D Grid' }
  ];

  return (
    <div className="flex flex-col gap-8 max-w-5xl mx-auto pt-4 pb-10">
       <div className="flex items-center gap-6 mb-2">
           <h2 className="text-4xl font-black text-green-700 uppercase tracking-tighter bg-green-900/10 px-4 py-1">
               SETTINGS
           </h2>
       </div>

       {/* Theme Selection */}
       <div className="border-2 border-green-600 p-8 relative">
           <h3 className="absolute -top-4 left-4 bg-[#050505] px-4 text-green-500 font-bold tracking-widest text-lg uppercase">
               THEME
           </h3>
           <div className="flex flex-wrap gap-6 mt-2">
               {['PHOSPHOR GREEN', 'AMBER', 'CYAN'].map((theme) => {
                   const themeKey = theme.replace(' ', '_') as 'PHOSPHOR_GREEN' | 'AMBER' | 'CYAN';
                   const isSelected = settings.theme === themeKey;
                   return (
                       <button
                            key={theme}
                            onClick={() => onUpdate('theme', themeKey)}
                            className={`
                                px-8 py-4 border-2 font-bold tracking-widest text-xl transition-all uppercase flex-grow md:flex-grow-0 min-w-[200px]
                                ${isSelected 
                                    ? 'border-green-400 bg-green-900/30 text-green-300 shadow-[0_0_20px_rgba(74,222,128,0.3)]' 
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

       {/* Effects List */}
       <div className="border-2 border-green-600 p-8 relative">
           <h3 className="absolute -top-4 left-4 bg-[#050505] px-4 text-green-500 font-bold tracking-widest text-lg uppercase">
               EFFECTS
           </h3>
           <div className="grid grid-cols-1 md:grid-cols-2 gap-y-6 gap-x-12 mt-2">
               {effects.map((effect) => {
                   const isActive = settings[effect.id as keyof AppSettings];
                   
                   return (
                       <button 
                           key={effect.id}
                           onClick={() => onUpdate(effect.id as keyof AppSettings, !isActive)}
                           className="flex items-center gap-4 group text-left"
                       >
                           {/* Checkbox Visual */}
                           <div className={`
                                w-8 h-8 border-2 flex items-center justify-center transition-all
                                ${isActive 
                                    ? 'bg-green-500 border-green-400 text-black shadow-[0_0_10px_#4ade80]' 
                                    : 'border-green-800 bg-black group-hover:border-green-500'
                                }
                           `}>
                               {isActive && <Check className="w-6 h-6 stroke-[4]" />}
                           </div>

                           {/* Label */}
                           <div className={`text-xl font-bold tracking-wider transition-colors ${isActive ? 'text-green-300' : 'text-green-700 group-hover:text-green-500'}`}>
                               {effect.label}
                           </div>
                       </button>
                   );
               })}
           </div>
       </div>
       
       {/* About */}
       <div className="border-2 border-green-600 p-8 relative">
            <h3 className="absolute -top-4 left-4 bg-[#050505] px-4 text-green-500 font-bold tracking-widest text-lg uppercase">
               ABOUT
            </h3>
            <div className="text-green-600 font-mono text-lg space-y-2 mt-2 leading-relaxed">
                <p>Agent Tracker Web Console v0.1.0</p>
                <p>Built with React 19 + Tailwind CSS 4.0</p>
                <p>© 2026 HEYGO</p>
            </div>
       </div>
    </div>
  );
};