const out = Bun.argv[2];
export {};
const wad = Bun.argv[3];
if (!out || !wad) throw new Error("usage: bun generate-open-arena.ts <out.map> <halflife.wad>");

type Box = [number, number, number, number, number, number, string];
const brush = ([x1, y1, z1, x2, y2, z2, t]: Box) => `{
( ${x1} ${y1} ${z2} ) ( ${x1} ${y1} ${z1} ) ( ${x1} ${y2} ${z2} ) ${t} [ 0 1 0 0 ] [ 0 0 -1 0 ] 0 1 1
( ${x2} ${y2} ${z2} ) ( ${x2} ${y2} ${z1} ) ( ${x2} ${y1} ${z2} ) ${t} [ 0 1 0 0 ] [ 0 0 -1 0 ] 0 1 1
( ${x1} ${y2} ${z2} ) ( ${x1} ${y2} ${z1} ) ( ${x2} ${y2} ${z2} ) ${t} [ 1 0 0 0 ] [ 0 0 -1 0 ] 0 1 1
( ${x2} ${y1} ${z2} ) ( ${x2} ${y1} ${z1} ) ( ${x1} ${y1} ${z2} ) ${t} [ 1 0 0 0 ] [ 0 0 -1 0 ] 0 1 1
( ${x1} ${y2} ${z1} ) ( ${x1} ${y1} ${z1} ) ( ${x2} ${y2} ${z1} ) ${t} [ 1 0 0 0 ] [ 0 -1 0 0 ] 0 1 1
( ${x2} ${y1} ${z2} ) ( ${x1} ${y1} ${z2} ) ( ${x2} ${y2} ${z2} ) ${t} [ 1 0 0 0 ] [ 0 -1 0 0 ] 0 1 1
}`;

const world: Box[] = [
  [-2080, -2080, -32, 2080, 2080, 0, "GENERIC015T"],
  [-2080, -2080, 0, -2048, 2080, 768, "SKY"],
  [2048, -2080, 0, 2080, 2080, 768, "SKY"],
  [-2048, -2080, 0, 2048, -2048, 768, "SKY"],
  [-2048, 2048, 0, 2048, 2080, 768, "SKY"],
  [-2080, -2080, 768, 2080, 2080, 800, "SKY"],
  // Central low cover: enough protection to cross, never tall enough to hide the horizon.
  [-224, -48, 0, -64, 48, 56, "GENERIC015Q"],
  [64, -48, 0, 224, 48, 56, "GENERIC015Q"],
  [-48, -224, 0, 48, -64, 56, "GENERIC015Q"],
  [-48, 64, 0, 48, 224, 56, "GENERIC015Q"],
  // Sparse waist-high perimeter blocks preserve broad diagonal sightlines.
  [-1560, -1160, 0, -1360, -1000, 64, "GENERIC040"],
  [1360, -1160, 0, 1560, -1000, 64, "GENERIC040"],
  [-1560, 1000, 0, -1360, 1160, 64, "GENERIC040"],
  [1360, 1000, 0, 1560, 1160, 64, "GENERIC040"],
  // Tall, thin parkour lanes. Their long faces give the player room to hold
  // jump and follow the wallrun arc without closing off the arena sightlines.
  [-920, -640, 0, -896, 640, 192, "GENERIC015Q"],
  [896, -640, 0, 920, 640, 192, "GENERIC015Q"],
  [-640, -920, 0, 640, -896, 192, "GENERIC015Q"],
  [-640, 896, 0, 640, 920, 192, "GENERIC015Q"],
];

const spawn = (x: number, y: number, yaw: number) => `{
"classname" "info_player_start"
"angles" "0 ${yaw} 0"
"origin" "${x} ${y} 36"
}`;
const light = (x: number, y: number) => `{
"classname" "light"
"_light" "255 248 230 750"
"origin" "${x} ${y} 384"
}`;
const daylight = `{
"classname" "light_environment"
"angles" "-55 35 0"
"_light" "235 242 255 500"
}`;

const map = `{
"classname" "worldspawn"
"mapversion" "220"
"skyname" "2desert"
"MaxRange" "8192"
"_minlight" "64"
"wad" "${wad}"
${world.map(brush).join("\n")}
}
${daylight}
${[
  [-1680, -1680, 45],
  [1680, 1680, 225],
].map(([x, y, yaw]) => spawn(x, y, yaw)).join("\n")}
${[[-1500, -1500], [1500, -1500], [-1500, 1500], [1500, 1500], [0, 0]].map(([x, y]) => light(x, y)).join("\n")}
`;

await Bun.write(out, map);
console.log(`generated ${out}`);
