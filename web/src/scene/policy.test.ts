import { describe, expect, it } from 'vitest';
import { engineFor, engineNote, parseEngineMode } from './policy';

describe('engineFor', () => {
  it('uses the gate engine exactly while gates are plainly in sight', () => {
    expect(engineFor('auto', 0)).toBe('fast');
    expect(engineFor('auto', 0.3)).toBe('fast');
    expect(engineFor('auto', 0.5)).toBe('gate');
    expect(engineFor('auto', 1)).toBe('gate');
  });

  it('runs the gates while any gate is broken, whatever was asked for', () => {
    for (const mode of ['auto', 'fast', 'gate'] as const) {
      expect(engineFor(mode, 0, 1)).toBe('gate');
      expect(engineFor(mode, 0, 3)).toBe('gate');
    }
    expect(engineNote('fast', 'gate', 0, 1)).toMatch(/^A gate is broken/);
    expect(engineNote('auto', 'gate', 0, 3)).toMatch(/^3 gates are broken/);
  });

  it('does as it is told when an engine was asked for', () => {
    for (const inside of [0, 0.3, 1]) {
      expect(engineFor('gate', inside)).toBe('gate');
      expect(engineFor('fast', inside)).toBe('fast');
    }
  });
});

describe('parseEngineMode', () => {
  it('reads what a link asks for and takes anything else as automatic', () => {
    expect(parseEngineMode('gate')).toBe('gate');
    expect(parseEngineMode('fast')).toBe('fast');
    expect(parseEngineMode('auto')).toBe('auto');
    expect(parseEngineMode('GATE')).toBe('auto');
    expect(parseEngineMode('')).toBe('auto');
    expect(parseEngineMode(null)).toBe('auto');
  });
});

describe('engineNote', () => {
  it('says why things are slow, or that the wires shown are not live', () => {
    expect(engineNote('auto', 'gate', 1)).toMatch(/looking inside the CPU/);
    expect(engineNote('gate', 'gate', 0)).toMatch(/every gate is being simulated/i);
    expect(engineNote('fast', 'fast', 0.4)).toMatch(/as the gate engine left them/);
    expect(engineNote('auto', 'fast', 0)).toMatch(/Zoom into a block/);
    expect(engineNote('fast', 'fast', 0)).toMatch(/full speed/);
  });
});
