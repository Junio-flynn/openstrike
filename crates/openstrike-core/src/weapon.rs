//! The rifle: config, firing state, timed effects, the procedural viewmodel
//! geometry (as plain data), and the shared deterministic RNG.

use alloc::vec::Vec;

use glam::Vec3;

pub const RANGE: f32 = 8192.0;

/// Weapon tuning — owned by the `strike` surface (mods set it through
/// `strike.configureWeapon`). Defaults are the base game's rifle.
#[derive(Clone, Debug)]
pub struct WeaponConfig {
    pub mag_size: u32,
    pub reserve: u32,
    /// Seconds between shots (0.105 ≈ 570 rpm).
    pub fire_interval: f32,
    pub reload_time: f32,
    pub damage_body: i32,
    pub damage_head: i32,
}

impl Default for WeaponConfig {
    fn default() -> Self {
        Self {
            mag_size: 30,
            reserve: 90,
            fire_interval: 0.105,
            reload_time: 2.4,
            damage_body: 25,
            damage_head: 75,
        }
    }
}

pub struct Weapon {
    pub cfg: WeaponConfig,
    pub ammo: u32,
    pub reserve: u32,
    pub cooldown: f32,
    /// Previous tick's cooldown for smooth render-time animation sampling.
    pub prev_cooldown: f32,
    /// Seconds since the last shot, sampled independently from fire-rate
    /// cooldown so the authored fire animation can finish naturally.
    pub shot_age: f32,
    pub prev_shot_age: f32,
    /// Counter-Strike-style spray state. The pattern resets after a short
    /// pause; aim punch recovers smoothly once firing stops.
    pub spray_index: u32,
    pub aim_punch_pitch: f32,
    pub prev_aim_punch_pitch: f32,
    pub aim_punch_yaw: f32,
    pub prev_aim_punch_yaw: f32,
    /// Purely visual left/right camera roll. It never participates in the
    /// hitscan direction or accuracy calculation.
    pub camera_kick_roll: f32,
    pub prev_camera_kick_roll: f32,
    pub reload_left: f32,
    /// Previous tick's reload timer for smooth render-time animation sampling.
    pub prev_reload_left: f32,
    /// 0..1 visual recoil, decays.
    pub recoil: f32,
    /// `recoil` at the previous tick (the viewmodel interpolates between the
    /// two so recoil doesn't step at the tick rate on high-Hz displays).
    pub prev_recoil: f32,
}

impl Default for Weapon {
    fn default() -> Self {
        Self::with_config(WeaponConfig::default())
    }
}

impl Weapon {
    pub fn with_config(cfg: WeaponConfig) -> Self {
        Self {
            ammo: cfg.mag_size,
            reserve: cfg.reserve,
            cfg,
            cooldown: 0.0,
            prev_cooldown: 0.0,
            shot_age: 999.0,
            prev_shot_age: 999.0,
            spray_index: 0,
            aim_punch_pitch: 0.0,
            prev_aim_punch_pitch: 0.0,
            aim_punch_yaw: 0.0,
            prev_aim_punch_yaw: 0.0,
            camera_kick_roll: 0.0,
            prev_camera_kick_roll: 0.0,
            reload_left: 0.0,
            prev_reload_left: 0.0,
            recoil: 0.0,
            prev_recoil: 0.0,
        }
    }

    pub fn reloading(&self) -> bool {
        self.reload_left > 0.0
    }

    pub fn can_fire(&self) -> bool {
        self.cooldown <= 0.0 && self.ammo > 0 && !self.reloading()
    }

    pub fn tick(&mut self, dt: f32) {
        self.prev_cooldown = self.cooldown;
        self.prev_shot_age = self.shot_age;
        self.shot_age = (self.shot_age + dt).min(999.0);
        self.prev_aim_punch_pitch = self.aim_punch_pitch;
        self.prev_aim_punch_yaw = self.aim_punch_yaw;
        self.prev_camera_kick_roll = self.camera_kick_roll;
        let roll_response = 14.0 * dt;
        self.camera_kick_roll +=
            (0.0 - self.camera_kick_roll) * (roll_response / (1.0 + roll_response));
        if self.shot_age > 0.10 {
            let response = 8.0 * dt;
            let blend = response / (1.0 + response);
            self.aim_punch_pitch += (0.0 - self.aim_punch_pitch) * blend;
            self.aim_punch_yaw += (0.0 - self.aim_punch_yaw) * blend;
        }
        if self.shot_age > 0.38 {
            self.spray_index = 0;
        }
        self.prev_reload_left = self.reload_left;
        self.cooldown -= dt;
        self.prev_recoil = self.recoil;
        self.recoil = (self.recoil - dt * 3.0).max(0.0);
        if self.reload_left > 0.0 {
            self.reload_left -= dt;
            if self.reload_left <= 0.0 {
                let want = self.cfg.mag_size - self.ammo;
                let take = want.min(self.reserve);
                self.ammo += take;
                self.reserve -= take;
            }
        }
    }

    pub fn trigger_reload(&mut self) {
        if !self.reloading() && self.ammo < self.cfg.mag_size && self.reserve > 0 {
            self.reload_left = self.cfg.reload_time;
            self.prev_reload_left = self.reload_left;
        }
    }

    /// Consume one round; returns false if empty.
    pub fn fire(&mut self) -> bool {
        self.fire_with_kick(1.0)
    }

    /// Consume one round while scaling presentation kick. Ballistic aim
    /// punch is applied by the simulation after the hitscan is resolved.
    pub fn fire_with_kick(&mut self, kick_scale: f32) -> bool {
        if !self.can_fire() {
            return false;
        }
        let kick_scale = kick_scale.clamp(0.0, 1.0);
        if self.shot_age > 0.38 {
            self.spray_index = 0;
        }
        self.ammo -= 1;
        self.cooldown = self.cfg.fire_interval;
        self.prev_cooldown = self.cooldown;
        self.shot_age = 0.0;
        self.prev_shot_age = 0.0;
        self.spray_index = (self.spray_index + 1).min(30);
        let roll_side = if self.spray_index % 4 < 2 { 1.0 } else { -1.0 };
        self.prev_camera_kick_roll = self.camera_kick_roll;
        self.camera_kick_roll = (self.camera_kick_roll
            + roll_side * 0.42f32.to_radians() * kick_scale)
            .clamp(-1.0f32.to_radians(), 1.0f32.to_radians());
        self.recoil = (self.recoil + 0.35 * kick_scale).min(1.0);
        true
    }

    pub fn add_aim_punch(&mut self, pitch: f32, yaw: f32) {
        self.prev_aim_punch_pitch = self.aim_punch_pitch;
        self.prev_aim_punch_yaw = self.aim_punch_yaw;
        self.aim_punch_pitch = (self.aim_punch_pitch + pitch).clamp(0.0, 6f32.to_radians());
        self.aim_punch_yaw =
            (self.aim_punch_yaw + yaw).clamp(-2.5f32.to_radians(), 2.5f32.to_radians());
    }

    /// Strong one-shot presentation kick used by a scoped bolt-action rifle.
    /// It is applied after the hitscan, shaking the camera without changing
    /// the direction of the bullet that was already fired.
    pub fn add_camera_shake(&mut self, pitch: f32, yaw: f32, roll: f32) {
        self.prev_aim_punch_pitch = self.aim_punch_pitch;
        self.prev_aim_punch_yaw = self.aim_punch_yaw;
        self.prev_camera_kick_roll = self.camera_kick_roll;
        self.aim_punch_pitch = (self.aim_punch_pitch + pitch).clamp(0.0, 14.0f32.to_radians());
        self.aim_punch_yaw =
            (self.aim_punch_yaw + yaw).clamp(-5.0f32.to_radians(), 5.0f32.to_radians());
        self.camera_kick_roll =
            (self.camera_kick_roll + roll).clamp(-5.0f32.to_radians(), 5.0f32.to_radians());
    }

    pub fn camera_kick_interpolated(&self, alpha: f32) -> (f32, f32, f32) {
        // Show only part of the ballistic aim punch to soften the camera
        // hit. The full values remain in fire_shot for the spray pattern.
        const VISUAL_PUNCH_SCALE: f32 = 0.65;
        (
            (self.prev_aim_punch_pitch
                + (self.aim_punch_pitch - self.prev_aim_punch_pitch) * alpha)
                * VISUAL_PUNCH_SCALE,
            (self.prev_aim_punch_yaw + (self.aim_punch_yaw - self.prev_aim_punch_yaw) * alpha)
                * VISUAL_PUNCH_SCALE,
            self.prev_camera_kick_roll
                + (self.camera_kick_roll - self.prev_camera_kick_roll) * alpha,
        )
    }

    /// Fresh magazine under the current config (round reset).
    pub fn reset(&mut self) {
        *self = Self::with_config(self.cfg.clone());
    }
}

// ---------------------------------------------------------------------------
// Timed world-space effects (flashes, tracers, impacts)
// ---------------------------------------------------------------------------

pub enum EffectKind {
    MuzzleFlash { pos: Vec3, direction: Vec3 },
    Tracer { a: Vec3, b: Vec3 },
    Impact { pos: Vec3, normal: Vec3 },
    BloodPuff { pos: Vec3 },
    DoubleJumpBurst { pos: Vec3 },
    ShellEject { pos: Vec3, velocity: Vec3 },
}

pub struct Effect {
    pub kind: EffectKind,
    pub age: f32,
    pub ttl: f32,
}

/// Renderer-agnostic effect output (the desktop maps these to scene
/// sprites/beams; the PSP draws billboards).
#[derive(Clone, Copy, Debug)]
pub struct FxSprite {
    pub pos: Vec3,
    pub size: f32,
    pub color: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct FxBeam {
    pub a: Vec3,
    pub b: Vec3,
    pub width: f32,
    pub color: [f32; 4],
}

#[derive(Default)]
pub struct Effects {
    pub list: Vec<Effect>,
}

impl Effects {
    pub fn spawn(&mut self, kind: EffectKind, ttl: f32) {
        self.list.push(Effect {
            kind,
            age: 0.0,
            ttl,
        });
    }

    pub fn tick(&mut self, dt: f32) {
        for e in &mut self.list {
            e.age += dt;
        }
        self.list.retain(|e| e.age < e.ttl);
    }

    pub fn clear(&mut self) {
        self.list.clear();
    }

    /// Emit sprites/beams for this frame.
    pub fn emit(&self, sprites: &mut Vec<FxSprite>, beams: &mut Vec<FxBeam>) {
        for e in &self.list {
            let f = 1.0 - (e.age / e.ttl).clamp(0.0, 1.0);
            match e.kind {
                EffectKind::MuzzleFlash { pos, direction } => {
                    // Broad bloom, white-hot core, forward flame, and two
                    // crossing rays form a directional muzzle star.
                    sprites.push(FxSprite {
                        pos,
                        size: 27.0 + 14.0 * f,
                        color: [1.0, 0.32, 0.04, 0.58 * f],
                    });
                    sprites.push(FxSprite {
                        pos,
                        size: 9.0 + 11.0 * f,
                        color: [1.0, 0.95, 0.72, f],
                    });
                    let direction = direction.normalize_or_zero();
                    let flame_tip = pos + direction * (24.0 + 14.0 * f);
                    beams.push(FxBeam {
                        a: pos,
                        b: flame_tip,
                        width: 8.0 + 5.0 * f,
                        color: [1.0, 0.35, 0.04, 0.5 * f],
                    });
                    beams.push(FxBeam {
                        a: pos,
                        b: flame_tip,
                        width: 2.4 + 2.0 * f,
                        color: [1.0, 0.98, 0.78, 0.95 * f],
                    });
                    let side = direction.cross(Vec3::Y).normalize_or_zero();
                    let up = side.cross(direction).normalize_or_zero();
                    for axis in [side * 11.0, up * 8.0] {
                        beams.push(FxBeam {
                            a: pos - axis * f,
                            b: pos + axis * f,
                            width: 2.2,
                            color: [1.0, 0.7, 0.24, 0.72 * f],
                        });
                    }
                }
                EffectKind::Tracer { a, b } => {
                    beams.push(FxBeam {
                        a,
                        b,
                        width: 3.2,
                        color: [1.0, 0.35, 0.06, 0.28 * f],
                    });
                    beams.push(FxBeam {
                        a,
                        b,
                        width: 1.15,
                        color: [1.0, 0.94, 0.62, 0.9 * f],
                    });
                    let progress = (e.age / e.ttl).clamp(0.0, 1.0);
                    let head = a.lerp(b, progress.sqrt());
                    let tail = a.lerp(b, (progress.sqrt() - 0.16).max(0.0));
                    beams.push(FxBeam {
                        a: tail,
                        b: head,
                        width: 4.6,
                        color: [1.0, 0.62, 0.16, 0.72 * f],
                    });
                    sprites.push(FxSprite {
                        pos: head,
                        size: 5.0 + 4.0 * f,
                        color: [1.0, 0.92, 0.56, 0.92 * f],
                    });
                }
                EffectKind::Impact { pos, normal } => {
                    sprites.push(FxSprite {
                        pos,
                        size: 7.0 + 10.0 * (1.0 - f),
                        color: [1.0, 0.72, 0.28, 0.9 * f],
                    });
                    sprites.push(FxSprite {
                        pos: pos + normal * (2.0 + e.age * 9.0),
                        size: 13.0 + 14.0 * (1.0 - f),
                        color: [0.62, 0.48, 0.32, 0.28 * f],
                    });
                    let tangent = if normal.y.abs() < 0.9 {
                        normal.cross(Vec3::Y).normalize_or_zero()
                    } else {
                        normal.cross(Vec3::X).normalize_or_zero()
                    };
                    let bitangent = normal.cross(tangent).normalize_or_zero();
                    let spread = [
                        (-0.9, 0.2),
                        (-0.62, -0.55),
                        (-0.35, 0.8),
                        (-0.08, -0.92),
                        (0.25, -0.75),
                        (0.48, 0.88),
                        (0.75, 0.55),
                        (0.94, -0.18),
                    ];
                    for (side, up) in spread {
                        let velocity =
                            normal * 48.0 + tangent * (side * 64.0) + bitangent * (up * 52.0);
                        sprites.push(FxSprite {
                            pos: pos + velocity * e.age - Vec3::Y * (110.0 * e.age * e.age),
                            size: 2.0 + 2.5 * f,
                            color: [1.0, 0.58, 0.16, 0.8 * f],
                        });
                    }
                }
                EffectKind::BloodPuff { pos } => sprites.push(FxSprite {
                    pos,
                    size: 10.0 + 8.0 * (1.0 - f),
                    color: [0.75, 0.1, 0.05, 0.8 * f],
                }),
                EffectKind::DoubleJumpBurst { pos } => {
                    sprites.push(FxSprite {
                        pos: pos - Vec3::Y * (e.age * 34.0),
                        size: 13.0 + 9.0 * (1.0 - f),
                        color: [0.38, 0.9, 1.0, 0.55 * f],
                    });
                    let directions = [
                        Vec3::new(1.0, 0.0, 0.0),
                        Vec3::new(-1.0, 0.0, 0.0),
                        Vec3::new(0.0, 0.0, 1.0),
                        Vec3::new(0.0, 0.0, -1.0),
                        Vec3::new(0.707, 0.0, 0.707),
                        Vec3::new(-0.707, 0.0, 0.707),
                        Vec3::new(0.707, 0.0, -0.707),
                        Vec3::new(-0.707, 0.0, -0.707),
                    ];
                    for (i, direction) in directions.into_iter().enumerate() {
                        let speed = 42.0 + (i % 3) as f32 * 9.0;
                        sprites.push(FxSprite {
                            pos: pos + direction * (speed * e.age)
                                - Vec3::Y * (32.0 * e.age + 125.0 * e.age * e.age),
                            size: 3.0 + 4.0 * f,
                            color: [0.55, 0.94, 1.0, 0.82 * f],
                        });
                    }
                }
                EffectKind::ShellEject { pos, velocity } => {
                    let shell_pos = pos + velocity * e.age - Vec3::Y * (180.0 * e.age * e.age);
                    sprites.push(FxSprite {
                        pos: shell_pos,
                        size: 2.0 + 2.0 * f,
                        color: [1.0, 0.66, 0.18, 0.9 * f],
                    });
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Procedural rifle viewmodel (geometry only; platforms upload/draw it)
// ---------------------------------------------------------------------------

/// Material palette (one texel per entry on desktop; vertex colors on PSP).
pub const GUN_COLORS: [[u8; 4]; 6] = [
    [38, 38, 42, 255],   // 0 receiver: gunmetal
    [22, 22, 24, 255],   // 1 barrel: near-black
    [82, 58, 38, 255],   // 2 wood furniture
    [55, 55, 60, 255],   // 3 magazine
    [30, 30, 33, 255],   // 4 grip/sights
    [140, 120, 90, 255], // 5 accent
];

/// Where the muzzle sits in viewmodel-local space (gun points -Z).
pub const MUZZLE_LOCAL: Vec3 = Vec3::new(0.0, 0.6, -31.0);

/// One box of the rifle, in viewmodel-local space.
#[derive(Clone, Copy, Debug)]
pub struct RifleBox {
    pub min: Vec3,
    pub max: Vec3,
    /// Index into [`GUN_COLORS`].
    pub color: usize,
}

/// The rifle as boxes (receiver, barrel, furniture, ...). Kept as data so
/// each backend builds its own vertex format from one source of truth.
pub fn rifle_boxes() -> [RifleBox; 10] {
    let b = |min: Vec3, max: Vec3, color: usize| RifleBox { min, max, color };
    [
        // Receiver.
        b(Vec3::new(-1.3, -2.0, -16.0), Vec3::new(1.3, 1.6, 4.0), 0),
        // Barrel + muzzle.
        b(Vec3::new(-0.45, 0.1, -30.0), Vec3::new(0.45, 1.0, -16.0), 1),
        b(
            Vec3::new(-0.65, -0.05, -32.0),
            Vec3::new(0.65, 1.15, -30.0),
            4,
        ),
        // Wood handguard under the barrel.
        b(
            Vec3::new(-0.95, -1.3, -26.0),
            Vec3::new(0.95, 0.1, -16.0),
            2,
        ),
        // Magazine (slightly raked).
        b(
            Vec3::new(-0.95, -6.4, -10.5),
            Vec3::new(0.95, -2.0, -6.0),
            3,
        ),
        // Pistol grip.
        b(Vec3::new(-0.85, -5.2, -1.2), Vec3::new(0.85, -2.0, 1.6), 4),
        // Stock.
        b(Vec3::new(-1.05, -2.4, 4.0), Vec3::new(1.05, 1.0, 12.5), 2),
        // Front sight + rear sight.
        b(Vec3::new(-0.18, 1.0, -29.4), Vec3::new(0.18, 2.2, -28.6), 4),
        b(Vec3::new(-0.5, 1.6, -6.0), Vec3::new(0.5, 2.3, -4.8), 4),
        // Carry-handle hint above receiver.
        b(Vec3::new(-0.4, 1.6, -4.0), Vec3::new(0.4, 2.0, 2.0), 0),
    ]
}

/// A tiny deterministic PRNG (xorshift) — reproducible headless runs.
#[derive(Clone)]
pub struct Rng(pub u64);

impl Rng {
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 32) as u32
    }

    /// Uniform in [0, 1).
    pub fn f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1 << 24) as f32
    }

    /// Uniform in [-1, 1).
    pub fn signed(&mut self) -> f32 {
        self.f32() * 2.0 - 1.0
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.f32() * (hi - lo)
    }
}

#[cfg(test)]
mod tests {
    use super::Weapon;

    #[test]
    fn spray_builds_then_resets_and_aim_punch_recovers() {
        let mut weapon = Weapon::default();
        assert!(weapon.fire());
        assert_eq!(weapon.spray_index, 1);
        assert_ne!(weapon.camera_kick_roll, 0.0);
        assert_eq!(weapon.aim_punch_yaw, 0.0);
        weapon.add_aim_punch(0.05, 0.02);
        let kicked = weapon.aim_punch_pitch;

        weapon.tick(0.11);
        assert!(weapon.fire());
        assert_eq!(weapon.spray_index, 2);

        weapon.tick(0.40);
        assert_eq!(weapon.spray_index, 0);
        assert!(weapon.aim_punch_pitch < kicked);
    }

    #[test]
    fn scaled_fire_reduces_visible_kick_without_changing_the_shot() {
        let mut hip = Weapon::default();
        let mut aimed = Weapon::default();
        assert!(hip.fire_with_kick(1.0));
        assert!(aimed.fire_with_kick(0.4));

        assert_eq!(hip.ammo, aimed.ammo);
        assert_eq!(hip.spray_index, aimed.spray_index);
        assert!((aimed.recoil - hip.recoil * 0.4).abs() < 0.000_001);
        assert!((aimed.camera_kick_roll - hip.camera_kick_roll * 0.4).abs() < 0.000_001);
    }

    #[test]
    fn scoped_sniper_shake_is_large_and_recovers() {
        let mut weapon = Weapon::default();
        weapon.add_camera_shake(
            11.0f32.to_radians(),
            2.8f32.to_radians(),
            -3.2f32.to_radians(),
        );
        assert!(weapon.aim_punch_pitch > 10.0f32.to_radians());
        assert!(weapon.aim_punch_yaw > 2.0f32.to_radians());
        assert!(weapon.camera_kick_roll < -3.0f32.to_radians());
        let before = weapon.aim_punch_pitch;
        weapon.shot_age = 0.2;
        weapon.tick(0.1);
        assert!(weapon.aim_punch_pitch < before);
    }
}
