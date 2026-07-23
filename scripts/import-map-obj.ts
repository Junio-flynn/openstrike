// Convert an axis-oriented arena OBJ into a sealed GoldSrc .map. Downloaded
// arena packs often contain render meshes only; OpenStrike needs solid BSP
// brushes, collision hulls, lighting, and team spawns on every platform.

const [input, output, preset = "hammer"] = Bun.argv.slice(2);
if (!input || !output) {
  throw new Error("usage: bun scripts/import-map-obj.ts <input.obj> <output.map> [hammer|unity]");
}

type Vec3 = [number, number, number];
type Box = [number, number, number, number, number, number];

const source = await Bun.file(input).text();
const vertices: Vec3[] = [];
const faces: number[][] = [];
for (const raw of source.split(/\r?\n/)) {
  const line = raw.trim();
  if (line.startsWith("v ")) {
    const values = line.slice(2).trim().split(/\s+/).map(Number);
    if (values.length >= 3 && values.every(Number.isFinite)) vertices.push([values[0], values[1], values[2]]);
  } else if (line.startsWith("f ")) {
    const face = line.slice(2).trim().split(/\s+/).map((part) => {
      const value = Number(part.split("/", 1)[0]);
      return value < 0 ? vertices.length + value : value - 1;
    });
    if (face.length >= 3 && face.every((index) => vertices[index])) faces.push(face);
  }
}
if (vertices.length === 0 || faces.length === 0) throw new Error(`${input} has no usable mesh geometry`);

// Both exporters use Y-up. Hammer units are already game-sized. The supplied
// Unity asset is a thin vertical showcase mesh, so convert it into useful FPS
// cover with full arena length, broader depth, and human-scale height.
// Pocket3D performs its normal BSP axis conversion later.
const transformed = vertices.map(([x, y, z]) => preset === "unity"
  ? [x * 64, z * 64, y * 16] as Vec3
  : [x, z, y] as Vec3);
const horizontalLevels = new Map<number, number>();
if (preset === "hammer") {
  for (const face of faces) {
    const points = face.map((index) => transformed[index]);
    const z = points.map((point) => point[2]);
    if (Math.max(...z) - Math.min(...z) > 0.5) continue;
    const x = points.map((point) => point[0]);
    const y = points.map((point) => point[1]);
    const area = (Math.max(...x) - Math.min(...x)) * (Math.max(...y) - Math.min(...y));
    const level = Math.round(z[0]);
    horizontalLevels.set(level, (horizontalLevels.get(level) ?? 0) + area);
  }
}
const sourceFloor = horizontalLevels.size > 0
  ? [...horizontalLevels].sort((a, b) => b[1] - a[1])[0][0]
  : Math.min(...transformed.map((point) => point[2]));
for (const point of transformed) point[2] -= sourceFloor;

const boxes: Box[] = [];
const seen = new Set<string>();
for (const face of faces) {
  const points = face.map((index) => transformed[index]);
  const min = [0, 1, 2].map((axis) => Math.min(...points.map((point) => point[axis]))) as Vec3;
  const max = [0, 1, 2].map((axis) => Math.max(...points.map((point) => point[axis]))) as Vec3;
  // Turn each render surface into a thin collision brush. Axis-aligned arena
  // meshes remain exact; ramps receive a conservative box approximation.
  for (let axis = 0; axis < 3; axis++) {
    if (max[axis] - min[axis] < 4) {
      const center = (min[axis] + max[axis]) / 2;
      min[axis] = center - 4;
      max[axis] = center + 4;
    }
  }
  const rounded = [...min, ...max].map((value) => Math.round(value)) as Box;
  const key = rounded.join(",");
  if (!seen.has(key)) {
    seen.add(key);
    boxes.push(rounded);
  }
}

const meshMinX = Math.min(...transformed.map((point) => point[0]));
const meshMaxX = Math.max(...transformed.map((point) => point[0]));
const meshMinY = Math.min(...transformed.map((point) => point[1]));
const meshMaxY = Math.max(...transformed.map((point) => point[1]));
const centerX = (meshMinX + meshMaxX) / 2;
const centerY = (meshMinY + meshMaxY) / 2;
const halfX = Math.max(1024, (meshMaxX - meshMinX) / 2 + 448);
const halfY = Math.max(768, (meshMaxY - meshMinY) / 2 + 448);
const minX = Math.floor(centerX - halfX);
const maxX = Math.ceil(centerX + halfX);
const minY = Math.floor(centerY - halfY);
const maxY = Math.ceil(centerY + halfY);
const ceiling = Math.max(768, Math.ceil(Math.max(...transformed.map((point) => point[2])) + 192));

const brush = ([x1, y1, z1, x2, y2, z2]: Box, texture: string) => `{
( ${x1} ${y1} ${z2} ) ( ${x1} ${y1} ${z1} ) ( ${x1} ${y2} ${z2} ) ${texture} [ 0 1 0 0 ] [ 0 0 -1 0 ] 0 1 1
( ${x2} ${y2} ${z2} ) ( ${x2} ${y2} ${z1} ) ( ${x2} ${y1} ${z2} ) ${texture} [ 0 1 0 0 ] [ 0 0 -1 0 ] 0 1 1
( ${x1} ${y2} ${z2} ) ( ${x1} ${y2} ${z1} ) ( ${x2} ${y2} ${z2} ) ${texture} [ 1 0 0 0 ] [ 0 0 -1 0 ] 0 1 1
( ${x2} ${y1} ${z2} ) ( ${x2} ${y1} ${z1} ) ( ${x1} ${y1} ${z2} ) ${texture} [ 1 0 0 0 ] [ 0 0 -1 0 ] 0 1 1
( ${x1} ${y2} ${z1} ) ( ${x1} ${y1} ${z1} ) ( ${x2} ${y2} ${z1} ) ${texture} [ 1 0 0 0 ] [ 0 -1 0 0 ] 0 1 1
( ${x2} ${y1} ${z2} ) ( ${x1} ${y1} ${z2} ) ( ${x2} ${y2} ${z2} ) ${texture} [ 1 0 0 0 ] [ 0 -1 0 0 ] 0 1 1
}`;

const material = (box: Box, index: number) => {
  // Custom-map walkable floors use concrete. The old sand material looked
  // like an unrelated plate sitting underneath the imported geometry.
  if (box[5] <= 12) return "ARENAGREY";
  if (preset === "unity") return index % 4 === 0 ? "ARENAGREY" : "ARENAORANGE";
  return index % 5 === 0 ? "GENERIC040" : "GENERIC015Q";
};

// Find two distant, unobstructed grid positions at standing height.
const floorAt = (x: number, y: number) => boxes.reduce((floor, [x1, y1, z1, x2, y2, z2]) =>
  z2 <= 128 && z2 - z1 <= 12 && x > x1 + 20 && x < x2 - 20 && y > y1 + 20 && y < y2 - 20
    ? Math.max(floor, z2)
    : floor, 0);
const blocked = (x: number, y: number, floor: number) => boxes.some(([x1, y1, z1, x2, y2, z2]) =>
  z1 < floor + 72 && z2 > floor + 4 && x > x1 - 40 && x < x2 + 40 && y > y1 - 40 && y < y2 + 40
);
const candidates: [number, number, number][] = [];
// Prefer the authored arena interior. The shell margin exists for sealing and
// scenery, but spawning there would put the player outside boundary walls.
const spawnInsetX = Math.max(128, (meshMaxX - meshMinX) * 0.20);
const spawnInsetY = Math.max(128, (meshMaxY - meshMinY) * 0.20);
for (let y = Math.ceil(meshMinY + spawnInsetY); y <= Math.floor(meshMaxY - spawnInsetY); y += 64) {
  for (let x = Math.ceil(meshMinX + spawnInsetX); x <= Math.floor(meshMaxX - spawnInsetX); x += 64) {
    const floor = floorAt(x, y);
    if (!blocked(x, y, floor)) candidates.push([x, y, floor + 36]);
  }
}
// Very thin/solid source meshes may not contain a walkable interior; retain a
// safe sealed-arena fallback so conversion still produces a playable map.
if (candidates.length < 2) {
  for (let y = minY + 96; y <= maxY - 96; y += 96) {
    for (let x = minX + 96; x <= maxX - 96; x += 96) {
      const floor = floorAt(x, y);
      if (!blocked(x, y, floor)) candidates.push([x, y, floor + 36]);
    }
  }
}
if (candidates.length < 2) throw new Error("could not find two clear spawn locations");
let spawnA = candidates[0];
let spawnB = candidates[1];
let bestDistance = -1;
for (const a of candidates) for (const b of candidates) {
  const distance = (a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2;
  if (distance > bestDistance) {
    bestDistance = distance;
    spawnA = a;
    spawnB = b;
  }
}
const yawTo = (from: Vec3, to: Vec3) =>
  Math.round(Math.atan2(to[1] - from[1], to[0] - from[0]) * 180 / Math.PI);
const spawn = (classname: string, at: Vec3, toward: Vec3) => `{
"classname" "${classname}"
"angles" "0 ${yawTo(at, toward)} 0"
"origin" "${at[0]} ${at[1]} ${at[2]}"
}`;

const shell: Box[] = [
  [minX - 32, minY - 32, -32, maxX + 32, maxY + 32, 0],
  [minX - 32, minY - 32, 0, minX, maxY + 32, ceiling],
  [maxX, minY - 32, 0, maxX + 32, maxY + 32, ceiling],
  [minX, minY - 32, 0, maxX, minY, ceiling],
  [minX, maxY, 0, maxX, maxY + 32, ceiling],
  [minX - 32, minY - 32, ceiling, maxX + 32, maxY + 32, ceiling + 32],
];
const map = `{
"classname" "worldspawn"
"mapversion" "220"
"skyname" "2desert"
"MaxRange" "8192"
"_minlight" "12"
"wad" "${new URL("../assets/support/openstrike.wad", import.meta.url).pathname}"
${shell.map((box) => brush(box, "SKY")).join("\n")}
${boxes.map((box, index) => brush(box, material(box, index))).join("\n")}
}
{
"classname" "light_environment"
"angles" "-55 35 0"
"_light" "255 232 195 320"
}
{
"classname" "light"
"_light" "190 215 255 260"
"origin" "${Math.round(centerX)} ${Math.round(centerY)} ${Math.round(ceiling * 0.65)}"
}
${spawn("info_player_start", spawnA, spawnB)}
${spawn("info_player_deathmatch", spawnB, spawnA)}
`;

await Bun.write(output, map);
console.log(`imported ${faces.length} faces as ${boxes.length} brushes; spawns ${spawnA.join(",")} / ${spawnB.join(",")}`);
