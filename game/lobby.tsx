import { createSignal, For, Show, onCleanup, onMount } from "solid-js";
import { Text, View } from "@pocketjs/framework/components";
import { enableCursor, pushFocusGrid, pushFocusScope } from "@pocketjs/framework/input";
import { strike, type StrikeState } from "./sdk.ts";

const INK = "#e8f0f2";
const DIM = "#8fa3ad";
const LIME = "#b8f34a";
const ROW =
  "flex-row items-center gap-2 px-3 rounded-sm border-[#263440] bg-[#0b121a] transition-all duration-100 focus:bg-[#33470f] focus:border-[#b8f34a] focus:translate-x-1";

type Lobby = { name: string; address: string; players: string; map: string };
const decode = (value: string): Lobby => {
  const [name = "OPENSTRIKE", address = "", players = "1", map = "open_arena"] = value.split("|");
  return { name, address, players, map };
};

export default function Lobby(props: { onBack: () => void; onDeploy: () => void }) {
  const [state, setState] = createSignal<StrikeState>(strike.state());
  const stop = strike.onTick(setState);
  onCleanup(stop);

  let grid!: Parameters<typeof pushFocusGrid>[0];
  onMount(() => {
    const cursor = enableCursor({ speed: 520, dpadSpeed: 300, size: [1, 1] });
    const focusGrid = pushFocusGrid(grid, { columns: 1, wrap: true });
    const scope = pushFocusScope(grid, { autoFocus: true });
    onCleanup(() => {
      scope();
      focusGrid();
      cursor();
    });
  });

  return (
    <View class="w-full h-full justify-center items-center" style={{ bgColor: "#03070cF2" }}>
      <View class="flex-col rounded-md" style={{ width: 620, height: 390, bgColor: "#071019F4" }}>
        <View class="flex-row items-center px-5 py-3">
          <View style={{ width: 5, height: 34, bgColor: LIME }} />
          <View class="flex-col ml-3">
            <Text class="text-3xl font-bold tracking-wide" style={{ textColor: INK }}>LAN LOBBY</Text>
            <Text class="text-sm font-bold tracking-wide" style={{ textColor: DIM }}>
              {state().lobbyStatus}
            </Text>
          </View>
          <View class="flex-1" />
          <Text class="text-sm font-bold" style={{ textColor: LIME }}>
            {state().connectedPlayers}/16 ONLINE
          </Text>
        </View>
        <View style={{ width: 620, height: 1, bgColor: "#263440" }} />

        <View ref={(el) => (grid = el)} class="flex-col flex-1 gap-2 px-5 py-4">
          <Show when={state().lobbyMode === "offline"}>
            <View focusable onPress={() => strike.hostLobby()} class={ROW} style={{ height: 48 }}>
              <Text class="text-sm font-bold" style={{ textColor: LIME, width: 80 }}>HOST</Text>
              <View class="flex-col">
                <Text class="text-lg font-bold" style={{ textColor: INK }}>CREATE LAN LOBBY</Text>
                <Text class="text-xs" style={{ textColor: DIM }}>Friends on this network will see it automatically</Text>
              </View>
            </View>
            <View focusable onPress={() => strike.startLocalTest()} class={ROW} style={{ height: 48 }}>
              <Text class="text-sm font-bold" style={{ textColor: LIME, width: 80 }}>TEST</Text>
              <View class="flex-col">
                <Text class="text-lg font-bold" style={{ textColor: INK }}>OPEN SECOND WINDOW</Text>
                <Text class="text-xs" style={{ textColor: DIM }}>Host and connect Player 2 on this computer</Text>
              </View>
            </View>
            <Text class="text-xs font-bold tracking-wide mt-2" style={{ textColor: DIM }}>
              LOBBIES ON YOUR NETWORK
            </Text>
            <For each={state().lanLobbies}>
              {(encoded, index) => {
                const lobby = decode(encoded);
                return (
                  <View focusable onPress={() => strike.joinLobby(index())} class={ROW} style={{ height: 48 }}>
                    <Text class="text-sm font-bold" style={{ textColor: LIME, width: 80 }}>
                      {lobby.players}/16
                    </Text>
                    <View class="flex-col">
                      <Text class="text-lg font-bold" style={{ textColor: INK }}>{lobby.name}</Text>
                      <Text class="text-xs" style={{ textColor: DIM }}>{lobby.map} / {lobby.address}</Text>
                    </View>
                    <View class="flex-1" />
                    <Text class="text-sm font-bold" style={{ textColor: LIME }}>JOIN</Text>
                  </View>
                );
              }}
            </For>
            <Show when={state().lanLobbies.length === 0}>
              <View class="items-center justify-center" style={{ height: 54 }}>
                <Text class="text-sm" style={{ textColor: DIM }}>SCANNING… NO LOBBIES FOUND YET</Text>
              </View>
            </Show>
          </Show>

          <Show when={state().lobbyMode === "host"}>
            <View class="flex-col items-center justify-center gap-2" style={{ height: 130 }}>
              <Text class="text-2xl font-bold" style={{ textColor: INK }}>YOUR LOBBY IS LIVE</Text>
              <Text class="text-sm" style={{ textColor: DIM }}>Other players can select it from their LAN Lobby screen.</Text>
              <Text class="text-xl font-bold" style={{ textColor: LIME }}>
                {state().connectedPlayers} PLAYER{state().connectedPlayers === 1 ? "" : "S"} READY
              </Text>
            </View>
            <View
              focusable
              onPress={() => state().connectedPlayers >= 2 && strike.startMatch()}
              class={ROW}
              style={{ height: 52 }}
            >
              <Text class="text-xl font-bold" style={{ textColor: state().connectedPlayers >= 2 ? LIME : DIM }}>
                {state().connectedPlayers >= 2 ? "START MATCH" : "WAITING FOR PLAYER 2"}
              </Text>
            </View>
          </Show>

          <Show when={state().lobbyMode === "client"}>
            <View class="flex-col items-center justify-center gap-3" style={{ height: 190 }}>
              <Text class="text-2xl font-bold" style={{ textColor: INK }}>CONNECTED TO HOST</Text>
              <Text class="text-xl font-bold" style={{ textColor: LIME }}>{state().connectedPlayers} PLAYERS READY</Text>
              <Text class="text-sm" style={{ textColor: DIM }}>Waiting for the host to start the match…</Text>
            </View>
          </Show>

          <View class="flex-1" />
          <View
            focusable
            onPress={() => {
              if (state().lobbyMode !== "offline") strike.leaveLobby();
              props.onBack();
            }}
            class={ROW}
            style={{ height: 38 }}
          >
            <Text class="text-sm font-bold" style={{ textColor: DIM }}>BACK</Text>
          </View>
        </View>
      </View>
    </View>
  );
}
