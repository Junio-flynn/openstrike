// The OpenStrike HUD — a PocketJS app (Solid + Tailwind subset) composited
// over the 3D frame. Three rules keep it 60 fps on a 333 MHz interpreter:
//
//   1. ZERO structural changes during gameplay: every element is mounted
//      once at boot and toggled via opacity (paint skips opacity-0
//      subtrees). A mid-combat <Show> mount costs a component build in
//      QuickJS plus first layout in the core — measured ~20 ms on hardware.
//   2. Per-frame values (hp, ammo, bars, flash decay) bypass Solid via the
//      framework's imperative hot path (@pocketjs/framework/hot): one
//      gated FFI call per actual change. A Solid signal write with even a
//      few subscribers costs ~8 ms of interpretation on the PSP — that was
//      the "every shot stutters" bug.
//   3. Hot numbers live in FIXED cells (definite width+height), so a text
//      swap skips relayout entirely; bar fills move via scaleX/translateX
//      (paint-only), never via width (layout).
//
// Solid still owns everything rare: phase banner, score strip, kill feed.
// Class strings are ternaries of FULL literals (the Tailwind subset bakes
// at build time); `S` picks the compact PSP set or the scaled desktop set.

import { createSignal, onCleanup, onMount, Show } from "solid-js";
import { Image, Text, View } from "@pocketjs/framework/components";
import * as hot from "@pocketjs/framework/hot";
import { pushFocusScope } from "@pocketjs/framework/input";
import { onButtonPress, onFrame } from "@pocketjs/framework/lifecycle";
import { strike, type StrikeState } from "./sdk.ts";
import { ROUND_FREEZE, phaseAge } from "./rules.ts";
import PlayerCrosshair from "./crosshair-preview.tsx";

const TICK = 1 / 64;

// Palette (military night-ops): lime reticle, amber warnings, blood red.
const INK = "#e8f0f2";
const DIM = "#8fa3ad";
const LIME = "#b8f34a";
const AMBER = "#fbbf24";
const RED = "#f43f3f";
const STRIP = "#04070a90";

// Packed ABGR for hot color swaps (hot.prop is numeric-only).
const abgr = (hex: string): number => {
  const n = parseInt(hex.slice(1), 16);
  return (((0xff << 24) | ((n & 0xff) << 16) | (n & 0xff00) | ((n >> 16) & 0xff)) >>> 0);
};
const INK_N = abgr(INK);
const DIM_N = abgr(DIM);
const AMBER_N = abgr(AMBER);
const RED_N = abgr(RED);
const LIME_N = abgr(LIME);

// The same bundle renders at 480x272 on the PSP and at the window size on
// desktop; scale the chrome so it occupies the same screen fraction.
const vp = (globalThis as { ui?: { __viewport?: { w: number; h: number } } }).ui
  ?.__viewport ?? { w: 480, h: 272 };
const S = Math.max(1, Math.round(vp.h / 272));

const FEED_ROWS = 3;
const BAR_W = 90;
const AMMO_BAR_W = 64;

type Ref = NonNullable<Parameters<typeof hot.text>[0]>;

export default function Hud() {
  const s0 = strike.state();
  // Solid signals: RARE updates only (phase flow and reserve).
  const [phase, setPhase] = createSignal(s0.phase);
  const [reserve, setReserve] = createSignal(s0.reserve);
  const [weaponName, setWeaponName] = createSignal(s0.weaponName ?? "SCORPION EVO 3");
  const [countdown, setCountdown] = createSignal(0);
  const [multiplayer, setMultiplayer] = createSignal(s0.multiplayer);
  const [matchLive, setMatchLive] = createSignal(s0.matchLive);
  const [matchWinner, setMatchWinner] = createSignal(s0.matchWinner);
  const [matchRound, setMatchRound] = createSignal(s0.matchRound);
  // SELECT opens/closes the quit dialog (BTN.SELECT = 0x0001). The mount is
  // structural but user-initiated — never on the combat hot path.
  const [dialog, setDialog] = createSignal(false);
  onButtonPress(0x0001, () => {
    // Desktop Escape returns directly to its in-process menu. Handhelds keep
    // the confirmation dialog because SELECT is easier to hit accidentally.
    if (strike.maps.length === 0) strike.toMenu();
    else setDialog((d) => !d);
  });

  // Hot refs: PER-FRAME values, written imperatively (rule 2).
  let hpText: Ref;
  let hpFill: Ref;
  let speedText: Ref;
  let doubleJumpGroup: Ref;
  let doubleJumpText: Ref;
  let doubleJumpStatus: Ref;
  let doubleJumpPipA: Ref;
  let doubleJumpPipB: Ref;
  let ammoText: Ref;
  let ammoFill: Ref;
  let reloadGroup: Ref;
  let reloadFill: Ref;
  let flashOverlay: Ref;
  let vignette: Ref;
  let crosshair: Ref;
  let scopeOverlay: Ref;
  let hitmarker: Ref;
  const feedRows: Ref[] = [];
  const feedTexts: Ref[] = [];

  let flash = 0;
  let hitmark = 0;
  const feedTtl: number[] = Array.from({ length: FEED_ROWS }, () => 0);
  const feedStr: string[] = Array.from({ length: FEED_ROWS }, () => " ");
  const pushFeed = (text: string) => {
    for (let i = 0; i < FEED_ROWS - 1; i++) {
      feedStr[i] = feedStr[i + 1];
      feedTtl[i] = feedTtl[i + 1];
    }
    feedStr[FEED_ROWS - 1] = text;
    feedTtl[FEED_ROWS - 1] = 2.4;
  };

  strike.on("playerDamaged", () => (flash = 0.55));
  strike.on("hit", (e) => {
    if (e.type !== "hit") return;
    hitmark = e.headshot ? 0.24 : 0.16;
    if (e.fatal) pushFeed(e.headshot ? "HEADSHOT" : "TARGET DOWN");
  });
  strike.on("roundReset", () => {
    for (let i = 0; i < FEED_ROWS; i++) feedTtl[i] = 0;
    flash = 0;
  });

  /** Left-anchored fill: scaleX shrinks about the center, so pull the bar
   *  left by half the lost width. Paint-only — never touches layout. */
  const fill = (node: Ref, frac: number, w: number) => {
    const f = Math.max(0, Math.min(1, frac));
    hot.prop(node, "scaleX", f);
    hot.prop(node, "translateX", (-(1 - f) * w * S) / 2);
  };

  // Interpreter discipline: on a 333 MHz QuickJS even a GATED call costs
  // real time, so the quiet-frame path below is straight-line number
  // compares on closure locals — zero function calls when nothing changed.
  let lHp = -1;
  let lSpeed = -1;
  let lDoubleJump = true;
  let lAmmo = -1;
  let lMagSize = -1;
  let lWeaponName = "";
  let lPhase = "";
  let lReserve = -1;
  let lCount = -1;
  let lReloading = false;
  let lAlive = true;
  let lScope = -1;
  let lMultiplayer = false;
  let lMatchLive = false;
  let lMatchWinner = -1;
  let lMatchRound = -1;
  let lFlash = 0;
  let lHit = 0;
  onFrame(() => {
    const s = strike.state();

    if (s.multiplayer !== lMultiplayer) {
      lMultiplayer = s.multiplayer;
      setMultiplayer(s.multiplayer);
    }
    if (s.matchLive !== lMatchLive) {
      lMatchLive = s.matchLive;
      setMatchLive(s.matchLive);
    }
    if (s.matchWinner !== lMatchWinner) {
      lMatchWinner = s.matchWinner;
      setMatchWinner(s.matchWinner);
    }
    if (s.matchRound !== lMatchRound) {
      lMatchRound = s.matchRound;
      setMatchRound(s.matchRound);
    }

    if (s.phase !== lPhase) {
      lPhase = s.phase;
      setPhase(s.phase);
    }
    if (s.phase === "starting") {
      const left = ROUND_FREEZE - phaseAge();
      const c = Math.max(0, Math.ceil(left));
      if (c !== lCount) {
        lCount = c;
        setCountdown(c);
      }
    }
    if (s.reserve !== lReserve) {
      lReserve = s.reserve;
      setReserve(s.reserve);
    }
    const currentWeaponName = s.weaponName ?? "SCORPION EVO 3";
    if (currentWeaponName !== lWeaponName) {
      lWeaponName = currentWeaponName;
      setWeaponName(currentWeaponName);
    }

    // Hot values: imperative, change-guarded, zero-layout.
    const hp = s.hp > 0 ? s.hp : 0;
    if (hp !== lHp) {
      lHp = hp;
      hot.text(hpText, hp);
      const hpColor = hp > 60 ? INK_N : hp > 25 ? AMBER_N : RED_N;
      hot.prop(hpText, "textColor", hpColor);
      hot.prop(hpFill, "bgColor", hpColor);
      fill(hpFill, hp / 100, BAR_W);
    }
    const speed = Math.round(s.speed);
    if (speed !== lSpeed) {
      lSpeed = speed;
      hot.text(speedText, speed);
    }
    if (s.doubleJumpAvailable !== lDoubleJump) {
      lDoubleJump = s.doubleJumpAvailable;
      const color = s.doubleJumpAvailable ? LIME_N : DIM_N;
      hot.prop(doubleJumpText, "textColor", color);
      hot.prop(doubleJumpStatus, "textColor", color);
      hot.text(doubleJumpStatus, s.doubleJumpAvailable ? "READY" : "SPENT");
      hot.prop(doubleJumpPipA, "bgColor", color);
      hot.prop(doubleJumpPipB, "bgColor", color);
      hot.prop(doubleJumpGroup, "opacity", s.doubleJumpAvailable ? 1 : 0.62);
    }
    const magSize = s.magSize ?? 30;
    if (s.ammo !== lAmmo || magSize !== lMagSize) {
      lAmmo = s.ammo;
      lMagSize = magSize;
      hot.text(ammoText, s.ammo);
      hot.prop(ammoText, "textColor", s.ammo === 0 ? RED_N : INK_N);
      hot.prop(ammoFill, "bgColor", s.ammo <= 5 ? RED_N : LIME_N);
      fill(ammoFill, s.ammo / Math.max(1, magSize), AMMO_BAR_W);
    }
    if (s.reloading !== lReloading) {
      lReloading = s.reloading;
      hot.prop(reloadGroup, "opacity", s.reloading ? 1 : 0);
    }
    if (s.reloading) fill(reloadFill, s.reloadFrac, AMMO_BAR_W);
    if (s.alive !== lAlive) {
      lAlive = s.alive;
      hot.prop(vignette, "opacity", s.alive ? 0 : 0.4);
    }
    const scope = s.alive && s.weaponSlot === 3
      ? Math.max(0, Math.min(1, (s.aimFraction - 0.35) / 0.57))
      : 0;
    if (scope !== lScope) {
      lScope = scope;
      hot.prop(scopeOverlay, "opacity", scope);
      hot.prop(crosshair, "opacity", s.alive ? 1 - scope : 0);
    }

    if (flash > 0 || lFlash > 0) {
      flash = Math.max(0, flash - TICK * 1.3);
      lFlash = flash;
      hot.prop(flashOverlay, "opacity", flash * 0.5);
    }
    if (hitmark > 0 || lHit > 0) {
      hitmark = Math.max(0, hitmark - TICK);
      lHit = hitmark;
      hot.prop(hitmarker, "opacity", hitmark > 0 ? 1 : 0);
    }

    for (let i = 0; i < FEED_ROWS; i++) {
      if (feedTtl[i] <= 0) continue;
      feedTtl[i] -= TICK;
      hot.text(feedTexts[i], feedStr[i]);
      hot.prop(feedRows[i], "opacity", feedTtl[i] > 0.4 ? 1 : feedTtl[i] < 0 ? 0 : feedTtl[i] / 0.4);
    }
  });

  const bannerOn = () => (multiplayer() ? !matchLive() : phase() === "starting");
  const bannerColor = () => multiplayer() ? LIME : INK;
  const bannerTitle = () =>
    multiplayer()
      ? matchWinner() > 0 ? `PLAYER ${matchWinner()} WINS` : "ROUND COMPLETE"
      : "ROUND START";
  const bannerSub = () =>
    multiplayer()
      ? `ROUND ${matchRound()} / RESETTING`
      : "GO IN " + countdown() + " ";

  return (
    <View class="w-full h-full">
      {/* Damage flash + death vignette (pre-mounted, hot opacity) */}
      <View
        ref={(el) => (flashOverlay = el)}
        class="absolute inset-0"
        style={{ bgColor: "#c40e0e", opacity: 0, zIndex: 5 }}
      />
      <View
        ref={(el) => (vignette = el)}
        class="absolute inset-0"
        style={{ bgColor: "#2a0404", opacity: 0, zIndex: 5 }}
      />

      {/* Sniper optic: dark side vision and the downloaded transparent reticle. */}
      <View
        ref={(el) => (scopeOverlay = el)}
        class="absolute inset-0"
        style={{ opacity: 0, zIndex: 9 }}
      >
        <View class="absolute inset-0 flex-row justify-center items-center">
          <View style={{ width: (vp.w - vp.h) / 2, height: vp.h, bgColor: "#000000" }} />
          <View class="relative" style={{ width: vp.h, height: vp.h }}>
            <CircularScopeMask size={vp.h} />
            <MilDotReticle size={vp.h} />
          </View>
          <View style={{ width: (vp.w - vp.h) / 2, height: vp.h, bgColor: "#000000" }} />
        </View>
        <View class="absolute flex-col px-2 py-1 rounded-sm" style={{ insetL: 12 * S, insetT: 12 * S, bgColor: "#020609C8", zIndex: 3 }}>
          <Text class="text-xs font-bold tracking-wide" style={{ textColor: LIME }}>OPTIC / 8X</Text>
          <Text class="text-xs tracking-wide" style={{ textColor: INK }}>BOLT READY · 5 RND</Text>
        </View>
        <View class="absolute flex-col px-2 py-1 rounded-sm" style={{ insetL: 12 * S, insetT: 42 * S, bgColor: "#020609C8", zIndex: 3 }}>
          <Text class="text-xs font-bold tracking-wide" style={{ textColor: LIME }}>STEADY</Text>
          <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>ADS ACCURACY</Text>
        </View>
      </View>

      {/* Crosshair + hitmarker */}
      <View
        ref={(el) => (crosshair = el)}
        class="absolute inset-0 justify-center items-center"
        style={{ zIndex: 10 }}
      >
        <View style={{ width: 36 * S, height: 36 * S }}>
          <PlayerCrosshair scale={S} />
          <View
            ref={(el) => (hitmarker = el)}
            class="absolute inset-0"
            style={{ opacity: 0 }}
          >
            <Cross color={RED} gap={6 * S} len={6 * S} thick={2 * S} rotated />
          </View>
        </View>
      </View>

      {/* Frame chrome: one full-screen column, everything anchored off it */}
      <View
        class={
          S >= 2
            ? "absolute inset-0 flex-col justify-between p-6"
            : "absolute inset-0 flex-col justify-between p-3"
        }
        style={{ zIndex: 20 }}
      >
        {/* ---- top row ---- */}
        <View class="flex-row justify-between items-start">
          <View style={{ width: 90 * S }} />
          {/* Phase banner: one instance, text/color swapped in place */}
          <View class="flex-col items-center" style={{ opacity: bannerOn() ? 1 : 0 }}>
            <View
              class="flex-col items-center gap-1 px-4 py-1 rounded-sm"
              style={{ bgColor: STRIP }}
            >
              <View style={{ width: 120 * S, height: 1 * S, bgColor: bannerColor() }} />
              <Text
                class={S >= 2 ? "text-xl font-bold" : "text-sm font-bold"}
                style={{ textColor: bannerColor() }}
              >
                {bannerTitle()}
              </Text>
              <Text
                class={
                  S >= 2
                    ? "text-sm font-bold tracking-wide"
                    : "text-xs font-bold tracking-wide"
                }
                style={{ textColor: AMBER }}
              >
                {bannerSub()}
              </Text>
              <View style={{ width: 120 * S, height: 1 * S, bgColor: bannerColor() }} />
            </View>
          </View>
          {/* Pooled combat feed */}
          <View class="flex-col items-end gap-1" style={{ width: 90 * S }}>
            {Array.from({ length: FEED_ROWS }, (_, i) => (
              <View
                ref={(el) => (feedRows[i] = el)}
                class="px-2 py-1 rounded-sm"
                style={{ bgColor: STRIP, opacity: 0 }}
              >
                <Text
                  ref={(el) => (feedTexts[i] = el)}
                  class={
                    S >= 2
                      ? "text-sm font-bold tracking-wide"
                      : "text-xs font-bold tracking-wide"
                  }
                  style={{ textColor: INK, width: S >= 2 ? 260 : 170, height: S >= 2 ? 20 : 14 }}
                >
                  {" "}
                </Text>
              </View>
            ))}
          </View>
        </View>

        {/* ---- bottom row ---- */}
        <View class="flex-row justify-between items-end">
          {/* Health: fixed number cell + paint-only bar fill */}
          <View class="flex-col gap-1">
            <View
              class={
                S >= 2
                  ? "flex-row items-end gap-2 px-3 py-2 rounded-md"
                  : "flex-row items-end gap-1 px-2 py-1 rounded-sm"
              }
              style={{ bgColor: STRIP }}
            >
              <Text
                class={
                  S >= 2
                    ? "text-sm font-bold tracking-wide"
                    : "text-xs font-bold tracking-wide"
                }
                style={{ textColor: DIM }}
              >
                HP
              </Text>
              <Text
                ref={(el) => (hpText = el)}
                class={S >= 2 ? "text-4xl font-bold" : "text-lg font-bold"}
                style={{
                  textColor: INK,
                  width: S >= 2 ? 66 : 30,
                  height: S >= 2 ? 40 : 20,
                }}
              >
                100
              </Text>
              <View class="flex-row items-end gap-1">
                <Text class="text-xs font-bold tracking-wide" style={{ textColor: LIME }}>
                  SPEED:
                </Text>
                <Text
                  ref={(el) => (speedText = el)}
                  class="text-xs font-bold"
                  style={{ textColor: INK, width: 34 * S, height: 12 * S }}
                >
                  0
                </Text>
              </View>
            </View>
            <View style={{ width: BAR_W * S, height: 2 * S, bgColor: "#e8f0f21c" }}>
              <View
                ref={(el) => (hpFill = el)}
                style={{ width: BAR_W * S, height: 2 * S, bgColor: INK }}
              />
            </View>
          </View>
          {/* Double-jump charge: stays mounted; spending the air jump drains
              its color so availability is readable without extra text. */}
          <View
            ref={(el) => (doubleJumpGroup = el)}
            class="flex-col items-center gap-1 px-2 py-1 rounded-sm"
            style={{ bgColor: STRIP }}
          >
            <View class="flex-row gap-1">
              <View
                ref={(el) => (doubleJumpPipA = el)}
                style={{ width: 8 * S, height: 2 * S, bgColor: LIME }}
              />
              <View
                ref={(el) => (doubleJumpPipB = el)}
                style={{ width: 8 * S, height: 2 * S, bgColor: LIME }}
              />
            </View>
            <Text
              ref={(el) => (doubleJumpText = el)}
              class="text-xs font-bold tracking-wide"
              style={{ textColor: LIME }}
            >
              DOUBLE JUMP
            </Text>
            <Text
              ref={(el) => (doubleJumpStatus = el)}
              class="text-xs font-bold tracking-wide"
              style={{ textColor: LIME }}
            >
              READY
            </Text>
          </View>
          {/* Ammo: fixed mag cell / reserve + reload bar (all pre-mounted) */}
          <View class="flex-col items-end gap-1">
            <Text class="text-xs font-bold tracking-wide" style={{ textColor: DIM }}>
              {weaponName()}
            </Text>
            <View
              ref={(el) => (reloadGroup = el)}
              class="flex-col items-end gap-1"
              style={{ opacity: 0 }}
            >
              <Text
                class={
                  S >= 2
                    ? "text-sm font-bold tracking-wide"
                    : "text-xs font-bold tracking-wide"
                }
                style={{ textColor: AMBER }}
              >
                RELOADING
              </Text>
              <View style={{ width: AMMO_BAR_W * S, height: 2 * S, bgColor: "#e8f0f21c" }}>
                <View
                  ref={(el) => (reloadFill = el)}
                  style={{ width: AMMO_BAR_W * S, height: 2 * S, bgColor: AMBER }}
                />
              </View>
            </View>
            <View
              class={
                S >= 2
                  ? "flex-row items-end gap-2 px-3 py-2 rounded-md"
                  : "flex-row items-end gap-1 px-2 py-1 rounded-sm"
              }
              style={{ bgColor: STRIP }}
            >
              <Text
                ref={(el) => (ammoText = el)}
                class={S >= 2 ? "text-4xl font-bold" : "text-lg font-bold"}
                style={{
                  textColor: INK,
                  width: S >= 2 ? 44 : 20,
                  height: S >= 2 ? 40 : 20,
                }}
              >
                30
              </Text>
              <Text
                class={
                  S >= 2
                    ? "text-sm font-bold tracking-wide"
                    : "text-xs font-bold tracking-wide"
                }
                style={{ textColor: DIM }}
              >
                {"/ " + reserve()}
              </Text>
            </View>
            <View style={{ width: AMMO_BAR_W * S, height: 2 * S, bgColor: "#e8f0f21c" }}>
              <View
                ref={(el) => (ammoFill = el)}
                style={{ width: AMMO_BAR_W * S, height: 2 * S, bgColor: LIME }}
              />
            </View>
          </View>
        </View>
      </View>

      {/* Quit dialog (SELECT). CS-style: the world keeps running behind it. */}
      <Show when={dialog()}>
        <QuitDialog
          onStay={() => setDialog(false)}
          onQuit={() => {
            setDialog(false);
            strike.toMenu();
          }}
        />
      </Show>
    </View>
  );
}

/** SELECT dialog. On mount it traps focus (STAY lit by default — the safe
 *  choice); ↔ moves between the two buttons, each with a clear selected
 *  state (neutral for STAY, danger-red for QUIT), `○` confirms the lit one. */
function QuitDialog(props: { onStay: () => void; onQuit: () => void }) {
  let panel!: Ref;
  onMount(() => {
    const dispose = pushFocusScope(panel, { autoFocus: true, restoreFocus: true });
    onCleanup(dispose);
  });
  // Whole class strings must be single literals (the subset bakes them as a
  // unit — a `base + variant` concat would never resolve at runtime).
  const label = S >= 2 ? "text-sm font-bold tracking-wide" : "text-xs font-bold tracking-wide";
  return (
    <View
      class="absolute inset-0 justify-center items-center"
      style={{ bgColor: "#02040788", zIndex: 40 }}
    >
      <View class="flex-col items-center gap-2 px-5 py-3 rounded-md" style={{ bgColor: "#0a121aF0" }}>
        <Text
          class={S >= 2 ? "text-xl font-bold tracking-wide" : "text-sm font-bold tracking-wide"}
          style={{ textColor: INK }}
        >
          RETURN TO MAIN MENU?
        </Text>
        <View ref={(el) => (panel = el)} class="flex-row gap-2">
          <View
            focusable
            onPress={props.onStay}
            class="px-4 py-1 rounded-sm border-[#00000000] transition-colors duration-100 bg-[#111a24] focus:bg-slate-600 focus:border-slate-300"
          >
            <Text class={label} style={{ textColor: INK }}>
              STAY
            </Text>
          </View>
          <View
            focusable
            onPress={props.onQuit}
            class="px-4 py-1 rounded-sm border-[#00000000] transition-colors duration-100 bg-[#241014] focus:bg-red-800 focus:border-red-400"
          >
            <Text class={label} style={{ textColor: RED }}>
              QUIT
            </Text>
          </View>
        </View>
        <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>
          ↔ SELECT · ○ CONFIRM · SELECT CLOSE
        </Text>
      </View>
    </View>
  );
}

/** Four reticle bars around a center gap (rotated 45° for the hitmarker). */
function Cross(props: {
  color: string;
  gap: number;
  len: number;
  thick: number;
  rotated?: boolean;
}) {
  const c = 18 * S; // center of the reticle box
  const bar = (x: number, y: number, w: number, h: number) => (
    <View
      class="absolute"
      style={{ insetL: x, insetT: y, width: w, height: h, bgColor: props.color }}
    />
  );
  return (
    <View
      class="absolute inset-0"
      style={props.rotated ? { rotate: 45 } : undefined}
    >
      {bar(c - props.thick / 2, c - props.gap / 2 - props.len, props.thick, props.len)}
      {bar(c - props.thick / 2, c + props.gap / 2, props.thick, props.len)}
      {bar(c - props.gap / 2 - props.len, c - props.thick / 2, props.len, props.thick)}
      {bar(c + props.gap / 2, c - props.thick / 2, props.len, props.thick)}
    </View>
  );
}

/** Antialiased black peripheral mask around the scope's circular aperture. */
function CircularScopeMask(props: { size: number }) {
  return <Image class="absolute inset-0" style={{ width: props.size, height: props.size, zIndex: 0 }} src="sniper-scope-mask.png" />;
}

/** Scalable recreation of the downloaded MIL-DOT reticle. */
function MilDotReticle(props: { size: number }) {
  const c = props.size / 2;
  const heavy = Math.max(2, 2 * S);
  const fine = Math.max(1, S * 0.55);
  const inner = props.size * 0.17;
  const outer = props.size * 0.43;
  const line = (x: number, y: number, w: number, h: number) => (
    <View class="absolute" style={{ insetL: x, insetT: y, width: w, height: h, bgColor: "#050505" }} />
  );
  const dots = [-4, -3, -2, -1, 1, 2, 3, 4];
  const dotStep = props.size * 0.035;
  const dotSize = Math.max(2, 2.2 * S);
  return <View class="absolute inset-0" style={{ zIndex: 2 }}>
    {line(c - heavy / 2, c - outer, heavy, outer - inner)}
    {line(c - heavy / 2, c + inner, heavy, outer - inner)}
    {line(c - outer, c - heavy / 2, outer - inner, heavy)}
    {line(c + inner, c - heavy / 2, outer - inner, heavy)}
    {line(c - fine / 2, c - inner, fine, inner * 2)}
    {line(c - inner, c - fine / 2, inner * 2, fine)}
    {dots.map((step) => line(c - dotSize / 2, c + step * dotStep - dotSize / 2, dotSize, dotSize))}
    {dots.map((step) => line(c + step * dotStep - dotSize / 2, c - dotSize / 2, dotSize, dotSize))}
    {line(c - Math.max(1, S), c - Math.max(1, S), Math.max(2, 2 * S), Math.max(2, 2 * S))}
  </View>;
}
