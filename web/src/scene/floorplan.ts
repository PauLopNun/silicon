/**
 * The CPU's floor plan: where its blocks go, for the two modules where a
 * hand does better than the packer. Everything below them is packed in the
 * order it was declared.
 *
 * Only the arrangement is decided here. What the blocks are, how many gates
 * each has and how large each is come from the netlist; a test checks that
 * every name below is a block of the real CPU and that none is left out.
 */
import type { Hints } from './layout';

export const FLOORPLAN: Hints = {
  // Above: control on the left with what it reads (the instruction, the
  // step, the flags), the registers in the middle, and on the right the
  // registers that hold addresses and what goes out on the data bus.
  // Below: the two adders, one for data and one for addresses.
  cpu: {
    column: [
      {
        row: [
          { column: ['control', { row: ['ir', 'step', 'flags'] }] },
          'regs',
          { column: [{ row: ['pc', 'sp'] }, { row: ['t', 'dout'] }] },
        ],
      },
      { row: ['alu', 'agu'] },
    ],
  },
  // The memory made of gates: the address decoder on the left, the eight
  // words in two rows of four in the middle, the read multiplexer on the right.
  ram: {
    row: [
      'decode',
      {
        column: [
          { row: ['word0', 'word1', 'word2', 'word3'] },
          { row: ['word4', 'word5', 'word6', 'word7'] },
        ],
      },
      'read',
    ],
  },
  // The eight registers in two rows, between what chooses one to write
  // (above) and the three ports that read them (below).
  'cpu/regs': {
    column: [
      { row: ['wdec', 'select'] },
      { row: ['a', 'b', 'xl', 'xh'] },
      { row: ['yl', 'yh', 'zl', 'zh'] },
      { row: ['porta', 'portb', 'pair'] },
    ],
  },
};
