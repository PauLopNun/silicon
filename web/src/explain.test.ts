import { describe, expect, it } from 'vitest';
import { demoMachine } from './testing/core';
import { blockOf, blocksAtWork, headline, memoryText, parseExplanation } from './explain';
import { buildTree, moduleAt } from './scene/tree';
import { netlistOf } from './testing/core';

describe('parseExplanation', () => {
  it('reads the head line and the signals', () => {
    const e = parseExplanation('3169\tld a, [x]\tLD_A_PTR\t1\nBASE_PAIR\taddress unit: base is the pair\nRD\tread memory this cycle');
    expect(e).toEqual({
      at: 3169,
      text: 'ld a, [x]',
      form: 'LD_A_PTR',
      step: 1,
      signals: [
        { name: 'BASE_PAIR', doc: 'address unit: base is the pair' },
        { name: 'RD', doc: 'read memory this cycle' },
      ],
    });
    expect(headline(e)).toBe('0c61  ld a, [x]  ·  step 1 (LD_A_PTR)');
  });

  it('knows when nothing has been read yet', () => {
    const e = parseExplanation('-1\t\tFETCH\t0\nIR_WE\tinstruction register ← data in');
    expect(e.at).toBeUndefined();
    expect(headline(e)).toMatch(/power-on/);
  });
});

describe('the real core', () => {
  const machine = demoMachine();
  machine.load('hello');
  const tree = buildTree(netlistOf(machine));

  it('explains every cycle with signals that belong to real blocks of the CPU', () => {
    for (let i = 0; i < 200; i++) {
      const e = parseExplanation(machine.explain());
      expect(e.signals.length).toBeGreaterThan(0);
      for (const { name, doc } of e.signals) {
        expect(doc.length).toBeGreaterThan(5);
        const block = blockOf(name);
        if (name === 'RD' || name === 'WR') expect(block).toBeUndefined();
        else expect(moduleAt(tree, block ?? 'none'), `${name} → ${block}`).toBeGreaterThan(0);
      }
      machine.run(1);
    }
  });

  it('walks instruction by instruction, each one just read', () => {
    machine.reset();
    const seen: string[] = [];
    for (let i = 0; i < 5; i++) {
      machine.step_instruction();
      const e = parseExplanation(machine.explain());
      expect(e.at).toBeDefined();
      expect(e.step).toBe(1);
      seen.push(e.text);
    }
    expect(seen.every((t) => t.length > 0)).toBe(true);
  });
});

describe('what a cycle sets to work', () => {
  it('lists each block once, and says what happens with memory', () => {
    const signals = ['AGU_CIN', 'RD', 'PC_WE', 'IR_WE', 'NS0'].map((name) => ({ name, doc: '' }));
    expect(blocksAtWork(signals)).toEqual(['cpu/agu', 'cpu/pc', 'cpu/ir', 'cpu/step']);
    expect(memoryText(signals)).toBe('reads the next opcode from memory');
    expect(memoryText([{ name: 'WR', doc: '' }])).toBe('writes a byte to memory');
    expect(memoryText([{ name: 'REG_WE', doc: '' }])).toBe('does not use memory');
  });
});
