//! Self-contained desktop game audio.
//!
//! User-provided recordings are embedded for weapons, footsteps, aiming, and
//! reloads. Small supporting effects are synthesized once at startup, keeping
//! the executable self-contained when it is distributed.

use std::io::Cursor;

use pocket3d::input::Input;
use pocket3d::winit::event::MouseButton;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source, buffer::SamplesBuffer};

use openstrike_core::weapon::EffectKind;

use crate::game::OpenStrike;

const RATE: u32 = 44_100;

#[derive(Clone, Copy)]
enum Sound {
    Gun(usize),
    Footstep(usize),
    WallRun,
    AimIn,
    AimOut,
    Reload(usize),
    Land,
    Impact,
    Hit,
    Hurt,
    Death,
    Switch,
    Empty,
}

struct Clips {
    guns: [Vec<f32>; 4],
    footsteps: [Vec<f32>; 2],
    aim_in: Vec<f32>,
    aim_out: Vec<f32>,
    reloads: [Vec<f32>; 4],
    land: Vec<f32>,
    impact: Vec<f32>,
    hit: Vec<f32>,
    hurt: Vec<f32>,
    death: Vec<f32>,
    switch: Vec<f32>,
    empty: Vec<f32>,
}

impl Clips {
    fn new() -> Self {
        Self {
            guns: [
                recording(
                    include_bytes!("../../../assets/sounds/scorpion_shot.wav"),
                    gun(0),
                ),
                recording(
                    include_bytes!("../../../assets/sounds/ak74_shot.wav"),
                    gun(1),
                ),
                recording(
                    include_bytes!("../../../assets/sounds/uzi_shot.wav"),
                    gun(2),
                ),
                recording(
                    include_bytes!("../../../assets/sounds/sniper_shot.wav"),
                    gun(3),
                ),
            ],
            footsteps: [
                recording(
                    include_bytes!("../../../assets/sounds/footstep_walk.wav"),
                    thud(0.11, 105.0, 0.28, 11),
                ),
                recording(
                    include_bytes!("../../../assets/sounds/footstep_run.wav"),
                    thud(0.11, 92.0, 0.28, 29),
                ),
            ],
            aim_in: recording(
                include_bytes!("../../../assets/sounds/aim_in.wav"),
                mechanism(true),
            ),
            aim_out: recording(
                include_bytes!("../../../assets/sounds/aim_out.wav"),
                mechanism(false),
            ),
            reloads: [
                recording(
                    include_bytes!("../../../assets/sounds/scorpion_reload.wav"),
                    reload(),
                ),
                recording(
                    include_bytes!("../../../assets/sounds/ak74_reload.wav"),
                    reload(),
                ),
                recording(
                    include_bytes!("../../../assets/sounds/uzi_reload.wav"),
                    reload(),
                ),
                recording(
                    include_bytes!("../../../assets/sounds/sniper_reload.wav"),
                    reload(),
                ),
            ],
            land: recording(
                include_bytes!("../../../assets/sounds/landing.wav"),
                thud(0.18, 64.0, 0.42, 151),
            ),
            impact: thud(0.09, 185.0, 0.24, 181),
            hit: noise_burst(0.10, 0.18, 211),
            hurt: sweep(0.18, 180.0, 72.0, 0.18),
            death: sweep(0.42, 120.0, 38.0, 0.24),
            switch: mechanism(true),
            empty: click(0.045, 0.22, 251),
        }
    }

    fn get(&self, sound: Sound) -> (&[f32], f32) {
        match sound {
            Sound::Gun(slot) => {
                let slot = slot.min(3);
                let volume = match slot {
                    2 => 0.40,
                    3 => 0.78,
                    _ => 0.72,
                };
                (&self.guns[slot], volume)
            }
            Sound::Footstep(step) => (&self.footsteps[step & 1], 0.55),
            Sound::WallRun => (&self.footsteps[1], 0.48),
            Sound::AimIn => (&self.aim_in, 0.42),
            Sound::AimOut => (&self.aim_out, 0.36),
            Sound::Reload(slot) => (&self.reloads[slot.min(3)], 0.46),
            Sound::Land => (&self.land, 0.58),
            Sound::Impact => (&self.impact, 0.42),
            Sound::Hit => (&self.hit, 0.36),
            Sound::Hurt => (&self.hurt, 0.45),
            Sound::Death => (&self.death, 0.52),
            Sound::Switch => (&self.switch, 0.38),
            Sound::Empty => (&self.empty, 0.44),
        }
    }
}

/// Event-to-sound adapter owned by the interactive desktop host.
pub struct GameAudio {
    _stream: Option<OutputStream>,
    handle: Option<OutputStreamHandle>,
    clips: Clips,
    footstep_time: f32,
    wall_time: f32,
    dry_time: f32,
    previous_grounded: bool,
    previous_aiming: bool,
    previous_reloading: bool,
    previous_weapon: usize,
    previous_health: i32,
    previous_alive: bool,
}

impl GameAudio {
    pub fn new() -> Self {
        let (stream, handle) = match OutputStream::try_default() {
            Ok(pair) => (Some(pair.0), Some(pair.1)),
            Err(error) => {
                log::warn!("audio output unavailable; continuing silently: {error}");
                (None, None)
            }
        };
        Self {
            _stream: stream,
            handle,
            clips: Clips::new(),
            footstep_time: 0.0,
            wall_time: 0.0,
            dry_time: 0.0,
            previous_grounded: true,
            previous_aiming: false,
            previous_reloading: false,
            previous_weapon: 0,
            previous_health: 100,
            previous_alive: true,
        }
    }

    pub fn pause(&mut self) {
        if self.previous_aiming {
            self.play(Sound::AimOut);
        }
        self.previous_aiming = false;
        self.footstep_time = 0.0;
        self.wall_time = 0.0;
    }

    pub fn tick(&mut self, game: &OpenStrike, input: &Input, dt: f32) {
        self.dry_time = (self.dry_time - dt).max(0.0);
        let player = &game.player;
        let aiming = input.mouse_button_down(MouseButton::Right) && player.alive;
        if aiming != self.previous_aiming {
            self.play(if aiming { Sound::AimIn } else { Sound::AimOut });
        }

        if game.fired_this_tick {
            self.play(Sound::Gun(game.active_weapon));
        } else if input.mouse_button_down(MouseButton::Left)
            && game.weapon.ammo == 0
            && self.dry_time <= 0.0
        {
            self.play(Sound::Empty);
            self.dry_time = 0.24;
        }

        let reloading = game.weapon.reloading();
        if reloading && !self.previous_reloading {
            self.play(Sound::Reload(game.active_weapon));
        }
        if game.active_weapon != self.previous_weapon {
            self.play(Sound::Switch);
        }

        if !self.previous_grounded && player.state.on_ground {
            self.play(Sound::Land);
        }

        let speed = game.ground_speed();
        if player.is_wall_running() {
            self.wall_time -= dt;
            if self.wall_time <= 0.0 {
                self.play(Sound::WallRun);
                self.wall_time = 0.17;
            }
        } else {
            self.wall_time = 0.0;
        }
        if player.state.on_ground && !player.sliding && speed > 45.0 {
            self.footstep_time -= dt;
            if self.footstep_time <= 0.0 {
                self.play(Sound::Footstep(usize::from(speed >= 180.0)));
                self.footstep_time = (0.46 - speed.min(360.0) / 1_500.0).max(0.20);
            }
        } else {
            self.footstep_time = 0.0;
        }

        for effect in &game.effects.list {
            if effect.age != 0.0 {
                continue;
            }
            match effect.kind {
                EffectKind::Impact { .. } => self.play(Sound::Impact),
                EffectKind::BloodPuff { .. } => self.play(Sound::Hit),
                _ => {}
            }
        }
        if player.health < self.previous_health && player.alive {
            self.play(Sound::Hurt);
        }
        if self.previous_alive && !player.alive {
            self.play(Sound::Death);
        }

        self.previous_grounded = player.state.on_ground;
        self.previous_aiming = aiming;
        self.previous_reloading = reloading;
        self.previous_weapon = game.active_weapon;
        self.previous_health = player.health;
        self.previous_alive = player.alive;
    }

    fn play(&self, sound: Sound) {
        let Some(handle) = &self.handle else { return };
        let Ok(sink) = Sink::try_new(handle) else {
            return;
        };
        let (samples, volume) = self.clips.get(sound);
        sink.set_volume(volume);
        sink.append(SamplesBuffer::new(1, RATE, samples.to_vec()));
        sink.detach();
    }
}

fn samples(seconds: f32) -> usize {
    (seconds * RATE as f32) as usize
}

fn recording(bytes: &'static [u8], fallback: Vec<f32>) -> Vec<f32> {
    Decoder::new_wav(Cursor::new(bytes))
        .map(|source| source.convert_samples().collect())
        .unwrap_or(fallback)
}

fn noise(seed: &mut u32) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    (*seed as f32 / u32::MAX as f32) * 2.0 - 1.0
}

fn gun(slot: usize) -> Vec<f32> {
    let duration = [0.19, 0.25, 0.16, 0.34][slot];
    let boom = [78.0, 64.0, 92.0, 52.0][slot];
    let mut seed = 401 + slot as u32 * 97;
    (0..samples(duration))
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let envelope = (-t * [24.0, 19.0, 29.0, 15.0][slot]).exp();
            let crack = noise(&mut seed) * (-t * 38.0).exp();
            let body = (t * boom * core::f32::consts::TAU).sin() * envelope;
            (crack * 0.72 + body * 0.58).clamp(-1.0, 1.0)
        })
        .collect()
}

fn thud(duration: f32, hz: f32, gain: f32, mut seed: u32) -> Vec<f32> {
    (0..samples(duration))
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let env = (-t * 25.0).exp();
            ((t * hz * core::f32::consts::TAU).sin() * 0.75 + noise(&mut seed) * 0.25) * env * gain
        })
        .collect()
}

fn click(duration: f32, gain: f32, mut seed: u32) -> Vec<f32> {
    (0..samples(duration))
        .map(|i| noise(&mut seed) * (-(i as f32) / 180.0).exp() * gain)
        .collect()
}

fn mechanism(forward: bool) -> Vec<f32> {
    let mut out = vec![0.0; samples(0.13)];
    let first = if forward { 0 } else { samples(0.055) };
    let second = if forward { samples(0.065) } else { 0 };
    add_click(&mut out, first, 0.23, 307);
    add_click(&mut out, second, 0.16, 331);
    out
}

fn reload() -> Vec<f32> {
    let mut out = vec![0.0; samples(0.48)];
    add_click(&mut out, 0, 0.24, 353);
    add_click(&mut out, samples(0.19), 0.18, 367);
    add_click(&mut out, samples(0.38), 0.28, 389);
    out
}

fn add_click(out: &mut [f32], start: usize, gain: f32, seed: u32) {
    for (dst, src) in out[start..].iter_mut().zip(click(0.045, gain, seed)) {
        *dst = (*dst + src).clamp(-1.0, 1.0);
    }
}

fn sweep(duration: f32, from: f32, to: f32, gain: f32) -> Vec<f32> {
    let count = samples(duration);
    let mut phase = 0.0;
    (0..count)
        .map(|i| {
            let f = i as f32 / count as f32;
            phase += (from + (to - from) * f) * core::f32::consts::TAU / RATE as f32;
            phase.sin() * (1.0 - f) * gain
        })
        .collect()
}

fn noise_burst(duration: f32, gain: f32, mut seed: u32) -> Vec<f32> {
    let count = samples(duration);
    (0..count)
        .map(|i| noise(&mut seed) * (1.0 - i as f32 / count as f32) * gain)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_clips_are_finite_and_in_range() {
        let clips = Clips::new();
        let sounds = [
            Sound::Gun(0),
            Sound::Gun(1),
            Sound::Gun(2),
            Sound::Gun(3),
            Sound::Footstep(0),
            Sound::Footstep(1),
            Sound::WallRun,
            Sound::AimIn,
            Sound::AimOut,
            Sound::Reload(0),
            Sound::Reload(1),
            Sound::Reload(2),
            Sound::Reload(3),
            Sound::Land,
            Sound::Impact,
            Sound::Hit,
            Sound::Hurt,
            Sound::Death,
            Sound::Switch,
            Sound::Empty,
        ];
        for sound in sounds {
            let (samples, volume) = clips.get(sound);
            assert!(!samples.is_empty());
            assert!(volume > 0.0 && volume <= 1.0);
            assert!(
                samples
                    .iter()
                    .all(|sample| sample.is_finite() && sample.abs() <= 1.0)
            );
        }
    }
}
