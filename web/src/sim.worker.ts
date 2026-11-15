/// <reference lib="webworker" />
/**
 * Simulation worker. Owns the WASM instance; the main thread never touches it.
 * It runs the computer's clock in short slices so that commands are handled
 * promptly, and ships the state to the page as a few small buffers at most
 * once per display frame.
 *
 * The programs arrive as source text inside the WASM core and are assembled
 * or compiled there when one is chosen.
 */
import init, { Machine, program_list, version } from './wasm-pkg/silicon_wasm.js';
import { chunkSize, Pacer, RateMeter } from './clock';
import { parsePrograms } from './programs';
import type { EngineKind, FromWorker, ToWorker } from './protocol';
import { commandQueue } from './queue';

/** The clock rate the computer is meant to run at. */
const NOMINAL_HZ = 4_000_000;
/** How long to simulate before looking at commands again. */
const SLICE_MS = 10;
/** Minimum time between state messages. */
const PUBLISH_MS = 16;

const post = (msg: FromWorker, transfer: Transferable[] = []) =>
  (self as DedicatedWorkerGlobalScope).postMessage(msg, transfer);

// Listen from the very first moment: commands that arrive while the WASM core
// is still loading are queued and replayed, never dropped.
const commands = commandQueue<ToWorker>();
self.onmessage = (e: MessageEvent<ToWorker>) => commands.push(e.data);

async function main(): Promise<void> {
  const wasm = await init();
  const programs = parsePrograms(program_list());
  // A computer with nothing in ROM yet. (Memory full of zeroes is a `hlt`
  // at address 0: it would stop at once.)
  const machine = new Machine(new Uint8Array(0));
  /** The program in ROM; none until the first is loaded, below. */
  let program = '';
  /** Times the computer has been started; see `FrameState.boot`. */
  let boot = 0;

  let running = true;
  let dirty = true;
  let lastTick = performance.now();
  let lastPublish = -Infinity;
  const pacer = new Pacer(NOMINAL_HZ);
  // Speed of the engine itself, used only to size the chunks of work.
  const capacity = new RateMeter();

  // A copy of a region of WASM memory. (The view is re-created each time:
  // the memory buffer is replaced if WASM memory grows.)
  const copy = (ptr: number, len: number) => new Uint8Array(wasm.memory.buffer, ptr, len).slice();

  const publish = () => {
    const framebuffer = copy(machine.framebuffer_ptr(), machine.framebuffer_len());
    // The levels now, and everything that changed since the last look.
    machine.look_at_wires();
    const wires = copy(machine.wires_ptr(), machine.wires_len());
    const activity = copy(machine.activity_ptr(), machine.activity_len());
    const arch = machine.arch_state();
    const written = machine.take_console();
    post(
      {
        type: 'frame',
        program,
        boot,
        engine: machine.engine() as EngineKind,
        running,
        halted: machine.halted(),
        cycles: machine.cycles(),
        frames: machine.frames(),
        framebuffer,
        wires,
        activity,
        arch,
        console: written,
      },
      [framebuffer.buffer, wires.buffer, activity.buffer, arch.buffer, written.buffer],
    );
    dirty = false;
    lastPublish = performance.now();
  };

  post({
    type: 'ready',
    version: version(),
    engine: machine.engine() as EngineKind,
    programs,
    gates: machine.gate_table(),
    moduleParents: machine.module_parents(),
    moduleNames: machine.module_names().split('\n'),
    wireCount: machine.wires_len(),
  });

  /** Build a program from its source and start it from power-on. */
  const load = (name: string) => {
    try {
      machine.load(name);
    } catch (err) {
      // The computer keeps the program it had.
      post({ type: 'error', message: String(err) });
      return;
    }
    program = name;
    boot += 1;
    pacer.clear();
  };

  commands.start((command) => {
    switch (command.type) {
      case 'run':
        running = command.running;
        pacer.clear();
        break;
      case 'step':
        machine.run(command.cycles);
        break;
      case 'engine':
        machine.use_gate_engine(command.engine === 'gate');
        // The two engines differ in speed by three orders of magnitude.
        capacity.reset();
        pacer.clear();
        break;
      case 'clock':
        pacer.hz = command.hz;
        pacer.clear();
        break;
      case 'keys':
        machine.set_keys(command.keys);
        break;
      case 'reset':
        machine.reset();
        boot += 1;
        pacer.clear();
        break;
      case 'program':
        if (programs.some((p) => p.name === command.name)) load(command.name);
        break;
    }
    dirty = true;
  });
  // Unless a command that was already waiting chose one, start with the
  // first program of the list.
  const first = programs[0];
  if (program === '' && first) load(first.name);

  // Yielding through a message channel lets queued commands run between
  // slices without the minimum delay of setTimeout.
  const channel = new MessageChannel();
  const tick = () => {
    const now = performance.now();
    const due = running ? pacer.advance(now - lastTick) : 0;
    lastTick = now;

    let ran = 0;
    if (due >= 1) {
      const chunk = chunkSize(capacity.rate);
      const deadline = now + SLICE_MS;
      while (ran < due && performance.now() < deadline) {
        const cycles = Math.min(chunk, due - ran);
        machine.run(cycles);
        ran += cycles;
      }
      capacity.sample(ran, performance.now() - now);
      pacer.ran(ran);
      dirty = true;
    }

    if (dirty && performance.now() - lastPublish >= PUBLISH_MS) publish();
    // Keep going at once if there was work; otherwise sleep a little.
    if (ran > 0) channel.port2.postMessage(0);
    else setTimeout(tick, 4);
  };
  channel.port1.onmessage = tick;
  tick();
}

main().catch((err: unknown) => post({ type: 'error', message: String(err) }));
