// Tiny PNG encoder for the headless screenshot tools (RGBA8).
import zlib from 'node:zlib';

const crcTable = new Uint32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type), data]);
  const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(td));
  return Buffer.concat([len, td, crc]);
}
export function encodePng(w, h, rgba) {
  const raw = Buffer.alloc((w * 4 + 1) * h);
  for (let y = 0; y < h; y++) {
    raw[y * (w * 4 + 1)] = 0;
    Buffer.from(rgba.buffer, rgba.byteOffset + y * w * 4, w * 4).copy(raw, y * (w * 4 + 1) + 1);
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 6;
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk('IHDR', ihdr), chunk('IDAT', zlib.deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]);
}

/** Crop a region out of an RGBA8 image and enlarge it by an integer factor (nearest neighbour). */
export function cropScale(w, rgba, x0, y0, cw, ch, scale = 1) {
  const out = new Uint8Array(cw * scale * ch * scale * 4);
  for (let y = 0; y < ch * scale; y++) {
    for (let x = 0; x < cw * scale; x++) {
      const si = ((y0 + Math.floor(y / scale)) * w + x0 + Math.floor(x / scale)) * 4;
      out.set(rgba.subarray(si, si + 4), (y * cw * scale + x) * 4);
    }
  }
  return { w: cw * scale, h: ch * scale, rgba: out };
}
