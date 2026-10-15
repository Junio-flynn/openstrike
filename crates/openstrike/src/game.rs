//! Desktop game shell: wraps the platform-free simulation
//! (`openstrike_core::StrikeSim`) with wgpu presentation — scene/camera/HUD
//! composition, GPU asset upload — and keyboard/mouse input mapping.
//! `Deref`s to the sim so gameplay state reads the same as before the split.

use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use pocket3d::bsp::Hull;
use pocket3d::bsp::MapData;
use pocket3d::anim::Clip;
use pocket3d::input::Input;
use pocket3d::prelude::*;
use pocket3d::winit::event::MouseButton;
use pocket3d::winit::keyboard::KeyCode;

use openstrike_core::StrikeSim;
use openstrike_core::sim::ray_aabb;
pub use openstrike_core::sim::{Command, GameEvent, Phase, SimInput};
use openstrike_core::weapon::{EffectKind, MUZZLE_LOCAL, RANGE};

use crate::network::{LobbyInfo, PlayerState as RemotePlayer};
use crate::weapon::{build_bullet_hole, build_rifle};

const REMOTE_ANIM_BLEND_SECONDS: f32 = 0.16;

fn compact_name_starts_with(value: &str, prefix: &str) -> bool {
    let mut prefix = prefix.chars();
    let mut expected = prefix.next();
    for ch in value.chars().filter(|ch| ch.is_ascii_alphanumeric()) {
        let Some(want) = expected else {
            return true;
        };
        if !ch.eq_ignore_ascii_case(&want) {
            return false;
        }
        expected = prefix.next();
    }
    expected.is_none()
}

fn clip_name_matches(value: &str, prefix: &str) -> bool {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .any(|part| compact_name_starts_with(part, prefix))
}

fn clip_has_token(value: &str, token: &str) -> bool {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .any(|part| part.eq_ignore_ascii_case(token))
}

fn weapon_clip(clips: &[Clip], action: &str) -> Option<usize> {
    let aliases: &[&str] = match action {
        "idle" => &["idle"],
        "fire" => &["fire", "shoot", "shot", "firing"],
        "reload_empty" => &["reloadempty", "reloadfull", "reload2", "reloatt"],
        "reload" => &["reload", "reloat", "reloadfast", "reload1", "reloatt", "reloatf"],
        "inspect" => &["inspect", "inspec", "lookat"],
        "equip" => &["equip", "draw", "deploy"],
        _ => &[],
    };
    aliases.iter().find_map(|alias| {
        clips
            .iter()
            .position(|clip| {
                clip_name_matches(&clip.name, alias)
                    && (action != "fire" || !clip_has_token(&clip.name, "additive"))
            })
    })
}

fn weapon_default_yaw(slot: usize) -> f32 {
    use core::f32::consts::{FRAC_PI_2, PI};
    match slot {
        // These imports have their barrel along +Z in their authored pose.
        3 | 4 | 5 | 7 | 8 | 10 | 12 | 13 | 14 | 15 => PI,
        // AKM and Beretta imports are laid along +X.
        6 | 11 => FRAC_PI_2,
        // Pistol 43's barrel already points along -Z.
        _ => 0.0,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HitZone {
    Head,
    Body,
    Limbs,
}

fn multiplayer_damage(weapon: usize, zone: HitZone) -> i32 {
    match (weapon, zone) {
        (3, HitZone::Head) => 100,
        (3, HitZone::Body) => 50,
        (3, HitZone::Limbs) => 25,
        (2, HitZone::Head) => 15,
        (2, HitZone::Body) => 10,
        (2, HitZone::Limbs) => 5,
        (1, HitZone::Head) => 20,
        (1, HitZone::Body) => 15,
        (1, HitZone::Limbs) => 10,
        (_, HitZone::Head) => 10,
        (_, HitZone::Body) => 5,
        (_, HitZone::Limbs) => 3,
    }
}

fn player_hit_zone(hit: Vec3, center: Vec3, half_y: f32, right: Vec3) -> HitZone {
    let offset = hit - center;
    if offset.y > half_y * 0.58 {
        HitZone::Head
    } else if offset.y < -half_y * 0.20 || offset.dot(right).abs() > 10.0 {
        // Lower legs and the narrower areas to either side of the torso are
        // limbs. Keeping this geometric makes the visible impact point decide
        // damage rather than the crosshair position.
        HitZone::Limbs
    } else {
        HitZone::Body
    }
}

fn damage_camera_shake(age: f32, strength: f32) -> (f32, f32, f32) {
    const DURATION: f32 = 0.32;
    if !(0.0..DURATION).contains(&age) || strength <= 0.0 {
        return (0.0, 0.0, 0.0);
    }
    let envelope = (1.0 - age / DURATION).powi(2) * strength;
    (
        (age * 93.0 + 0.35).sin() * 2.6f32.to_radians() * envelope,
        (age * 117.0 + 1.1).sin() * 2.1f32.to_radians() * envelope,
        (age * 81.0 + 2.0).sin() * 3.0f32.to_radians() * envelope,
    )
}

#[derive(Clone, Copy)]
struct RemoteTransition {
    clip: usize,
    started_at: f32,
    duration: f32,
    playback_speed: f32,
}

#[derive(Clone, Copy)]
struct RemoteAnimTrack {
    current: AnimState,
    last_updated_at: f32,
    previous: Option<AnimState>,
    blend_started_at: f32,
}

#[derive(Clone, Copy)]
struct BulletHoleMark {
    pos: Vec3,
    normal: Vec3,
}

pub struct OpenStrike {
    pub sim: StrikeSim,
    pub map: MapData,
    pub scene: Scene,
    pub camera: Camera,
    pub hud: Hud,
    pub debug_overlay: bool,

    pub bot_asset: Option<Arc<ModelAsset>>,
    /// Clip index of the bot model's walk cycle (see load in `upload`).
    pub(crate) bot_walk_clip: usize,
    pub(crate) bot_idle_clip: usize,
    pub(crate) bot_run_clip: usize,
    pub(crate) bot_crouch_idle_clip: usize,
    pub(crate) bot_crouch_walk_clip: usize,
    pub(crate) bot_slide_clip: usize,
    pub(crate) bot_slide_loop_clip: usize,
    pub(crate) bot_death_clip: usize,
    pub(crate) bot_walk_start_clip: usize,
    pub(crate) bot_walk_stop_clip: usize,
    pub(crate) bot_crouch_enter_clip: usize,
    pub(crate) bot_crouch_exit_clip: usize,
    pub(crate) bot_crouch_stop_clip: usize,
    pub(crate) bot_jump_up_clip: usize,
    pub(crate) bot_jump_loop_clip: usize,
    pub(crate) bot_jump_down_clip: usize,
    pub(crate) bot_jump_forward_clip: usize,
    /// Compact third-person rifle rendered in remote players' hands.
    pub world_weapon_asset: Option<Arc<ModelAsset>>,
    bullet_hole_asset: Option<Arc<ModelAsset>>,
    bullet_holes: Vec<BulletHoleMark>,
    /// Viewmodels in shared primary/secondary catalog order.
    pub weapon_assets: [Option<Arc<ModelAsset>>; 16],
    /// Elapsed time in the one-shot viewmodel inspect animation; negative
    /// while inspection is inactive.
    pub inspect_time: f32,
    pub prev_inspect_time: f32,
    /// Elapsed time since the weapon was equipped or the player respawned.
    pub equip_time: f32,
    pub prev_equip_time: f32,
    /// Per-weapon tuning: scale, local X/Y/Z, yaw degrees, pitch degrees.
    pub viewmodel_settings: [[f32; 6]; 16],
    /// Exit the app after this many seconds (smoke tests).
    pub auto_quit: Option<f32>,
    pub pointer: Vec2,
    /// Latest LAN snapshots, maintained by the desktop network session.
    pub remote_players: Vec<RemotePlayer>,
    remote_transitions: HashMap<u32, RemoteTransition>,
    remote_animation_tracks: HashMap<u32, RemoteAnimTrack>,
    remote_muzzles: HashMap<u32, (Vec3, Vec3)>,
    remote_scopes: HashMap<u32, Vec3>,
    damage_shake_started: f32,
    damage_shake_strength: f32,
    pub lobby_mode: String,
    pub lobby_status: String,
    pub lan_lobbies: Vec<LobbyInfo>,
    pub connected_players: usize,
    pub match_started: bool,
    pub multiplayer: bool,
    pub match_round: u32,
    pub match_live: bool,
    pub match_winner: Option<u32>,
}

impl Deref for OpenStrike {
    type Target = StrikeSim;
    fn deref(&self) -> &StrikeSim {
        &self.sim
    }
}

impl DerefMut for OpenStrike {
    fn deref_mut(&mut self) -> &mut StrikeSim {
        &mut self.sim
    }
}

impl OpenStrike {
    pub fn new(map: MapData, spawn_pos: Vec3, spawn_yaw: f32, bot_count: usize) -> Self {
        let mut scene = Scene::default();
        if let Some(sun) = map.sun {
            scene.sky.sun_dir = sun.dir;
            scene.lighting.sun_dir = sun.dir;
            scene.lighting.sun_color = sun.color * 0.94;
        }
        // Dusty late-afternoon palette: enough blue overhead to separate dark
        // weapons and players, with warm horizon bounce matching the arena.
        scene.sky.zenith = Vec3::new(0.22, 0.39, 0.62);
        scene.sky.horizon = Vec3::new(0.88, 0.68, 0.43);
        scene.sky.sun_color = Vec3::new(1.0, 0.86, 0.64);
        scene.lighting.ambient = Vec3::new(0.34, 0.37, 0.40);
        let camera = Camera {
            fov_y: 74f32.to_radians(),
            ..Default::default()
        };
        let bot_spawns = if map.t_spawns.is_empty() {
            map.ct_spawns.clone()
        } else {
            map.t_spawns.clone()
        };
        let sim = StrikeSim::new(spawn_pos, spawn_yaw, bot_spawns, bot_count);
        Self {
            sim,
            map,
            scene,
            camera,
            hud: Hud::default(),
            debug_overlay: false,
            bot_asset: None,
            bot_walk_clip: 0,
            bot_idle_clip: 0,
            bot_run_clip: 0,
            bot_crouch_idle_clip: 0,
            bot_crouch_walk_clip: 0,
            bot_slide_clip: 0,
            bot_slide_loop_clip: 0,
            bot_death_clip: 0,
            bot_walk_start_clip: 0,
            bot_walk_stop_clip: 0,
            bot_crouch_enter_clip: 0,
            bot_crouch_exit_clip: 0,
            bot_crouch_stop_clip: 0,
            bot_jump_up_clip: 0,
            bot_jump_loop_clip: 0,
            bot_jump_down_clip: 0,
            bot_jump_forward_clip: 0,
            world_weapon_asset: None,
            bullet_hole_asset: None,
            bullet_holes: Vec::new(),
            weapon_assets: [const { None }; 16],
            inspect_time: -1.0,
            prev_inspect_time: -1.0,
            equip_time: 0.0,
            prev_equip_time: 0.0,
            viewmodel_settings: [[2.2, 0.0, 0.0, 0.0, 0.0, 0.0]; 16],
            auto_quit: None,
            pointer: Vec2::new(-1.0, -1.0),
            remote_players: Vec::new(),
            remote_transitions: HashMap::new(),
            remote_animation_tracks: HashMap::new(),
            remote_muzzles: HashMap::new(),
            remote_scopes: HashMap::new(),
            damage_shake_started: -999.0,
            damage_shake_strength: 0.0,
            lobby_mode: "offline".into(),
            lobby_status: "CHOOSE HOST OR JOIN".into(),
            lan_lobbies: Vec::new(),
            connected_players: 1,
            match_started: false,
            multiplayer: false,
            match_round: 0,
            match_live: false,
            match_winner: None,
        }
    }

    /// Upload GPU resources (called from `Game::init` or headless setup).
    pub fn upload_world(&mut self, gpu: &Gpu, renderer: &Renderer) {
        let world = Arc::new(WorldModel::from_bsp(
            gpu,
            &renderer.world_material_layout,
            &renderer.samplers,
            &self.map,
        ));
        self.scene.world = Some(world);
        self.world_weapon_asset = Some(build_rifle(gpu, renderer));
        self.bullet_hole_asset = Some(build_bullet_hole(gpu, renderer));
        let load_weapon = |asset_path: &str, label: &str| {
            crate::args::find_asset(asset_path).and_then(|path| {
                match ModelAsset::load_glb(
                    gpu,
                    &renderer.model_material_layout,
                    &renderer.samplers,
                    &path,
                ) {
                    Ok(asset) => {
                        log::info!(
                            "loaded {label} viewmodel ({} skeletal clips)",
                            asset.clips.len()
                        );
                        Some(asset)
                    }
                    Err(e) => {
                        log::warn!("{label} viewmodel failed to load: {e:#}");
                        None
                    }
                }
            })
        };
        self.weapon_assets[0] = load_weapon("models/CZ_Scorpion_Viewmodel.glb", "Scorpion")
            .or_else(|| {
                log::warn!("using procedural rifle fallback");
                self.world_weapon_asset.clone()
            });
        self.weapon_assets[1] = load_weapon("models/AK74M_Viewmodel.glb", "AK-74M");
        self.weapon_assets[2] = load_weapon("models/UZI_Viewmodel.glb", "UZI");
        self.weapon_assets[3] = load_weapon(
            "models/BoltActionSniper_Viewmodel.glb",
            "Bolt-action sniper",
        );
        for (slot, path, label) in [
            (4, "models/weapons/ak12/model.glb", "AK-12"),
            (5, "models/weapons/ak74u/model.glb", "AKS-74U"),
            (6, "models/weapons/akm/model.glb", "AKM"),
            (7, "models/weapons/remington/model.glb", "Remington 870"),
            (8, "models/weapons/sniper/model.glb", "Sniper"),
            (9, "models/weapons/pistol43/model.glb", "Pistol 43"),
            (10, "models/weapons/pl2000/model.glb", "PL2000"),
            (11, "models/weapons/beretta/model.glb", "Beretta"),
            (12, "models/weapons/fps_pistol/model.glb", "FPS pistol"),
            (13, "models/weapons/makarov/model.glb", "Makarov PM"),
            (14, "models/weapons/mark23/model.glb", "Mark 23"),
            (15, "models/weapons/usps/model.glb", "USP-S"),
        ] {
            self.weapon_assets[slot] = load_weapon(path, label);
        }

        match crate::args::find_asset("models/Soldier.glb") {
            Some(path) => {
                match ModelAsset::load_glb(
                    gpu,
                    &renderer.model_material_layout,
                    &renderer.samplers,
                    &path,
                ) {
                    Ok(asset) => {
                        // Bots move on their Walk cycle; fall back to clip 0
                        // for single-clip models.
                        self.bot_walk_clip = asset
                            .clips
                            .iter()
                            .position(|c| c.name.eq_ignore_ascii_case("walk"))
                            .unwrap_or(0);
                        self.bot_idle_clip = asset
                            .clips
                            .iter()
                            .position(|c| c.name.eq_ignore_ascii_case("idle"))
                            .unwrap_or(self.bot_walk_clip);
                        self.bot_run_clip = asset
                            .clips
                            .iter()
                            .position(|c| c.name.eq_ignore_ascii_case("run"))
                            .unwrap_or(self.bot_walk_clip);
                        self.bot_crouch_idle_clip = asset
                            .clips
                            .iter()
                            .position(|c| c.name.eq_ignore_ascii_case("crouchidle"))
                            .unwrap_or(self.bot_idle_clip);
                        self.bot_crouch_walk_clip = asset
                            .clips
                            .iter()
                            .position(|c| c.name.eq_ignore_ascii_case("crouchwalk"))
                            .unwrap_or(self.bot_walk_clip);
                        self.bot_slide_clip = asset
                            .clips
                            .iter()
                            .position(|c| c.name.eq_ignore_ascii_case("slide"))
                            .unwrap_or(self.bot_run_clip);
                        self.bot_slide_loop_clip = asset
                            .clips
                            .iter()
                            .position(|c| c.name.eq_ignore_ascii_case("slideloop"))
                            .unwrap_or(self.bot_slide_clip);
                        self.bot_death_clip = asset
                            .clips
                            .iter()
                            .position(|c| c.name.eq_ignore_ascii_case("death"))
                            .unwrap_or(self.bot_idle_clip);
                        let named = |name: &str, fallback: usize| {
                            asset
                                .clips
                                .iter()
                                .position(|clip| clip.name.eq_ignore_ascii_case(name))
                                .unwrap_or(fallback)
                        };
                        self.bot_walk_start_clip = named("walkstart", self.bot_walk_clip);
                        self.bot_walk_stop_clip = named("walkstop", self.bot_idle_clip);
                        self.bot_crouch_enter_clip =
                            named("crouchenter", self.bot_crouch_idle_clip);
                        self.bot_crouch_exit_clip = named("crouchexit", self.bot_idle_clip);
                        self.bot_crouch_stop_clip = named("crouchstop", self.bot_crouch_idle_clip);
                        self.bot_jump_up_clip = named("jumpup", self.bot_run_clip);
                        self.bot_jump_loop_clip = named("jumploop", self.bot_jump_up_clip);
                        self.bot_jump_down_clip = named("jumpdown", self.bot_jump_loop_clip);
                        self.bot_jump_forward_clip = named("jumpforward", self.bot_jump_loop_clip);
                        self.bot_asset = Some(asset);
                        let clip = self.bot_walk_clip;
                        for bot in &mut self.sim.bots {
                            bot.anim.clip = clip;
                        }
                    }
                    Err(e) => log::warn!("bot model failed to load: {e:#}"),
                }
            }
            None => {
                log::warn!("bot model not found (models/Soldier.glb); bots render as nothing")
            }
        }
    }

    pub fn reset_round(&mut self) {
        let clip = self.bot_walk_clip;
        self.sim.reset_round(clip);
        self.inspect_time = -1.0;
        self.prev_inspect_time = -1.0;
        self.equip_time = 0.0;
        self.prev_equip_time = 0.0;
        self.bullet_holes.clear();
        self.damage_shake_started = -999.0;
        self.damage_shake_strength = 0.0;
    }

    pub fn set_remote_players(&mut self, players: Vec<RemotePlayer>) {
        let now = self.sim.time;
        let mut transitions = Vec::new();
        for player in &players {
            let Some(previous) = self
                .remote_players
                .iter()
                .find(|previous| previous.id == player.id)
            else {
                continue;
            };
            if !previous.alive || !player.alive || !previous.on_ground || !player.on_ground {
                continue;
            }
            let previous_moving = previous.speed >= 8.0;
            let moving = player.speed >= 8.0;
            let clip = if previous.crouched != player.crouched {
                Some(if player.crouched {
                    self.bot_crouch_enter_clip
                } else {
                    self.bot_crouch_exit_clip
                })
            } else if !previous_moving && moving && !player.crouched {
                Some(self.bot_walk_start_clip)
            } else if previous_moving && !moving {
                Some(if player.crouched {
                    self.bot_crouch_stop_clip
                } else {
                    self.bot_walk_stop_clip
                })
            } else {
                None
            };
            if let Some(clip) = clip {
                transitions.push((player.id, clip));
            }
        }
        for (id, clip) in transitions {
            let duration = self
                .bot_asset
                .as_ref()
                .and_then(|asset| asset.clips.get(clip))
                .map_or(0.25, |clip| clip.duration);
            let response_time =
                if clip == self.bot_crouch_enter_clip || clip == self.bot_crouch_exit_clip {
                    0.28
                } else if clip == self.bot_walk_start_clip {
                    0.32
                } else if clip == self.bot_walk_stop_clip || clip == self.bot_crouch_stop_clip {
                    0.25
                } else {
                    0.3
                };
            self.remote_transitions.insert(
                id,
                RemoteTransition {
                    clip,
                    started_at: now,
                    duration,
                    playback_speed: (duration / response_time).max(1.0),
                },
            );
        }
        self.remote_transitions
            .retain(|id, _| players.iter().any(|player| player.id == *id));
        self.remote_animation_tracks
            .retain(|id, _| players.iter().any(|player| player.id == *id));
        self.remote_muzzles
            .retain(|id, _| players.iter().any(|player| player.id == *id));
        self.remote_scopes
            .retain(|id, _| players.iter().any(|player| player.id == *id));
        self.remote_players = players;
    }

    /// Apply one guest command (drained after each guest turn).
    pub fn apply(&mut self, cmd: Command) {
        let resets_round = matches!(&cmd, Command::ResetRound);
        let selects_weapon = matches!(&cmd, Command::SelectWeapon(_));
        let previous_weapon = self.sim.active_weapon;
        let was_weapon_equipped = self.sim.weapon_equipped;
        let clip = self.bot_walk_clip;
        self.sim.apply(cmd, clip);
        if resets_round
            || (selects_weapon
                && (self.sim.active_weapon != previous_weapon
                    || (!was_weapon_equipped && self.sim.weapon_equipped)))
        {
            self.inspect_time = -1.0;
            self.prev_inspect_time = -1.0;
            self.equip_time = 0.0;
            self.prev_equip_time = 0.0;
        }
    }

    /// Select a desktop weapon slot and restart its authored equip animation.
    /// Called from the render-frame input path so short number-key taps cannot
    /// fall between fixed simulation ticks.
    pub fn select_weapon(&mut self, slot: usize) -> bool {
        if !self.sim.choose_weapon(slot) {
            return false;
        }
        self.inspect_time = -1.0;
        self.prev_inspect_time = -1.0;
        self.equip_time = 0.0;
        self.prev_equip_time = 0.0;
        log::info!(
            "selected weapon slot {} ({})",
            slot + 1,
            self.sim.weapon_name()
        );
        true
    }

    /// Switch between the selected primary (1) and secondary (2) weapons.
    pub fn switch_loadout_slot(&mut self, class: usize) -> bool {
        if !self.sim.switch_loadout_slot(class) {
            return false;
        }
        self.inspect_time = -1.0;
        self.prev_inspect_time = -1.0;
        self.equip_time = 0.0;
        self.prev_equip_time = 0.0;
        true
    }

    /// Full fixed-step game tick, from raw keyboard/mouse input.
    pub fn tick(&mut self, dt: f32, input: &Input) {
        self.tick_with_bindings(
            dt,
            input,
            &[
                [1, 7],
                [3, 8],
                [2, 9],
                [4, 10],
                [11, 0],
                [12, 0],
                [14, 0],
                [13, 0],
            ],
        );
    }

    pub fn tick_with_scheme(&mut self, dt: f32, input: &Input, scheme: u8) {
        let bindings = if scheme == 1 {
            [
                [5, 0],
                [4, 0],
                [3, 0],
                [6, 0],
                [11, 0],
                [12, 0],
                [14, 0],
                [13, 0],
            ]
        } else {
            [
                [1, 7],
                [3, 8],
                [2, 9],
                [4, 10],
                [11, 0],
                [12, 0],
                [14, 0],
                [13, 0],
            ]
        };
        self.tick_with_bindings(dt, input, &bindings);
    }

    pub fn tick_with_bindings(&mut self, dt: f32, input: &Input, bindings: &[[u8; 2]; 8]) {
        self.prev_inspect_time = self.inspect_time;
        self.prev_equip_time = self.equip_time;
        self.equip_time += dt;
        if input.key_pressed(KeyCode::KeyV) {
            self.sim.toggle_fly();
        }
        let mut sim_input = SimInput {
            walk: binding_down(input, bindings[7]),
            jump: binding_down(input, bindings[4]),
            fire: binding_down(input, bindings[6]),
            aim: input.mouse_button_down(MouseButton::Right),
            reload: binding_pressed(input, bindings[5]),
            crouch: input.key_down(KeyCode::ControlLeft) || input.key_down(KeyCode::ControlRight),
            ..Default::default()
        };
        if binding_down(input, bindings[0]) {
            sim_input.move_y += 1.0;
        }
        if binding_down(input, bindings[1]) {
            sim_input.move_y -= 1.0;
        }
        if binding_down(input, bindings[3]) {
            sim_input.move_x += 1.0;
        }
        if binding_down(input, bindings[2]) {
            sim_input.move_x -= 1.0;
        }
        let col = &self.map.collision;
        self.sim.tick(col, dt, &sim_input);
        let fresh_holes: Vec<_> = self
            .sim
            .effects
            .list
            .iter()
            .filter_map(|effect| match &effect.kind {
                EffectKind::Impact { pos, normal } if effect.age == 0.0 => Some((*pos, *normal)),
                _ => None,
            })
            .collect();
        for (pos, normal) in fresh_holes {
            self.add_bullet_hole(pos, normal);
        }

        // Inspection is presentation state rather than weapon simulation: F
        // starts it, while firing or reloading immediately returns control to
        // the corresponding weapon animation.
        if input.key_pressed(KeyCode::KeyF)
            && self.sim.weapon_equipped
            && !self.sim.weapon.reloading()
            && self.sim.weapon.cooldown <= 0.0
        {
            self.inspect_time = 0.0;
            self.prev_inspect_time = 0.0;
        }
        if self.sim.weapon.reloading() || self.sim.weapon.cooldown > 0.0 {
            self.inspect_time = -1.0;
        } else if self.inspect_time >= 0.0 {
            self.inspect_time += dt;
            let duration = self.weapon_assets[self.sim.active_weapon]
                .as_ref()
                .and_then(|rifle| {
                    weapon_clip(&rifle.clips, "inspect").map(|clip| &rifle.clips[clip])
                })
                .map(|clip| clip.duration)
                .unwrap_or(7.9);
            if self.inspect_time >= duration {
                self.inspect_time = -1.0;
            }
        }
    }

    fn add_bullet_hole(&mut self, pos: Vec3, normal: Vec3) {
        if normal.length_squared() < 0.5 {
            return;
        }
        const MAX_BULLET_HOLES: usize = 96;
        if self.bullet_holes.len() >= MAX_BULLET_HOLES {
            self.bullet_holes.remove(0);
        }
        self.bullet_holes.push(BulletHoleMark {
            pos,
            normal: normal.normalize(),
        });
    }

    /// Apply a remote player's hitscan shot to this locally-owned player.
    /// The receiver owns its health, so a forged packet cannot damage anyone
    /// other than the machine accepting it.
    pub fn apply_remote_shot(&mut self, shooter: &RemotePlayer) {
        if !self.sim.player.alive || !shooter.alive {
            return;
        }
        // Use the actual scattered hitscan ray generated when the trigger was
        // pulled. Camera yaw/pitch is only where the crosshair points and is
        // intentionally different for an unscoped sniper shot.
        let direction = shooter.shot_direction.normalize_or_zero();
        if direction == Vec3::ZERO {
            return;
        }
        let eye_height = if shooter.crouched { 24.0 } else { 28.0 };
        let origin = shooter.pos + Vec3::Y * eye_height;
        let (muzzle, visual_direction) = self
            .remote_muzzles
            .get(&shooter.id)
            .copied()
            .unwrap_or_else(|| {
                let gun_height = if shooter.crouched { 7.0 } else { 13.0 };
                (
                    shooter.pos + Vec3::Y * gun_height + direction * 12.0,
                    direction,
                )
            });
        let right = visual_direction.cross(Vec3::Y).normalize_or_zero();
        let up = right.cross(visual_direction).normalize_or_zero();
        self.sim.effects.spawn(
            EffectKind::MuzzleFlash {
                pos: muzzle,
                direction: visual_direction,
            },
            0.075,
        );
        self.sim.effects.spawn(
            EffectKind::ShellEject {
                pos: muzzle - visual_direction * 8.0,
                velocity: right * 62.0 + up * 38.0 - visual_direction * 10.0,
            },
            0.32,
        );
        let world_hit = self
            .map
            .collision
            .trace(Hull::Point, origin, origin + direction * RANGE);
        let world_distance = world_hit.fraction * RANGE;
        let half_y = if self.sim.player.crouched { 18.0 } else { 36.0 };
        let center = self.sim.player.state.pos;
        let player_hit = ray_aabb(
            origin,
            direction,
            center - Vec3::new(16.0, half_y, 16.0),
            center + Vec3::new(16.0, half_y, 16.0),
        )
        .filter(|distance| *distance <= world_distance);
        let end = origin + direction * player_hit.unwrap_or(world_distance);
        self.sim
            .effects
            .spawn(EffectKind::Tracer { a: muzzle, b: end }, 0.07);
        let Some(distance) = player_hit else {
            if world_hit.fraction < 1.0 {
                self.sim.effects.spawn(
                    EffectKind::Impact {
                        pos: end,
                        normal: world_hit.normal,
                    },
                    0.3,
                );
                self.add_bullet_hole(end, world_hit.normal);
            }
            return;
        };
        let hit = origin + direction * distance;
        let zone = player_hit_zone(hit, center, half_y, self.sim.player.right());
        let damage = multiplayer_damage(shooter.weapon, zone);
        self.sim.player.health = (self.sim.player.health - damage).max(0);
        self.damage_shake_started = self.sim.time;
        self.damage_shake_strength = (0.55 + damage as f32 / 100.0 * 0.75).clamp(0.55, 1.3);
        self.sim
            .effects
            .spawn(EffectKind::BloodPuff { pos: hit }, 0.22);
        if self.sim.player.health == 0 {
            self.sim.player.alive = false;
        }
        self.sim.events.push(GameEvent::PlayerDamaged {
            amount: damage,
            hp: self.sim.player.health,
        });
        if !self.sim.player.alive {
            self.sim.events.push(GameEvent::PlayerDied);
        }
    }

    /// Build camera, scene models/effects, and HUD for the current frame.
    pub fn compose_base(&mut self, alpha: f32, time: f32, screen: (f32, f32)) {
        self.scene.time = time;
        self.camera.pos = self.sim.player.eye_interpolated(alpha);
        let (punch_pitch, punch_yaw, shot_roll) = self.sim.weapon.camera_kick_interpolated(alpha);
        let (hit_pitch, hit_yaw, hit_roll) = damage_camera_shake(
            self.sim.time - self.damage_shake_started,
            self.damage_shake_strength,
        );
        self.camera.yaw = self.sim.player.yaw + punch_yaw + hit_yaw;
        self.camera.pitch = (self.sim.player.pitch + punch_pitch + hit_pitch)
            .clamp(-89f32.to_radians(), 89f32.to_radians());
        self.camera.roll = self.sim.player.camera_roll_interpolated(alpha) + shot_roll + hit_roll;
        self.camera.fov_y = 74f32.to_radians()
            + self.sim.player.camera_fov_boost_interpolated(alpha)
            + self.sim.aim_fov_offset_interpolated(alpha);

        // Bots.
        self.scene.models.clear();
        self.remote_scopes.clear();
        if let Some(asset) = &self.bot_asset {
            let scale = 70.0 / asset.height();
            for bot in &self.sim.bots {
                let mut inst = ModelInstance::new(asset.clone());
                inst.transform = bot.transform_scaled(scale);
                inst.anim = AnimState {
                    clip: bot.anim.clip,
                    time: bot.anim.time,
                    speed: bot.anim.speed,
                    looping: bot.anim.looping,
                };
                inst.tint = bot.tint();
                self.scene.models.push(inst);
            }
            for player in &self.remote_players {
                let mut inst = ModelInstance::new(asset.clone());
                let feet = player.pos - Vec3::Y * if player.crouched { 18.0 } else { 36.0 };
                let pose = if player.alive && player.wall_running {
                    Mat4::from_rotation_z(player.wall_roll * 2.2)
                        * Mat4::from_rotation_x(10f32.to_radians())
                } else if player.alive && !player.on_ground {
                    Mat4::from_rotation_x(8f32.to_radians())
                } else {
                    Mat4::IDENTITY
                };
                inst.transform = Mat4::from_translation(feet)
                    * Mat4::from_rotation_y(player.yaw)
                    // Mixamo's SWAT faces +Z while OpenStrike gameplay faces
                    // -Z at yaw zero.
                    * Mat4::from_rotation_y(core::f32::consts::PI)
                    * pose
                    * Mat4::from_scale(Vec3::splat(scale));
                let active_transition = self.remote_transitions.get(&player.id).filter(|state| {
                    self.sim.time - state.started_at < state.duration / state.playback_speed
                });
                let previous_clip = self
                    .remote_animation_tracks
                    .get(&player.id)
                    .map(|track| track.current.clip);
                let running = player.speed
                    > if previous_clip == Some(self.bot_run_clip) {
                        180.0
                    } else {
                        215.0
                    };
                let moving = player.speed
                    >= if previous_clip == Some(self.bot_idle_clip)
                        || previous_clip == Some(self.bot_crouch_idle_clip)
                    {
                        12.0
                    } else {
                        4.0
                    };
                let (clip, looping, anim_speed) = if !player.alive {
                    (self.bot_death_clip, false, 1.0)
                } else if player.sliding {
                    (self.bot_slide_loop_clip, true, 1.0)
                } else if !player.on_ground && player.speed > 135.0 {
                    (self.bot_jump_forward_clip, false, 1.0)
                } else if !player.on_ground && player.vertical_velocity > 70.0 {
                    (self.bot_jump_up_clip, false, 1.0)
                } else if !player.on_ground && player.vertical_velocity < -70.0 {
                    (self.bot_jump_down_clip, false, 1.0)
                } else if !player.on_ground {
                    (self.bot_jump_loop_clip, true, 1.0)
                } else if let Some(transition) = active_transition {
                    (transition.clip, false, transition.playback_speed)
                } else if player.crouched && moving {
                    (
                        self.bot_crouch_walk_clip,
                        true,
                        (player.speed / 85.0).clamp(1.0, 1.7),
                    )
                } else if player.crouched {
                    (self.bot_crouch_idle_clip, true, 1.0)
                } else if !moving {
                    (self.bot_idle_clip, true, 1.0)
                } else if running || player.wall_running {
                    (
                        self.bot_run_clip,
                        true,
                        (player.speed / 245.0).clamp(0.72, 1.35),
                    )
                } else {
                    (
                        self.bot_walk_clip,
                        true,
                        (player.speed / 150.0).clamp(0.68, 1.4),
                    )
                };
                let now = self.sim.time;
                let track =
                    self.remote_animation_tracks
                        .entry(player.id)
                        .or_insert(RemoteAnimTrack {
                            current: AnimState {
                                clip,
                                time: 0.0,
                                speed: anim_speed,
                                looping,
                            },
                            last_updated_at: now,
                            previous: None,
                            blend_started_at: now - REMOTE_ANIM_BLEND_SECONDS,
                        });
                // Accumulate from the previously sampled time. Recomputing
                // `elapsed * speed` made every network speed adjustment jump
                // to a different frame and was the source of locomotion jitter.
                let animation_dt = (now - track.last_updated_at).clamp(0.0, 0.1);
                track.current.time += animation_dt * track.current.speed;
                if let Some(previous) = &mut track.previous {
                    previous.time += animation_dt * previous.speed;
                }
                track.last_updated_at = now;
                if track.current.clip != clip {
                    track.previous = Some(track.current);
                    track.current = AnimState {
                        clip,
                        time: 0.0,
                        speed: anim_speed,
                        looping,
                    };
                    track.blend_started_at = now;
                } else {
                    track.current.speed = anim_speed;
                    track.current.looping = looping;
                }
                let blend_factor =
                    ((now - track.blend_started_at) / REMOTE_ANIM_BLEND_SECONDS).clamp(0.0, 1.0);
                inst.anim = track.current;
                inst.blend_from = track.previous.filter(|_| blend_factor < 1.0);
                inst.blend_factor = blend_factor;
                inst.upper_body_pitch =
                    (player.pitch * 0.72).clamp(-60f32.to_radians(), 60f32.to_radians());
                let mut node_globals = Vec::new();
                asset.node_globals_blended(
                    &inst.anim,
                    inst.blend_from.as_ref(),
                    inst.blend_factor,
                    inst.upper_body_pitch,
                    &mut node_globals,
                );

                // Crouch/slide animations bend the legs and can lift the
                // skinned feet even though the physics hull remains grounded.
                // Keep the lowest animated foot/toe at its rest-pose height.
                if player.on_ground {
                    let foot_nodes = [
                        "mixamorig:LeftFoot",
                        "mixamorig:LeftToeBase",
                        "mixamorig:RightFoot",
                        "mixamorig:RightToeBase",
                    ];
                    let indices: Vec<_> = foot_nodes
                        .iter()
                        .filter_map(|name| asset.node_index(name))
                        .collect();
                    if !indices.is_empty() {
                        let mut rest_globals = Vec::new();
                        asset.rest_node_globals(&mut rest_globals);
                        let rest_low = indices
                            .iter()
                            .map(|&index| rest_globals[index].transform_point3(Vec3::ZERO).y)
                            .fold(f32::MAX, f32::min);
                        let animated_low = indices
                            .iter()
                            .map(|&index| node_globals[index].transform_point3(Vec3::ZERO).y)
                            .fold(f32::MAX, f32::min);
                        let correction = (rest_low - animated_low) * scale;
                        inst.transform =
                            Mat4::from_translation(Vec3::Y * correction) * inst.transform;
                    }
                }
                inst.tint = if player.alive {
                    [0.68, 0.82, 1.0, 1.0]
                } else {
                    [0.38, 0.44, 0.52, 1.0]
                };
                let character_transform = inst.transform;
                self.scene.models.push(inst);

                if player.alive && player.weapon < self.weapon_assets.len() {
                    if let Some(weapon) = &self.world_weapon_asset {
                        let mut held = ModelInstance::new(weapon.clone());
                        let right_hand = asset
                            .node_index("mixamorig:RightHand")
                            .and_then(|index| node_globals.get(index))
                            .map(|joint| {
                                character_transform
                                    .transform_point3(joint.transform_point3(Vec3::ZERO))
                            });
                        let left_hand = asset
                            .node_index("mixamorig:LeftHand")
                            .and_then(|index| node_globals.get(index))
                            .map(|joint| {
                                character_transform
                                    .transform_point3(joint.transform_point3(Vec3::ZERO))
                            });
                        held.transform = match (right_hand, left_hand) {
                            (Some(right), Some(left)) if right.distance_squared(left) > 1.0 => {
                                // Anchor the pistol grip to the trigger hand,
                                // but aim local -Z from replicated view yaw and
                                // pitch. Hand separation changes dramatically
                                // during a slide, so only the rest rig controls
                                // weapon scale.
                                let (sy, cy) = player.yaw.sin_cos();
                                let (sp, cp) = player.pitch.sin_cos();
                                let aim = Vec3::new(-sy * cp, sp, -cy * cp).normalize_or_zero();
                                let model_up = character_transform
                                    .transform_vector3(Vec3::Y)
                                    .normalize_or_zero();
                                let z_axis = -aim;
                                let side = model_up.cross(z_axis);
                                let x_axis = if side.length_squared() > 0.001 {
                                    side.normalize()
                                } else {
                                    character_transform
                                        .transform_vector3(Vec3::X)
                                        .normalize_or_zero()
                                };
                                let y_axis = z_axis.cross(x_axis).normalize_or_zero();
                                let rotation = Mat4::from_cols(
                                    x_axis.extend(0.0),
                                    y_axis.extend(0.0),
                                    z_axis.extend(0.0),
                                    Vec4::W,
                                );
                                let hand_scale = match player.weapon {
                                    1 => 0.76,
                                    2 => 0.64,
                                    3 => 0.88,
                                    _ => 0.72,
                                };
                                let grip = Vec3::new(0.0, -3.5, 0.0);
                                Mat4::from_translation(right)
                                    * rotation
                                    * Mat4::from_scale(Vec3::splat(hand_scale))
                                    * Mat4::from_translation(-grip)
                            }
                            _ => character_transform,
                        };
                        held.tint = match player.weapon {
                            1 => [0.72, 0.80, 0.68, 1.0],
                            2 => [0.82, 0.72, 0.62, 1.0],
                            3 => [0.32, 0.36, 0.31, 1.0],
                            _ => [0.72, 0.76, 0.82, 1.0],
                        };
                        held.lit = 1.0;
                        let muzzle = held.transform.transform_point3(MUZZLE_LOCAL);
                        let muzzle_direction = held
                            .transform
                            .transform_vector3(-Vec3::Z)
                            .normalize_or_zero();
                        self.remote_muzzles
                            .insert(player.id, (muzzle, muzzle_direction));
                        if matches!(player.weapon, 3 | 8) {
                            // Rear optic lens on the shared third-person rifle.
                            let scope = held.transform.transform_point3(Vec3::new(0.0, 2.35, -5.4));
                            self.remote_scopes.insert(player.id, scope);
                        }
                        self.scene.models.push(held);
                    }
                }
            }
        }

        if let Some(asset) = &self.bullet_hole_asset {
            for mark in &self.bullet_holes {
                let mut hole = ModelInstance::new(asset.clone());
                let rotation = Quat::from_rotation_arc(Vec3::Z, mark.normal);
                hole.transform = Mat4::from_translation(mark.pos + mark.normal * 0.12)
                    * Mat4::from_quat(rotation)
                    * Mat4::from_scale(Vec3::splat(2.1));
                hole.lit = 0.45;
                self.scene.models.push(hole);
            }
        }

        // Viewmodel (interpolated with the camera — see viewmodel_transform_at).
        let active_asset = &self.weapon_assets[self.sim.active_weapon];
        let sniper_scoped = matches!(self.sim.active_weapon, 3 | 8)
            && self.sim.aim_fraction_interpolated(alpha) >= 0.92;
        self.scene.viewmodel = match (
            self.sim.weapon_equipped,
            active_asset,
            self.sim.player.alive && !sniper_scoped,
        ) {
            (true, Some(rifle), true) => {
                let mut vm = ModelInstance::new(rifle.clone());
                let [scale, x, y, z, yaw, pitch] = self.viewmodel_settings[self.sim.active_weapon];
                // Imported packs use different source units. Normalize every
                // weapon to the Scorpion's authored bounds so a size of 4.0,
                // for example, has the same visual scale in every slot.
                let reference_height = self.weapon_assets[0]
                    .as_ref()
                    .map(|asset| asset.height())
                    .unwrap_or_else(|| rifle.height());
                let model_extent = (rifle.aabb.1 - rifle.aabb.0).max_element().max(0.001);
                let reference_extent = self.weapon_assets[0]
                    .as_ref()
                    .map(|asset| (asset.aabb.1 - asset.aabb.0).max_element())
                    .unwrap_or(reference_height);
                let normalized_scale = if self.sim.active_weapon >= 4 {
                    // Imported GLBs don't share a consistent up axis, so
                    // their Y extent is not a useful size reference. Match
                    // their longest weapon dimension to the original rifle.
                    // Sidearms get a compact silhouette at the same time.
                    let weapon_class_scale = if self.sim.active_weapon >= 9 {
                        0.72
                    } else if self.sim.active_weapon == 8 {
                        1.08
                    } else {
                        1.0
                    };
                    scale * reference_extent / model_extent * weapon_class_scale
                } else {
                    let asset_scale = if self.sim.active_weapon == 3 {
                        0.18
                    } else {
                        1.0
                    };
                    scale * reference_height / rifle.height().max(0.001) * asset_scale
                };
                // Scale around the visible model center, while preserving
                // the established 2.2-size placement. Scaling around the
                // glTF origin also changed camera depth, making equal slider
                // steps look progressively smaller.
                let center = (rifle.aabb.0 + rifle.aabb.1) * 0.5;
                // Keep horizontal scaling centered, but anchor vertical
                // scaling at the model's lower edge. Increasing SIZE now
                // grows the weapon upward without also changing its Y level.
                let scale_anchor = Vec3::new(center.x, rifle.aabb.0.y, center.z);
                let scale_compensation = scale_anchor * (2.2 - normalized_scale);
                let screen_x = if self.sim.active_weapon >= 4 {
                    x - 3.5
                } else {
                    x
                };
                vm.transform = self.sim.viewmodel_transform_at(alpha)
                    * Mat4::from_translation(Vec3::new(screen_x, y, z))
                    * Mat4::from_rotation_y(yaw.to_radians() + weapon_default_yaw(self.sim.active_weapon))
                    * Mat4::from_rotation_x(pitch.to_radians())
                    * Mat4::from_translation(scale_compensation)
                    * Mat4::from_translation(center)
                    * Mat4::from_translation(-center)
                    * Mat4::from_scale(Vec3::splat(normalized_scale));
                if !rifle.clips.is_empty() {
                    // The asset root is already baked into its authored FBX
                    // camera space; the shared viewmodel transform adds only
                    // interpolated placement, recoil, bob, and sway.

                    let named = |name: &str| weapon_clip(&rifle.clips, name);
                    let render_time =
                        (self.sim.time - (1.0 - alpha.clamp(0.0, 1.0)) / 64.0).max(0.0);
                    let (clip, time, looping) = if self.sim.weapon.reloading() {
                        let action = if self.sim.weapon.ammo == 0 {
                            "reload_empty"
                        } else {
                            "reload"
                        };
                        if let Some(clip) = named(action).or_else(|| named("reload")) {
                            let reload_left = self.sim.weapon.prev_reload_left
                                + (self.sim.weapon.reload_left - self.sim.weapon.prev_reload_left)
                                    * alpha;
                            let reload_frac = (1.0
                                - reload_left / self.sim.weapon.cfg.reload_time)
                                .clamp(0.0, 1.0);
                            (clip, reload_frac * rifle.clips[clip].duration, false)
                        } else {
                            (0, 0.0, false)
                        }
                    } else if named("fire").is_some_and(|clip| {
                        self.sim.weapon.shot_age < rifle.clips[clip].duration
                    }) {
                        let clip = named("fire").unwrap_or(0);
                        let shot_age = self.sim.weapon.prev_shot_age
                            + (self.sim.weapon.shot_age - self.sim.weapon.prev_shot_age) * alpha;
                        (clip, shot_age, false)
                    } else if self.inspect_time >= 0.0 && named("inspect").is_some() {
                        let clip = named("inspect").unwrap_or(0);
                        let inspect_time = self.prev_inspect_time
                            + (self.inspect_time - self.prev_inspect_time) * alpha;
                        (clip, inspect_time, false)
                    } else if named("equip")
                        .is_some_and(|clip| self.equip_time < rifle.clips[clip].duration)
                    {
                        let clip = named("equip").unwrap_or(0);
                        let equip_time =
                            self.prev_equip_time + (self.equip_time - self.prev_equip_time) * alpha;
                        (clip, equip_time, false)
                    } else {
                        if let Some(clip) = named("idle") {
                            (clip, render_time, true)
                        } else {
                            // Generic FBX/scene clips can contain a whole
                            // timeline of actions but expose no usable action
                            // names. Never loop clip zero as fake idle: that
                            // made the sniper play the same motion forever.
                            (0, 0.0, false)
                        }
                    };
                    vm.anim = AnimState {
                        clip,
                        time,
                        speed: 1.0,
                        looping,
                    };
                }
                vm.lit = 1.0;
                Some(vm)
            }
            _ => None,
        };

        // Effects.
        self.scene.sprites.clear();
        self.scene.beams.clear();
        let mut sprites = Vec::new();
        let mut beams = Vec::new();
        self.sim.effects.emit(&mut sprites, &mut beams);
        self.scene
            .sprites
            .extend(sprites.into_iter().map(|s| Sprite {
                pos: s.pos,
                size: s.size,
                color: s.color,
            }));
        self.scene.beams.extend(beams.into_iter().map(|b| Beam {
            a: b.a,
            b: b.b,
            width: b.width,
            color: b.color,
        }));
        // A large pulsing additive starburst warns opponents whenever a remote
        // sniper is actively scoped. It is anchored to the third-person optic
        // and its crossed rays face the local camera from every viewing angle.
        let glint_pulse = 0.88 + (self.sim.time * 8.0).sin().abs() * 0.32;
        for player in self
            .remote_players
            .iter()
            .filter(|player| player.alive && matches!(player.weapon, 3 | 8) && player.aiming)
        {
            if let Some(&pos) = self.remote_scopes.get(&player.id) {
                let toward_camera = (self.camera.pos - pos).normalize_or_zero();
                let horizontal = {
                    let axis = Vec3::Y.cross(toward_camera);
                    if axis.length_squared() > 0.001 {
                        axis.normalize()
                    } else {
                        Vec3::X
                    }
                };
                let vertical = toward_camera.cross(horizontal).normalize_or_zero();
                let diagonal_a = (horizontal + vertical).normalize_or_zero();
                let diagonal_b = (horizontal - vertical).normalize_or_zero();
                for (axis, radius) in [
                    (horizontal, 82.0),
                    (vertical, 82.0),
                    (diagonal_a, 58.0),
                    (diagonal_b, 58.0),
                ] {
                    let extent = radius * glint_pulse;
                    self.scene.beams.push(Beam {
                        a: pos - axis * extent,
                        b: pos + axis * extent,
                        width: 11.0,
                        color: [0.30, 0.70, 1.0, 1.0],
                    });
                    self.scene.beams.push(Beam {
                        a: pos - axis * extent,
                        b: pos + axis * extent,
                        width: 3.2,
                        color: [0.96, 1.0, 1.0, 1.0],
                    });
                }
                self.scene.sprites.push(Sprite {
                    pos,
                    size: 118.0 * glint_pulse,
                    color: [0.30, 0.70, 1.0, 1.0],
                });
                self.scene.sprites.push(Sprite {
                    pos,
                    size: 25.0 + 9.0 * glint_pulse,
                    color: [0.96, 1.0, 1.0, 1.0],
                });
            }
        }

        self.compose_hud(screen);
    }

    fn compose_hud(&mut self, screen: (f32, f32)) {
        let aspect = screen.0 / screen.1.max(1.0);
        let view_proj = self.camera.view_proj(aspect);
        let mut player_labels = Vec::new();
        for player in self.remote_players.iter().filter(|player| player.alive) {
            let overhead = player.pos + Vec3::Y * if player.crouched { 31.0 } else { 49.0 };
            let to_label = overhead - self.camera.pos;
            let distance = to_label.length();
            if distance > 2_200.0 {
                continue;
            }
            let visibility = self
                .map
                .collision
                .trace(Hull::Point, self.camera.pos, overhead);
            if visibility.fraction < 0.98 {
                continue;
            }
            let clip = view_proj * overhead.extend(1.0);
            if clip.w <= 0.0 || clip.z < 0.0 || clip.z > clip.w {
                continue;
            }
            let ndc = clip.truncate() / clip.w;
            let x = (ndc.x * 0.5 + 0.5) * screen.0;
            let y = (0.5 - ndc.y * 0.5) * screen.1;
            if x >= 0.0 && x <= screen.0 && y >= 0.0 && y <= screen.1 {
                player_labels.push((x, y, player.name.clone(), distance));
            }
        }
        let hud = &mut self.hud;
        hud.clear();
        for (x, y, name, distance) in player_labels {
            let scale = ((screen.1 / 900.0) * (1_000.0 / distance.max(500.0))).clamp(0.75, 1.35);
            let width = Hud::text_width(&name, scale);
            hud.rect(
                x - width * 0.5 - 4.0,
                y - 3.0,
                width + 8.0,
                8.0 * scale + 6.0,
                [0.01, 0.02, 0.03, 0.68],
            );
            hud.text_centered(x, y, scale, [0.72, 0.95, 0.30, 1.0], &name);
        }
        // The player-facing HUD is a PocketJS app composited by the host
        // (game/hud.tsx via pocket-ui-wgpu); the bitmap HUD only carries the
        // F3 debug overlay now.
        if self.debug_overlay {
            let p = &self.sim.player.state;
            let phase = format!("{:?}", self.sim.phase);
            hud.text(
                8.0,
                8.0,
                2.0,
                [1.0, 1.0, 1.0, 0.8],
                &format!(
                    "POS {:6.0} {:6.0} {:6.0}  VEL {:5.0}  {}  {}",
                    p.pos.x,
                    p.pos.y,
                    p.pos.z,
                    (p.vel.x * p.vel.x + p.vel.z * p.vel.z).sqrt(),
                    if p.on_ground { "GND" } else { "AIR" },
                    phase,
                ),
            );
            if !self.remote_players.is_empty() {
                hud.text(
                    8.0,
                    32.0,
                    2.0,
                    [0.68, 0.82, 1.0, 0.9],
                    &format!("LAN PLAYERS {}", self.remote_players.len() + 1),
                );
            }
        }
    }
}

fn binding_key(id: u8) -> Option<KeyCode> {
    Some(match id {
        1 => KeyCode::KeyW,
        2 => KeyCode::KeyA,
        3 => KeyCode::KeyS,
        4 => KeyCode::KeyD,
        5 => KeyCode::KeyE,
        6 => KeyCode::KeyF,
        7 => KeyCode::ArrowUp,
        8 => KeyCode::ArrowDown,
        9 => KeyCode::ArrowLeft,
        10 => KeyCode::ArrowRight,
        11 => KeyCode::Space,
        12 => KeyCode::KeyR,
        13 => KeyCode::ShiftLeft,
        _ => return None,
    })
}

fn binding_down(input: &Input, binding: [u8; 2]) -> bool {
    binding.into_iter().any(|id| match id {
        14 => input.mouse_button_down(MouseButton::Left),
        15 => input.mouse_button_down(MouseButton::Right),
        _ => binding_key(id).is_some_and(|key| input.key_down(key)),
    })
}

fn binding_pressed(input: &Input, binding: [u8; 2]) -> bool {
    binding.into_iter().any(|id| match id {
        14 => input.mouse_button_pressed(MouseButton::Left),
        15 => input.mouse_button_pressed(MouseButton::Right),
        _ => binding_key(id).is_some_and(|key| input.key_pressed(key)),
    })
}

#[cfg(test)]
mod multiplayer_damage_tests {
    use super::*;

    #[test]
    fn requested_weapon_damage_applies_to_all_three_hit_zones() {
        assert_eq!(multiplayer_damage(3, HitZone::Head), 100);
        assert_eq!(multiplayer_damage(3, HitZone::Body), 50);
        assert_eq!(multiplayer_damage(3, HitZone::Limbs), 25);

        assert_eq!(multiplayer_damage(2, HitZone::Head), 15);
        assert_eq!(multiplayer_damage(2, HitZone::Body), 10);
        assert_eq!(multiplayer_damage(2, HitZone::Limbs), 5);

        assert_eq!(multiplayer_damage(1, HitZone::Head), 20);
        assert_eq!(multiplayer_damage(1, HitZone::Body), 15);
        assert_eq!(multiplayer_damage(1, HitZone::Limbs), 10);
    }

    #[test]
    fn hit_zone_uses_the_actual_impact_point() {
        let center = Vec3::ZERO;
        let right = Vec3::X;
        assert_eq!(
            player_hit_zone(Vec3::new(0.0, 24.0, 0.0), center, 36.0, right),
            HitZone::Head
        );
        assert_eq!(
            player_hit_zone(Vec3::new(0.0, 5.0, 0.0), center, 36.0, right),
            HitZone::Body
        );
        assert_eq!(
            player_hit_zone(Vec3::new(0.0, -12.0, 0.0), center, 36.0, right),
            HitZone::Limbs
        );
        assert_eq!(
            player_hit_zone(Vec3::new(13.0, 4.0, 0.0), center, 36.0, right),
            HitZone::Limbs
        );
    }

    #[test]
    fn damage_camera_shake_starts_strong_and_fades_out() {
        let start = damage_camera_shake(0.0, 1.0);
        assert!(start.0.abs() > 0.005);
        assert!(start.1.abs() > 0.005);
        assert!(start.2.abs() > 0.005);
        assert_eq!(damage_camera_shake(0.32, 1.0), (0.0, 0.0, 0.0));
        assert_eq!(damage_camera_shake(0.1, 0.0), (0.0, 0.0, 0.0));
    }
}
