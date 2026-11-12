/// <reference lib="webworker" />
/**
 * Simulation worker. Owns the WASM instance; the main thread never touches it.
 * State is read through a typed-array view over WASM memory (no per-gate JS
 * calls) and shipped to the renderer as one buffer.
 */
import init, { XorDemo, version } from './wasm-pkg/silicon_wasm.js';
import type { EngineKind, FromWorker, GateInfo, ToWorker } from './protocol';

const post = (msg: FromWorker, transfer: Transferable[] = []) =>
  (self as DedicatedWorkerGlobalScope).postMessage(msg, transfer);

async function main(): Promise<void> {
  const wasm = await init();
  const demo = new XorDemo();
  const engine = demo.engine() as EngineKind;

  const gates: GateInfo[] = [];
  for (let i = 0; i < demo.gate_count(); i++) {
    const [a, b, out] = demo.gate_wires(i);
    gates.push({ path: demo.gate_path(i), a: a!, b: b!, out: out! });
  }

  const publish = () => {
    // View straight into WASM memory, then copy once into a transferable.
    // (Re-created each time: the memory buffer is replaced if WASM memory grows.)
    const view = new Uint8Array(wasm.memory.buffer, demo.state_ptr(), demo.state_len());
    const wires = view.slice();
    post({ type: 'state', engine, wires }, [wires.buffer]);
  };

  self.onmessage = (e: MessageEvent<ToWorker>) => {
    if (e.data.type === 'setInputs') {
      demo.set_inputs(e.data.a, e.data.b);
      publish();
    }
  };

  post({ type: 'ready', version: version(), engine, gates });
  publish();
}

main().catch((err: unknown) => post({ type: 'error', message: String(err) }));
