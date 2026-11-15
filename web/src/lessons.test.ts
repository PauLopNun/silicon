import { describe, expect, it } from 'vitest';
import { LESSON, lessonPlaces, type LessonContext } from './lessons';
import { buildTree, moduleAt, resolvePath } from './scene/tree';
import { demoMachine, netlistOf, ramNetlist } from './testing/core';

const cpu = buildTree(netlistOf(demoMachine()));
const ram = buildTree(ramNetlist());

/** The gates of a module, by kind. */
const count = (tree: typeof cpu, path: string, kind: 0 | 1) =>
  Array.from(tree.gates[moduleAt(tree, path)]!).filter((g) => tree.kindOfGate[g] === kind).length;

describe('the lesson', () => {
  it('names only places that are there', () => {
    for (const { view, at } of lessonPlaces()) {
      expect(resolvePath(view === 'ram' ? ram : cpu, at), `${view}: ${at}`).toBeDefined();
    }
  });

  it('states the design as it is', () => {
    // "Nine NAND gates make a full adder."
    for (let i = 0; i < 8; i++) {
      expect(count(cpu, `cpu/alu/adder8/fa${i}`, 0)).toBe(9);
      expect(count(cpu, `cpu/alu/adder8/fa${i}`, 1)).toBe(0);
    }
    // "Eight full adders side by side", fa0 to fa7.
    const adder = moduleAt(cpu, 'cpu/alu/adder8');
    expect(cpu.children[adder]!.map((c) => cpu.names[c])).toEqual(['fa0', 'fa1', 'fa2', 'fa3', 'fa4', 'fa5', 'fa6', 'fa7']);
    // "Register a is eight flip-flops."
    expect(count(cpu, 'cpu/regs/a', 1)).toBe(8);
    // "The register file has eight of these."
    for (const r of ['a', 'b', 'xl', 'xh', 'yl', 'yh', 'zl', 'zh']) expect(count(cpu, `cpu/regs/${r}`, 1)).toBe(8);
    // "the 127 flip-flops that hold its state".
    expect(Array.from(cpu.kindOfGate).filter((k) => k === 1).length).toBe(127);
    // The ALU's parts, and control's.
    const names = (path: string) => cpu.children[moduleAt(cpu, path)]!.map((c) => cpu.names[c]);
    expect(names('cpu/alu')).toEqual(expect.arrayContaining(['xsel', 'ysel', 'yinv', 'logic', 'mux', 'status']));
    expect(names('cpu/control')).toEqual(expect.arrayContaining(['decode', 'lines', 'plane']));
    // The memory: eight words, a decoder and a read tree.
    const words = ram.children[0]!.map((c) => ram.names[c]);
    expect(words).toEqual(expect.arrayContaining(['decode', 'read', 'word0', 'word7']));
    expect(count(ram, 'ram/word5', 1)).toBe(4);
  });

  it('has tasks that are not done before anything is done', () => {
    const fresh: LessonContext = {
      focus: 'cpu',
      showing: 'cpu',
      paused: false,
      engine: 'fast',
      chosen: '',
      broken: 0,
      ramOut: 0,
      ramWords: [0, 0, 0, 0, 0, 0, 0, 0],
      stepped: 0,
    };
    for (const step of LESSON) {
      if (step.task) expect(step.task.done(fresh), step.title).toBe(false);
    }
    const done = (title: string, c: Partial<LessonContext>) => LESSON.find((s) => s.title === title)!.task!.done({ ...fresh, ...c });
    expect(done('One gate', { chosen: 'cpu/alu/adder8/fa0/nand3' })).toBe(true);
    expect(done('One gate', { chosen: 'cpu/regs/a/dff0' })).toBe(false);
    expect(done('Memory', { ramWords: [0, 0, 0, 0, 0, 0b1010, 0, 0] })).toBe(true);
    expect(done('One instruction at a time', { stepped: 3 })).toBe(true);
    expect(done('Break it', { broken: 1 })).toBe(true);
  });
});
