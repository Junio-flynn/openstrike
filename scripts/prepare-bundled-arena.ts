// Produces the redistributable arena assets from a compiled open_arena BSP.
// Geometry/entities are OpenStrike-authored; the replacement WAD contains
// only CC0 Poly Haven materials and no Valve data. Source photographs are
// reduced to a handheld-friendly 256px, optimized 256-color palette here.

import { mkdir } from "node:fs/promises";

const input = Bun.argv[2] ?? ".local/test-maps/maps/open_arena.bsp";
const outRoot = Bun.argv[3] ?? "assets";

const source = new Uint8Array(await Bun.file(input).arrayBuffer());
const view = new DataView(source.buffer, source.byteOffset, source.byteLength);
if (view.getInt32(0, true) !== 30) throw new Error(`${input} is not a GoldSrc BSP v30`);

type Lump = { offset: number; length: number };
const lumps: Lump[] = Array.from({ length: 15 }, (_, index) => ({
  offset: view.getInt32(4 + index * 8, true),
  length: view.getInt32(8 + index * 8, true),
}));
const entities = lumps[0];
const text = new TextDecoder().decode(source.slice(entities.offset, entities.offset + entities.length));
const withoutSpawns = text.replace(/\{[^{}]*"classname"\s+"info_player_start"[^{}]*\}\s*/g, "");
const spawn = (x: number, y: number, yaw: number) => `{
"classname" "info_player_start"
"angles" "0 ${yaw} 0"
"origin" "${x} ${y} 36"
}\n`;
const replacement = new TextEncoder().encode(
  withoutSpawns.replace(/\0+$/, "").trimEnd() + "\n" + spawn(-1680, -1680, 45) + spawn(1680, 1680, 225) + "\0",
);
const paddedLength = (replacement.length + 3) & ~3;
const delta = paddedLength - entities.length;
const bsp = new Uint8Array(source.length + delta);
bsp.set(source.slice(0, entities.offset), 0);
bsp.set(replacement, entities.offset);
bsp.set(source.slice(entities.offset + entities.length), entities.offset + paddedLength);
const outView = new DataView(bsp.buffer);
outView.setInt32(8, replacement.length, true);
for (let index = 1; index < lumps.length; index++) {
  if (lumps[index].offset > entities.offset) {
    outView.setInt32(4 + index * 8, lumps[index].offset + delta, true);
  }
}

const textureSources = [
  ["GENERIC015T", "concrete_floor_03_diff_1k.jpg"],
  ["GENERIC015Q", "brick_wall_11_diff_1k.jpg"],
  ["GENERIC040", "corrugated_iron_02_diff_1k.jpg"],
  ["ARENAORANGE", "arena_orange.png"],
  ["ARENAGREY", "arena_grey.png"],
  // The compiler requires a palette entry even though Pocket3D replaces SKY
  // faces with its procedural atmosphere at runtime.
  ["SKY", "arena_grey.png"],
] as const;
const textureRoot = new URL("../assets/source-textures/", import.meta.url).pathname;
const textureSize = 256;

const decodeTexture = (file: string) => {
  const process = Bun.spawnSync([
    "ffmpeg", "-v", "error", "-i", `${textureRoot}${file}`,
    "-vf", `scale=${textureSize}:${textureSize}:flags=lanczos`, "-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1",
  ]);
  if (process.exitCode !== 0) {
    throw new Error(`ffmpeg failed for ${file}: ${new TextDecoder().decode(process.stderr)}`);
  }
  if (process.stdout.length !== textureSize * textureSize * 3) throw new Error(`unexpected RGB size for ${file}`);
  return process.stdout;
};

type PaletteColor = [number, number, number];

/** Median-cut palette: compact enough for the offline build, much cleaner than RGB332. */
const makePalette = (rgb: Uint8Array): PaletteColor[] => {
  const buckets: number[][] = [Array.from({ length: rgb.length / 3 }, (_, index) => index)];
  while (buckets.length < 256) {
    let chosen = -1;
    let chosenChannel = 0;
    let chosenScore = -1;
    for (let index = 0; index < buckets.length; index++) {
      const bucket = buckets[index];
      if (bucket.length < 2) continue;
      const min = [255, 255, 255];
      const max = [0, 0, 0];
      for (const pixel of bucket) {
        for (let channel = 0; channel < 3; channel++) {
          min[channel] = Math.min(min[channel], rgb[pixel * 3 + channel]);
          max[channel] = Math.max(max[channel], rgb[pixel * 3 + channel]);
        }
      }
      const ranges = max.map((value, channel) => value - min[channel]);
      const channel = ranges.indexOf(Math.max(...ranges));
      const score = ranges[channel] * bucket.length;
      if (score > chosenScore) {
        chosen = index;
        chosenChannel = channel;
        chosenScore = score;
      }
    }
    if (chosen < 0) break;
    const bucket = buckets[chosen];
    bucket.sort((a, b) => rgb[a * 3 + chosenChannel] - rgb[b * 3 + chosenChannel]);
    const middle = Math.floor(bucket.length / 2);
    buckets.splice(chosen, 1, bucket.slice(0, middle), bucket.slice(middle));
  }
  return buckets.map((bucket) => {
    const total = [0, 0, 0];
    for (const pixel of bucket) {
      total[0] += rgb[pixel * 3];
      total[1] += rgb[pixel * 3 + 1];
      total[2] += rgb[pixel * 3 + 2];
    }
    return total.map((value) => Math.round(value / bucket.length)) as PaletteColor;
  });
};

const nearestColor = (palette: PaletteColor[], r: number, g: number, b: number) => {
  let nearest = 0;
  let nearestDistance = Number.POSITIVE_INFINITY;
  for (let index = 0; index < palette.length; index++) {
    const color = palette[index];
    const dr = r - color[0];
    const dg = g - color[1];
    const db = b - color[2];
    const distance = dr * dr + dg * dg + db * db;
    if (distance < nearestDistance) {
      nearest = index;
      nearestDistance = distance;
    }
  }
  return nearest;
};

const lumpForTexture = (name: string, rgb: Uint8Array) => {
  const width = textureSize;
  const height = textureSize;
  const palette = makePalette(rgb);
  const mipSizes = [width, width / 2, width / 4, width / 8];
  const pixelsForMips = mipSizes.reduce((total, size) => total + size * size, 0);
  const result = new Uint8Array(40 + pixelsForMips + 2 + 256 * 3);
  const data = new DataView(result.buffer);
  new TextEncoder().encodeInto(name, result.subarray(0, 16));
  data.setUint32(16, width, true);
  data.setUint32(20, height, true);
  let offset = 40;
  for (const [index, size] of mipSizes.entries()) {
    data.setUint32(24 + index * 4, offset, true);
    for (let y = 0; y < size; y++) {
      for (let x = 0; x < size; x++) {
        const scale = width / size;
        let r = 0, g = 0, b = 0;
        for (let sy = 0; sy < scale; sy++) {
          for (let sx = 0; sx < scale; sx++) {
            const source = ((y * scale + sy) * width + x * scale + sx) * 3;
            r += rgb[source];
            g += rgb[source + 1];
            b += rgb[source + 2];
          }
        }
        const samples = scale * scale;
        result[offset++] = nearestColor(palette, r / samples, g / samples, b / samples);
      }
    }
  }
  data.setUint16(offset, 256, true);
  offset += 2;
  for (let index = 0; index < palette.length; index++) {
    result[offset + index * 3] = palette[index][0];
    result[offset + index * 3 + 1] = palette[index][1];
    result[offset + index * 3 + 2] = palette[index][2];
  }
  return result;
};

const textureLumps = textureSources.map(([name, file]) => ({
  name,
  bytes: lumpForTexture(name, decodeTexture(file)),
}));
const dataSize = textureLumps.reduce((sum, lump) => sum + lump.bytes.length, 0);
const directoryOffset = 12 + dataSize;
const wad = new Uint8Array(directoryOffset + textureLumps.length * 32);
wad.set(new TextEncoder().encode("WAD3"), 0);
const wadView = new DataView(wad.buffer);
wadView.setInt32(4, textureLumps.length, true);
wadView.setInt32(8, directoryOffset, true);
let fileOffset = 12;
for (const [index, lump] of textureLumps.entries()) {
  wad.set(lump.bytes, fileOffset);
  const directory = directoryOffset + index * 32;
  wadView.setInt32(directory, fileOffset, true);
  wadView.setInt32(directory + 4, lump.bytes.length, true);
  wadView.setInt32(directory + 8, lump.bytes.length, true);
  wad[directory + 12] = 0x43; // TYP_MIPTEX
  new TextEncoder().encodeInto(lump.name, wad.subarray(directory + 16, directory + 32));
  fileOffset += lump.bytes.length;
}

await mkdir(`${outRoot}/maps`, { recursive: true });
await mkdir(`${outRoot}/support`, { recursive: true });
await Bun.write(`${outRoot}/maps/open_arena.bsp`, bsp);
await Bun.write(`${outRoot}/support/openstrike.wad`, wad);
console.log(`wrote ${outRoot}/maps/open_arena.bsp (2 spawns) and support/openstrike.wad`);
