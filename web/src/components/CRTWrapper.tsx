import React from 'react';
import { AppSettings } from '../types';

interface CRTWrapperProps {
  children: React.ReactNode;
  settings: AppSettings;
}

export const CRTWrapper: React.FC<CRTWrapperProps> = ({ children, settings }) => {

  // Modern theme: clean render, no CRT effects
  if (settings.theme === 'MODERN') {
    return (
      <div className="relative min-h-screen w-full bg-[#0d1117] overflow-hidden">
        <div className="relative z-10 h-full w-full p-4 md:p-8">
          {children}
        </div>
      </div>
    );
  }

  // CRT themes: hue rotation + effects
  const getThemeFilter = () => {
      switch (settings.theme) {
          case 'AMBER': return 'hue-rotate(-105deg) saturate(1.2) sepia(0.2)';
          case 'CYAN': return 'hue-rotate(45deg) saturate(1.1)';
          default: return 'none';
      }
  };

  return (
    <div
        className={`relative min-h-screen w-full bg-[#050505] overflow-hidden ${settings.rgbShift ? 'rgb-shifted' : ''} ${!settings.glow ? 'glow-disabled' : ''}`}
        style={{ filter: getThemeFilter() }}
    >

      {/* 3D Grid Effect */}
      {settings.perspectiveGrid && (
        <div className="perspective-grid animate-[pulse_4s_infinite]"></div>
      )}

      {/* Content Layer */}
      <div className={`relative z-10 h-full w-full p-4 md:p-8 ${settings.flicker ? 'crt-flicker' : ''}`}>
        {children}
      </div>

      {/* Visual Effects Layer - Fixed position to cover screen while scrolling */}
      <div className="pointer-events-none fixed inset-0 z-50 h-full w-full">
        {/* Scanlines */}
        {settings.scanlines && (
             <div className="scanlines absolute inset-0 opacity-10"></div>
        )}

        {/* Noise */}
        {settings.noise && (
             <div className="noise-overlay"></div>
        )}

        {/* Vignette & Glow */}
        <div className={`absolute inset-0 bg-[radial-gradient(circle_at_center,transparent_50%,rgba(0,0,0,0.6)_100%)] ${settings.glow ? 'shadow-[inset_0_0_100px_rgba(0,0,0,0.9)] drop-shadow-[0_0_15px_rgba(74,222,128,0.2)]' : ''}`}></div>
      </div>
    </div>
  );
};
