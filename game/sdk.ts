// The `strike` surface SDK — the JS half of OpenStrike's vocabulary.
// Host side: crates/openstrike/src/guest.rs.
//
// Per tick the host calls `strike.__dispatch(state, events)` (facts), then
// the PocketJS frame turn runs (HUD). Commands issued here queue on the host
// and apply after the guest turn — state read through this module is always
// the host's last-published snapshot, never a guess.

export interface StrikeState {
  time: number;
  phase: "menu" | "starting" | "live" | "won" | "lost";
  hp: number;
  alive: boolean;
  ammo: number;
  reserve: number;
  magSize: number;
  weaponName: string;
  weaponSlot: number;
  aimFraction: number;
  reloading: boolean;
  reloadFrac: number;
  aliveBots: number;
  totalBots: number;
  wins: number;
  losses: number;
  speed: number;
  doubleJumpAvailable: boolean;
  pointerX: number;
  pointerY: number;
  lobbyMode: "offline" | "host" | "client";
  lobbyStatus: string;
  connectedPlayers: number;
  matchStarted: boolean;
  multiplayer: boolean;
  matchRound: number;
  matchLive: boolean;
  matchWinner: number;
  lanLobbies: string[];
}

export type StrikeEvent =
  | { type: "hit"; bot: number; headshot: boolean; damage: number; fatal: boolean }
  | { type: "playerDamaged"; amount: number; hp: number }
  | { type: "playerDied" }
  | { type: "roundReset" };

export interface WeaponConfig {
  magSize: number;
  reserve: number;
  fireInterval: number;
  reloadTime: number;
  damageBody: number;
  damageHead: number;
}

export interface BotsConfig {
  count: number;
  speed: number;
  attackInterval: number;
  damageMin: number;
  damageMax: number;
}

interface NativeStrike {
  /** Cooked maps available to loadMap (index-aligned), host-injected. */
  maps?: string[];
  desktopMapHost?: boolean;
  loadMap?(index: number): void;
  toMenu?(): void;
  setPaused?(paused: boolean): void;
  setMouseSensitivity?(scale: number): void;
  setScopeMultiplier?(scale: number): void;
  setControlScheme?(scheme: number): void;
  setKeybind?(action: number, slot: number, key: number): void;
  selectWeapon?(slot: number): void;
  setViewmodel?(slot: number, scale: number, x: number, y: number, z: number, yaw: number, pitch: number): void;
  savedSettings?: string;
  playerName?: string;
  saveSettings?(json: string): void;
  setPlayerName?(name: string): void;
  quit?(): void;
  hostLobby?(): void;
  startLocalTest?(): void;
  joinLobby?(index: number): void;
  startMatch?(): void;
  leaveLobby?(): void;
  __requestMenu?(): void;
  setPhase(phase: string): void;
  resetRound(): void;
  addWin(): void;
  addLoss(): void;
  setBotCount(n: number): void;
  configureWeapon(cfg: WeaponConfig): void;
  configureBots(cfg: BotsConfig): void;
  __dispatch?: (state: StrikeState, events: StrikeEvent[]) => void;
}

const native = (globalThis as { strike?: NativeStrike }).strike;
if (!native) {
  throw new Error("openstrike: no `strike` surface — is this running under the game host?");
}

let current: StrikeState = {
  time: 0,
  phase: "starting",
  hp: 100,
  alive: true,
  ammo: 30,
  reserve: 90,
  magSize: 30,
  weaponName: "SCORPION EVO 3",
  weaponSlot: 0,
  aimFraction: 0,
  reloading: false,
  reloadFrac: 0,
  aliveBots: 0,
  totalBots: 0,
  wins: 0,
  losses: 0,
  speed: 0,
  doubleJumpAvailable: true,
  pointerX: -1,
  pointerY: -1,
  lobbyMode: "offline",
  lobbyStatus: "SEARCHING LOCAL NETWORK",
  connectedPlayers: 1,
  matchStarted: false,
  multiplayer: false,
  matchRound: 0,
  matchLive: false,
  matchWinner: 0,
  lanLobbies: [],
};

type Handler = (e: StrikeEvent) => void;
type TickHandler = (s: StrikeState) => void;
const handlers = new Map<string, Set<Handler>>();
const tickHandlers = new Set<TickHandler>();
const menuHandlers = new Set<() => void>();

native.__dispatch = (state, events) => {
  current = state;
  for (const e of events) {
    const set = handlers.get(e.type);
    if (set) for (const h of [...set]) h(e);
  }
  for (const h of [...tickHandlers]) h(state);
};

export const strike = {
  /** The last state snapshot the host published (this tick). */
  state: (): StrikeState => current,

  /** Subscribe to a game event; returns an unsubscribe. */
  on(type: StrikeEvent["type"], fn: Handler): () => void {
    let set = handlers.get(type);
    if (!set) handlers.set(type, (set = new Set()));
    set.add(fn);
    return () => set.delete(fn);
  },

  /** Runs once per tick, after events, with the fresh state. */
  onTick(fn: TickHandler): () => void {
    tickHandlers.add(fn);
    return () => tickHandlers.delete(fn);
  },

  /** Subscribe to a request to show the product's main menu. */
  onMenuRequest(fn: () => void): () => void {
    menuHandlers.add(fn);
    return () => menuHandlers.delete(fn);
  },

  // ---- intent (queued host-side, applied after this guest turn) ----------
  /** Map names the host can load (empty on hosts that boot pre-loaded). */
  maps: (native.maps ?? []) as readonly string[],
  desktopMapHost: native.desktopMapHost ?? false,
  /** Ask the host to load a cooked map and start a round (menu hosts). */
  loadMap: (index: number) => native.loadMap?.(index),
  /** Leave the round and return to the main menu (menu hosts). */
  toMenu: () => {
    for (const h of [...menuHandlers]) h();
    native.toMenu?.();
  },
  /** Pause/resume simulation on hosts with an in-process menu (desktop). */
  setPaused: (paused: boolean) => native.setPaused?.(paused),
  setMouseSensitivity: (scale: number) => native.setMouseSensitivity?.(scale),
  setScopeMultiplier: (scale: number) => native.setScopeMultiplier?.(scale),
  setControlScheme: (scheme: number) => native.setControlScheme?.(scheme),
  setKeybind: (action: number, slot: number, key: number) => native.setKeybind?.(action, slot, key),
  selectWeapon: (slot: number) => native.selectWeapon?.(slot),
  setViewmodel: (slot: number, scale: number, x: number, y: number, z: number, yaw: number, pitch: number) =>
    native.setViewmodel?.(slot, scale, x, y, z, yaw, pitch),
  savedSettings: () => native.savedSettings ?? "",
  initialPlayerName: native.playerName ?? "Player",
  saveSettings: (json: string) => native.saveSettings?.(json),
  setPlayerName: (name: string) => native.setPlayerName?.(name),
  quit: () => native.quit?.(),
  hostLobby: () => native.hostLobby?.(),
  startLocalTest: () => native.startLocalTest?.(),
  joinLobby: (index: number) => native.joinLobby?.(index),
  startMatch: () => native.startMatch?.(),
  leaveLobby: () => native.leaveLobby?.(),
  setPhase: (phase: StrikeState["phase"]) => native.setPhase(phase),
  resetRound: () => native.resetRound(),
  addWin: () => native.addWin(),
  addLoss: () => native.addLoss(),
  setBotCount: (n: number) => native.setBotCount(n),
  configureWeapon: (cfg: WeaponConfig) => native.configureWeapon(cfg),
  configureBots: (cfg: BotsConfig) => native.configureBots(cfg),
};

// Host-to-product control path. The native `toMenu` function only represents
// the platform operation; desktop must enter through the SDK wrapper so its
// in-process App subscribers switch from HUD to MainMenu as well.
native.__requestMenu = strike.toMenu;
