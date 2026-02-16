import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { SettingsView } from '../components/SettingsView';

// Mock API calls used by sub-components
vi.mock('../services/api', () => ({
  fetchAlertRules: vi.fn().mockResolvedValue([]),
  createAlertRule: vi.fn(),
  updateAlertRule: vi.fn(),
  deleteAlertRule: vi.fn(),
  fetchBackups: vi.fn().mockResolvedValue([]),
  createBackup: vi.fn(),
}));

const defaultSettings = {
  theme: 'PHOSPHOR_GREEN' as const,
  scanlines: true,
  flicker: false,
  glow: true,
  noise: false,
  rgbShift: false,
  perspectiveGrid: false,
};

describe('SettingsView', () => {
  it('renders the SETTINGS heading', () => {
    render(<SettingsView settings={defaultSettings} onUpdate={vi.fn()} />);
    expect(screen.getByText('SETTINGS')).toBeInTheDocument();
  });

  it('renders theme buttons', () => {
    render(<SettingsView settings={defaultSettings} onUpdate={vi.fn()} />);
    expect(screen.getByText('PHOSPHOR GREEN')).toBeInTheDocument();
    expect(screen.getByText('AMBER')).toBeInTheDocument();
    expect(screen.getByText('CYAN')).toBeInTheDocument();
    expect(screen.getByText('MODERN')).toBeInTheDocument();
  });

  it('renders effects section for non-MODERN theme', () => {
    render(<SettingsView settings={defaultSettings} onUpdate={vi.fn()} />);
    expect(screen.getByText('Scanlines')).toBeInTheDocument();
    expect(screen.getByText('Glow Effects')).toBeInTheDocument();
  });

  it('hides effects section for MODERN theme', () => {
    const modernSettings = { ...defaultSettings, theme: 'MODERN' as const };
    render(<SettingsView settings={modernSettings} onUpdate={vi.fn()} />);
    expect(screen.queryByText('Scanlines')).not.toBeInTheDocument();
  });
});
