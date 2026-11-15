// An animated PNG (APNG) from PNG pictures of the same size, with nothing
// to install. Browsers and GitHub show APNG as an animation; anything else
// shows its first frame.
//
// Every frame keeps its own compressed image data as it is: the first
// frame's goes in IDAT chunks, the others' in fdAT chunks, which are IDAT
// with a sequence number in front. So the pictures must share their size,
// bit depth and colour type, which pictures taken by the same browser do.
import { deflateSync, inflateSync } from 'node:zlib';

const SIGNATURE = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

function crc32(bytes) {
  let c = 0xffffffff;
  for (const b of bytes) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const out = Buffer.alloc(12 + data.length);
  out.writeUInt32BE(data.length, 0);
  out.write(type, 4, 'ascii');
  data.copy(out, 8);
  out.writeUInt32BE(crc32(out.subarray(4, 8 + data.length)), 8 + data.length);
  return out;
}

/** The chunks of a PNG, in order. */
export function chunks(png) {
  if (!png.subarray(0, 8).equals(SIGNATURE)) throw new Error('not a PNG');
  const out = [];
  for (let at = 8; at < png.length; ) {
    const length = png.readUInt32BE(at);
    out.push({ type: png.toString('ascii', at + 4, at + 8), data: png.subarray(at + 8, at + 8 + length) });
    at += 12 + length;
  }
  return out;
}

/**
 * The image data of a PNG, recompressed harder: Chrome compresses its
 * screenshots fast rather than small.
 */
function recompress(idat) {
  return deflateSync(inflateSync(idat), { level: 9 });
}

/**
 * An APNG of `frames` (PNG buffers), each shown `delayMs`, looping for ever.
 * The last frame can be held longer with `holdLastMs`.
 */
export function apng(frames, delayMs, holdLastMs = delayMs) {
  const first = chunks(frames[0]);
  const ihdr = first.find((c) => c.type === 'IHDR').data;
  const width = ihdr.readUInt32BE(0);
  const height = ihdr.readUInt32BE(4);
  let sequence = 0;
  const control = (index) => {
    const data = Buffer.alloc(26);
    data.writeUInt32BE(sequence++, 0);
    data.writeUInt32BE(width, 4);
    data.writeUInt32BE(height, 8);
    data.writeUInt32BE(0, 12); // x offset
    data.writeUInt32BE(0, 16); // y offset
    data.writeUInt16BE(index === frames.length - 1 ? holdLastMs : delayMs, 20);
    data.writeUInt16BE(1000, 22); // the delay is in thousandths of a second
    data[24] = 0; // dispose: none
    data[25] = 0; // blend: source
    return chunk('fcTL', data);
  };
  const actl = Buffer.alloc(8);
  actl.writeUInt32BE(frames.length, 0);
  actl.writeUInt32BE(0, 4); // loop for ever

  const parts = [SIGNATURE, chunk('IHDR', ihdr), chunk('acTL', actl)];
  frames.forEach((png, index) => {
    const own = chunks(png);
    const header = own.find((c) => c.type === 'IHDR').data;
    if (!header.equals(ihdr)) throw new Error(`frame ${index} differs in size or format from the first`);
    const data = recompress(Buffer.concat(own.filter((c) => c.type === 'IDAT').map((c) => c.data)));
    parts.push(control(index));
    if (index === 0) {
      parts.push(chunk('IDAT', data));
    } else {
      const fdat = Buffer.alloc(4 + data.length);
      fdat.writeUInt32BE(sequence++, 0);
      data.copy(fdat, 4);
      parts.push(chunk('fdAT', fdat));
    }
  });
  parts.push(chunk('IEND', Buffer.alloc(0)));
  return Buffer.concat(parts);
}
