// OpenStrike's two-stage front end: the main command screen leads to a
// visual map picker. Both pages share one focus scope so mouse, keyboard and
// handheld controls all activate the same buttons.

import { createSignal, For, Show, onCleanup, onMount } from "solid-js";
import { Image, Text, View } from "@pocketjs/framework/components";
import { enableCursor, pushFocusGrid, pushFocusScope } from "@pocketjs/framework/input";
import { strike } from "./sdk.ts";
import { playerName } from "./player-settings.ts";

const INK = "#e8f0f2";
const DIM = "#8fa3ad";
const LIME = "#b8f34a";
const AMBER = "#fbbf24";
const vp = (globalThis as { ui?: { __viewport?: { w: number; h: number } } }).ui
  ?.__viewport ?? { w: 480, h: 272 };
const S = Math.max(1, Math.floor(vp.h / 272));

const BUTTON =
  "flex-row items-center gap-2 px-3 rounded-sm border-[#263440] bg-[#0b121a] transition-all duration-100 focus:bg-[#33470f] focus:border-[#b8f34a] focus:translate-x-1";
const MAP_CARD =
  "flex-col p-1 rounded-sm border-[#263440] bg-[#0b121a] transition-all duration-100 focus:bg-[#33470f] focus:border-[#b8f34a] focus:translate-y-1";

const pretty = (raw: string): string => raw.replaceAll("_", " ").toUpperCase();
const preview = (raw: string): string => {
  if (raw === "arena_map_v3") return "map-arena-map-v3.png";
  if (raw === "small_pvp_arena") return "map-small-pvp-arena.png";
  return "map-open-arena.png";
};

export default function MainMenu(props: {
  onOpenSettings: () => void;
  onOpenLobby: () => void;
  onEditName: () => void;
  onPlayDesktop: () => void;
  onQuit: () => void;
}) {
  const [mapPage, setMapPage] = createSignal(false);
  const [loading, setLoading] = createSignal(-1);
  const chooseMap = (index: number) => {
    if (loading() >= 0) return;
    setLoading(index);
    strike.loadMap(index);
    if (strike.desktopMapHost) props.onPlayDesktop();
  };

  let controls!: Parameters<typeof pushFocusGrid>[0];
  onMount(() => {
    const cursor = enableCursor({ speed: 520, dpadSpeed: 300, size: [1, 1] });
    const grid = pushFocusGrid(controls, { columns: 3, wrap: true });
    const scope = pushFocusScope(controls, { autoFocus: true });
    onCleanup(() => {
      scope();
      grid();
      cursor();
    });
  });

  return (
    <View class="w-full h-full justify-center items-center" style={{ bgColor: "#03070cF2" }}>
      <View class="flex-col rounded-md" style={{ width: 440 * S, height: 250 * S, bgColor: "#071019F4" }}>
        <View class="flex-row items-center px-4 py-2" style={{ height: 46 * S }}>
          <View style={{ width: 4 * S, height: 28 * S, bgColor: LIME }} />
          <View class="flex-col ml-2">
            <Text class={S >= 2 ? "text-4xl font-bold tracking-wide" : "text-xl font-bold tracking-wide"} style={{ textColor: INK }}>
              OPENSTRIKE
            </Text>
            <Text class="text-xs font-bold tracking-wide" style={{ textColor: DIM }}>
              {mapPage() ? "SELECT A MAP" : "MAIN MENU"}
            </Text>
          </View>
          <View class="flex-1" />
          <View class="flex-row items-center gap-1 px-2 py-1 rounded-sm" style={{ bgColor: "#101c25" }}>
            <View style={{ width: 5 * S, height: 5 * S, bgColor: LIME }} />
            <Text class="text-xs font-bold tracking-wide" style={{ textColor: LIME }}>READY</Text>
          </View>
        </View>

        <View style={{ width: 440 * S, height: 1 * S, bgColor: "#263440" }} />

        <View ref={(el) => (controls = el)} class="flex-1 px-3 py-2">
          <Show
            when={mapPage()}
            fallback={
              <View class="flex-row gap-3 h-full">
                <View class="flex-col justify-center gap-2" style={{ width: 150 * S }}>
                  <Text class="text-lg font-bold tracking-wide" style={{ textColor: INK }}>READY TO PLAY?</Text>
                  <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>
                    PICK A MAP, CHOOSE YOUR LOADOUT, AND START THE MATCH.
                  </Text>
                  <Text class="text-xs font-bold tracking-wide" style={{ textColor: LIME }}>
                    {String(Math.max(1, strike.maps.length)).padStart(2, "0")} MAPS INSTALLED
                  </Text>
                </View>
                <View style={{ width: 1 * S, height: 150 * S, bgColor: "#263440" }} />
                <View class="flex-col justify-center gap-1 flex-1">
                  <Show
                    when={strike.desktopMapHost}
                    fallback={
                      <View focusable onPress={() => setMapPage(true)} class={BUTTON} style={{ width: 238 * S, height: 34 * S }}>
                        <Text class="text-sm font-bold" style={{ textColor: LIME, width: 34 * S }}>PLAY</Text>
                        <Text class="text-sm font-bold tracking-wide" style={{ textColor: INK }}>CHOOSE MAP</Text>
                      </View>
                    }
                  >
                    <View focusable onPress={props.onPlayDesktop} class={BUTTON} style={{ width: 238 * S, height: 34 * S }}>
                      <Text class="text-sm font-bold" style={{ textColor: LIME, width: 34 * S }}>PLAY</Text>
                      <Text class="text-sm font-bold tracking-wide" style={{ textColor: INK }}>RESUME MATCH</Text>
                    </View>
                    <View focusable onPress={() => setMapPage(true)} class={BUTTON} style={{ width: 238 * S, height: 27 * S }}>
                      <Text class="text-xs font-bold" style={{ textColor: LIME }}>MAPS</Text>
                      <Text class="text-sm font-bold tracking-wide" style={{ textColor: INK }}>CHOOSE MAP</Text>
                    </View>
                  </Show>
                  <View focusable onPress={props.onOpenSettings} class={BUTTON} style={{ width: 238 * S, height: 27 * S }}>
                    <Text class="text-xs font-bold" style={{ textColor: LIME }}>CFG</Text>
                    <Text class="text-sm font-bold" style={{ textColor: INK }}>SETTINGS</Text>
                  </View>
                  <View focusable onPress={props.onEditName} class={BUTTON} style={{ width: 238 * S, height: 27 * S }}>
                    <Text class="text-xs font-bold" style={{ textColor: LIME }}>ID</Text>
                    <Text class="text-sm font-bold" style={{ textColor: INK }}>CALLSIGN</Text>
                    <View class="flex-1" />
                    <Text class="text-xs" style={{ textColor: DIM }}>{playerName()}</Text>
                  </View>
                  <Show when={strike.desktopMapHost}>
                    <View focusable onPress={props.onOpenLobby} class={BUTTON} style={{ width: 238 * S, height: 27 * S }}>
                      <Text class="text-xs font-bold" style={{ textColor: LIME }}>LAN</Text>
                      <Text class="text-sm font-bold" style={{ textColor: INK }}>MULTIPLAYER</Text>
                    </View>
                    <View focusable onPress={props.onQuit} class={BUTTON} style={{ width: 238 * S, height: 27 * S }}>
                      <Text class="text-xs font-bold" style={{ textColor: "#f43f3f" }}>X</Text>
                      <Text class="text-sm font-bold" style={{ textColor: INK }}>QUIT</Text>
                    </View>
                  </Show>
                </View>
              </View>
            }
          >
            <View class="flex-col h-full gap-2">
              <View class="flex-row items-center">
                <Text class="text-xs font-bold tracking-wide" style={{ textColor: LIME }}>
                  {strike.maps.length === 1 ? "CHOOSE THE INSTALLED MAP" : `CHOOSE ONE OF ${strike.maps.length} MAPS`}
                </Text>
                <View class="flex-1" />
                <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>{loading() < 0 ? "CLICK TO PLAY" : "LOADING…"}</Text>
              </View>
              <View class="flex-row gap-2">
                <For each={strike.maps as string[]}>
                  {(raw, index) => (
                    <View focusable onPress={() => chooseMap(index())} class={MAP_CARD} style={{ width: 132 * S, height: 112 * S }}>
                      <Image src={preview(raw)} style={{ width: 122 * S, height: 61 * S }} />
                      <View class="flex-row items-center px-1 mt-1">
                        <View style={{ width: 3 * S, height: 25 * S, bgColor: loading() === index() ? AMBER : LIME }} />
                        <View class="flex-col ml-1">
                          <Text class="text-xs font-bold tracking-wide" style={{ textColor: INK }}>{pretty(raw)}</Text>
                          <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>{loading() === index() ? "LOADING" : "PLAY MAP"}</Text>
                        </View>
                      </View>
                    </View>
                  )}
                </For>
              </View>
              <View focusable onPress={() => setMapPage(false)} class={BUTTON} style={{ width: 86 * S, height: 24 * S }}>
                <Text class="text-xs font-bold" style={{ textColor: LIME }}>← BACK</Text>
              </View>
            </View>
          </Show>
        </View>

        <View class="flex-row items-center px-3" style={{ height: 24 * S, bgColor: "#0b141d" }}>
          <Text class="text-xs font-bold tracking-wide" style={{ textColor: LIME }}>NAV</Text>
          <Text class="text-xs tracking-wide ml-1" style={{ textColor: DIM }}>ARROWS / D-PAD</Text>
          <View class="flex-1" />
          <Text class="text-xs font-bold tracking-wide" style={{ textColor: LIME }}>SELECT</Text>
          <Text class="text-xs tracking-wide ml-1" style={{ textColor: DIM }}>ENTER / CIRCLE</Text>
        </View>
      </View>
    </View>
  );
}
