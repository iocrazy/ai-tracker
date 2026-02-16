import { describe, it, expect } from 'vitest';
import { INITIAL_CONSOLE_LOGS } from '../constants';

describe('constants', () => {
  it('exports INITIAL_CONSOLE_LOGS with expected shape', () => {
    expect(INITIAL_CONSOLE_LOGS).toHaveLength(3);
    expect(INITIAL_CONSOLE_LOGS[0]).toEqual({
      id: '1',
      type: 'system',
      text: '> Initializing console...',
    });
  });
});
