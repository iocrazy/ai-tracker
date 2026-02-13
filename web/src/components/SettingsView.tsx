import React from 'react';
import { AppSettings } from '../types';
import { Check } from 'lucide-react';

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
    <div className="flex flex-col gap-4 sm:gap-8 max-w-5xl mx-auto pt-4 pb-10 px-2 sm:px-0">
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
