//! OpenStrike — a single-player CS-like FPS on the Pocket runtime family:
//! a Pocket3D core, gameplay rules as QuickJS mods, and a Solid JSX HUD.

mod args;
mod audio;
mod bot;
mod game;
mod guest;
mod network;
mod scripts;
mod weapon;

use std::path::PathBuf;
use std::process::{Child, Command as ProcessCommand};

use anyhow::{Context, Result};
use pocket3d::app::{AppConfig, run};
use pocket3d::input::Input;
use pocket3d::prelude::*;
use pocket3d::winit::keyboard::KeyCode;

use crate::args::Args;
use crate::audio::GameAudio;
use crate::game::OpenStrike;
use crate::game::Phase;
use crate::guest::StrikeGuest;
use crate::network::{LobbyBrowser, LobbyRequest, LocalState, NetworkSession, host_addr};

const WINDOW_SIZE: (u32, u32) = (1600, 900);

fn scope_sensitivity_scale(_active_weapon: usize, aim_fraction: f32, multiplier: f32) -> f32 {
    let scope_blend = aim_fraction.clamp(0.0, 1.0);
    1.0 + (multiplier - 1.0) * scope_blend
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = Args::parse()?;

    let map_path = args.resolve_map_path()?;
    log::info!("loading {}", map_path.display());
    let map = pocket3d::bsp::load_map(&map_path, &args.wad_dirs())?;

    // Spawn selection.
    let spawn = if args.spawn_t {
        map.t_spawns.first().or(map.ct_spawns.first())
    } else {
        map.ct_spawns
            .get(args.spawn_index)
            .or(map.ct_spawns.first())
    }
    .copied()
    .context("map has no player spawns")?;
    let mut game = OpenStrike::new(map, spawn.pos, spawn.yaw, args.bots);
    if let Some(pos) = args.pos {
        game.player.state.pos = pos;
        game.player.prev_pos = pos;
    }
    if let Some(yaw) = args.yaw_deg {
        game.player.yaw = yaw.to_radians();
    }
    if let Some(pitch) = args.pitch_deg {
        game.player.pitch = pitch.to_radians();
    }
    game.debug_overlay = args.debug_overlay;
    game.auto_quit = args.auto_quit;

    if let Some(script) = &args.script {
        return scripts::run_script(game, script, &args);
    }
    if args.screenshot.is_some() {
        return scripts::run_screenshot(game, &args);
    }

    // Windowed play needs the product bundle (rules drive the round loop).
    let mut map_catalogue = args.map_catalogue();
    if map_catalogue.is_empty() {
        let name = map_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("map")
            .to_owned();
        map_catalogue.push((name, map_path.clone()));
    }
    let map_names = map_catalogue
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    let strike = StrikeGuest::boot(WINDOW_SIZE, &args.player_name, &map_names)?;
    let network = if args.host {
        Some(NetworkSession::host(
            host_addr(args.port),
            &args.map,
            args.player_name.clone(),
        )?)
    } else if let Some(server) = &args.join {
        Some(NetworkSession::join(
            server,
            args.port,
            &args.map,
            args.player_name.clone(),
        )?)
    } else {
        None
    };
    let browser = LobbyBrowser::new(args.port)
        .map_err(|error| {
            log::warn!("LAN lobby discovery is unavailable: {error:#}");
            error
        })
        .ok();
    if args.host {
        game.lobby_mode = "host".into();
        game.lobby_status = "WAITING FOR PLAYERS".into();
    } else if args.join.is_some() {
        game.lobby_mode = "client".into();
        game.lobby_status = "CONNECTING TO HOST".into();
    }
    game.multiplayer = network.is_some();
    run(
        AppConfig {
            title: "OpenStrike (Pocket3D)".into(),
            size: WINDOW_SIZE,
            tick_hz: 64.0,
            // OpenStrike owns capture through wants_mouse_capture: the menu
            // starts free/visible and gameplay starts locked/hidden.
            capture_mouse: false,
        },
        Windowed {
            game,
            audio: GameAudio::new(),
            strike,
            guest_error: None,
            surface_size: WINDOW_SIZE,
            network,
            browser,
            network_map: args.map.clone(),
            player_name: args.player_name.clone(),
            network_port: args.port,
            last_network_id: None,
            maps_dir: args.maps_dir.clone(),
            test_client: None,
            map_catalogue,
            map_wad_dirs: args.wad_dirs(),
            pending_map: None,
        },
    )
}

/// Adapter between the app loop and the game + its guest.
struct Windowed {
    game: OpenStrike,
    audio: GameAudio,
    strike: StrikeGuest,
    guest_error: Option<anyhow::Error>,
    surface_size: (u32, u32),
    network: Option<NetworkSession>,
    browser: Option<LobbyBrowser>,
    network_map: String,
    player_name: String,
    network_port: u16,
    last_network_id: Option<u32>,
    maps_dir: Option<PathBuf>,
    test_client: Option<Child>,
    map_catalogue: Vec<(String, PathBuf)>,
    map_wad_dirs: Vec<PathBuf>,
    pending_map: Option<usize>,
}

impl Windowed {
    fn place_at_network_spawn(&mut self, id: u32) {
        if let Some(spawn) = self
            .game
            .map
            .ct_spawns
            .get((id.saturating_sub(1) as usize) % self.game.map.ct_spawns.len().max(1))
            .copied()
        {
            self.game.player.state.pos = spawn.pos;
            self.game.player.state.vel = Vec3::ZERO;
            self.game.player.prev_pos = spawn.pos;
            self.game.player.yaw = spawn.yaw;
            self.game.player.pitch = 0.0;
        }
    }

    fn handle_lobby_request(&mut self, request: LobbyRequest) {
        match request {
            LobbyRequest::Host if self.network.is_none() => {
                match NetworkSession::host(
                    host_addr(self.network_port),
                    &self.network_map,
                    self.player_name.clone(),
                ) {
                    Ok(session) => {
                        self.network = Some(session);
                        self.game.lobby_mode = "host".into();
                        self.game.lobby_status = "WAITING FOR PLAYERS".into();
                        self.game.match_started = false;
                        self.game.multiplayer = true;
                    }
                    Err(error) => self.game.lobby_status = format!("HOST FAILED: {error}"),
                }
            }
            LobbyRequest::LocalTest if self.network.is_none() => {
                let host = match NetworkSession::host(
                    host_addr(self.network_port),
                    &self.network_map,
                    self.player_name.clone(),
                ) {
                    Ok(session) => session,
                    Err(error) => {
                        self.game.lobby_status = format!("HOST FAILED: {error}");
                        return;
                    }
                };
                let executable = match std::env::current_exe() {
                    Ok(path) => path,
                    Err(error) => {
                        self.game.lobby_status = format!("SECOND WINDOW FAILED: {error}");
                        return;
                    }
                };
                let mut command = ProcessCommand::new(executable);
                command
                    .arg("--map")
                    .arg(&self.network_map)
                    .arg("--join")
                    .arg(format!("127.0.0.1:{}", self.network_port))
                    .arg("--name")
                    .arg("Local Player 2")
                    .arg("--spawn")
                    .arg("1");
                if let Some(maps_dir) = &self.maps_dir {
                    command.arg("--maps-dir").arg(maps_dir);
                }
                match command.spawn() {
                    Ok(child) => {
                        self.network = Some(host);
                        self.test_client = Some(child);
                        self.game.lobby_mode = "host".into();
                        self.game.lobby_status = "STARTING LOCAL PLAYER 2".into();
                        self.game.match_started = false;
                        self.game.multiplayer = true;
                    }
                    Err(error) => {
                        self.game.lobby_status = format!("SECOND WINDOW FAILED: {error}");
                    }
                }
            }
            LobbyRequest::Join(index) if self.network.is_none() => {
                let Some(lobby) = self.game.lan_lobbies.get(index).cloned() else {
                    self.game.lobby_status = "LOBBY IS NO LONGER AVAILABLE".into();
                    return;
                };
                if lobby.map != self.network_map {
                    self.game.lobby_status = format!("MAP MISMATCH: NEED {}", lobby.map);
                    return;
                }
                match NetworkSession::join_addr(
                    lobby.address,
                    &self.network_map,
                    self.player_name.clone(),
                ) {
                    Ok(session) => {
                        self.network = Some(session);
                        self.game.lobby_mode = "client".into();
                        self.game.lobby_status = format!("JOINING {}", lobby.name.to_uppercase());
                        self.game.match_started = false;
                        self.game.multiplayer = true;
                    }
                    Err(error) => self.game.lobby_status = format!("JOIN FAILED: {error}"),
                }
            }
            LobbyRequest::StartMatch => {
                if let Some(network) = &mut self.network {
                    network.request_start();
                }
            }
            LobbyRequest::Leave => {
                if let Some(mut child) = self.test_client.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                self.network = None;
                self.last_network_id = None;
                self.game.set_remote_players(Vec::new());
                self.game.connected_players = 1;
                self.game.match_started = false;
                self.game.multiplayer = false;
                self.game.match_round = 0;
                self.game.match_live = false;
                self.game.match_winner = None;
                self.game.lobby_mode = "offline".into();
                self.game.lobby_status = "SEARCHING LOCAL NETWORK".into();
            }
            _ => {}
        }
    }
}

impl Game for Windowed {
    fn init(&mut self, gpu: &Gpu, renderer: &mut Renderer) -> Result<()> {
        self.game.upload_world(gpu, renderer);
        Ok(())
    }

    fn frame(&mut self, _dt: f32, input: &Input) {
        if let Some(position) = input.cursor_position() {
            self.game.pointer = Vec2::new(
                position.x * WINDOW_SIZE.0 as f32 / self.surface_size.0.max(1) as f32,
                position.y * WINDOW_SIZE.1 as f32 / self.surface_size.1.max(1) as f32,
            );
        } else if self.strike.paused() || self.strike.mouse_cursor_unlocked() {
            self.game.pointer = Vec2::new(-1.0, -1.0);
        }
        if !self.strike.paused() && (!self.game.multiplayer || self.game.match_live) {
            let weapon_slot = if self.strike.mouse_cursor_unlocked() {
                None
            } else if input.key_pressed(KeyCode::Digit1) || input.key_pressed(KeyCode::Numpad1) {
                Some(0)
            } else if input.key_pressed(KeyCode::Digit2) || input.key_pressed(KeyCode::Numpad2) {
                Some(1)
            } else {
                None
            };
            if let Some(class) = weapon_slot {
                self.game.switch_loadout_slot(class);
            }
            if !self.strike.mouse_cursor_unlocked() {
                let d = input.mouse_delta();
                let scope_scale = scope_sensitivity_scale(
                    self.game.active_weapon,
                    self.game.aim_fraction_interpolated(1.0),
                    self.strike.scope_multiplier(),
                );
                let sensitivity = self.strike.mouse_sensitivity() * scope_scale;
                self.game.apply_look(d.x * sensitivity, d.y * sensitivity);
            }
        }
        if input.key_pressed(KeyCode::F3) {
            self.game.debug_overlay = !self.game.debug_overlay;
        }
    }

    fn tick(&mut self, dt: f32, input: &Input) {
        if self
            .test_client
            .as_mut()
            .is_some_and(|child| child.try_wait().ok().flatten().is_some())
        {
            self.test_client = None;
        }
        if self.network.is_none() {
            if let Some(browser) = &mut self.browser {
                self.game.lan_lobbies = browser.tick();
                self.game.lobby_status = if self.game.lan_lobbies.is_empty() {
                    "SEARCHING LOCAL NETWORK".into()
                } else {
                    format!("{} LOBBY FOUND", self.game.lan_lobbies.len())
                };
            }
        }
        if !self.strike.paused() {
            self.game
                .tick_with_bindings(dt, input, &self.strike.keybindings());
            self.audio.tick(&self.game, input, dt);
        } else {
            self.audio.pause();
        }
        if let Some(network) = &mut self.network {
            let player = &self.game.player;
            let update = network.tick(LocalState {
                pos: player.state.pos,
                yaw: player.yaw,
                pitch: player.pitch,
                shot_direction: self.game.last_shot_direction,
                health: player.health,
                alive: player.alive,
                crouched: player.crouched,
                sliding: player.sliding,
                wall_running: player.is_wall_running(),
                on_ground: player.state.on_ground,
                moving: player.state.vel.x * player.state.vel.x
                    + player.state.vel.z * player.state.vel.z
                    > 4.0,
                aiming: matches!(self.game.active_weapon, 3 | 8)
                    && self.game.aim_fraction_interpolated(1.0) >= 0.75,
                speed: (player.state.vel.x * player.state.vel.x
                    + player.state.vel.z * player.state.vel.z)
                    .sqrt(),
                vertical_velocity: player.state.vel.y,
                wall_roll: player.wall_run_camera_roll(),
                weapon: if self.game.weapon_equipped {
                    self.game.active_weapon
                } else {
                    16
                },
                fired: self.game.fired_this_tick,
            });
            self.game.set_remote_players(update.players);
            self.game.connected_players = update.connected_players;
            self.game.lobby_status = if update.local_id.is_some() {
                format!("CONNECTED / {} PLAYERS", update.connected_players)
            } else {
                "CONNECTING TO HOST".into()
            };
            if update.local_id != self.last_network_id {
                if let Some(id) = update.local_id {
                    self.place_at_network_spawn(id);
                }
                self.last_network_id = update.local_id;
            }
            if update.start_match && !self.game.match_started {
                // LAN matches are player-vs-player.
                self.game.bot_count = 0;
                self.game.bots.clear();
                self.game.match_started = true;
            }
            self.game.match_round = update.round;
            self.game.match_live = update.round_live;
            self.game.match_winner = update.winner_id;
            if let Some(local_id) = update.local_id {
                self.game.score.wins = update
                    .scores
                    .iter()
                    .find(|(id, _)| *id == local_id)
                    .map_or(0, |(_, score)| *score);
                self.game.score.losses = update
                    .scores
                    .iter()
                    .filter(|(id, _)| *id != local_id)
                    .map(|(_, score)| *score)
                    .sum();
            }
            if update.reset_round {
                self.game.reset_round();
                self.game.phase = Phase::Live;
                if let Some(id) = update.local_id {
                    self.place_at_network_spawn(id);
                }
            }
            for shot in &update.shots {
                self.game.apply_remote_shot(&shot.shooter);
            }
        }
        // Pocket button layout: arrows navigate, Enter confirms, Escape is
        // SELECT (opens the in-game return-to-menu dialog).
        let mut buttons = 0u32;
        if input.key_down(KeyCode::ArrowUp) {
            buttons |= 0x0010;
        }
        if input.key_down(KeyCode::ArrowRight) {
            buttons |= 0x0020;
        }
        if input.key_down(KeyCode::ArrowDown) {
            buttons |= 0x0040;
        }
        if input.key_down(KeyCode::ArrowLeft) {
            buttons |= 0x0080;
        }
        if input.key_down(KeyCode::Enter) {
            buttons |= 0x2000;
        }
        // Escape is handled directly above on desktop. Handheld hosts still
        // synthesize SELECT in their platform-specific input adapters.
        if (self.strike.paused() || self.strike.mouse_cursor_unlocked())
            && input.mouse_button_down(pocket3d::winit::event::MouseButton::Left)
        {
            buttons |= 0x2000;
        }
        let delta = if self.strike.paused() || self.strike.mouse_cursor_unlocked() {
            Vec2::ZERO
        } else {
            input.mouse_delta()
        };
        let analog_x = (128.0 + delta.x * 15.0).clamp(0.0, 255.0) as u32;
        let analog_y = (128.0 + delta.y * 15.0).clamp(0.0, 255.0) as u32;
        let analog = (analog_x << 8) | analog_y;
        if let Err(e) = self.strike.turn_with_input(&mut self.game, buttons, analog) {
            log::error!("guest turn failed: {e:#}");
            self.guest_error = Some(e);
        }
        self.player_name = self.strike.player_name();
        if let Some(network) = &mut self.network {
            network.set_name(self.player_name.clone());
        }
        for request in self.strike.drain_lobby_requests() {
            self.handle_lobby_request(request);
        }
        if self.network.is_none() {
            if let Some(index) = self.strike.drain_map_requests().last().copied() {
                if index < self.map_catalogue.len() {
                    self.pending_map = Some(index);
                } else {
                    log::warn!("ignored out-of-range map selection {index}");
                }
            }
        }
    }

    fn prepare(&mut self, gpu: &Gpu, renderer: &Renderer) -> Result<()> {
        let Some(index) = self.pending_map.take() else {
            return Ok(());
        };
        let (name, path) = &self.map_catalogue[index];
        log::info!("loading selected map {}", path.display());
        let map = pocket3d::bsp::load_map(path, &self.map_wad_dirs)
            .with_context(|| format!("loading selected map {}", path.display()))?;
        let spawn = map
            .ct_spawns
            .first()
            .or(map.t_spawns.first())
            .copied()
            .context("selected map has no player spawns")?;
        let weapon_config = self.game.weapon.cfg.clone();
        let bot_config = self.game.bot_cfg.clone();
        let mut selected = OpenStrike::new(map, spawn.pos, spawn.yaw, bot_config.count);
        selected.apply(crate::game::Command::ConfigureWeapon(weapon_config));
        selected.apply(crate::game::Command::ConfigureBots(bot_config));
        selected.debug_overlay = self.game.debug_overlay;
        selected.viewmodel_settings = self.strike.viewmodel_settings();
        selected.upload_world(gpu, renderer);
        self.game = selected;
        self.network_map = name.clone();
        self.audio.pause();
        Ok(())
    }

    fn compose(&mut self, alpha: f32, time: f32, size: (u32, u32)) -> (&Scene, &Camera, &Hud) {
        self.surface_size = size;
        self.game.viewmodel_settings = self.strike.viewmodel_settings();
        self.game
            .compose_base(alpha, time, (size.0 as f32, size.1 as f32));
        (&self.game.scene, &self.game.camera, &self.game.hud)
    }

    fn overlay(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        _size: (u32, u32),
    ) {
        if let Err(e) = self.strike.render_overlay(gpu, encoder, view, format) {
            log::error!("hud overlay failed: {e:#}");
        }
    }

    fn wants_exit(&self) -> bool {
        self.guest_error.is_some()
            || self.strike.quit_requested()
            || self
                .game
                .auto_quit
                .is_some_and(|limit| self.game.time >= limit)
    }

    fn wants_mouse_capture(&self) -> Option<bool> {
        Some(!self.strike.paused() && !self.strike.mouse_cursor_unlocked())
    }

    fn escape_pressed(&mut self) {
        log::info!(
            "input: OpenStrike Escape handler (paused={})",
            self.strike.paused()
        );
        if self.strike.paused() {
            return;
        }
        if let Err(e) = self.strike.request_menu() {
            log::error!("could not open desktop menu: {e:#}");
            self.guest_error = Some(e);
        } else {
            log::info!(
                "input: main menu requested (paused={})",
                self.strike.paused()
            );
        }
    }
}

impl Drop for Windowed {
    fn drop(&mut self) {
        if let Some(mut child) = self.test_client.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod scope_sensitivity_tests {
    use super::scope_sensitivity_scale;

    #[test]
    fn aim_sensitivity_multiplier_blends_for_all_weapons() {
        assert_eq!(scope_sensitivity_scale(3, 1.0, 1.0), 1.0);
        assert_eq!(scope_sensitivity_scale(3, 1.0, 0.5), 0.5);
        assert_eq!(scope_sensitivity_scale(3, 0.5, 0.5), 0.75);
        assert_eq!(scope_sensitivity_scale(2, 1.0, 0.5), 0.5);
    }
}
