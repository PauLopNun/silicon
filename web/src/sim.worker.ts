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
import init, { Machine, program_list, RamExhibit, version } from './wasm-pkg/silicon_wasm.js';
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
  /** The memory made of gates, apart from the computer. */
  const ram = new RamExhibit();
  /** The program in ROM; none until the first is loaded, below. */
  let program = '';
  /** Times the computer has been started; see `FrameState.boot`. */
  let boot = 0;

  let running = true;
  /** The page can be seen; while it cannot, the clock stands still. */
  let visible = true;
  let dirty = true;
  /**
   * Clock cycles of every frame to compute on the gates while the fast
   * engine runs the program, so that the page can show the wires; 0: none.
   */
  let sampling = 0;
  let lastTick = performance.now();
  let lastPublish = -Infinity;
  const pacer = new Pacer(NOMINAL_HZ);
  // Speed of the engine itself, used only to size the chunks of work.
  const capacity = new RateMeter();

  // A copy of a region of WASM memory. (The view is re-created each time:
  // the memory buffer is replaced if WASM memory grows.)
  const copy = (ptr: number, len: number) => new Uint8Array(wasm.memory.buffer, ptr, len).slice();

  /**
   * Send the page everything it shows. `sample` clock cycles, which the
   * clock was about to run anyway, are run here on the gates if the fast
   * engine is running the CPU and sampling is on.
   */
  const publish = (sample: number) => {
    let wiresFrom: 'gate' | 'sample' | 'old';
    let sampled = 0;
    if (machine.engine() === 'gate') {
      // The levels now, and everything that changed since the last look.
      machine.look_at_wires();
      wiresFrom = 'gate';
    } else if (sampling > 0) {
      // A sample of no cycles (the clock is stopped) still gives the wires
      // the levels they have in the state the CPU is in.
      machine.sample_on_gates(sample);
      wiresFrom = 'sample';
      sampled = sample;
    } else {
      machine.look_at_wires();
      wiresFrom = 'old';
    }
    const framebuffer = copy(machine.framebuffer_ptr(), machine.framebuffer_len());
    const wires = copy(machine.wires_ptr(), machine.wires_len());
    const activity = copy(machine.activity_ptr(), machine.activity_len());
    const arch = machine.arch_state();
    const written = machine.take_console();
    ram.look_at_wires();
    const ramWires = copy(ram.wires_ptr(), ram.wires_len());
    const ramActivity = copy(ram.activity_ptr(), ram.wires_len());
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
        wiresFrom,
        sampled,
        activity,
        arch,
        console: written,
        // Only while paused: running, it would be stale before it arrived.
        explain: running ? '' : machine.explain(),
        faults: machine.faults(),
        ram: { wires: ramWires, activity: ramActivity, words: ram.words(), out: ram.read() },
      },
      [framebuffer.buffer, wires.buffer, activity.buffer, arch.buffer, written.buffer, ramWires.buffer, ramActivity.buffer],
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
    levels: machine.gate_levels(),
    moduleParents: machine.module_parents(),
    moduleNames: machine.module_names().split('\n'),
    wireCount: machine.wires_len(),
    ram: {
      gates: ram.gate_table(),
      levels: ram.gate_levels(),
      moduleParents: ram.module_parents(),
      moduleNames: ram.module_names().split('\n'),
      wireCount: ram.wires_len(),
      shape: ram.shape(),
    },
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
      case 'step-instruction':
        machine.step_instruction();
        break;
      case 'break':
        machine.break_gate(command.gate, command.level ?? -1);
        // Breaking a gate may have moved the CPU to the gate engine.
        capacity.reset();
        pacer.clear();
        break;
      case 'sampling':
        sampling = Math.max(0, Math.floor(command.cycles));
        break;
      case 'ram':
        ram.set_inputs(command.addr, command.data, command.write);
        break;
      case 'ram-clock':
        ram.clock();
        break;
      case 'mend-all':
        machine.mend_all();
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
      case 'visible':
        visible = command.visible;
        // No catching up on the time spent out of sight.
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
    const due = running && visible ? pacer.advance(now - lastTick) : 0;
    lastTick = now;

    // With a sample to take for the gates, a message is decided on before the
    // cycles are run, because its sample is taken out of them: the last
    // cycles before the message, never cycles added to what is due, so the
    // clock keeps its rate whatever it is (at 10 Hz every cycle is in a
    // sample). Otherwise it is decided once they have run.
    const sampled = sampling > 0 && due >= 1 && machine.engine() === 'fast';
    const early = sampled && now - lastPublish >= PUBLISH_MS;
    let ran = 0;
    let sample = 0;
    if (due >= 1) {
      if (early) sample = Math.min(sampling, due);
      const onActive = due - sample;
      const chunk = chunkSize(capacity.rate);
      const deadline = now + SLICE_MS;
      while (ran < onActive && performance.now() < deadline) {
        const cycles = Math.min(chunk, onActive - ran);
        machine.run(cycles);
        ran += cycles;
      }
      if (ran > 0) capacity.sample(ran, performance.now() - now);
      pacer.ran(ran + sample);
      dirty = true;
    }

    // (With sampling on and no sample taken, a message now would show the
    // wires as having done nothing, between two that show them busy.)
    const publishing = sampled ? early : performance.now() - lastPublish >= PUBLISH_MS;
    if (dirty && publishing) publish(sample);
    // Keep going at once if there was work; otherwise sleep a little.
    if (ran + sample > 0) channel.port2.postMessage(0);
    else setTimeout(tick, visible ? 4 : 100);
  };
  channel.port1.onmessage = tick;
  tick();
}

main().catch((err: unknown) => post({ type: 'error', message: String(err) }));
