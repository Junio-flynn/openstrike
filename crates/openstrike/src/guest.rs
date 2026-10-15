//! The guest: one QuickJS realm running the OpenStrike product bundle —
//! gameplay rules (the base game is the first mod) plus the Solid JSX HUD.
//!
//! Two surfaces are mounted (RUNTIMES.md):
//!   - `ui` — the PocketJS 2D runtime (pocket-ui-wgpu), composited over
//!     the 3D frame as the HUD;
//!   - `strike` — this game's vocabulary. Facts flow guest-ward as per-tick
//!     event batches (`strike.__dispatch(state, events)`); intent flows
//!     host-ward as commands queued by ops and applied after the guest
//!     turn. No shared state, no re-entrancy.
//!
//! Turn order per fixed tick (Law 3 — one guest turn per tick):
//!   game.tick() → __dispatch(state, events) → frame(buttons) → ui.tick()
//!   → drain commands into the game.

use std::cell::Cell;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use anyhow::{Context, Result, anyhow};
use pocket_mod::Guest;
use pocket_mod::qjs::{Array, CatchResultExt, Function, Object};
use pocket_ui_wgpu::{Blit, UiRenderer, UiSurface};
use pocket3d::gpu::{Gpu, OffscreenTarget};

use crate::bot::BotConfig;
use crate::game::{Command, GameEvent, OpenStrike, Phase};
use crate::network::LobbyRequest;
use crate::weapon::WeaponConfig;

pub struct StrikeGuest {
    guest: Guest,
    ui: UiSurface,
    commands: Rc<RefCell<Vec<Command>>>,
    paused: Rc<Cell<bool>>,
    mouse_cursor_unlocked: Rc<Cell<bool>>,
    mouse_sensitivity: Rc<Cell<f32>>,
    scope_multiplier: Rc<Cell<f32>>,
    control_scheme: Rc<Cell<u8>>,
    keybindings: Rc<RefCell<[[u8; 2]; 8]>>,
    viewmodel_settings: Rc<RefCell<[[f32; 6]; 16]>>,
    player_name: Rc<RefCell<String>>,
    quit_requested: Rc<Cell<bool>>,
    lobby_requests: Rc<RefCell<Vec<LobbyRequest>>>,
    map_requests: Rc<RefCell<Vec<usize>>>,
    /// Logical UI size (the core's viewport).
    ui_size: (u32, u32),
    gfx: Option<OverlayGfx>,
}

struct OverlayGfx {
    renderer: UiRenderer,
    offscreen: OffscreenTarget,
    blit: Blit,
    target_format: wgpu::TextureFormat,
}

/// Locate the PSP-baseline product bundle (`dist/pocket/psp`).
pub fn find_bundle() -> Result<(PathBuf, PathBuf)> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(d) = std::env::var_os("OPENSTRIKE_UI_DIST") {
        roots.push(PathBuf::from(d));
    }
    roots.push(PathBuf::from("dist/pocket/psp"));
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/pocket/psp"));
    for root in &roots {
        let js = root.join("openstrike.js");
        let pak = root.join("openstrike.pak");
        if js.is_file() && pak.is_file() {
            return Ok((js, pak));
        }
    }
    Err(anyhow!(
        "HUD/rules bundle not found — build it first: `bun run build:ui` \
         (searched dist/pocket/psp next to the repo root; override with OPENSTRIKE_UI_DIST)"
    ))
}

impl StrikeGuest {
    /// Boot the realm: feed the pak, mount `ui` + `strike`, eval the bundle.
    /// `ui_size` is the logical HUD resolution (window logical size).
    pub fn boot(
        ui_size: (u32, u32),
        initial_player_name: &str,
        map_names: &[String],
    ) -> Result<StrikeGuest> {
        let (js_path, pak_path) = find_bundle()?;
        let bundle = std::fs::read_to_string(&js_path)
            .with_context(|| format!("reading {}", js_path.display()))?;
        let pak =
            std::fs::read(&pak_path).with_context(|| format!("reading {}", pak_path.display()))?;

        let ui = UiSurface::new((ui_size.0 as f32, ui_size.1 as f32));
        ui.feed_pak(&pak);
        let guest = Guest::new()?;
        ui.mount(&guest)?;

        let commands: Rc<RefCell<Vec<Command>>> = Rc::new(RefCell::new(Vec::new()));
        let paused = Rc::new(Cell::new(false));
        let mouse_cursor_unlocked = Rc::new(Cell::new(false));
        let mouse_sensitivity = Rc::new(Cell::new(1.0));
        let scope_multiplier = Rc::new(Cell::new(1.0));
        let control_scheme = Rc::new(Cell::new(0));
        let keybindings = Rc::new(RefCell::new([
            [1, 7],
            [3, 8],
            [2, 9],
            [4, 10],
            [11, 0],
            [12, 0],
            [14, 0],
            [13, 0],
        ]));
        let viewmodel_settings = Rc::new(RefCell::new([[2.2, 0.0, 0.0, 0.0, 0.0, 0.0]; 16]));
        let player_name = Rc::new(RefCell::new(initial_player_name.to_owned()));
        let quit_requested = Rc::new(Cell::new(false));
        let lobby_requests = Rc::new(RefCell::new(Vec::new()));
        let map_requests = Rc::new(RefCell::new(Vec::new()));
        mount_strike(
            &guest,
            &commands,
            &paused,
            &mouse_cursor_unlocked,
            &mouse_sensitivity,
            &scope_multiplier,
            &control_scheme,
            &keybindings,
            &viewmodel_settings,
            &player_name,
            &quit_requested,
            &lobby_requests,
            &map_requests,
            map_names,
        )?;

        guest.eval("openstrike", &bundle)?;
        if !guest.has_frame() {
            return Err(anyhow!(
                "bundle evaluated but installed no frame() — HUD missing?"
            ));
        }
        log::info!(
            "guest: booted {} ({} bytes js) at {}x{}",
            js_path.display(),
            bundle.len(),
            ui_size.0,
            ui_size.1
        );
        Ok(StrikeGuest {
            guest,
            ui,
            commands,
            paused,
            mouse_cursor_unlocked,
            mouse_sensitivity,
            scope_multiplier,
            control_scheme,
            keybindings,
            viewmodel_settings,
            player_name,
            quit_requested,
            lobby_requests,
            map_requests,
            ui_size,
            gfx: None,
        })
    }

    /// One guest turn for one game tick.
    pub fn turn(&self, game: &mut OpenStrike) -> Result<()> {
        self.turn_with_buttons(game, 0)
    }

    /// Desktop window turn with Pocket button bits synthesized from keyboard input.
    pub fn turn_with_buttons(&self, game: &mut OpenStrike, buttons: u32) -> Result<()> {
        self.turn_with_input(game, buttons, 0x8080)
    }

    pub fn turn_with_input(&self, game: &mut OpenStrike, buttons: u32, analog: u32) -> Result<()> {
        let events = std::mem::take(&mut game.events);
        self.guest.with(|ctx| -> Result<()> {
            let strike: Object = ctx
                .globals()
                .get("strike")
                .context("strike surface missing")?;
            let Ok(dispatch) = strike.get::<_, Function>("__dispatch") else {
                return Ok(()); // no SDK loaded — state simply doesn't flow
            };
            let state = build_state(&ctx, game)?;
            let batch = Array::new(ctx.clone())?;
            for (i, e) in events.iter().enumerate() {
                batch.set(i, build_event(&ctx, e)?)?;
            }
            dispatch
                .call::<_, ()>((state, batch))
                .catch(&ctx)
                .map_err(|e| anyhow!("strike.__dispatch threw: {e}"))?;
            Ok(())
        })?;
        self.guest.frame_with_analog(buttons, analog)?;
        self.ui.tick();
        // Presentation settings are live state, not queued gameplay commands.
        // Copy them immediately after the UI turn so changes made while the
        // game is paused are visible in the very next composed frame.
        game.viewmodel_settings = *self.viewmodel_settings.borrow();
        for cmd in self.commands.borrow_mut().drain(..) {
            game.apply(cmd);
        }
        Ok(())
    }

    /// Ask the mounted product UI to show its in-process main menu.
    ///
    /// Desktop Escape uses this direct path instead of emulating the PSP
    /// SELECT button, so opening the menu does not depend on a fixed tick or
    /// on which HUD component currently owns the button lifecycle hook.
    pub fn request_menu(&self) -> Result<()> {
        self.guest.with(|ctx| -> Result<()> {
            let strike: Object = ctx
                .globals()
                .get("strike")
                .context("strike surface missing")?;
            let to_menu: Function = strike
                .get("__requestMenu")
                .context("strike.__requestMenu missing")?;
            to_menu
                .call::<_, ()>(())
                .catch(&ctx)
                .map_err(|e| anyhow!("strike.__requestMenu threw: {e}"))?;
            Ok(())
        })
    }

    pub fn paused(&self) -> bool {
        self.paused.get()
    }

    pub fn mouse_cursor_unlocked(&self) -> bool {
        self.mouse_cursor_unlocked.get()
    }

    pub fn mouse_sensitivity(&self) -> f32 {
        self.mouse_sensitivity.get()
    }

    pub fn scope_multiplier(&self) -> f32 {
        self.scope_multiplier.get()
    }

    pub fn control_scheme(&self) -> u8 {
        self.control_scheme.get()
    }

    pub fn keybindings(&self) -> [[u8; 2]; 8] {
        *self.keybindings.borrow()
    }

    pub fn viewmodel_settings(&self) -> [[f32; 6]; 16] {
        *self.viewmodel_settings.borrow()
    }

    pub fn player_name(&self) -> String {
        self.player_name.borrow().clone()
    }

    pub fn quit_requested(&self) -> bool {
        self.quit_requested.get()
    }

    pub fn drain_lobby_requests(&self) -> Vec<LobbyRequest> {
        self.lobby_requests.borrow_mut().drain(..).collect()
    }

    pub fn drain_map_requests(&self) -> Vec<usize> {
        self.map_requests.borrow_mut().drain(..).collect()
    }

    /// Render the HUD over `view` (`target_px` physical pixels): the UI draws
    /// 1:1 at its logical size offscreen, then composites with a linear blit
    /// (hidpi swapchains scale smoothly).
    pub fn render_overlay(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        target_format: wgpu::TextureFormat,
    ) -> Result<()> {
        if self
            .gfx
            .as_ref()
            .is_none_or(|g| g.target_format != target_format)
        {
            let offscreen = OffscreenTarget::new(gpu, self.ui_size.0, self.ui_size.1);
            let blit = Blit::new(
                gpu,
                &offscreen.view,
                target_format,
                wgpu::FilterMode::Linear,
                true,
            );
            self.gfx = Some(OverlayGfx {
                renderer: UiRenderer::new(gpu, pocket3d::gpu::OFFSCREEN_FORMAT),
                offscreen,
                blit,
                target_format,
            });
        }
        let gfx = self.gfx.as_mut().unwrap();
        let ui_size = self.ui_size;
        self.ui.with_ui(|ui| {
            gfx.renderer.render(
                gpu,
                ui,
                encoder,
                &gfx.offscreen.view,
                ui_size,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            )
        })?;
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hud composite"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            gfx.blit.draw(&mut pass);
        }
        Ok(())
    }
}

fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Starting => "starting",
        Phase::Live => "live",
        Phase::Ended { won: true } => "won",
        Phase::Ended { won: false } => "lost",
    }
}

fn parse_phase(name: &str) -> Option<Phase> {
    Some(match name {
        "starting" => Phase::Starting,
        "live" => Phase::Live,
        "won" => Phase::Ended { won: true },
        "lost" => Phase::Ended { won: false },
        _ => return None,
    })
}

fn build_state<'js>(
    ctx: &pocket_mod::qjs::Ctx<'js>,
    game: &OpenStrike,
) -> pocket_mod::qjs::Result<Object<'js>> {
    let o = Object::new(ctx.clone())?;
    o.set("time", game.time as f64)?;
    o.set("phase", phase_name(game.phase))?;
    o.set("hp", game.player.health)?;
    o.set("alive", game.player.alive)?;
    o.set(
        "ammo",
        if game.weapon_equipped {
            game.weapon.ammo
        } else {
            0
        },
    )?;
    o.set(
        "reserve",
        if game.weapon_equipped {
            game.weapon.reserve
        } else {
            0
        },
    )?;
    o.set("magSize", game.weapon.cfg.mag_size)?;
    o.set("weaponEquipped", game.weapon_equipped)?;
    o.set("weaponName", game.weapon_name())?;
    o.set("weaponSlot", game.active_weapon as u32)?;
    o.set(
        "primaryWeaponSlot",
        game.primary_weapon_slot.map(|slot| slot as u32),
    )?;
    o.set(
        "secondaryWeaponSlot",
        game.secondary_weapon_slot.map(|slot| slot as u32),
    )?;
    o.set("aimFraction", game.aim_fraction_interpolated(1.0) as f64)?;
    o.set("reloading", game.weapon.reloading())?;
    let reload_frac = if game.weapon.reloading() {
        1.0 - (game.weapon.reload_left / game.weapon.cfg.reload_time).clamp(0.0, 1.0)
    } else {
        0.0
    };
    o.set("reloadFrac", reload_frac as f64)?;
    o.set("aliveBots", game.alive_bots() as u32)?;
    o.set("totalBots", game.bots.len() as u32)?;
    o.set("wins", game.score.wins)?;
    o.set("losses", game.score.losses)?;
    let v = game.player.state.vel;
    o.set("speed", ((v.x * v.x + v.z * v.z).sqrt()) as f64)?;
    o.set("doubleJumpAvailable", game.player.double_jump_available())?;
    o.set("pointerX", game.pointer.x as f64)?;
    o.set("pointerY", game.pointer.y as f64)?;
    o.set("lobbyMode", game.lobby_mode.as_str())?;
    o.set("lobbyStatus", game.lobby_status.as_str())?;
    o.set("connectedPlayers", game.connected_players as u32)?;
    o.set("matchStarted", game.match_started)?;
    o.set("multiplayer", game.multiplayer)?;
    o.set("matchRound", game.match_round)?;
    o.set("matchLive", game.match_live)?;
    o.set("matchWinner", game.match_winner.unwrap_or(0))?;
    let lobbies = Array::new(ctx.clone())?;
    for (index, lobby) in game.lan_lobbies.iter().enumerate() {
        let value = format!(
            "{}|{}|{}|{}",
            lobby.name, lobby.address, lobby.players, lobby.map
        );
        lobbies.set(index, value)?;
    }
    o.set("lanLobbies", lobbies)?;
    Ok(o)
}

fn build_event<'js>(
    ctx: &pocket_mod::qjs::Ctx<'js>,
    e: &GameEvent,
) -> pocket_mod::qjs::Result<Object<'js>> {
    let o = Object::new(ctx.clone())?;
    match e {
        GameEvent::Hit {
            bot,
            headshot,
            damage,
            fatal,
        } => {
            o.set("type", "hit")?;
            o.set("bot", *bot as u32)?;
            o.set("headshot", *headshot)?;
            o.set("damage", *damage)?;
            o.set("fatal", *fatal)?;
        }
        GameEvent::PlayerDamaged { amount, hp } => {
            o.set("type", "playerDamaged")?;
            o.set("amount", *amount)?;
            o.set("hp", *hp)?;
        }
        GameEvent::PlayerDied => o.set("type", "playerDied")?,
        GameEvent::RoundReset => o.set("type", "roundReset")?,
    }
    Ok(o)
}

/// Mount the `strike` namespace: intent ops that queue [`Command`]s.
fn mount_strike(
    guest: &Guest,
    commands: &Rc<RefCell<Vec<Command>>>,
    paused: &Rc<Cell<bool>>,
    mouse_cursor_unlocked: &Rc<Cell<bool>>,
    mouse_sensitivity: &Rc<Cell<f32>>,
    scope_multiplier: &Rc<Cell<f32>>,
    control_scheme: &Rc<Cell<u8>>,
    keybindings: &Rc<RefCell<[[u8; 2]; 8]>>,
    viewmodel_settings: &Rc<RefCell<[[f32; 6]; 16]>>,
    player_name: &Rc<RefCell<String>>,
    quit_requested: &Rc<Cell<bool>>,
    lobby_requests: &Rc<RefCell<Vec<LobbyRequest>>>,
    map_requests: &Rc<RefCell<Vec<usize>>>,
    map_names: &[String],
) -> Result<()> {
    let settings_file = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("openstrike/player-settings.json");
    let saved_settings = std::fs::read_to_string(&settings_file).unwrap_or_default();
    guest.mount("strike", move |ctx, ns| {
        macro_rules! op {
            ($name:literal, $f:expr) => {
                ns.set($name, Function::new(ctx.clone(), $f)?)?;
            };
        }

        // Desktop and handheld hosts expose the same index-aligned catalogue.
        ns.set("maps", map_names.to_vec())?;
        ns.set("desktopMapHost", true)?;
        ns.set("savedSettings", saved_settings)?;
        ns.set("playerName", player_name.borrow().as_str())?;
        let save_path = settings_file.clone();
        op!("saveSettings", move |json: String| {
            if json.len() > 64 * 1024 {
                log::warn!("player settings rejected: file is too large");
                return;
            }
            if let Some(parent) = save_path.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                log::warn!("could not create settings directory: {e}");
                return;
            }
            if let Err(e) = std::fs::write(&save_path, json) {
                log::warn!("could not save player settings: {e}");
            }
        });
        let quit_state = quit_requested.clone();
        op!("quit", move || quit_state.set(true));
        let requested_maps = map_requests.clone();
        op!("loadMap", move |i: i32| {
            if i >= 0 {
                requested_maps.borrow_mut().push(i as usize);
            }
        });
        op!("toMenu", move || {
            log::debug!("strike.toMenu: showing desktop product menu");
        });
        let lobby = lobby_requests.clone();
        op!("hostLobby", move || lobby
            .borrow_mut()
            .push(LobbyRequest::Host));
        let lobby = lobby_requests.clone();
        op!("startLocalTest", move || lobby
            .borrow_mut()
            .push(LobbyRequest::LocalTest));
        let lobby = lobby_requests.clone();
        op!("joinLobby", move |index: i32| {
            if index >= 0 {
                lobby.borrow_mut().push(LobbyRequest::Join(index as usize));
            }
        });
        let lobby = lobby_requests.clone();
        op!("startMatch", move || lobby
            .borrow_mut()
            .push(LobbyRequest::StartMatch));
        let lobby = lobby_requests.clone();
        op!("leaveLobby", move || lobby
            .borrow_mut()
            .push(LobbyRequest::Leave));
        let pause_state = paused.clone();
        op!("setPaused", move |value: bool| pause_state.set(value));
        let cursor_state = mouse_cursor_unlocked.clone();
        op!("setMouseCursorUnlocked", move |value: bool| cursor_state
            .set(value));
        let sensitivity_state = mouse_sensitivity.clone();
        op!("setMouseSensitivity", move |value: f32| {
            sensitivity_state.set(value.clamp(0.1, 3.0))
        });
        let scope_multiplier_state = scope_multiplier.clone();
        op!("setScopeMultiplier", move |value: f32| {
            scope_multiplier_state.set(value.clamp(0.1, 2.0))
        });
        let scheme_state = control_scheme.clone();
        op!("setControlScheme", move |value: i32| {
            scheme_state.set(value.clamp(0, 1) as u8)
        });
        let bindings_state = keybindings.clone();
        op!("setKeybind", move |action: i32, slot: i32, key: i32| {
            if (0..8).contains(&action) && (0..2).contains(&slot) {
                bindings_state.borrow_mut()[action as usize][slot as usize] =
                    key.clamp(0, 15) as u8;
            }
        });
        let viewmodel_state = viewmodel_settings.clone();
        op!(
            "setViewmodel",
            move |slot: i32, scale: f32, x: f32, y: f32, z: f32, yaw: f32, pitch: f32| {
                let settings = [
                    scale.clamp(0.5, 12.0),
                    x.clamp(-20.0, 20.0),
                    y.clamp(-20.0, 20.0),
                    z.clamp(-20.0, 20.0),
                    yaw.clamp(-60.0, 60.0),
                    pitch.clamp(-60.0, 60.0),
                ];
                if (0..16).contains(&slot) {
                    log::debug!("viewmodel slot {slot} settings updated: {settings:?}");
                    viewmodel_state.borrow_mut()[slot as usize] = settings;
                }
            }
        );
        let player_name_state = player_name.clone();
        op!("setPlayerName", move |value: String| {
            let cleaned: String = value
                .chars()
                .filter(|ch| !ch.is_control() && ch.is_ascii())
                .take(24)
                .collect();
            *player_name_state.borrow_mut() = if cleaned.trim().is_empty() {
                "Player".into()
            } else {
                cleaned
            };
        });

        let q = commands.clone();
        op!("setPhase", move |name: String| {
            if let Some(p) = parse_phase(&name) {
                q.borrow_mut().push(Command::SetPhase(p));
            } else {
                log::warn!("strike.setPhase: unknown phase '{name}'");
            }
        });

        let q = commands.clone();
        op!("resetRound", move || q
            .borrow_mut()
            .push(Command::ResetRound));

        let q = commands.clone();
        op!("addWin", move || q.borrow_mut().push(Command::AddWin));

        let q = commands.clone();
        op!("addLoss", move || q.borrow_mut().push(Command::AddLoss));

        let q = commands.clone();
        op!("setBotCount", move |n: i32| {
            q.borrow_mut().push(Command::SetBotCount(n.max(0) as usize))
        });

        let q = commands.clone();
        op!("selectWeapon", move |slot: i32| {
            if (0..16).contains(&slot) {
                q.borrow_mut().push(Command::SelectWeapon(slot as usize));
            }
        });

        let q = commands.clone();
        op!("configureWeapon", move |o: Object| {
            let d = WeaponConfig::default();
            let cfg = WeaponConfig {
                mag_size: get_u32(&o, "magSize", d.mag_size),
                reserve: get_u32(&o, "reserve", d.reserve),
                fire_interval: get_f32(&o, "fireInterval", d.fire_interval),
                reload_time: get_f32(&o, "reloadTime", d.reload_time),
                damage_body: get_i32(&o, "damageBody", d.damage_body),
                damage_head: get_i32(&o, "damageHead", d.damage_head),
            };
            q.borrow_mut().push(Command::ConfigureWeapon(cfg));
        });

        let q = commands.clone();
        op!("configureBots", move |o: Object| {
            let d = BotConfig::default();
            let cfg = BotConfig {
                count: get_u32(&o, "count", d.count as u32) as usize,
                speed: get_f32(&o, "speed", d.speed),
                attack_interval: get_f32(&o, "attackInterval", d.attack_interval),
                damage_min: get_i32(&o, "damageMin", d.damage_min),
                damage_max: get_i32(&o, "damageMax", d.damage_max),
            };
            q.borrow_mut().push(Command::ConfigureBots(cfg));
        });

        Ok(())
    })
}

fn get_f32(o: &Object, key: &str, default: f32) -> f32 {
    o.get::<_, f64>(key).map(|v| v as f32).unwrap_or(default)
}

fn get_i32(o: &Object, key: &str, default: i32) -> i32 {
    o.get::<_, i32>(key).unwrap_or(default)
}

fn get_u32(o: &Object, key: &str, default: u32) -> u32 {
    o.get::<_, i32>(key)
        .map(|v| v.max(0) as u32)
        .unwrap_or(default)
}
