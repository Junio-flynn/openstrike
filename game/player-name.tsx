import { createSignal, For, onCleanup, onMount } from "solid-js";
import { Text, View } from "@pocketjs/framework/components";
import { enableCursor, pushFocusGrid, pushFocusScope } from "@pocketjs/framework/input";
import { playerName, savePlayerName } from "./player-settings.ts";

const INK = "#e8f0f2";
const DIM = "#8fa3ad";
const LIME = "#b8f34a";
const ROWS = ["1234567890", "QWERTYUIOP", "ASDFGHJKL'", "ZXCVBNM.?!"];
const vp = (globalThis as { ui?: { __viewport?: { h: number } } }).ui?.__viewport ?? { h: 272 };
const S = Math.max(1, Math.floor(vp.h / 272));
const KEY = "items-center justify-center rounded-sm border-[#263440] bg-[#0b121a] focus:bg-[#33470f] focus:border-[#b8f34a]";

export default function PlayerNameEditor(props: { onBack: () => void }) {
  const [draft, setDraft] = createSignal(playerName());
  let grid!: Parameters<typeof pushFocusGrid>[0];
  onMount(() => {
    const cursor = enableCursor({ speed: 520, dpadSpeed: 300, size: [1, 1] });
    const focusGrid = pushFocusGrid(grid, { columns: 10, wrap: true });
    const scope = pushFocusScope(grid, { autoFocus: true });
    onCleanup(() => { scope(); focusGrid(); cursor(); });
  });
  const add = (value: string) => setDraft((name) => (name + value).slice(0, 24));
  const save = () => { savePlayerName(draft()); props.onBack(); };

  return <View class="w-full h-full justify-center items-center" style={{ bgColor: "#03070cF2" }}>
    <View class="flex-col items-center gap-2 rounded-md px-4 py-3" style={{ width: 440 * S, height: 250 * S, bgColor: "#071019F4" }}>
      <Text class={S >= 2 ? "text-3xl font-bold tracking-wide" : "text-xl font-bold tracking-wide"} style={{ textColor: INK }}>EDIT CALLSIGN</Text>
      <View class="flex-row items-center px-2 rounded-sm border-[#263440] bg-[#0b121a]" style={{ width: 370 * S, height: 28 * S }}>
        <Text class="text-sm font-bold tracking-wide" style={{ textColor: LIME }}>{draft() || "_"}</Text>
        <View class="flex-1" />
        <Text class="text-xs" style={{ textColor: DIM }}>{draft().length}/24</Text>
      </View>
      <View ref={(el) => (grid = el)} class="flex-col gap-1">
        <For each={ROWS}>{(row) => <View class="flex-row gap-1">
          <For each={row.split("")}>{(key) => <View focusable onPress={() => add(key)} class={KEY} style={{ width: 34 * S, height: 20 * S }}>
            <Text class="text-xs font-bold" style={{ textColor: INK }}>{key}</Text>
          </View>}</For>
        </View>}</For>
        <View class="flex-row gap-1 justify-center">
          <View focusable onPress={() => add(" ")} class={KEY} style={{ width: 80 * S, height: 21 * S }}><Text class="text-xs font-bold" style={{ textColor: INK }}>SPACE</Text></View>
          <View focusable onPress={() => setDraft((name) => name.slice(0, -1))} class={KEY} style={{ width: 80 * S, height: 21 * S }}><Text class="text-xs font-bold" style={{ textColor: INK }}>DELETE</Text></View>
          <View focusable onPress={save} class={KEY} style={{ width: 80 * S, height: 21 * S }}><Text class="text-xs font-bold" style={{ textColor: LIME }}>SAVE</Text></View>
          <View focusable onPress={props.onBack} class={KEY} style={{ width: 80 * S, height: 21 * S }}><Text class="text-xs font-bold" style={{ textColor: DIM }}>BACK</Text></View>
        </View>
      </View>
    </View>
  </View>;
}
