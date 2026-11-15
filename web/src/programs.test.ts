import { describe, expect, it } from 'vitest';
import { appendConsole, consoleText, parsePrograms, searchFor } from './programs';

const LIST = 'maze3d\tMaze\tFind the way out.\nbounce\tBounce\tFive balls.\nhello\tHello\tWrites a line.';
const programs = parsePrograms(LIST);

describe('the program list', () => {
  it('is read from the tab-separated lines the core gives', () => {
    expect(programs).toEqual([
      { name: 'maze3d', title: 'Maze', about: 'Find the way out.' },
      { name: 'bounce', title: 'Bounce', about: 'Five balls.' },
      { name: 'hello', title: 'Hello', about: 'Writes a line.' },
    ]);
    expect(parsePrograms('')).toEqual([]);
    expect(parsePrograms(`${LIST}\n`)).toHaveLength(3);
  });

  it('rejects a line that is not a program instead of showing half of one', () => {
    expect(() => parsePrograms('maze3d\tMaze')).toThrow(SyntaxError);
    expect(() => parsePrograms('\tMaze\tabout')).toThrow(SyntaxError);
  });
});

describe('the link to a program', () => {
  it('names the program, leaving the default out and the rest alone', () => {
    expect(searchFor('', 'bounce', programs)).toBe('?program=bounce');
    expect(searchFor('?engine=gate', 'bounce', programs)).toBe('?engine=gate&program=bounce');
    expect(searchFor('?program=bounce&engine=gate', 'hello', programs)).toBe('?program=hello&engine=gate');
    expect(searchFor('?program=bounce', 'maze3d', programs)).toBe('');
    expect(searchFor('?program=bounce&engine=gate', 'maze3d', programs)).toBe('?engine=gate');
  });

  it('is unchanged when it already says so', () => {
    expect(searchFor('?program=bounce', 'bounce', programs)).toBe('?program=bounce');
    expect(searchFor('?engine=gate', 'maze3d', programs)).toBe('?engine=gate');
    expect(searchFor('', 'maze3d', programs)).toBe('');
    // A link to a program that does not exist is put right.
    expect(searchFor('?program=nothing', 'maze3d', programs)).toBe('');
  });
});

describe('the console', () => {
  it('shows ASCII and marks everything else', () => {
    expect(consoleText(Uint8Array.of(72, 105, 33, 10))).toBe('Hi!\n');
    expect(consoleText(Uint8Array.of(0, 7, 13, 127, 200, 65))).toBe('·····A');
    expect(consoleText(new Uint8Array())).toBe('');
  });

  it('keeps the last lines', () => {
    expect(appendConsole('', 'Maze 1\n', 3)).toBe('Maze 1\n');
    expect(appendConsole('Maze 1\n', 'Maze 2\n', 3)).toBe('Maze 1\nMaze 2\n');
    // Three lines: two finished and the one being written.
    expect(appendConsole('Maze 1\nMaze 2\n', 'Maze 3\n', 3)).toBe('Maze 2\nMaze 3\n');
    expect(appendConsole('a\nb\nc', 'd', 2)).toBe('b\ncd');
    expect(appendConsole('2 3 5 ', '7 11 ', 4)).toBe('2 3 5 7 11 ');
  });
});
