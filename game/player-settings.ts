import { createSignal } from "solid-js";
import { strike } from "./sdk.ts";

export const SENSITIVITIES = [
  { name: "0.50", value: 0.5 }, { name: "0.75", value: 0.75 },
  { name: "1.00", value: 1.0 }, { name: "1.25", value: 1.25 },
  { name: "1.50", value: 1.5 }, { name: "2.00", value: 2.0 },
] as const;
export const KEY_CHOICES = [
  "NONE", "W", "A", "S", "D", "E", "F", "UP", "DOWN", "LEFT", "RIGHT", "SPACE", "R", "SHIFT", "MOUSE1", "MOUSE2",
] as const;
export const BIND_ACTIONS = ["FORWARD", "BACK", "LEFT", "RIGHT", "JUMP", "RELOAD", "FIRE", "WALK"] as const;
const DEFAULT_BINDS: readonly (readonly [number, number])[] = [
  [1, 7], [3, 8], [2, 9], [4, 10], [11, 0], [12, 0], [14, 0], [13, 0],
];
export const CROSSHAIR_COLORS = [
  { name: "LIME", value: "#b8f34a" }, { name: "WHITE", value: "#e8f0f2" },
  { name: "RED", value: "#f43f3f" }, { name: "AMBER", value: "#fbbf24" },
  { name: "CYAN", value: "#38d9ff" },
] as const;

type SavedSettings = {
  playerName?: string;
  sensitivity?: number; scopeMultiplier?: number; bindings?: [number, number][]; color?: number; bars?: boolean;
  dot?: boolean; dotSize?: number; gap?: number; length?: number; thickness?: number;
  outline?: boolean; outlineSize?: number; outlineColor?: number; tilt?: number;
  viewmodelScale?: number; viewmodelX?: number; viewmodelY?: number; viewmodelZ?: number;
  viewmodelYaw?: number; viewmodelPitch?: number;
  viewmodels?: ViewmodelSettings[];
};
type ViewmodelSettings = {
  scale: number; x: number; y: number; z: number; yaw: number; pitch: number;
};
export const VIEWMODEL_WEAPONS = ["SCORPION", "AK-74M", "UZI", "SNIPER"] as const;
const DEFAULT_VIEWMODELS: readonly ViewmodelSettings[] = VIEWMODEL_WEAPONS.map(() => ({
  scale: 2.2, x: 0, y: 0, z: 0, yaw: 0, pitch: 0,
}));
let saved: SavedSettings = {};
try { saved = JSON.parse(strike.savedSettings() || "{}") as SavedSettings; } catch { saved = {}; }
const num = (value: unknown, fallback: number) =>
  typeof value === "number" && Number.isFinite(value) ? value : fallback;
const index = (value: unknown, length: number, fallback: number) =>
  Math.max(0, Math.min(length - 1, Math.floor(num(value, fallback))));
const cleanPlayerName = (value: unknown) => {
  const cleaned = String(value ?? "").replace(/[^A-Za-z0-9 .'!?_-]/g, "").slice(0, 24).trim();
  return cleaned || "Player";
};

export const [playerName, setPlayerNameSignal] = createSignal(
  cleanPlayerName(saved.playerName ?? strike.initialPlayerName),
);

export const [sensitivityIndex, setSensitivityIndex] = createSignal(Math.max(0, Math.min(SENSITIVITIES.length - 1, num(saved.sensitivity, 2))));
export const [scopeMultiplier, setScopeMultiplierSignal] = createSignal(
  Math.max(0.1, Math.min(2.0, num(saved.scopeMultiplier, 1.0))),
);
export function setScopeMultiplier(value: number) {
  const next = Math.max(0.1, Math.min(2.0, value));
  setScopeMultiplierSignal(next);
  strike.setScopeMultiplier(next);
}
const initialBindings = saved.bindings?.length === DEFAULT_BINDS.length
  ? saved.bindings.map((pair) => [index(pair[0], KEY_CHOICES.length, 0), index(pair[1], KEY_CHOICES.length, 0)] as [number, number])
  : DEFAULT_BINDS.map((pair) => [...pair] as [number, number]);
const [bindings, setBindings] = createSignal(initialBindings);
export { bindings };

export const [crosshairColorIndex, setCrosshairColorIndex] = createSignal(index(saved.color, CROSSHAIR_COLORS.length, 0));
export const [crosshairBars, setCrosshairBars] = createSignal(saved.bars ?? true);
export const [crosshairDot, setCrosshairDot] = createSignal(saved.dot ?? false);
export const [crosshairDotSize, setCrosshairDotSize] = createSignal(num(saved.dotSize, 2.0));
export const [crosshairGap, setCrosshairGap] = createSignal(num(saved.gap, 11.0));
export const [crosshairLength, setCrosshairLength] = createSignal(num(saved.length, 8.0));
export const [crosshairThickness, setCrosshairThickness] = createSignal(num(saved.thickness, 2.0));
export const [crosshairOutline, setCrosshairOutline] = createSignal(saved.outline ?? true);
export const [crosshairOutlineSize, setCrosshairOutlineSize] = createSignal(num(saved.outlineSize, 1.0));
export const [crosshairOutlineColorIndex, setCrosshairOutlineColorIndex] = createSignal(index(saved.outlineColor, CROSSHAIR_COLORS.length, 1));
export const [crosshairTilt, setCrosshairTilt] = createSignal(num(saved.tilt, 0.0));

const legacyViewmodel: ViewmodelSettings = {
  scale: num(saved.viewmodelScale, 2.2), x: num(saved.viewmodelX, 0),
  y: num(saved.viewmodelY, 0), z: num(saved.viewmodelZ, 0),
  yaw: num(saved.viewmodelYaw, 0), pitch: num(saved.viewmodelPitch, 0),
};
const initialViewmodels = DEFAULT_VIEWMODELS.map((defaults, slot) => {
  const source = saved.viewmodels?.[slot] ?? (slot === 0 ? legacyViewmodel : defaults);
  return {
    scale: Math.max(0.5, Math.min(12, num(source.scale, defaults.scale))),
    x: num(source.x, defaults.x), y: num(source.y, defaults.y), z: num(source.z, defaults.z),
    yaw: num(source.yaw, defaults.yaw), pitch: num(source.pitch, defaults.pitch),
  };
});
export const [viewmodelWeaponIndex, setViewmodelWeaponIndex] = createSignal(0);
const [viewmodels, setViewmodels] = createSignal(initialViewmodels);
const selectedViewmodel = () => viewmodels()[viewmodelWeaponIndex()];
export const viewmodelScale = () => selectedViewmodel().scale;
export const viewmodelX = () => selectedViewmodel().x;
export const viewmodelY = () => selectedViewmodel().y;
export const viewmodelZ = () => selectedViewmodel().z;
export const viewmodelYaw = () => selectedViewmodel().yaw;
export const viewmodelPitch = () => selectedViewmodel().pitch;

function applyViewmodelSettings(slot: number) {
  const vm = viewmodels()[slot];
  strike.setViewmodel(slot, vm.scale, vm.x, vm.y, vm.z, vm.yaw, vm.pitch);
}
const viewmodelSetter = (field: keyof ViewmodelSettings) => (value: number) => {
  const slot = viewmodelWeaponIndex();
  setViewmodels((current) => current.map((vm, i) => i === slot ? { ...vm, [field]: value } : vm));
  applyViewmodelSettings(slot);
};
export const setViewmodelScale = viewmodelSetter("scale");
export const setViewmodelX = viewmodelSetter("x");
export const setViewmodelY = viewmodelSetter("y");
export const setViewmodelZ = viewmodelSetter("z");
export const setViewmodelYaw = viewmodelSetter("yaw");
export const setViewmodelPitch = viewmodelSetter("pitch");
export function cycleViewmodelWeapon() {
  const next = (viewmodelWeaponIndex() + 1) % VIEWMODEL_WEAPONS.length;
  setViewmodelWeaponIndex(next);
  strike.selectWeapon(next);
}

export const crosshairColor = () => CROSSHAIR_COLORS[crosshairColorIndex()].value;
export const crosshairOutlineColor = () => CROSSHAIR_COLORS[crosshairOutlineColorIndex()].value;

export function savePlayerSettings() {
  strike.saveSettings(JSON.stringify({
    playerName: playerName(),
    sensitivity: sensitivityIndex(), scopeMultiplier: scopeMultiplier(), bindings: bindings(), color: crosshairColorIndex(),
    bars: crosshairBars(), dot: crosshairDot(), dotSize: crosshairDotSize(), gap: crosshairGap(),
    length: crosshairLength(), thickness: crosshairThickness(), outline: crosshairOutline(),
    outlineSize: crosshairOutlineSize(), outlineColor: crosshairOutlineColorIndex(), tilt: crosshairTilt(),
    viewmodels: viewmodels(),
  }));
}
export function savePlayerName(value: string) {
  const cleaned = cleanPlayerName(value);
  setPlayerNameSignal(cleaned);
  strike.setPlayerName(cleaned);
  savePlayerSettings();
}
export function cycleSensitivity() {
  const next = (sensitivityIndex() + 1) % SENSITIVITIES.length;
  setSensitivityIndex(next);
  strike.setMouseSensitivity(SENSITIVITIES[next].value);
  savePlayerSettings();
}
export function cycleBinding(action: number, slot: number) {
  const next = bindings().map((pair) => [...pair] as [number, number]);
  next[action][slot] = (next[action][slot] + 1) % KEY_CHOICES.length;
  setBindings(next);
  strike.setKeybind(action, slot, next[action][slot]);
  savePlayerSettings();
}
export function stepValue(setter: (value: number) => void, value: number, delta: number, min: number, max: number) {
  setter(Math.round(Math.max(min, Math.min(max, value + delta)) * 10) / 10);
}

strike.setMouseSensitivity(SENSITIVITIES[sensitivityIndex()].value);
strike.setScopeMultiplier(scopeMultiplier());
strike.setPlayerName(playerName());
for (let action = 0; action < bindings().length; action++) {
  strike.setKeybind(action, 0, bindings()[action][0]);
  strike.setKeybind(action, 1, bindings()[action][1]);
}
for (let slot = 0; slot < VIEWMODEL_WEAPONS.length; slot++) applyViewmodelSettings(slot);
