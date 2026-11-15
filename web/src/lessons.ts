/**
 * A guided tour of the computer, from one NAND gate to a running program.
 *
 * Each step says where to look (the camera flies there), what to notice, and
 * sometimes asks for something to be done, which the page sees happen. The
 * steps are plain data; every place they name is checked against the real
 * netlists by a test, so a lesson cannot point at a gate that is not there.
 * The facts they state are the design's: nine NANDs to a full adder, eight
 * flip-flops to a register, and so on, also checked by that test.
 */

/** What the page knows that a task can be about. */
export interface LessonContext {
  /** Path of the module the camera is in. */
  focus: string;
  showing: 'cpu' | 'ram';
  paused: boolean;
  engine: 'gate' | 'fast';
  /** Path of the gate chosen with a click, or ''. */
  chosen: string;
  /** Number of broken gates. */
  broken: number;
  /** What the memory made of gates reads out, and its words. */
  ramOut: number;
  ramWords: readonly number[];
  /** Times the explanation's step went forward since the lesson step began. */
  stepped: number;
}

export interface LessonStep {
  title: string;
  /** What to notice; paragraphs separated by blank lines. */
  text: string;
  /** Which scene, and where in it the camera goes (a module or gate path). */
  view: 'cpu' | 'ram';
  at: string;
  /** Put the computer in explain mode (paused, on the gates) when the step begins. */
  explain?: boolean;
  /** Something to do, and how the page knows it is done. */
  task?: { text: string; done: (c: LessonContext) => boolean };
}

export const LESSON: readonly LessonStep[] = [
  {
    title: 'One gate',
    view: 'cpu',
    at: 'cpu/alu/adder8/fa0/nand0',
    text:
      'This block is a NAND gate: its output is 0 only when both inputs are 1. The two dots on the left are its inputs, and they light up when they are at 1; the block itself lights up when its output is.\n\n' +
      'Every gate of this CPU is a NAND, except the 127 flip-flops that hold its state. Nothing else is used.',
    task: {
      text: 'Point at a gate and read its inputs and output in the box at the bottom. Click it to choose it.',
      done: (c) => /\/nand\d+$/.test(c.chosen),
    },
  },
  {
    title: 'Adding two bits',
    view: 'cpu',
    at: 'cpu/alu/adder8/fa0',
    text:
      'Nine NAND gates make a full adder: it adds two bits and a carry coming in, and gives a sum bit and a carry going out.\n\n' +
      'nand0 to nand3 make the exclusive-or of the two bits; nand4 to nand7 add the carry in to that; nand8 is the carry out.',
  },
  {
    title: 'Adding two bytes',
    view: 'cpu',
    at: 'cpu/alu/adder8',
    text:
      'Eight full adders side by side add two bytes. Each passes its carry to the next, from fa0 (the lowest bit) to fa7: a ripple-carry adder.\n\n' +
      'That chain is the slowest path through the ALU, because the top bit has to wait for the carry to ripple up from the bottom.',
  },
  {
    title: 'The ALU',
    view: 'cpu',
    at: 'cpu/alu',
    text:
      'Around the adder: xsel and ysel choose the two operands, yinv can invert the second one (that turns adding into subtracting), logic computes AND, OR and XOR of every bit at once, and mux picks which result goes out.\n\n' +
      'status works out the flags from the result: zero, negative, carry, overflow. Every register write in this CPU passes through here.',
  },
  {
    title: 'Holding a byte',
    view: 'cpu',
    at: 'cpu/regs/a',
    text:
      'Register a is eight flip-flops, the cyan blocks. At each clock edge a flip-flop takes whatever is at its input. The NANDs in front of each one choose that input: the new value when the register is being written, its own old value otherwise. So it holds.\n\n' +
      'The register file has eight of these, and three ports that read them.',
  },
  {
    title: 'Memory',
    view: 'ram',
    at: 'ram',
    text:
      "The computer's 64 KiB of memory are an array in the simulator, not gates: as flip-flops they would be half a million of them. This is the same thing, small enough to see.\n\n" +
      'The address goes into decode, which lights one line of eight. That line lets one word take the data in at the clock edge, if writing is on. read is a tree of multiplexers that puts the chosen word on the data out, with no clock at all.',
    task: {
      text: 'Write 1010 into word 5: choose address 5, set the bits, turn write on and press clock.',
      done: (c) => c.ramWords[5] === 0b1010,
    },
  },
  {
    title: 'Control',
    view: 'cpu',
    at: 'cpu/control',
    text:
      'Control decides what every other block does in each clock cycle. It is a table, the microcode: for every instruction and every step of it, which control signals are on.\n\n' +
      'The table is built in gates the way a PLA is: decode recognises bit patterns of the instruction, lines combines a pattern with a step, and plane raises each signal when any of its lines is on.',
  },
  {
    title: 'One instruction at a time',
    view: 'cpu',
    at: 'cpu',
    explain: true,
    text:
      'The computer is now paused on the gate engine. The panel on the left says which instruction is running, which of its steps comes next, and every control signal that step raises.\n\n' +
      'Step forward and watch: the blocks it names light up in the scene, and the wires that changed glow.',
    task: {
      text: 'Press Next cycle or Next instruction a few times.',
      done: (c) => c.stepped >= 3,
    },
  },
  {
    title: 'Break it',
    view: 'cpu',
    at: 'cpu/alu/adder8/fa3',
    text:
      'Chips fail. A common way is a gate whose output is stuck at 0 or at 1, whatever its inputs say.\n\n' +
      'Choose a gate here and stick it. The game is computed by these gates, so watch its screen go wrong: one gate is enough. Mend it, and everything is as it was.',
    task: {
      text: 'Run the program, click a gate of this adder and press "stuck at 1".',
      done: (c) => c.broken > 0,
    },
  },
];

/** Every place the lesson names, for the test that they exist. */
export function lessonPlaces(): { view: 'cpu' | 'ram'; at: string }[] {
  return LESSON.map(({ view, at }) => ({ view, at }));
}
