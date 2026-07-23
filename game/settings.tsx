import { createSignal, For, Match, Show, Switch, onCleanup, onMount, type JSX } from "solid-js";
import { Text, View } from "@pocketjs/framework/components";
import { cursorX, enableCursor, getFocused, pushFocusScope } from "@pocketjs/framework/input";
import { BTN } from "@pocketjs/framework/input-api";
import { onFrame } from "@pocketjs/framework/lifecycle";
import PlayerCrosshair from "./crosshair-preview.tsx";
import {
  BIND_ACTIONS, CROSSHAIR_COLORS, KEY_CHOICES, SENSITIVITIES, VIEWMODEL_WEAPONS, bindings, crosshairBars,
  crosshairColorIndex, crosshairDot, crosshairDotSize, crosshairGap, crosshairLength,
  crosshairOutline, crosshairOutlineColorIndex, crosshairOutlineSize, crosshairThickness,
  crosshairTilt, cycleBinding, cycleSensitivity, cycleViewmodelWeapon, sensitivityIndex, setCrosshairBars, setCrosshairColorIndex, setCrosshairDot,
  setCrosshairDotSize, setCrosshairGap, setCrosshairLength, setCrosshairOutline,
  setCrosshairOutlineColorIndex, setCrosshairOutlineSize, setCrosshairThickness,
  setCrosshairTilt, setViewmodelPitch, setViewmodelScale, setViewmodelX, setViewmodelY,
  setViewmodelYaw, setViewmodelZ, stepValue, savePlayerSettings, viewmodelPitch,
  viewmodelScale, viewmodelWeaponIndex, viewmodelX, viewmodelY, viewmodelYaw, viewmodelZ,
  scopeMultiplier, setScopeMultiplier,
} from "./player-settings.ts";

const INK = "#e8f0f2";
const DIM = "#8fa3ad";
const LIME = "#b8f34a";
const vp = (globalThis as { ui?: { __viewport?: { h: number } } }).ui?.__viewport ?? { h: 272 };
const S = Math.max(1, Math.round(vp.h / 272));
const BUTTON = "px-2 py-1 rounded-sm border-[#0a121a00] bg-[#111a24] transition-colors duration-100 focus:bg-[#33470f] focus:border-[#b8f34a]";
const ROW = "flex-row items-center px-2 rounded-sm bg-[#0a121a80]";

export default function Settings(props: { onMainMenu: () => void }) {
  const [page, setPage] = createSignal<"home" | "controls" | "crosshair" | "viewmodel">("home");
  onMount(() => {
    const disposeCursor = enableCursor({ speed: 520, dpadSpeed: 300, size: [1, 1] });
    onCleanup(disposeCursor);
  });
  return <Switch>
    <Match when={page() === "home"}><SettingsHome onControls={() => setPage("controls")} onCrosshair={() => setPage("crosshair")} onViewmodel={() => setPage("viewmodel")} onMainMenu={props.onMainMenu} /></Match>
    <Match when={page() === "controls"}><ControlSettings onBack={() => setPage("home")} /></Match>
    <Match when={page() === "crosshair"}><CrosshairSettings onBack={() => setPage("home")} /></Match>
    <Match when={page() === "viewmodel"}><ViewmodelSettings onBack={() => setPage("home")} /></Match>
  </Switch>;
}

function Screen(props: { title: string; children: JSX.Element; preview?: boolean }) {
  let panel!: Parameters<typeof pushFocusScope>[0];
  onMount(() => {
    const dispose = pushFocusScope(panel, { autoFocus: true });
    onCleanup(dispose);
  });
  return (
    <View class="w-full h-full justify-center items-center" style={{ bgColor: props.preview ? "#05080cB8" : "#05080cE8" }}>
      <View ref={(el) => (panel = el)} class="flex-col items-center gap-1">
        <Text class={S >= 2 ? "text-4xl font-bold tracking-wide" : "text-xl font-bold tracking-wide"} style={{ textColor: INK }}>{props.title}</Text>
        {props.children}
      </View>
    </View>
  );
}

function SettingsHome(props: { onControls: () => void; onCrosshair: () => void; onViewmodel: () => void; onMainMenu: () => void }) {
  return <Screen title="PLAYER SETTINGS">
    <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>AIM, INPUT, AND WEAPON PRESENTATION</Text>
    <SettingsEntry tag="IN" title="CONTROLS" detail="KEYBINDS · SENSITIVITY · SCOPE MULTIPLIER" onPress={props.onControls} />
    <SettingsEntry tag="CH" title="CROSSHAIR" detail="COLOR · BARS · GAP · OUTLINE" onPress={props.onCrosshair} />
    <SettingsEntry tag="VM" title="VIEWMODEL" detail="PER-WEAPON SIZE · POSITION · ROTATION" onPress={props.onViewmodel} />
    <SettingsEntry tag="←" title="MAIN MENU" detail="RETURN TO OPERATIONS COMMAND" onPress={props.onMainMenu} />
    <Hint />
  </Screen>;
}

function SettingsEntry(props: { tag: string; title: string; detail: string; onPress: () => void }) {
  return <View focusable onPress={props.onPress} class="flex-row items-center px-2 py-1 rounded-sm border-[#0a121a00] bg-[#111a24] transition-colors duration-100 focus:bg-[#33470f] focus:border-[#b8f34a]" style={{ width: 300 * S, height: 30 * S }}>
    <View class="justify-center items-center rounded-sm" style={{ width: 24 * S, height: 20 * S, bgColor: "#263440" }}>
      <Text class="text-xs font-bold" style={{ textColor: LIME }}>{props.tag}</Text>
    </View>
    <View class="flex-col ml-2">
      <Text class="text-xs font-bold tracking-wide" style={{ textColor: INK }}>{props.title}</Text>
      <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>{props.detail}</Text>
    </View>
    <View class="flex-1" />
    <Text class="text-sm font-bold" style={{ textColor: LIME }}>›</Text>
  </View>;
}

function ViewmodelSettings(props: { onBack: () => void }) {
  return <Screen title="VIEWMODEL" preview>
    <View class="flex-col" style={{ width: 310 * S }}>
      <Option label="WEAPON" value={VIEWMODEL_WEAPONS[viewmodelWeaponIndex()]} onPress={cycleViewmodelWeapon} />
      <SliderOption label="SIZE" value={viewmodelScale()} min={0.5} max={12} set={setViewmodelScale} />
      <SliderOption label="X AXIS" value={viewmodelX()} min={-20} max={20} step={1} set={setViewmodelX} />
      <SliderOption label="Y AXIS" value={viewmodelY()} min={-20} max={20} step={1} set={setViewmodelY} />
      <SliderOption label="Z AXIS" value={viewmodelZ()} min={-20} max={20} step={1} set={setViewmodelZ} />
      <SliderOption label="YAW" value={viewmodelYaw()} min={-60} max={60} step={1} set={setViewmodelYaw} suffix="°" />
      <SliderOption label="PITCH" value={viewmodelPitch()} min={-60} max={60} step={1} set={setViewmodelPitch} suffix="°" />
    </View>
    <View focusable onPress={props.onBack} class={BUTTON}><Label text="BACK" /></View>
    <Hint />
  </Screen>;
}

function ControlSettings(props: { onBack: () => void }) {
  return <Screen title="KEYBINDS">
    <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>1.0 SCOPE MULTIPLIER KEEPS NORMAL SENSITIVITY</Text>
    <View focusable onPress={cycleSensitivity} class="flex-row items-center px-2 rounded-sm border-[#0a121a00] bg-[#0a121a80] focus:bg-[#33470f] focus:border-[#b8f34a]" style={{ width: 320 * S }}>
      <Text class="text-xs font-bold tracking-wide" style={{ textColor: INK }}>MOUSE SENSITIVITY</Text><View class="flex-1" />
      <Text class="text-xs font-bold" style={{ textColor: LIME }}>{SENSITIVITIES[sensitivityIndex()].name + "  ›"}</Text>
    </View>
    <View style={{ width: 320 * S }}>
      <SliderOption label="SCOPE MULTIPLIER" value={scopeMultiplier()} min={0.1} max={2.0} set={setScopeMultiplier} />
    </View>
    <View class="flex-row px-2" style={{ width: 320 * S }}>
      <Text class="text-xs tracking-wide" style={{ textColor: DIM, width: 120 * S }}>ACTION</Text>
      <Text class="text-xs tracking-wide" style={{ textColor: DIM, width: 84 * S }}>PRIMARY</Text>
      <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>SECONDARY</Text>
    </View>
    <View class="flex-col gap-1" style={{ width: 320 * S }}>
      <For each={BIND_ACTIONS}>{(action, i) =>
        <View class={ROW}>
          <Text class="text-xs font-bold tracking-wide" style={{ textColor: INK, width: 120 * S }}>{action}</Text>
          <BindButton value={KEY_CHOICES[bindings()[i()][0]]} onPress={() => cycleBinding(i(), 0)} />
          <BindButton value={KEY_CHOICES[bindings()[i()][1]]} onPress={() => cycleBinding(i(), 1)} />
        </View>
      }</For>
    </View>
    <View focusable onPress={props.onBack} class={BUTTON}><Label text="BACK" /></View>
    <Hint />
  </Screen>;
}

function BindButton(props: { value: string; onPress: () => void }) {
  return <View focusable onPress={props.onPress} class={BUTTON} style={{ width: 82 * S }}>
    <Text class="text-xs font-bold" style={{ textColor: LIME }}>{props.value}</Text>
  </View>;
}

function CrosshairSettings(props: { onBack: () => void }) {
  const toggle = (v: boolean) => v ? "ON" : "OFF";
  const change = (fn: () => void) => { fn(); savePlayerSettings(); };
  return <Screen title="CROSSHAIR">
    <View class="justify-center items-center rounded-sm" style={{ width: 54 * S, height: 54 * S, bgColor: "#111a24" }}>
      <PlayerCrosshair scale={S} box={48} />
    </View>
    <View class="flex-col" style={{ width: 310 * S }}>
      <Option label="COLOR" value={CROSSHAIR_COLORS[crosshairColorIndex()].name} onPress={() => change(() => setCrosshairColorIndex((crosshairColorIndex() + 1) % CROSSHAIR_COLORS.length))} />
      <Option label="BARS" value={toggle(crosshairBars())} onPress={() => change(() => setCrosshairBars(!crosshairBars()))} />
      <Option label="CENTER DOT" value={toggle(crosshairDot())} onPress={() => change(() => setCrosshairDot(!crosshairDot()))} />
      <SliderOption label="DOT SIZE" value={crosshairDotSize()} min={0.5} max={8} set={setCrosshairDotSize} />
      <SliderOption label="GAP" value={crosshairGap()} min={0} max={24} set={setCrosshairGap} />
      <SliderOption label="BAR LENGTH" value={crosshairLength()} min={0.5} max={24} set={setCrosshairLength} />
      <SliderOption label="THICKNESS" value={crosshairThickness()} min={0.5} max={8} set={setCrosshairThickness} />
      <Option label="OUTLINE" value={toggle(crosshairOutline())} onPress={() => change(() => setCrosshairOutline(!crosshairOutline()))} />
      <SliderOption label="OUTLINE SIZE" value={crosshairOutlineSize()} min={0.1} max={5} set={setCrosshairOutlineSize} />
      <Option label="OUTLINE COLOR" value={CROSSHAIR_COLORS[crosshairOutlineColorIndex()].name} onPress={() => change(() => setCrosshairOutlineColorIndex((crosshairOutlineColorIndex() + 1) % CROSSHAIR_COLORS.length))} />
      <SliderOption label="TILT" value={crosshairTilt()} min={0} max={90} set={setCrosshairTilt} suffix="°" />
    </View>
    <View focusable onPress={props.onBack} class={BUTTON}><Label text="BACK" /></View>
  </Screen>;
}

function Option(props: { label: string; value: string; onPress: () => void }) {
  return <View focusable onPress={props.onPress} class="flex-row items-center px-2 rounded-sm border-[#0a121a00] bg-[#0a121a80] focus:bg-[#33470f] focus:border-[#b8f34a]" style={{ height: 14 * S }}>
    <Text class="text-xs font-bold tracking-wide" style={{ textColor: INK }}>{props.label}</Text><View class="flex-1" />
    <Text class="text-xs font-bold tracking-wide" style={{ textColor: LIME }}>{props.value + "  ›"}</Text>
  </View>;
}
function SliderOption(props: { label: string; value: number; min: number; max: number; step?: number; set: (value: number) => void; suffix?: string }) {
  const frac = () => (props.value - props.min) / (props.max - props.min);
  let track!: NonNullable<ReturnType<typeof getFocused>>;
  let dragging = false;
  let lastX = 0;
  onFrame((buttons) => {
    const down = (buttons & BTN.CIRCLE) !== 0;
    if (!down) {
      if (dragging) savePlayerSettings();
      dragging = false;
      return;
    }
    const x = cursorX();
    if (!dragging) {
      if (getFocused() !== track || !Number.isFinite(x)) return;
      dragging = true;
      lastX = x;
      return;
    }
    if (!Number.isFinite(x) || x === lastX) return;
    const range = props.max - props.min;
    const next = Math.round(Math.max(props.min, Math.min(props.max, props.value + (x - lastX) * range / (100 * S))) * 10) / 10;
    lastX = x;
    props.set(next);
  });
  return <View class="flex-row items-center px-2 bg-[#0a121a80]" style={{ height: 16 * S }}>
    <Text class="text-xs font-bold tracking-wide" style={{ textColor: INK, width: 88 * S }}>{props.label}</Text>
    <View focusable onPress={() => { stepValue(props.set, props.value, -(props.step ?? 0.1), props.min, props.max); savePlayerSettings(); }} class={BUTTON}><Text class="text-xs font-bold" style={{ textColor: LIME }}>−</Text></View>
    <View ref={(el) => (track = el)} focusable class="mx-1 border-[#0a121a00] focus:border-[#b8f34a]" style={{ width: 100 * S, height: 5 * S, bgColor: "#263440" }}>
      <View style={{ width: 100 * S * frac(), height: 3 * S, bgColor: LIME }} />
    </View>
    <View focusable onPress={() => { stepValue(props.set, props.value, props.step ?? 0.1, props.min, props.max); savePlayerSettings(); }} class={BUTTON}><Text class="text-xs font-bold" style={{ textColor: LIME }}>+</Text></View>
    <Text class="text-xs font-bold" style={{ textColor: INK, width: 48 * S }}>{props.value.toFixed(1) + (props.suffix ?? "")}</Text>
  </View>;
}
function Label(props: { text: string }) { return <Text class="text-xs font-bold tracking-wide" style={{ textColor: INK }}>{props.text}</Text>; }
function Hint() { return <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>ARROWS SELECT · ENTER CHANGE</Text>; }
