import { describe, expect, it } from 'vitest';
import { demoMachine, netlistOf, ramNetlist } from '../testing/core';
import { gateLevels } from './levels';
import { syntheticNetlist } from './synthetic';

describe('gateLevels', () => {
  it('counts the gates between a flip-flop or an input and each gate', () => {
    // g0 = nand(0, 1); g1 = nand(w2, w2); g2 = dff(w3); g3 = nand(w4, w2)
    const gates = Uint32Array.from([0, 0, 1, 2, 0, 0, 2, 2, 3, 0, 1, 3, 0, 4, 0, 0, 4, 2, 5, 0]);
    expect(Array.from(gateLevels(gates, 6))).toEqual([1, 2, 0, 2]);
  });

  it('agrees with the simulation core on the real CPU and on the memory made of gates', () => {
    const cpu = netlistOf(demoMachine());
    expect(Array.from(gateLevels(cpu.gates, cpu.wireCount))).toEqual(Array.from(cpu.levels));
    // The critical path of the CPU.
    expect(Math.max(...cpu.levels)).toBe(67);
    const ram = ramNetlist();
    expect(Array.from(gateLevels(ram.gates, ram.wireCount))).toEqual(Array.from(ram.levels));
  });

  it('refuses a table in which a gate comes before what feeds it', () => {
    // g0 reads the wire that g1, made after it, drives.
    const gates = Uint32Array.from([0, 3, 3, 2, 0, 0, 0, 1, 3, 0]);
    expect(() => gateLevels(gates, 4)).toThrow(/order/);
  });

  it('gives every gate of a made-up netlist a depth', () => {
    const net = syntheticNetlist({ depth: 3, fanout: 3, gatesPerLeaf: 12 });
    expect(net.levels.length * 5).toBe(net.gates.length);
    expect(Math.max(...net.levels)).toBeGreaterThan(2);
  });
});
