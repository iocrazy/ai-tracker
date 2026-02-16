import { formatDuration, formatTime } from '../services/helpers';

describe('formatDuration', () => {
  it('formats seconds', () => {
    expect(formatDuration(0)).toBe('0s');
    expect(formatDuration(30)).toBe('30s');
    expect(formatDuration(59)).toBe('59s');
  });

  it('formats minutes', () => {
    expect(formatDuration(60)).toBe('1m');
    expect(formatDuration(90)).toBe('1m');
    expect(formatDuration(3599)).toBe('59m');
  });

  it('formats hours and minutes', () => {
    expect(formatDuration(3600)).toBe('1h0m');
    expect(formatDuration(3661)).toBe('1h1m');
    expect(formatDuration(7200)).toBe('2h0m');
    expect(formatDuration(86400)).toBe('24h0m');
  });
});

describe('formatTime', () => {
  it('returns --:-- for null', () => {
    expect(formatTime(null)).toBe('--:--');
  });

  it('formats ISO string to HH:MM', () => {
    const result = formatTime('2026-02-15T14:30:00Z');
    // Result depends on locale/timezone, but should match HH:MM pattern
    expect(result).toMatch(/^\d{2}:\d{2}$/);
  });
});
