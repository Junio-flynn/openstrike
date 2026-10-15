//! The round simulation: player, bots, combat, phase gate, and the strike
//! surface's command/event/state types.

use alloc::vec::Vec;

use glam::{Mat4, Vec3};
use pocket3d_bsp::collide::{
    CharacterState, HullKind, MoveInput, MoveParams, TraceWorld, step_character,
};
use pocket3d_bsp::trace::{Hull, MapCollision};
use pocket3d_bsp::types::SpawnPoint;

use crate::bot::{Bot, BotConfig};
use crate::weapon::{EffectKind, Effects, MUZZLE_LOCAL, RANGE, Rng, Weapon, WeaponConfig};
use crate::{sin_cos, sinf, sqrtf};

pub const MOUSE_SENS: f32 = 0.002;
pub const WALK_SPEED_SCALE: f32 = 0.52;
pub const CROUCH_SPEED_SCALE: f32 = 0.48;
const AIR_ACCELERATE: f32 = 7.0;
const AIR_SPEED_CAP: f32 = 25.0;
const LANDING_SPEED_RETENTION: f32 = 0.98;
const GRAVITY_SCALE: f32 = 1.0;
const STAND_TO_CROUCH: f32 = 18.0;
const STAND_VIEW_HEIGHT: f32 = 64.0;
const CROUCH_VIEW_HEIGHT: f32 = 42.0;
const VIEW_TRANSITION_SPEED: f32 = 150.0;
const SLIDE_MIN_SPEED: f32 = 12.0;
const SLIDE_BOOST: f32 = 260.0;
const MAX_SLIDE_SPEED: f32 = 460.0;
const MAX_SLIDE_JUMP_SPEED: f32 = 400.0;
const SLIDE_LEDGE_SPEED_BOOST: f32 = 1.15;
const MAX_SLIDE_LEDGE_SPEED: f32 = 520.0;
const AIRBORNE_SLIDE_GRAVITY_SCALE: f32 = 1.35;
const WALL_RUN_LAUNCH_SPEED: f32 = 150.0;
const WALL_RUN_MIN_SPEED: f32 = 220.0;
const WALL_RUN_MAX_SPEED: f32 = 430.0;
const WALL_RUN_STICK_SPEED: f32 = 140.0;
const WALL_RUN_ENTRY_RETENTION: f32 = 0.10;
const WALL_RUN_DURATION_MIN: f32 = 1.75;
const WALL_RUN_DURATION_MAX: f32 = 2.65;
const WALL_JUMP_GRACE: f32 = 0.20;
const WALL_RELEASE_MIN_SPEED: f32 = 400.0;
const WALL_RELEASE_UP_SPEED: f32 = 130.0;
const WALL_RUN_GRAVITY_START: f32 = 0.18;
const WALL_RUN_GRAVITY_RAMP: f32 = 0.08;
const WALL_RUN_GRAVITY_MAX: f32 = 0.50;
const MAX_WALL_RUNS: u8 = 3;
const WALL_RUN_CAMERA_ROLL_MIN: f32 = 14.0 * core::f32::consts::PI / 180.0;
const WALL_RUN_CAMERA_ROLL_MAX: f32 = 20.0 * core::f32::consts::PI / 180.0;
const WALL_RUN_CAMERA_ROLL_IN_RESPONSE: f32 = 12.0;
const CAMERA_ROLL_OUT_RESPONSE: f32 = 7.0;
const WALL_RUN_CAMERA_FOV_MAX: f32 = 3.5 * core::f32::consts::PI / 180.0;
const SLIDE_CAMERA_ROLL: f32 = 7.0 * core::f32::consts::PI / 180.0;
const SLIDE_CAMERA_ROLL_RESPONSE: f32 = 6.0;
const SLIDE_CAMERA_FOV_BOOST: f32 = 4.0 * core::f32::consts::PI / 180.0;
const CAMERA_FOV_RESPONSE: f32 = 8.0;
const VIEWMODEL_LOOK_SWAY_MAX: f32 = 3.2 * core::f32::consts::PI / 180.0;
const VIEWMODEL_LOOK_SWAY_RESPONSE: f32 = 24.0;
const VIEWMODEL_LOOK_SWAY_RETURN: f32 = 8.0;
const AIM_IN_RESPONSE: f32 = 13.0;
const AIM_OUT_RESPONSE: f32 = 17.0;
const AIM_FOV_REDUCTION: f32 = 19.0 * core::f32::consts::PI / 180.0;
const SNIPER_AIM_FOV_REDUCTION: f32 = 46.0 * core::f32::consts::PI / 180.0;
const AIM_SPREAD_SCALE: f32 = 0.10;
const SNIPER_AIM_SPREAD_SCALE: f32 = 0.02;
const SNIPER_HIP_SPREAD: f32 = 0.12;
const AIM_RECOIL_SCALE: f32 = 0.40;
const AIM_MOVE_SPEED_SCALE: f32 = 0.68;
const SNIPER_AIM_MOVE_SPEED_SCALE: f32 = 0.40;
const BOT_HALF: Vec3 = Vec3::new(16.0, 36.0, 16.0);

pub struct Player {
    pub state: CharacterState,
    pub prev_pos: Vec3,
    prev_vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub params: MoveParams,
    pub health: i32,
    pub alive: bool,
    pub crouched: bool,
    pub sliding: bool,
    jumps_remaining: u8,
    jump_held: bool,
    slide_falling: bool,
    wall_running: bool,
    wall_run_armed: bool,
    wall_runs_remaining: u8,
    wall_run_time: f32,
    wall_run_duration: f32,
    wall_run_speed: f32,
    wall_contact_grace: f32,
    wall_normal: Vec3,
    has_last_wall: bool,
    last_wall_normal: Vec3,
    last_wall_plane: f32,
    slide_ready: bool,
    view_height: f32,
    prev_view_height: f32,
    prev_crouched: bool,
    camera_roll: f32,
    prev_camera_roll: f32,
    camera_fov_boost: f32,
    prev_camera_fov_boost: f32,
    look_sway: f32,
    prev_look_sway: f32,
    look_sway_target: f32,
    pending_look_sway: f32,
}

impl Player {
    pub fn spawn(pos: Vec3, yaw: f32) -> Self {
        let mut params = MoveParams::default();
        params.gravity *= GRAVITY_SCALE;
        params.air_accelerate = AIR_ACCELERATE;
        params.air_speed_cap = AIR_SPEED_CAP;
        Self {
            state: CharacterState::new(pos),
            prev_pos: pos,
            prev_vel: Vec3::ZERO,
            yaw,
            pitch: 0.0,
            params,
            health: 100,
            alive: true,
            crouched: false,
            sliding: false,
            jumps_remaining: 2,
            jump_held: false,
            slide_falling: false,
            wall_running: false,
            wall_run_armed: true,
            wall_runs_remaining: MAX_WALL_RUNS,
            wall_run_time: 0.0,
            wall_run_duration: WALL_RUN_DURATION_MIN,
            wall_run_speed: 0.0,
            wall_contact_grace: 0.0,
            wall_normal: Vec3::ZERO,
            has_last_wall: false,
            last_wall_normal: Vec3::ZERO,
            last_wall_plane: 0.0,
            slide_ready: true,
            view_height: STAND_VIEW_HEIGHT,
            prev_view_height: STAND_VIEW_HEIGHT,
            prev_crouched: false,
            camera_roll: 0.0,
            prev_camera_roll: 0.0,
            camera_fov_boost: 0.0,
            prev_camera_fov_boost: 0.0,
            look_sway: 0.0,
            prev_look_sway: 0.0,
            look_sway_target: 0.0,
            pending_look_sway: 0.0,
        }
    }

    pub fn eye_interpolated(&self, alpha: f32) -> Vec3 {
        let previous_half = if self.prev_crouched { 18.0 } else { 36.0 };
        let current_half = if self.crouched { 18.0 } else { 36.0 };
        let previous_eye = self.prev_pos + Vec3::Y * (self.prev_view_height - previous_half);
        let current_eye = self.state.pos + Vec3::Y * (self.view_height - current_half);
        previous_eye.lerp(current_eye, alpha)
    }

    pub fn eye(&self) -> Vec3 {
        let half = if self.crouched { 18.0 } else { 36.0 };
        self.state.pos + Vec3::Y * (self.view_height - half)
    }

    pub fn forward_flat(&self) -> Vec3 {
        let (sy, cy) = sin_cos(self.yaw);
        Vec3::new(-sy, 0.0, -cy)
    }

    pub fn right(&self) -> Vec3 {
        let (sy, cy) = sin_cos(self.yaw);
        Vec3::new(cy, 0.0, -sy)
    }

    pub fn view_dir(&self) -> Vec3 {
        let (sy, cy) = sin_cos(self.yaw);
        let (sp, cp) = sin_cos(self.pitch);
        Vec3::new(-sy * cp, sp, -cy * cp)
    }

    /// Whether the airborne follow-up jump is still available. The first
    /// (ground) jump leaves one charge; spending that charge clears this
    /// until landing or entering a wall run refreshes it.
    pub fn double_jump_available(&self) -> bool {
        self.jumps_remaining > 0
    }

    /// Smoothed wall-run camera roll at the current simulation tick.
    pub fn wall_run_camera_roll(&self) -> f32 {
        self.camera_roll
    }

    /// Interpolated camera roll for render frames between simulation ticks.
    pub fn camera_roll_interpolated(&self, alpha: f32) -> f32 {
        self.prev_camera_roll + (self.camera_roll - self.prev_camera_roll) * alpha
    }

    /// FOV expansion coupled to the smoothed wall-run lean. This makes fast
    /// entries feel quicker without introducing a separate camera snap.
    pub fn wall_run_camera_fov_boost(&self) -> f32 {
        self.camera_fov_boost
    }

    pub fn camera_fov_boost_interpolated(&self, alpha: f32) -> f32 {
        self.prev_camera_fov_boost + (self.camera_fov_boost - self.prev_camera_fov_boost) * alpha
    }

    pub fn is_wall_running(&self) -> bool {
        self.wall_running
    }

    pub fn viewmodel_look_sway(&self) -> f32 {
        self.look_sway
    }

    fn update_look_sway(&mut self, dt: f32) {
        self.prev_look_sway = self.look_sway;
        self.look_sway_target = (self.look_sway_target + self.pending_look_sway)
            .clamp(-VIEWMODEL_LOOK_SWAY_MAX, VIEWMODEL_LOOK_SWAY_MAX);
        self.pending_look_sway = 0.0;
        self.look_sway_target /= 1.0 + VIEWMODEL_LOOK_SWAY_RETURN * dt;
        let response = VIEWMODEL_LOOK_SWAY_RESPONSE * dt;
        self.look_sway += (self.look_sway_target - self.look_sway) * (response / (1.0 + response));
    }

    fn update_camera(&mut self, dt: f32, slide_steer: f32) {
        self.prev_camera_roll = self.camera_roll;
        self.prev_camera_fov_boost = self.camera_fov_boost;
        let wall_roll = if self.wall_running {
            let momentum = ((self.wall_run_speed - WALL_RUN_MIN_SPEED)
                / (WALL_RUN_MAX_SPEED - WALL_RUN_MIN_SPEED))
                .clamp(0.0, 1.0);
            let magnitude = WALL_RUN_CAMERA_ROLL_MIN
                + (WALL_RUN_CAMERA_ROLL_MAX - WALL_RUN_CAMERA_ROLL_MIN) * momentum;
            self.wall_normal.dot(self.right()) * magnitude
        } else {
            0.0
        };
        let target = if self.wall_running {
            wall_roll
        } else if self.sliding {
            // A slide has a visible lean even without sideways input. Mirror
            // it when momentum is backwards so both slide directions feel
            // intentional, then allow steering to bias the angle slightly.
            let along_view = self.state.vel.dot(self.forward_flat());
            let direction = if along_view < 0.0 { -1.0 } else { 1.0 };
            direction * SLIDE_CAMERA_ROLL * (0.72 + slide_steer.abs().min(1.0) * 0.28)
        } else {
            0.0
        };
        let response = if self.wall_running {
            WALL_RUN_CAMERA_ROLL_IN_RESPONSE
        } else if self.sliding {
            SLIDE_CAMERA_ROLL_RESPONSE
        } else {
            CAMERA_ROLL_OUT_RESPONSE
        };
        let roll_response = response * dt;
        self.camera_roll += (target - self.camera_roll) * (roll_response / (1.0 + roll_response));

        let fov_target = if self.wall_running {
            (wall_roll.abs() / WALL_RUN_CAMERA_ROLL_MAX).clamp(0.0, 1.0) * WALL_RUN_CAMERA_FOV_MAX
        } else if self.sliding {
            SLIDE_CAMERA_FOV_BOOST
        } else {
            0.0
        };
        let fov_response = CAMERA_FOV_RESPONSE * dt;
        self.camera_fov_boost +=
            (fov_target - self.camera_fov_boost) * (fov_response / (1.0 + fov_response));
    }
}

#[derive(Clone, Copy)]
struct WallContact {
    normal: Vec3,
    plane: f32,
}

fn side_wall_contact(
    col: &MapCollision,
    hull: HullKind,
    pos: Vec3,
    right: Vec3,
) -> Option<WallContact> {
    for direction in [right, -right] {
        let trace = TraceWorld::trace(col, hull, pos, pos + direction * 12.0);
        if trace.fraction < 1.0 && !trace.start_solid && trace.normal.y.abs() < 0.2 {
            return Some(WallContact {
                normal: trace.normal,
                plane: trace.normal.dot(trace.end),
            });
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// Round countdown; movement frozen. The countdown itself lives in the
    /// gameplay mod (JS) — Rust only knows the gate is closed.
    Starting,
    Live,
    Ended {
        won: bool,
    },
}

/// Facts the core reports to the guest, batched per tick (RUNTIMES.md Law 2:
/// facts cross as events).
#[derive(Clone, Debug)]
pub enum GameEvent {
    /// The player's shot connected.
    Hit {
        bot: usize,
        headshot: bool,
        damage: i32,
        fatal: bool,
    },
    PlayerDamaged {
        amount: i32,
        hp: i32,
    },
    PlayerDied,
    RoundReset,
}

/// Intent the guest sends back through the `strike` surface ops. Queued
/// during the guest turn, applied by the host afterwards.
#[derive(Clone, Debug)]
pub enum Command {
    SetPhase(Phase),
    ResetRound,
    AddWin,
    AddLoss,
    SetBotCount(usize),
    SelectWeapon(usize),
    ConfigureWeapon(WeaponConfig),
    ConfigureBots(BotConfig),
}

#[derive(Default, Clone, Copy)]
pub struct Score {
    pub wins: u32,
    pub losses: u32,
}

/// One tick of player intent, already mapped from whatever the platform's
/// input device is (keyboard+mouse or PSP pad).
#[derive(Clone, Copy, Debug, Default)]
pub struct SimInput {
    /// Strafe axis, -1..1 (right positive).
    pub move_x: f32,
    /// Forward axis, -1..1 (forward positive).
    pub move_y: f32,
    pub walk: bool,
    pub jump: bool,
    /// Trigger held.
    pub fire: bool,
    /// Secondary trigger held to aim down sights.
    pub aim: bool,
    /// Reload pressed this tick.
    pub reload: bool,
    /// Hold to use the crouch hull; crouching at running speed starts a slide.
    pub crouch: bool,
}

/// The platform-free simulation. Platforms own presentation and input; this
/// owns state and time.
pub struct StrikeSim {
    pub player: Player,
    pub bots: Vec<Bot>,
    pub bot_count: usize,
    pub weapon: Weapon,
    /// Weapon currently selected from the shared primary/secondary catalog.
    pub active_weapon: usize,
    /// Starts false; the player chooses and equips one primary at spawn.
    pub weapon_equipped: bool,
    pub primary_weapon_slot: Option<usize>,
    pub secondary_weapon_slot: Option<usize>,
    /// Inactive weapons retain their own magazine and reserve ammunition.
    weapon_stash: [Option<Weapon>; 16],
    pub effects: Effects,
    pub rng: Rng,
    pub phase: Phase,
    pub score: Score,
    pub bot_cfg: BotConfig,
    /// Per-tick event batch drained by the guest turn.
    pub events: Vec<GameEvent>,
    pub time: f32,
    pub fly_mode: bool,
    pub fired_this_tick: bool,
    /// Exact world-space direction of the most recently fired hitscan ray.
    /// Desktop multiplayer sends this instead of reconstructing a centered
    /// ray from the camera, so weapon spread and damage always agree.
    pub last_shot_direction: Vec3,
    fire_held: bool,

    aim_fraction: f32,
    prev_aim_fraction: f32,

    spawn_point: (Vec3, f32),
    bot_spawns: Vec<SpawnPoint>,
    bob_time: f32,
    prev_bob_time: f32,
    viewmodel_ground_weight: f32,
    prev_viewmodel_ground_weight: f32,
}

impl StrikeSim {
    /// `bot_spawns` is the enemy spawn list (T side, falling back to CT on
    /// maps without one — the caller picks).
    pub fn new(
        spawn_pos: Vec3,
        spawn_yaw: f32,
        bot_spawns: Vec<SpawnPoint>,
        bot_count: usize,
    ) -> Self {
        let mut sim = Self {
            player: Player::spawn(spawn_pos, spawn_yaw),
            bots: Vec::new(),
            bot_count,
            weapon: Weapon::default(),
            active_weapon: 0,
            weapon_equipped: false,
            primary_weapon_slot: None,
            secondary_weapon_slot: None,
            weapon_stash: [
                Some(Weapon::default()),
                Some(Weapon::with_config(WeaponConfig {
                    mag_size: 30,
                    reserve: 90,
                    fire_interval: 0.140,
                    reload_time: 2.65,
                    damage_body: 15,
                    damage_head: 20,
                })),
                Some(Weapon::with_config(WeaponConfig {
                    mag_size: 32,
                    reserve: 96,
                    fire_interval: 0.075,
                    reload_time: 2.30,
                    damage_body: 10,
                    damage_head: 15,
                })),
                Some(Weapon::with_config(WeaponConfig {
                    mag_size: 5,
                    reserve: 25,
                    fire_interval: 1.70,
                    reload_time: 3.6,
                    damage_body: 50,
                    damage_head: 100,
                })),
                Some(Weapon::with_config(WeaponConfig {
                    mag_size: 30,
                    reserve: 90,
                    fire_interval: 0.105,
                    reload_time: 2.45,
                    damage_body: 17,
                    damage_head: 30,
                })),
                Some(Weapon::with_config(WeaponConfig {
                    mag_size: 30,
                    reserve: 90,
                    fire_interval: 0.115,
                    reload_time: 2.35,
                    damage_body: 16,
                    damage_head: 28,
                })),
                Some(Weapon::with_config(WeaponConfig {
                    mag_size: 30,
                    reserve: 90,
                    fire_interval: 0.125,
                    reload_time: 2.55,
                    damage_body: 18,
                    damage_head: 32,
                })),
                Some(Weapon::with_config(WeaponConfig {
                    mag_size: 8,
                    reserve: 32,
                    fire_interval: 0.80,
                    reload_time: 2.8,
                    damage_body: 35,
                    damage_head: 80,
                })),
                Some(Weapon::with_config(WeaponConfig {
                    mag_size: 5,
                    reserve: 25,
                    fire_interval: 1.60,
                    reload_time: 3.4,
                    damage_body: 52,
                    damage_head: 100,
                })),
                Some(Weapon::with_config(WeaponConfig {
                    mag_size: 17,
                    reserve: 68,
                    fire_interval: 0.17,
                    reload_time: 1.8,
                    damage_body: 12,
                    damage_head: 24,
                })),
                Some(Weapon::with_config(WeaponConfig {
                    mag_size: 12,
                    reserve: 48,
                    fire_interval: 0.19,
                    reload_time: 1.9,
                    damage_body: 14,
                    damage_head: 27,
                })),
                Some(Weapon::with_config(WeaponConfig {
                    mag_size: 15,
                    reserve: 60,
                    fire_interval: 0.17,
                    reload_time: 1.8,
                    damage_body: 13,
                    damage_head: 26,
                })),
                Some(Weapon::with_config(WeaponConfig {
                    mag_size: 15,
                    reserve: 60,
                    fire_interval: 0.18,
                    reload_time: 1.8,
                    damage_body: 13,
                    damage_head: 26,
                })),
                Some(Weapon::with_config(WeaponConfig {
                    mag_size: 8,
                    reserve: 32,
                    fire_interval: 0.25,
                    reload_time: 1.9,
                    damage_body: 16,
                    damage_head: 30,
                })),
                Some(Weapon::with_config(WeaponConfig {
                    mag_size: 12,
                    reserve: 48,
                    fire_interval: 0.18,
                    reload_time: 2.0,
                    damage_body: 18,
                    damage_head: 34,
                })),
                Some(Weapon::with_config(WeaponConfig {
                    mag_size: 12,
                    reserve: 48,
                    fire_interval: 0.20,
                    reload_time: 1.9,
                    damage_body: 14,
                    damage_head: 28,
                })),
            ],
            effects: Effects::default(),
            rng: Rng(0x0DDB1A5E5BAD5EED),
            phase: Phase::Starting,
            score: Score::default(),
            bot_cfg: BotConfig::default(),
            events: Vec::new(),
            time: 0.0,
            fly_mode: false,
            fired_this_tick: false,
            last_shot_direction: Vec3::ZERO,
            fire_held: false,
            aim_fraction: 0.0,
            prev_aim_fraction: 0.0,
            spawn_point: (spawn_pos, spawn_yaw),
            bot_spawns,
            bob_time: 0.0,
            prev_bob_time: 0.0,
            viewmodel_ground_weight: 1.0,
            prev_viewmodel_ground_weight: 1.0,
        };
        sim.spawn_bots(0);
        sim
    }

    /// (Re)spawn bots; `walk_clip` is the platform's walk-cycle clip index.
    pub fn spawn_bots(&mut self, walk_clip: usize) {
        self.bots.clear();
        if self.bot_spawns.is_empty() {
            return;
        }
        for i in 0..self.bot_count {
            // Spread bots over the spawn list.
            let sp = self.bot_spawns[(i * 3 + 1) % self.bot_spawns.len()];
            let mut bot = Bot::spawn(sp.pos, sp.yaw);
            bot.anim.clip = walk_clip;
            self.bots.push(bot);
        }
    }

    pub fn reset_round(&mut self, walk_clip: usize) {
        let (pos, yaw) = self.spawn_point;
        let pitch = self.player.pitch;
        self.player = Player::spawn(pos, yaw);
        self.player.pitch = pitch * 0.25;
        if self.weapon_equipped {
            let active = self.active_weapon;
            self.weapon_stash[active] =
                Some(core::mem::replace(&mut self.weapon, Weapon::default()));
            self.weapon_equipped = false;
        }
        self.primary_weapon_slot = None;
        self.secondary_weapon_slot = None;
        self.weapon.reset();
        for weapon in self.weapon_stash.iter_mut().flatten() {
            weapon.reset();
        }
        self.effects.clear();
        self.spawn_bots(walk_clip);
        self.phase = Phase::Starting;
        self.events.push(GameEvent::RoundReset);
    }

    /// Switch weapons while preserving each slot's ammunition. Reloads and
    /// recoil presentation are cancelled, matching an immediate weapon swap.
    pub fn switch_weapon(&mut self, slot: usize) -> bool {
        if slot >= self.weapon_stash.len() || (self.weapon_equipped && slot == self.active_weapon) {
            return false;
        }
        self.activate_weapon(slot)
    }

    fn activate_weapon(&mut self, slot: usize) -> bool {
        let Some(mut incoming) = self.weapon_stash[slot].take() else {
            return false;
        };
        self.weapon.reload_left = 0.0;
        self.weapon.prev_reload_left = 0.0;
        incoming.reload_left = 0.0;
        incoming.prev_reload_left = 0.0;
        // Pulling a weapon out takes half a second before its trigger is
        // ready. Reuse the normal fire gate so every platform observes the
        // same delay without introducing presentation-only timing.
        incoming.cooldown = 0.5;
        incoming.prev_cooldown = 0.5;
        incoming.shot_age = 999.0;
        incoming.prev_shot_age = 999.0;
        incoming.spray_index = 0;
        incoming.aim_punch_pitch = 0.0;
        incoming.prev_aim_punch_pitch = 0.0;
        incoming.aim_punch_yaw = 0.0;
        incoming.prev_aim_punch_yaw = 0.0;
        incoming.camera_kick_roll = 0.0;
        incoming.prev_camera_kick_roll = 0.0;
        incoming.recoil = 0.0;
        incoming.prev_recoil = 0.0;
        if self.weapon_equipped {
            let outgoing = core::mem::replace(&mut self.weapon, incoming);
            self.weapon_stash[self.active_weapon] = Some(outgoing);
        } else {
            self.weapon = incoming;
        }
        self.active_weapon = slot;
        self.weapon_equipped = true;
        true
    }

    /// Assign one weapon to the primary (0..9) or secondary (9..16) loadout
    /// slot. The first chosen weapon becomes active; choosing the other class
    /// leaves the current weapon active until the player presses 1 or 2.
    pub fn choose_weapon(&mut self, slot: usize) -> bool {
        if slot >= self.weapon_stash.len() {
            return false;
        }
        let primary = slot < 9;
        let current_primary = self.active_weapon < 9;
        if primary {
            self.primary_weapon_slot = Some(slot);
        } else {
            self.secondary_weapon_slot = Some(slot);
        }
        if !self.weapon_equipped
            || (primary && !current_primary)
            || (primary && current_primary)
            || (!primary && !current_primary)
        {
            if slot != self.active_weapon || !self.weapon_equipped {
                return self.activate_weapon(slot);
            }
        }
        true
    }

    /// Switch to the selected primary (0) or secondary (1) loadout slot.
    pub fn switch_loadout_slot(&mut self, class: usize) -> bool {
        let slot = match class {
            0 => self.primary_weapon_slot,
            1 => self.secondary_weapon_slot,
            _ => None,
        };
        slot.is_some_and(|slot| slot != self.active_weapon && self.activate_weapon(slot))
    }

    pub fn weapon_name(&self) -> &'static str {
        if !self.weapon_equipped {
            return "NO WEAPON";
        }
        const NAMES: [&str; 16] = [
            "SCORPION EVO 3",
            "AK-74M",
            "UZI",
            "BOLT-ACTION SNIPER",
            "AK-12",
            "AKS-74U",
            "AKM",
            "REMINGTON 870",
            "SNIPER",
            "PISTOL 43",
            "PL2000",
            "BERETTA",
            "FPS PISTOL",
            "MAKAROV PM",
            "MARK 23",
            "USP-S",
        ];
        NAMES
            .get(self.active_weapon)
            .copied()
            .unwrap_or("UNKNOWN WEAPON")
    }

    /// Apply one guest command (drained after each guest turn).
    pub fn apply(&mut self, cmd: Command, walk_clip: usize) {
        match cmd {
            Command::SetPhase(p) => self.phase = p,
            Command::ResetRound => self.reset_round(walk_clip),
            Command::AddWin => self.score.wins += 1,
            Command::AddLoss => self.score.losses += 1,
            Command::SetBotCount(n) => self.bot_count = n.min(16),
            Command::SelectWeapon(slot) => {
                self.choose_weapon(slot);
            }
            Command::ConfigureWeapon(cfg) => {
                self.weapon.cfg = cfg;
                let mag = self.weapon.cfg.mag_size;
                self.weapon.ammo = self.weapon.ammo.min(mag);
            }
            Command::ConfigureBots(cfg) => {
                self.bot_count = cfg.count.min(16);
                self.bot_cfg = cfg;
            }
        }
    }

    pub fn apply_look(&mut self, dx: f32, dy: f32) {
        let yaw_delta = -dx * MOUSE_SENS;
        self.player.yaw += yaw_delta;
        // The weapon lags behind horizontal camera motion, then recenters.
        self.player.pending_look_sway -= yaw_delta * 1.4;
        self.player.pitch =
            (self.player.pitch - dy * MOUSE_SENS).clamp(-89f32.to_radians(), 89f32.to_radians());
    }

    pub fn toggle_fly(&mut self) {
        self.fly_mode = !self.fly_mode;
    }

    pub fn alive_bots(&self) -> usize {
        self.bots.iter().filter(|b| b.alive()).count()
    }

    pub fn reload_frac(&self) -> f32 {
        if self.weapon.reloading() {
            1.0 - (self.weapon.reload_left / self.weapon.cfg.reload_time).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    pub fn ground_speed(&self) -> f32 {
        let v = self.player.state.vel;
        sqrtf(v.x * v.x + v.z * v.z)
    }

    /// Smoothed 0..1 aim-down-sights amount for platform presentation.
    pub fn aim_fraction_interpolated(&self, alpha: f32) -> f32 {
        self.prev_aim_fraction
            + (self.aim_fraction - self.prev_aim_fraction) * alpha.clamp(0.0, 1.0)
    }

    /// Negative camera-FOV adjustment while aiming.
    pub fn aim_fov_offset_interpolated(&self, alpha: f32) -> f32 {
        let reduction = if matches!(self.active_weapon, 3 | 8) {
            SNIPER_AIM_FOV_REDUCTION
        } else {
            AIM_FOV_REDUCTION
        };
        -reduction * self.aim_fraction_interpolated(alpha)
    }

    fn aim_move_speed_scale(&self) -> f32 {
        let aimed_scale = if matches!(self.active_weapon, 3 | 8) {
            SNIPER_AIM_MOVE_SPEED_SCALE
        } else {
            AIM_MOVE_SPEED_SCALE
        };
        1.0 - (1.0 - aimed_scale) * self.aim_fraction
    }

    /// Full fixed-step game tick.
    pub fn tick(&mut self, col: &MapCollision, dt: f32, input: &SimInput) {
        self.time += dt;
        self.effects.tick(dt);
        self.weapon.tick(dt);

        self.prev_aim_fraction = self.aim_fraction;
        let aim_target = if input.aim && self.player.alive {
            1.0
        } else {
            0.0
        };
        let aim_response = if aim_target > self.aim_fraction {
            AIM_IN_RESPONSE
        } else {
            AIM_OUT_RESPONSE
        };
        let aim_step = aim_response * dt;
        self.aim_fraction += (aim_target - self.aim_fraction) * (aim_step / (1.0 + aim_step));

        // Round phase is a gate here; countdowns and transitions live in the
        // gameplay mod (JS), which drives them through `strike` commands.
        let movement_frozen = self.phase == Phase::Starting;

        self.player.update_look_sway(dt);
        self.tick_player_movement(col, dt, input, movement_frozen);
        self.player.update_camera(dt, input.move_x);

        let fire_pressed = input.fire && !self.fire_held;
        self.fire_held = input.fire;

        // Combat.
        self.fired_this_tick = false;
        let live = self.phase == Phase::Live;
        if live && self.player.alive && !movement_frozen {
            if self.weapon_equipped && input.reload {
                self.weapon.trigger_reload();
            }
            let recoil_scale = 1.0 - (1.0 - AIM_RECOIL_SCALE) * self.aim_fraction;
            let trigger = self.weapon_equipped
                && input.fire
                && (!matches!(self.active_weapon, 3 | 8) || fire_pressed);
            if trigger && self.weapon.fire_with_kick(recoil_scale) {
                self.fire_shot(col);
            }
        }

        // Bots.
        let player_eye = self.player.eye();
        let player_alive = self.player.alive;
        let mut incoming = 0i32;
        let bot_cfg = self.bot_cfg.clone();
        for bot in &mut self.bots {
            let shot = bot.tick(
                col,
                player_eye,
                player_alive && live,
                dt,
                &bot_cfg,
                &mut self.rng,
                &mut self.effects,
            );
            if live {
                if let Some(s) = shot {
                    incoming += s.damage;
                }
            }
        }
        if incoming > 0 && self.player.alive {
            self.player.health -= incoming;
            if self.player.health <= 0 {
                self.player.health = 0;
                self.player.alive = false;
            }
            self.events.push(GameEvent::PlayerDamaged {
                amount: incoming,
                hp: self.player.health,
            });
            if !self.player.alive {
                self.events.push(GameEvent::PlayerDied);
            }
        }

        // Soft push-out so bots don't share space with the player.
        if self.player.alive {
            for bot in self.bots.iter().filter(|b| b.alive()) {
                let d = self.player.state.pos - bot.state.pos;
                let horiz = Vec3::new(d.x, 0.0, d.z);
                let dist = horiz.length();
                if dist < 34.0 && d.y.abs() < 72.0 && dist > 0.001 {
                    self.player.state.pos += horiz / dist * (34.0 - dist) * 0.35;
                }
            }
        }
    }

    fn tick_player_movement(
        &mut self,
        col: &MapCollision,
        dt: f32,
        input: &SimInput,
        frozen: bool,
    ) {
        let aim_move_speed_scale = self.aim_move_speed_scale();
        let p = &mut self.player;
        p.prev_pos = p.state.pos;
        p.prev_vel = p.state.vel;
        p.prev_view_height = p.view_height;
        p.prev_crouched = p.crouched;
        if !p.alive {
            return;
        }

        // Platform inputs report jump as a held button. Turn that into a
        // press so one hold cannot consume both jumps, and refill the two
        // available jumps whenever collision says the player is grounded.
        let jump_pressed = input.jump && !p.jump_held;
        let jump_released = !input.jump && p.jump_held;
        p.jump_held = input.jump;
        if p.state.on_ground {
            p.jumps_remaining = 2;
            p.wall_running = false;
            p.wall_run_armed = true;
            p.wall_runs_remaining = MAX_WALL_RUNS;
            p.wall_run_time = 0.0;
            p.wall_run_duration = WALL_RUN_DURATION_MIN;
            p.wall_run_speed = 0.0;
            p.wall_contact_grace = 0.0;
            p.wall_normal = Vec3::ZERO;
            p.has_last_wall = false;
        }

        // A ground jump beside a wall must not turn into a wall run merely
        // because the same button is still held on the following tick. The
        // player has to release jump once airborne before wall running is
        // armed again.
        if !frozen && input.jump && p.state.on_ground {
            p.wall_run_armed = false;
        } else if !p.state.on_ground && !input.jump {
            p.wall_run_armed = true;
        }

        let mut wish = Vec3::ZERO;
        if !frozen {
            wish += p.forward_flat() * input.move_y;
            wish += p.right() * input.move_x;
        }

        if self.fly_mode {
            let mut v = Vec3::ZERO;
            v += p.view_dir() * 600.0 * input.move_y;
            v += p.right() * 600.0 * input.move_x;
            if input.jump {
                v += Vec3::Y * 400.0;
            }
            p.state.pos += v * dt;
            p.state.vel = Vec3::ZERO;
            return;
        }

        // Up to three separate wall runs may be entered per airtime. Contact
        // itself grants no jump: kicking away grants exactly one follow-up.
        let wall_hull = if p.crouched {
            HullKind::Crouch
        } else {
            HullKind::Stand
        };
        let wall_contact = if !frozen
            && input.jump
            && p.wall_run_armed
            && !p.state.on_ground
            && (p.wall_runs_remaining > 0 || p.wall_running)
            && p.wall_run_time < p.wall_run_duration
        {
            side_wall_contact(col, wall_hull, p.state.pos, p.right())
        } else {
            None
        };
        let wall_contact = wall_contact.filter(|contact| {
            p.wall_running
                || !p.has_last_wall
                || contact.normal.dot(p.last_wall_normal) < 0.98
                || (contact.plane - p.last_wall_plane).abs() > 4.0
        });
        if let Some(contact) = wall_contact {
            let normal = contact.normal;
            let entering = !p.wall_running;
            p.wall_running = true;
            p.wall_normal = normal;
            p.wall_contact_grace = WALL_JUMP_GRACE;
            if entering {
                p.has_last_wall = true;
                p.last_wall_normal = contact.normal;
                p.last_wall_plane = contact.plane;
                p.wall_runs_remaining -= 1;
                p.wall_run_time = 0.0;
                p.jumps_remaining = 0;
                p.state.vel.y = p.state.vel.y.max(WALL_RUN_LAUNCH_SPEED);
                let incoming_speed = Vec3::new(p.state.vel.x, 0.0, p.state.vel.z).length();
                p.wall_run_speed =
                    (incoming_speed * 1.04).clamp(WALL_RUN_MIN_SPEED, WALL_RUN_MAX_SPEED);
                let momentum = ((p.wall_run_speed - WALL_RUN_MIN_SPEED)
                    / (WALL_RUN_MAX_SPEED - WALL_RUN_MIN_SPEED))
                    .clamp(0.0, 1.0);
                p.wall_run_duration = WALL_RUN_DURATION_MIN
                    + (WALL_RUN_DURATION_MAX - WALL_RUN_DURATION_MIN) * momentum;
            }

            let horizontal = Vec3::new(p.state.vel.x, 0.0, p.state.vel.z);
            let mut along = horizontal - normal * horizontal.dot(normal);
            if along.length_squared() < 0.001 {
                along = wish - normal * wish.dot(normal);
            }
            if along.length_squared() < 0.001 {
                let forward = p.forward_flat();
                along = forward - normal * forward.dot(normal);
            }
            let direction = along.normalize_or_zero();
            // Preserve entry speed briefly, then settle onto the
            // momentum-derived wallrun speed instead of forcing one speed.
            let speed = if p.wall_run_time < WALL_RUN_ENTRY_RETENTION {
                horizontal.length().min(WALL_RUN_MAX_SPEED)
            } else {
                p.wall_run_speed
            };
            p.state.vel.x = direction.x * speed - normal.x * WALL_RUN_STICK_SPEED;
            p.state.vel.z = direction.z * speed - normal.z * WALL_RUN_STICK_SPEED;
            p.wall_run_time += dt;
        } else {
            p.wall_running = false;
            p.wall_contact_grace = (p.wall_contact_grace - dt).max(0.0);
        }

        // Releasing jump during a wallrun immediately launches toward the
        // crosshair. A fresh press during the short grace window is retained
        // as a fallback. Releasing grants one airborne follow-up jump.
        let wall_jump_requested = !frozen
            && (jump_released || jump_pressed)
            && !p.state.on_ground
            && !p.wall_running
            && p.wall_contact_grace > 0.0;
        if wall_jump_requested {
            let horizontal = Vec3::new(p.state.vel.x, 0.0, p.state.vel.z);
            // The release has no wall-normal kick. Its main impulse follows
            // the horizontal crosshair heading, with a modest upward lift.
            let view_direction = p.forward_flat();
            let launch_speed =
                (horizontal.length().max(p.wall_run_speed) * 1.08).max(WALL_RELEASE_MIN_SPEED);
            p.state.vel = view_direction * launch_speed;
            p.state.vel.y = WALL_RELEASE_UP_SPEED;
            p.jumps_remaining = 1;
            p.wall_contact_grace = 0.0;
        }

        // A held jump automatically fires again on each landing. The
        // airborne extra jumps still need fresh presses so holding the
        // button does not spend them immediately after takeoff.
        let ground_jump_requested =
            !frozen && input.jump && p.state.on_ground && p.jumps_remaining > 0;
        let air_jump_requested = !frozen
            && jump_pressed
            && !p.state.on_ground
            && !p.wall_running
            && !wall_jump_requested
            && p.jumps_remaining > 0;
        let jump_requested = ground_jump_requested || air_jump_requested;

        if air_jump_requested {
            let half_height = if p.crouched { 18.0 } else { 36.0 };
            let feet = p.state.pos - Vec3::Y * (half_height + 2.0);
            self.effects
                .spawn(EffectKind::DoubleJumpBurst { pos: feet }, 0.30);
        }

        let horizontal_speed = sqrtf(p.state.vel.x * p.state.vel.x + p.state.vel.z * p.state.vel.z);
        // Jumping is the one intentional way to break out of an active
        // slide. Repeated crouch presses are ignored until the slide ends.
        if jump_requested && p.sliding {
            p.sliding = false;
            p.slide_ready = true;
            if horizontal_speed > MAX_SLIDE_JUMP_SPEED {
                let scale = MAX_SLIDE_JUMP_SPEED / horizontal_speed;
                p.state.vel.x *= scale;
                p.state.vel.z *= scale;
            }
        }

        if input.crouch && !p.crouched {
            p.state.pos.y -= STAND_TO_CROUCH;
            p.crouched = true;
            if p.slide_ready
                && p.state.on_ground
                && wish.length_squared() > 0.001
                && horizontal_speed >= SLIDE_MIN_SPEED
            {
                p.sliding = true;
                p.slide_ready = false;
                let velocity_direction =
                    Vec3::new(p.state.vel.x, 0.0, p.state.vel.z).normalize_or_zero();
                let direction = wish.normalize_or_zero();
                let direction = if direction == Vec3::ZERO {
                    velocity_direction
                } else {
                    direction
                };
                let launch_speed = (horizontal_speed + SLIDE_BOOST).min(MAX_SLIDE_SPEED);
                p.state.vel.x = direction.x * launch_speed;
                p.state.vel.z = direction.z * launch_speed;
            }
        } else if !input.crouch && p.crouched && !p.sliding {
            let stand_pos = p.state.pos + Vec3::Y * STAND_TO_CROUCH;
            let clearance = TraceWorld::trace(col, HullKind::Stand, stand_pos, stand_pos);
            if !clearance.start_solid {
                p.state.pos = stand_pos;
                p.crouched = false;
                p.sliding = false;
            }
        }

        if p.sliding {
            let slide_speed = sqrtf(p.state.vel.x * p.state.vel.x + p.state.vel.z * p.state.vel.z);
            if slide_speed <= p.params.max_speed * CROUCH_SPEED_SCALE {
                p.sliding = false;
                p.slide_ready = true;
            }
        }
        if !p.sliding && horizontal_speed <= p.params.max_speed * CROUCH_SPEED_SCALE {
            p.slide_ready = true;
        }

        let target_view = if p.crouched {
            CROUCH_VIEW_HEIGHT
        } else {
            STAND_VIEW_HEIGHT
        };
        let view_step = VIEW_TRANSITION_SPEED * dt;
        if p.view_height < target_view {
            p.view_height = (p.view_height + view_step).min(target_view);
        } else {
            p.view_height = (p.view_height - view_step).max(target_view);
        }
        let hull = if p.crouched {
            HullKind::Crouch
        } else {
            HullKind::Stand
        };
        let mut move_params = p.params;
        if p.sliding {
            move_params.friction = 0.65;
            move_params.accelerate = 1.2;
            move_params.step_height = 8.0;
        }
        if p.wall_running {
            let gravity_scale = (WALL_RUN_GRAVITY_START + p.wall_run_time * WALL_RUN_GRAVITY_RAMP)
                .min(WALL_RUN_GRAVITY_MAX);
            move_params.gravity *= gravity_scale;
        } else if p.slide_falling {
            move_params.gravity *= AIRBORNE_SLIDE_GRAVITY_SCALE;
        }

        let ground_jump = ground_jump_requested;
        if jump_requested {
            p.jumps_remaining -= 1;
            if !p.state.on_ground {
                // step_character handles grounded jumps itself. An airborne
                // press spends another jump and starts a fresh ascent.
                p.state.vel.y = move_params.jump_speed;
            }
        }

        let base_speed = if p.sliding {
            0.15
        } else if p.crouched {
            CROUCH_SPEED_SCALE
        } else if input.walk {
            WALK_SPEED_SCALE
        } else {
            1.0
        };
        let minput = MoveInput {
            wish_dir: wish,
            speed: base_speed * aim_move_speed_scale,
            jump: ground_jump,
        };
        let was_on_ground = p.state.on_ground;
        step_character(col, hull, &mut p.state, &move_params, &minput, dt);
        if was_on_ground && !p.state.on_ground && p.sliding {
            // Carrying a slide over a ledge gives it a small forward burst,
            // then stronger gravity pulls the player down until landing.
            let speed = sqrtf(p.state.vel.x * p.state.vel.x + p.state.vel.z * p.state.vel.z);
            if speed > 0.0 {
                let boosted = (speed * SLIDE_LEDGE_SPEED_BOOST).min(MAX_SLIDE_LEDGE_SPEED);
                let scale = boosted / speed;
                p.state.vel.x *= scale;
                p.state.vel.z *= scale;
            }
            p.slide_falling = true;
        }
        if !was_on_ground && p.state.on_ground {
            // Bleed a small amount of carried momentum on every landing.
            p.state.vel.x *= LANDING_SPEED_RETENTION;
            p.state.vel.z *= LANDING_SPEED_RETENTION;
        }
        if p.state.on_ground {
            p.jumps_remaining = 2;
            p.slide_falling = false;
            p.wall_running = false;
            p.wall_run_armed = true;
            p.wall_runs_remaining = MAX_WALL_RUNS;
            p.wall_run_time = 0.0;
            p.wall_run_duration = WALL_RUN_DURATION_MIN;
            p.wall_run_speed = 0.0;
            p.wall_contact_grace = 0.0;
            p.wall_normal = Vec3::ZERO;
            p.has_last_wall = false;
        }

        // Weapon bob clock follows ground speed. Keep the previous value so
        // the viewmodel can interpolate between ticks like the camera does.
        self.prev_bob_time = self.bob_time;
        self.prev_viewmodel_ground_weight = self.viewmodel_ground_weight;
        let ground_target = if p.state.on_ground { 1.0 } else { 0.2 };
        let ground_response = dt * 12.0;
        self.viewmodel_ground_weight += (ground_target - self.viewmodel_ground_weight)
            * (ground_response / (1.0 + ground_response));
        let speed = sqrtf(p.state.vel.x * p.state.vel.x + p.state.vel.z * p.state.vel.z);
        if p.state.on_ground {
            self.bob_time += dt * (speed / 250.0) * 11.0;
        }
    }

    fn fire_shot(&mut self, col: &MapCollision) {
        const DEG: f32 = core::f32::consts::PI / 180.0;
        // Per-shot recoil additions: mostly vertical at first, then a
        // controlled left/right spray that can be learned and countered.
        const RECOIL_PATTERN: [(f32, f32); 16] = [
            (0.34, 0.00),
            (0.40, -0.08),
            (0.46, 0.13),
            (0.51, -0.19),
            (0.54, -0.24),
            (0.50, 0.31),
            (0.45, 0.36),
            (0.39, -0.40),
            (0.34, -0.32),
            (0.29, 0.38),
            (0.25, 0.31),
            (0.22, -0.35),
            (0.19, -0.25),
            (0.17, 0.29),
            (0.15, 0.22),
            (0.13, -0.24),
        ];
        self.fired_this_tick = true;
        let p = &self.player;
        let eye = p.eye();
        let shot_yaw = p.yaw + self.weapon.aim_punch_yaw;
        let shot_pitch = (p.pitch + self.weapon.aim_punch_pitch).clamp(-89.0 * DEG, 89.0 * DEG);
        let (sy, cy) = sin_cos(shot_yaw);
        let (sp, cp) = sin_cos(shot_pitch);
        let dir = Vec3::new(-sy * cp, sp, -cy * cp);
        let right = Vec3::new(cy, 0.0, -sy);
        let up = right.cross(dir).normalize_or_zero();

        // CS-like inaccuracy: the first stationary shot is precise, running
        // blooms quadratically, crouching tightens the cone, and airborne
        // fire is intentionally unreliable. Sustained spray adds a smaller
        // cone on top of the learnable recoil pattern.
        let speed = sqrtf(p.state.vel.x * p.state.vel.x + p.state.vel.z * p.state.vel.z);
        let move_frac = (speed / p.params.max_speed.max(1.0)).clamp(0.0, 1.4);
        let spray_shots = self.weapon.spray_index.saturating_sub(1).min(12) as f32;
        let mut spread = 0.0022 + move_frac * move_frac * 0.018 + spray_shots * 0.00065;
        if !p.state.on_ground {
            spread += 0.040;
        } else if p.crouched {
            spread *= 0.65;
        }
        // The sniper is deliberately unusable from the hip. Raising its
        // scope removes that extra cone smoothly, then settles more tightly
        // than the other weapons. This prevents both no-scope accuracy and a
        // one-frame right-click accuracy exploit.
        if matches!(self.active_weapon, 3 | 8) {
            let hip_weight = 1.0 - self.aim_fraction;
            spread += SNIPER_HIP_SPREAD * hip_weight * hip_weight;
            spread *= 1.0 - (1.0 - SNIPER_AIM_SPREAD_SCALE) * self.aim_fraction;
        } else {
            spread *= 1.0 - (1.0 - AIM_SPREAD_SCALE) * self.aim_fraction;
        }
        let dir = (dir + right * self.rng.signed() * spread + up * self.rng.signed() * spread)
            .normalize();
        self.last_shot_direction = dir;

        // World hit.
        let wt = col.trace(Hull::Point, eye, eye + dir * RANGE);
        let mut best_t = wt.fraction * RANGE;
        let mut hit_bot: Option<usize> = None;
        for (i, bot) in self.bots.iter().enumerate() {
            if !bot.alive() {
                continue;
            }
            let c = bot.state.pos;
            if let Some(t) = ray_aabb(eye, dir, c - BOT_HALF, c + BOT_HALF) {
                if t < best_t {
                    best_t = t;
                    hit_bot = Some(i);
                }
            }
        }
        let hit_point = eye + dir * best_t;

        // Effects: muzzle flash + tracer + impact.
        let muzzle = self
            .viewmodel_transform_at(1.0)
            .transform_point3(MUZZLE_LOCAL);
        self.effects.spawn(
            EffectKind::MuzzleFlash {
                pos: muzzle,
                direction: dir,
            },
            0.075,
        );
        self.effects.spawn(
            EffectKind::ShellEject {
                pos: muzzle - dir * 10.0,
                velocity: right * 72.0 + up * 42.0 - dir * 12.0,
            },
            0.32,
        );
        self.effects.spawn(
            EffectKind::Tracer {
                a: muzzle,
                b: hit_point,
            },
            0.07,
        );

        if let Some(i) = hit_bot {
            let bot = &mut self.bots[i];
            let headshot = hit_point.y > bot.state.pos.y + 22.0;
            let dmg = if headshot {
                self.weapon.cfg.damage_head
            } else {
                self.weapon.cfg.damage_body
            };
            let died = bot.hurt(dmg);
            self.effects
                .spawn(EffectKind::BloodPuff { pos: hit_point }, 0.22);
            self.events.push(GameEvent::Hit {
                bot: i,
                headshot,
                damage: dmg,
                fatal: died,
            });
        } else if wt.fraction < 1.0 {
            self.effects.spawn(
                EffectKind::Impact {
                    pos: hit_point,
                    normal: wt.normal,
                },
                0.20,
            );
        }

        // Camera kick.
        let pattern_index = self
            .weapon
            .spray_index
            .saturating_sub(1)
            .min(RECOIL_PATTERN.len() as u32 - 1) as usize;
        let (pitch_kick, yaw_kick) = RECOIL_PATTERN[pattern_index];
        let sniper_scoped = matches!(self.active_weapon, 3 | 8) && self.aim_fraction >= 0.92;
        let recoil_scale = 1.0 - (1.0 - AIM_RECOIL_SCALE) * self.aim_fraction;
        self.weapon.add_aim_punch(
            pitch_kick * DEG * recoil_scale,
            yaw_kick * DEG * recoil_scale,
        );
        if sniper_scoped {
            let side = self.rng.signed();
            self.weapon
                .add_camera_shake(11.0 * DEG, side * 2.8 * DEG, -side.signum() * 3.2 * DEG);
        }
    }

    /// Viewmodel placement: camera-anchored with bob, movement sway, recoil,
    /// and reload dip.
    ///
    /// `alpha` is the render interpolation factor. The gun MUST ride the same
    /// interpolated eye as the camera — anchoring it to the raw tick position
    /// makes it jitter against the world at the tick rate whenever the player
    /// moves. Bob and recoil interpolate for the same reason.
    pub fn viewmodel_transform_at(&self, alpha: f32) -> Mat4 {
        let p = &self.player;
        let eye = p.eye_interpolated(alpha);
        let velocity = p.prev_vel.lerp(p.state.vel, alpha);
        let speed = sqrtf(velocity.x * velocity.x + velocity.z * velocity.z);
        let ground_weight = self.prev_viewmodel_ground_weight
            + (self.viewmodel_ground_weight - self.prev_viewmodel_ground_weight) * alpha;
        let bob_amp = (speed / 250.0).min(1.0) * ground_weight;
        let bob_t = self.prev_bob_time + (self.bob_time - self.prev_bob_time) * alpha;
        let bob_y = sinf(bob_t * 2.0) * 0.55 * bob_amp;
        let bob_x = crate::cosf(bob_t) * 0.4 * bob_amp;
        let lateral = (velocity.dot(p.right()) / 250.0).clamp(-1.0, 1.0);
        let forward = (velocity.dot(p.forward_flat()) / 250.0).clamp(-1.0, 1.0);
        let sway = sinf(bob_t * 0.5) * bob_amp;
        let sway_x = sway * 0.65 - lateral * 0.55;
        let sway_yaw = sway * 0.018 - lateral * 0.025;
        let sway_roll = sway * 0.028 - lateral * 0.035;
        let sway_pitch = crate::cosf(bob_t * 0.5) * bob_amp * 0.012 + forward * 0.01;
        let look_sway = p.prev_look_sway + (p.look_sway - p.prev_look_sway) * alpha;
        let aim = self.aim_fraction_interpolated(alpha);
        let motion_scale = 1.0 - aim * 0.78;

        let recoil =
            self.weapon.prev_recoil + (self.weapon.recoil - self.weapon.prev_recoil) * alpha;
        Mat4::from_translation(eye)
            * Mat4::from_rotation_y(p.yaw)
            * Mat4::from_rotation_x(p.pitch)
            * Mat4::from_translation(Vec3::new(
                7.2 * (1.0 - aim) + (bob_x + sway_x + look_sway * 18.0) * motion_scale,
                -7.0 + aim * 1.4 + bob_y * motion_scale,
                -8.5 + aim * 2.8 + recoil * 2.8,
            ))
            * Mat4::from_rotation_x(recoil * 0.10 + sway_pitch * motion_scale)
            * Mat4::from_rotation_y((-0.03 + sway_yaw + look_sway) * motion_scale)
            * Mat4::from_rotation_z((sway_roll - look_sway * 0.35) * motion_scale)
    }
}

/// Slab-method ray/AABB intersection; returns distance along `dir`.
pub fn ray_aabb(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let inv = dir.recip();
    let t0 = (min - origin) * inv;
    let t1 = (max - origin) * inv;
    let tmin = t0.min(t1);
    let tmax = t0.max(t1);
    let enter = tmin.x.max(tmin.y).max(tmin.z);
    let exit = tmax.x.min(tmax.y).min(tmax.z);
    if enter <= exit && exit >= 0.0 {
        Some(enter.max(0.0))
    } else {
        None
    }
}

#[cfg(test)]
mod weapon_slot_tests {
    use super::*;

    #[test]
    fn weapon_slots_keep_independent_ammo_and_tuning() {
        let mut sim = StrikeSim::new(Vec3::ZERO, 0.0, Vec::new(), 0);
        sim.weapon.ammo = 17;

        assert!(sim.switch_weapon(1));
        assert_eq!(sim.weapon_name(), "AK-74M");
        assert_eq!(sim.weapon.cooldown, 0.5);
        assert_eq!(sim.weapon.cfg.mag_size, 30);
        assert_eq!(sim.weapon.cfg.fire_interval, 0.140);
        assert_eq!(sim.weapon.cfg.reload_time, 2.65);
        assert_eq!(
            (sim.weapon.cfg.damage_body, sim.weapon.cfg.damage_head),
            (15, 20)
        );
        sim.weapon.ammo = 8;

        assert!(sim.switch_weapon(2));
        assert_eq!(sim.weapon_name(), "UZI");
        assert_eq!(sim.weapon.cfg.mag_size, 32);
        assert_eq!(sim.weapon.cfg.fire_interval, 0.075);
        assert_eq!(
            (sim.weapon.cfg.damage_body, sim.weapon.cfg.damage_head),
            (10, 15)
        );

        assert!(sim.switch_weapon(0));
        assert_eq!(sim.weapon.ammo, 17);
        assert_eq!(
            (sim.weapon.cfg.damage_body, sim.weapon.cfg.damage_head),
            (25, 75)
        );
        assert!(sim.switch_weapon(1));
        assert_eq!(sim.weapon.ammo, 8);

        assert!(sim.switch_weapon(3));
        assert_eq!(sim.weapon_name(), "BOLT-ACTION SNIPER");
        assert_eq!(sim.weapon.cfg.mag_size, 5);
        assert_eq!(sim.weapon.cfg.fire_interval, 1.70);
        assert_eq!(
            (sim.weapon.cfg.damage_body, sim.weapon.cfg.damage_head),
            (50, 100)
        );
    }

    #[test]
    fn aiming_centers_every_weapon_and_reduces_fov() {
        let mut sim = StrikeSim::new(Vec3::ZERO, 0.0, Vec::new(), 0);
        let hip_x = sim.viewmodel_transform_at(1.0).w_axis.x;

        sim.prev_aim_fraction = 1.0;
        sim.aim_fraction = 1.0;
        let aimed_x = sim.viewmodel_transform_at(1.0).w_axis.x;
        assert!(hip_x > 7.0);
        assert!(aimed_x.abs() < 0.001);
        assert!((sim.aim_fov_offset_interpolated(1.0) + AIM_FOV_REDUCTION).abs() < 0.000_001);

        for slot in 1..4 {
            assert!(sim.switch_weapon(slot));
            assert!(sim.viewmodel_transform_at(1.0).w_axis.x.abs() < 0.001);
        }
        assert!(
            (sim.aim_fov_offset_interpolated(1.0) + SNIPER_AIM_FOV_REDUCTION).abs() < 0.000_001
        );
        assert!((sim.aim_move_speed_scale() - SNIPER_AIM_MOVE_SPEED_SCALE).abs() < 0.000_001);
    }

    #[test]
    fn sliding_boosts_fov_and_tilts_in_both_directions() {
        let mut player = Player::spawn(Vec3::ZERO, 0.0);
        player.sliding = true;
        player.state.vel = player.forward_flat() * 300.0;
        player.update_camera(1.0, 0.0);
        assert!(player.camera_roll > 0.0);
        assert!(player.camera_fov_boost > 0.0);

        player.camera_roll = 0.0;
        player.prev_camera_roll = 0.0;
        player.state.vel = -player.forward_flat() * 300.0;
        player.update_camera(1.0, 0.0);
        assert!(player.camera_roll < 0.0);
        assert!(player.camera_fov_boost > 0.0);
    }
}
