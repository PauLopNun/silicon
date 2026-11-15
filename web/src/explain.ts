/**
 * What the CPU does in one clock cycle, for people: the instruction it is
 * running, which step of it, and which control signals the control unit
 * raises, with the blocks of the CPU each one sets to work.
 *
 * The facts come from the simulation core (`Machine.explain`, which reads the
 * microcode table the control unit's gates are tested against). This file
 * only reads them and arranges them.
 */
import { hex } from './protocol';

export interface Signal {
  /** `PC_WE` */
  name: string;
  /** `pc ← base + offset` */
  doc: string;
}

export interface Explanation {
  /** Address of the running instruction's opcode; undefined until one has been read. */
  at: number | undefined;
  /** The instruction as assembly: `ld a, [x]`. Empty until one has been read. */
  text: string;
  /** Its form in the microcode table, or `FETCH` while an opcode is being read. */
  form: string;
  step: number;
  /** The control signals raised in this cycle. */
  signals: Signal[];
}

/** Read what the core says: a head line, then one line per signal; fields split by tabs. */
export function parseExplanation(text: string): Explanation {
  const [head = '', ...rest] = text.split('\n');
  const [at = '-1', insn = '', form = '', step = '0'] = head.split('\t');
  const address = Number(at);
  return {
    at: address >= 0 ? address : undefined,
    text: insn,
    form,
    step: Number(step),
    signals: rest
      .filter((line) => line !== '')
      .map((line) => {
        const tab = line.indexOf('\t');
        return { name: line.slice(0, tab), doc: line.slice(tab + 1) };
      }),
  };
}

/**
 * The block of the CPU each control signal sets to work, by the start of
 * its name. Paths are those of the netlist.
 */
const BLOCK_OF: readonly [prefix: string, block: string][] = [
  ['BASE_', 'cpu/agu'],
  ['OFF_', 'cpu/agu'],
  ['AGU_', 'cpu/agu'],
  ['PRE', 'cpu/agu'],
  ['PC_WE', 'cpu/pc'],
  ['SP_WE', 'cpu/sp'],
  ['PAIR_WE', 'cpu/regs'],
  ['DOUT_', 'cpu/dout'],
  ['IR_WE', 'cpu/ir'],
  ['TL_WE', 'cpu/t'],
  ['TH_WE', 'cpu/t'],
  ['A_PD', 'cpu/regs'],
  ['B_', 'cpu/regs'],
  ['Y_', 'cpu/alu'],
  ['X_', 'cpu/alu'],
  ['CIN_', 'cpu/alu'],
  ['FN_', 'cpu/alu'],
  ['SIN_', 'cpu/alu'],
  ['WE_', 'cpu/flags'],
  ['Z_STICKY', 'cpu/flags'],
  ['REG_WE', 'cpu/regs'],
  ['W_', 'cpu/regs'],
  ['HALF', 'cpu/regs'],
  ['NS', 'cpu/step'],
  ['ALU_', 'cpu/control'],
];

/** The block a signal sets to work, or undefined for one that is about memory (`RD`, `WR`). */
export function blockOf(signal: string): string | undefined {
  return BLOCK_OF.find(([prefix]) => signal.startsWith(prefix))?.[1];
}

/** The blocks this cycle sets to work, each once, in the order of the signals. */
export function blocksAtWork(signals: readonly Signal[]): string[] {
  const blocks: string[] = [];
  for (const { name } of signals) {
    const block = blockOf(name);
    if (block && !blocks.includes(block)) blocks.push(block);
  }
  return blocks;
}

/** One sentence on what this cycle does with memory. */
export function memoryText(signals: readonly Signal[]): string {
  const names = new Set(signals.map((s) => s.name));
  if (names.has('IR_WE')) return 'reads the next opcode from memory';
  if (names.has('RD')) return 'reads a byte from memory';
  if (names.has('WR')) return 'writes a byte to memory';
  return 'does not use memory';
}

/** The headline: `0c61  ld a, [x]  ·  step 2 (LD_A_PTR)`. */
export function headline(e: Explanation): string {
  if (e.at === undefined) return 'power-on: the first opcode is about to be read';
  const where = `${hex(e.at, 4)}  ${e.text}`;
  return e.form === 'FETCH' ? `${where}  ·  fetch` : `${where}  ·  step ${e.step} (${e.form})`;
}
