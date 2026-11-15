/**
 * The tests run in Node, and this is all they use of it: reading the
 * compiled simulation core from disk. Declared here so that the page's
 * sources do not get the whole of Node's types in scope.
 */
declare module 'node:fs' {
  export function readFileSync(path: URL): Uint8Array;
}
