//! Tiny hand-rolled CLI parsing (no deps).

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use glam::Vec3;

pub struct Args {
    /// Directory containing maps (maps/*.bsp + support/*.wad, or flat).
    pub maps_dir: Option<PathBuf>,
    /// Map name (de_dust2) or explicit path to a .bsp.
    pub map: String,
    pub screenshot: Option<String>,
    pub size: (u32, u32),
    pub pos: Option<Vec3>,
    pub yaw_deg: Option<f32>,
    pub pitch_deg: Option<f32>,
    pub spawn_index: usize,
    pub spawn_t: bool,
    /// Headless scripted run (acceptance tests): walk, ...
    pub script: Option<String>,
    pub debug_overlay: bool,
    /// Number of enemy bots per round.
    pub bots: usize,
    /// Quit the windowed app after N seconds (CI smoke test).
    pub auto_quit: Option<f32>,
    /// Screenshot mode: stage a combat frame with the JSX HUD composited
    /// (boots the guest, spawns bots, fires). Plain mode is a clean vista.
    pub hud: bool,
    /// Run a LAN server in this desktop process.
    pub host: bool,
    /// Join a LAN server by hostname/IP (optional :port).
    pub join: Option<String>,
    pub port: u16,
    pub player_name: String,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            maps_dir: None,
            map: "de_dust2".into(),
            screenshot: None,
            size: (1280, 720),
            pos: None,
            yaw_deg: None,
            pitch_deg: None,
            spawn_index: 0,
            spawn_t: false,
            script: None,
            debug_overlay: false,
            bots: 0,
            auto_quit: None,
            hud: false,
            host: false,
            join: None,
            port: 27015,
            player_name: std::env::var("USER").unwrap_or_else(|_| "Player".into()),
        }
    }
}

impl Args {
    pub fn parse() -> Result<Self> {
        let mut a = Args::default();
        let mut it = std::env::args().skip(1);
        while let Some(arg) = it.next() {
            let mut value = |name: &str| -> Result<String> {
                it.next().with_context(|| format!("{name} needs a value"))
            };
            match arg.as_str() {
                "--maps-dir" => a.maps_dir = Some(PathBuf::from(value("--maps-dir")?)),
                "--map" => a.map = value("--map")?,
                "--screenshot" => a.screenshot = Some(value("--screenshot")?),
                "--size" => {
                    let v = value("--size")?;
                    let (w, h) = v.split_once('x').context("--size expects WxH")?;
                    a.size = (w.parse()?, h.parse()?);
                }
                "--pos" => {
                    let v = value("--pos")?;
                    let parts: Vec<f32> = v
                        .split(',')
                        .map(|s| s.trim().parse::<f32>())
                        .collect::<Result<_, _>>()
                        .context("--pos expects x,y,z")?;
                    if parts.len() != 3 {
                        bail!("--pos expects x,y,z");
                    }
                    a.pos = Some(Vec3::new(parts[0], parts[1], parts[2]));
                }
                "--yaw" => a.yaw_deg = Some(value("--yaw")?.parse()?),
                "--pitch" => a.pitch_deg = Some(value("--pitch")?.parse()?),
                "--spawn" => a.spawn_index = value("--spawn")?.parse()?,
                "--spawn-t" => a.spawn_t = true,
                "--script" => a.script = Some(value("--script")?),
                "--debug" => a.debug_overlay = true,
                "--bots" => a.bots = value("--bots")?.parse()?,
                "--auto-quit" => a.auto_quit = Some(value("--auto-quit")?.parse()?),
                "--hud" => a.hud = true,
                "--host" => a.host = true,
                "--join" => a.join = Some(value("--join")?),
                "--port" => a.port = value("--port")?.parse()?,
                "--name" => a.player_name = value("--name")?,
                "--help" | "-h" => {
                    println!("{USAGE}");
                    std::process::exit(0);
                }
                other => bail!("unknown argument: {other}\n{USAGE}"),
            }
        }
        if a.host && a.join.is_some() {
            bail!("--host and --join cannot be used together");
        }
        if a.port == 0 {
            bail!("--port must be between 1 and 65535");
        }
        Ok(a)
    }

    /// Resolve the map argument to a .bsp path.
    pub fn resolve_map_path(&self) -> Result<PathBuf> {
        let name = &self.map;
        if name.ends_with(".bsp") || name.contains('/') {
            let p = PathBuf::from(name);
            if p.exists() {
                return Ok(p);
            }
            bail!("map not found: {name}");
        }
        let mut roots: Vec<PathBuf> = Vec::new();
        if let Some(d) = &self.maps_dir {
            roots.push(d.clone());
        }
        if let Some(env) = std::env::var_os("OPENSTRIKE_MAPS") {
            roots.push(PathBuf::from(env));
        }
        roots.push(PathBuf::from("assets"));
        roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
        for root in &roots {
            for candidate in [
                root.join("maps").join(format!("{name}.bsp")),
                root.join(format!("{name}.bsp")),
            ] {
                if candidate.exists() {
                    return Ok(candidate);
                }
            }
        }
        bail!(
            "could not find {name}.bsp — pass --maps-dir DIR or set OPENSTRIKE_MAPS \
             (expects DIR/maps/*.bsp with DIR/support/*.wad)"
        )
    }

    /// Discover the desktop map-selection catalogue. Bundled maps are added
    /// last so they take precedence over same-named external copies, matching
    /// the PSP/Vita packagers.
    pub fn map_catalogue(&self) -> Vec<(String, PathBuf)> {
        let mut maps = BTreeMap::<String, PathBuf>::new();
        let mut roots = Vec::new();
        if let Some(root) = &self.maps_dir {
            roots.push(root.clone());
        }
        if let Some(root) = std::env::var_os("OPENSTRIKE_MAPS") {
            roots.push(PathBuf::from(root));
        }
        roots.push(PathBuf::from("assets"));
        roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
        for root in roots {
            for directory in [root.join("maps"), root] {
                let Ok(entries) = std::fs::read_dir(directory) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|value| value.to_str()) != Some("bsp") {
                        continue;
                    }
                    let Some(name) = path.file_stem().and_then(|value| value.to_str()) else {
                        continue;
                    };
                    maps.insert(name.to_lowercase(), path);
                }
            }
        }
        maps.into_iter().collect()
    }

    /// Extra directories to scan for WADs (load_map also checks siblings).
    pub fn wad_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        for root in self
            .maps_dir
            .iter()
            .cloned()
            .chain(std::env::var_os("OPENSTRIKE_MAPS").map(PathBuf::from))
        {
            dirs.push(root.join("support"));
            dirs.push(root.join("wads"));
            dirs.push(root);
        }
        dirs
    }
}

/// Locate a bundled asset (models, etc.) across common run layouts.
pub fn find_asset(rel: &str) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(root) = std::env::var_os("OPENSTRIKE_ASSETS") {
        candidates.push(PathBuf::from(root).join(rel));
    }
    candidates.push(PathBuf::from("assets").join(rel));
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets")
            .join(rel),
    );
    candidates.into_iter().find(|p| p.exists())
}

const USAGE: &str = "\
openstrike [options]
  --maps-dir DIR      directory with maps/*.bsp and support/*.wad
  --map NAME          map name or .bsp path (default de_dust2)
  --screenshot PATH   render one frame headlessly and save a PNG
  --size WxH          render size (default 1280x720)
  --pos x,y,z         camera position (Y-up units)
  --yaw DEG           camera yaw
  --pitch DEG         camera pitch
  --spawn N           use the Nth CT spawn (default 0)
  --spawn-t           use a T spawn instead
  --script NAME       run a headless scripted test (walk, wallrun, model, combat, round, lose)
  --debug             show the debug overlay
  --bots N            enemy bot count per round (default 0)
  --host              host a LAN multiplayer game
  --join HOST[:PORT]  join a LAN multiplayer game
  --port PORT         host/default join port (default 27015)
  --name NAME         multiplayer display name";
