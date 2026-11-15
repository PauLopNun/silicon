import { describe, expect, it } from 'vitest';
import { engineFor, engineNote, parseEngineMode, SAMPLE_CYCLES, samplingFor, type EngineNeeds } from './policy';

const far: EngineNeeds = { inside: 0, broken: 0, paused: false };

describe('engineFor', () => {
  it('uses the gate engine exactly while gates are plainly in sight', () => {
    expect(engineFor('auto', far)).toBe('fast');
    expect(engineFor('auto', { ...far, inside: 0.3 })).toBe('fast');
    expect(engineFor('auto', { ...far, inside: 0.5 })).toBe('gate');
    expect(engineFor('auto', { ...far, inside: 1 })).toBe('gate');
  });

  it('uses the gate engine while the clock is stopped, when left to itself', () => {
    expect(engineFor('auto', { ...far, paused: true })).toBe('gate');
    // An engine that was asked for stays, stopped or not.
    expect(engineFor('fast', { ...far, paused: true })).toBe('fast');
    expect(engineFor('gate', { ...far, paused: true })).toBe('gate');
  });

  it('runs the gates while any gate is broken, whatever was asked for', () => {
    for (const mode of ['auto', 'fast', 'gate'] as const) {
      expect(engineFor(mode, { ...far, broken: 1 })).toBe('gate');
      expect(engineFor(mode, { ...far, broken: 3 })).toBe('gate');
    }
  });

  it('does as it is told when an engine was asked for', () => {
    for (const inside of [0, 0.3, 1]) {
      expect(engineFor('gate', { ...far, inside })).toBe('gate');
      expect(engineFor('fast', { ...far, inside })).toBe('fast');
    }
  });
});

describe('samplingFor', () => {
  it('samples the gates only when left to itself with the CPU on view', () => {
    expect(samplingFor('auto', true)).toBe(SAMPLE_CYCLES);
    expect(samplingFor('auto', false)).toBe(0);
    // The fast engine alone was asked for: no gate is computed.
    expect(samplingFor('fast', true)).toBe(0);
    // On the gate engine every cycle is on the gates already.
    expect(samplingFor('gate', true)).toBe(0);
  });

  it('takes a small share of a frame', () => {
    // Against the 66,667 cycles the program runs in a frame at 4 MHz.
    expect(SAMPLE_CYCLES).toBeGreaterThanOrEqual(64);
    expect(SAMPLE_CYCLES / 66_667).toBeLessThan(0.01);
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
  it('says what is computed, and why', () => {
    expect(engineNote('auto', 'gate', 'gate', { ...far, inside: 1 })).toMatch(/looking at gates/);
    expect(engineNote('auto', 'gate', 'gate', { ...far, paused: true })).toMatch(/clock is stopped/);
    expect(engineNote('gate', 'gate', 'gate', far)).toMatch(/^Every gate is computed/);
    expect(engineNote('auto', 'fast', 'sample', far)).toMatch(new RegExp(`${SAMPLE_CYCLES} clock cycles of every frame`));
    expect(engineNote('auto', 'fast', 'sample', far)).toMatch(/a sample/);
    expect(engineNote('fast', 'fast', 'old', { ...far, inside: 0.4 })).toMatch(/as they were last/);
  });

  it('says first of all that a gate is broken', () => {
    expect(engineNote('fast', 'gate', 'gate', { ...far, broken: 1 })).toMatch(/^A gate is broken/);
    expect(engineNote('auto', 'gate', 'gate', { ...far, broken: 3, paused: true })).toMatch(/^3 gates are broken/);
  });
});
