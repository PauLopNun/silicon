/**
 * The programs the computer can be given, and the small decisions the page
 * makes about them: what the address bar should say, how much console output
 * to keep.
 */

/** A program of `programs/`, as the menu shows it. */
export interface ProgramInfo {
  /** Short name, used in links: `?program=maze3d`. */
  name: string;
  title: string;
  /** One sentence about what it does and which keys it reads. */
  about: string;
}

/**
 * Read the list the simulation core gives: one program per line, as name,
 * title and description separated by tabs.
 */
export function parsePrograms(list: string): ProgramInfo[] {
  return list
    .split('\n')
    .filter((line) => line !== '')
    .map((line) => {
      const [name, title, about] = line.split('\t');
      if (!name || !title || about === undefined) throw new SyntaxError(`not a program: ${JSON.stringify(line)}`);
      return { name, title, about };
    });
}

/**
 * The query string that leads back to `program`, keeping whatever else the
 * link says. The first program is the default and is left out.
 */
export function searchFor(search: string, program: string, programs: readonly ProgramInfo[]): string {
  const params = new URLSearchParams(search);
  if (program === programs[0]?.name) params.delete('program');
  else params.set('program', program);
  const text = params.toString();
  return text === '' ? '' : `?${text}`;
}

/** Console output is bytes; the programs write ASCII. Anything else is shown as `·`. */
export function consoleText(bytes: Uint8Array): string {
  let text = '';
  for (const byte of bytes) {
    text += byte === 10 || (byte >= 32 && byte < 127) ? String.fromCharCode(byte) : '·';
  }
  return text;
}

/** Add output to what the console shows, keeping only the last `maxLines` lines. */
export function appendConsole(shown: string, added: string, maxLines: number): string {
  const lines = (shown + added).split('\n');
  // A text that ends in a newline has an empty last piece: the line being written.
  return lines.slice(Math.max(0, lines.length - maxLines)).join('\n');
}
