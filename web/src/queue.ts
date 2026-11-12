/**
 * Holds commands that arrive before the simulation is ready and replays them,
 * in order, once it is. Without this, a command sent while the WASM module is
 * still loading would be dropped and the UI would show a state the simulation
 * never had.
 */
export interface CommandQueue<T> {
  /** Deliver a command now, or keep it until `start` is called. */
  push(command: T): void;
  /** Set the handler and replay everything received so far. */
  start(handler: (command: T) => void): void;
}

export function commandQueue<T>(): CommandQueue<T> {
  const waiting: T[] = [];
  let handler: ((command: T) => void) | undefined;
  return {
    push(command) {
      if (handler) handler(command);
      else waiting.push(command);
    },
    start(h) {
      handler = h;
      for (const command of waiting.splice(0)) h(command);
    },
  };
}
