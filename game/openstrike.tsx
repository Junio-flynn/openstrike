// @title OpenStrike
// The product bundle: gameplay rules (the base game as a mod) + the JSX HUD.
// One QuickJS guest runs both — apps, games and mods are the same kind of
// artifact here.

import "./rules.ts";
import { createSignal, Match, Show, Switch } from "solid-js";
import { mount } from "@pocketjs/framework";
import Hud from "./hud.tsx";
import MainMenu from "./menu.tsx";
import Settings from "./settings.tsx";
import Lobby from "./lobby.tsx";
import PlayerNameEditor from "./player-name.tsx";
import { strike } from "./sdk.ts";
import { clearCursorPosition, setCursorPosition } from "@pocketjs/framework/input";

// Phase-switched root: handheld menu hosts (PSP EBOOT / Vita VPK) boot into
// phase "menu";
// hosts that pre-load a map (desktop --map) never publish it and go
// straight to the HUD. The menu<->game swap is structural but rare.
function App() {
  const desktop = strike.desktopMapHost;
  const [inMenu, setInMenu] = createSignal(desktop || strike.state().phase === "menu");
  strike.setPaused(inMenu());
  const [menuPage, setMenuPage] = createSignal<"main" | "settings" | "lobby" | "name">("main");
  let openedConnectedLobby = false;
  let matchWasStarted = false;
  strike.onTick((s) => {
    if (desktop && inMenu()) {
      if (s.pointerX < 0 || s.pointerY < 0) clearCursorPosition();
      else setCursorPosition(s.pointerX, s.pointerY);
    }
    if (!desktop) setInMenu(s.phase === "menu");
    if (!desktop && s.phase !== "menu") setMenuPage("main");
    if (desktop && inMenu() && s.lobbyMode !== "offline" && !openedConnectedLobby) {
      openedConnectedLobby = true;
      setMenuPage("lobby");
    }
    if (desktop && inMenu() && s.matchStarted && !matchWasStarted) {
      setInMenu(false);
      strike.setPaused(false);
    }
    matchWasStarted = s.matchStarted;
  });
  strike.onMenuRequest(() => {
    setMenuPage("main");
    setInMenu(true);
    strike.setPaused(true);
  });
  return (
    <Show
      when={!inMenu()}
      fallback={
        <Show
          when={menuPage() !== "main"}
          fallback={
            <MainMenu
              onOpenSettings={() => setMenuPage("settings")}
              onOpenLobby={() => setMenuPage("lobby")}
              onEditName={() => setMenuPage("name")}
              onPlayDesktop={() => {
                setInMenu(false);
                strike.setPaused(false);
              }}
              onQuit={() => strike.quit()}
            />
          }
        >
          <Switch>
            <Match when={menuPage() === "settings"}>
              <Settings onMainMenu={() => setMenuPage("main")} />
            </Match>
            <Match when={menuPage() === "lobby"}>
              <Lobby onBack={() => setMenuPage("main")} onDeploy={() => {
                setInMenu(false);
                strike.setPaused(false);
              }} />
            </Match>
            <Match when={menuPage() === "name"}>
              <PlayerNameEditor onBack={() => setMenuPage("main")} />
            </Match>
          </Switch>
        </Show>
      }
    >
      <Hud />
    </Show>
  );
}

mount(() => <App />);
