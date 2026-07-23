//! Small dependency-free LAN transport for the desktop build.
//!
//! The host relays compact player snapshots over UDP. Movement stays locally
//! predicted and each machine owns its player's health, which keeps the first
//! multiplayer implementation responsive without pulling networking into the
//! portable PSP/Vita simulation crate.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use glam::Vec3;

const MAGIC: &[u8; 4] = b"OSMP";
const VERSION: u8 = 5;
const HELLO: u8 = 1;
const WELCOME: u8 = 2;
const STATE: u8 = 3;
const SNAPSHOT: u8 = 4;
const REJECT: u8 = 5;
const START_MATCH: u8 = 6;
const MATCH_STATE: u8 = 7;
const MAX_PLAYERS: usize = 16;
const PEER_TIMEOUT: Duration = Duration::from_secs(5);
const HELLO_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone, Debug)]
pub struct PlayerState {
    pub id: u32,
    pub name: String,
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub shot_direction: Vec3,
    pub health: i32,
    pub alive: bool,
    pub crouched: bool,
    pub sliding: bool,
    pub wall_running: bool,
    pub on_ground: bool,
    pub moving: bool,
    pub aiming: bool,
    pub speed: f32,
    pub vertical_velocity: f32,
    pub wall_roll: f32,
    pub weapon: usize,
    pub shots: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct LocalState {
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub shot_direction: Vec3,
    pub health: i32,
    pub alive: bool,
    pub crouched: bool,
    pub sliding: bool,
    pub wall_running: bool,
    pub on_ground: bool,
    pub moving: bool,
    pub aiming: bool,
    pub speed: f32,
    pub vertical_velocity: f32,
    pub wall_roll: f32,
    pub weapon: usize,
    pub fired: bool,
}

#[derive(Clone, Debug)]
pub struct RemoteShot {
    pub shooter: PlayerState,
}

#[derive(Default)]
pub struct NetworkUpdate {
    pub players: Vec<PlayerState>,
    pub shots: Vec<RemoteShot>,
    pub local_id: Option<u32>,
    pub connected_players: usize,
    pub start_match: bool,
    pub round: u32,
    pub round_live: bool,
    pub winner_id: Option<u32>,
    pub reset_round: bool,
    pub scores: Vec<(u32, u32)>,
}

#[derive(Default)]
struct MatchSync {
    started: bool,
    round: u32,
    live: bool,
    winner_id: u32,
    scores: HashMap<u32, u32>,
    reset_pending: bool,
    ended_at: Option<Instant>,
    round_started_at: Option<Instant>,
}

#[derive(Clone, Debug)]
pub struct LobbyInfo {
    pub name: String,
    pub address: SocketAddr,
    pub map: String,
    pub players: usize,
}

#[derive(Clone, Copy, Debug)]
pub enum LobbyRequest {
    Host,
    LocalTest,
    Join(usize),
    StartMatch,
    Leave,
}

struct Peer {
    id: u32,
    name: String,
    state: Option<PlayerState>,
    last_seq: u32,
    last_seen: Instant,
}

enum Role {
    Host {
        peers: HashMap<SocketAddr, Peer>,
        next_id: u32,
        discovery: UdpSocket,
        match_sync: MatchSync,
    },
    Client {
        server: SocketAddr,
        id: Option<u32>,
        last_hello: Option<Instant>,
        players: Vec<PlayerState>,
        match_sync: MatchSync,
    },
}

pub struct NetworkSession {
    socket: UdpSocket,
    role: Role,
    map_key: u64,
    map_name: String,
    name: String,
    sequence: u32,
    shots: u32,
    observed_shots: HashMap<u32, u32>,
    start_requested: bool,
}

impl NetworkSession {
    pub fn set_name(&mut self, name: String) {
        self.name = clean_name(name);
    }

    pub fn host(bind: SocketAddr, map: &str, name: String) -> Result<Self> {
        let socket = UdpSocket::bind(bind)
            .with_context(|| format!("could not host multiplayer on {bind}"))?;
        socket.set_nonblocking(true)?;
        let game_port = socket.local_addr()?.port();
        let discovery = UdpSocket::bind(("0.0.0.0", discovery_port(game_port)))
            .context("could not bind the LAN lobby discovery port")?;
        discovery.set_nonblocking(true)?;
        log::info!("multiplayer host listening on {}", socket.local_addr()?);
        Ok(Self {
            socket,
            role: Role::Host {
                peers: HashMap::new(),
                next_id: 2,
                discovery,
                match_sync: MatchSync::default(),
            },
            map_key: map_key(map),
            map_name: map.to_owned(),
            name: clean_name(name),
            sequence: 0,
            shots: 0,
            observed_shots: HashMap::new(),
            start_requested: false,
        })
    }

    pub fn join(server: &str, default_port: u16, map: &str, name: String) -> Result<Self> {
        let address = if server.contains(':') {
            server.to_owned()
        } else {
            format!("{server}:{default_port}")
        };
        let server = address
            .to_socket_addrs()
            .with_context(|| format!("could not resolve multiplayer host {address}"))?
            .find(|addr| addr.is_ipv4())
            .context("multiplayer host did not resolve to an IPv4 address")?;
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.set_nonblocking(true)?;
        log::info!("joining multiplayer host {server}");
        Ok(Self {
            socket,
            role: Role::Client {
                server,
                id: None,
                last_hello: None,
                players: Vec::new(),
                match_sync: MatchSync::default(),
            },
            map_key: map_key(map),
            map_name: map.to_owned(),
            name: clean_name(name),
            sequence: 0,
            shots: 0,
            observed_shots: HashMap::new(),
            start_requested: false,
        })
    }

    pub fn join_addr(server: SocketAddr, map: &str, name: String) -> Result<Self> {
        Self::join(&server.to_string(), server.port(), map, name)
    }

    pub fn request_start(&mut self) {
        if matches!(self.role, Role::Host { .. }) {
            self.start_requested = true;
        }
    }

    pub fn tick(&mut self, local: LocalState) -> NetworkUpdate {
        if local.fired {
            self.shots = self.shots.wrapping_add(1);
        }
        self.sequence = self.sequence.wrapping_add(1);
        let now = Instant::now();
        let mut shot_states = Vec::new();

        match &mut self.role {
            Role::Host {
                peers,
                next_id,
                discovery,
                match_sync,
            } => {
                respond_to_discovery(
                    discovery,
                    self.socket.local_addr().map_or(27015, |addr| addr.port()),
                    &self.name,
                    peers.len() + 1,
                    &self.map_name,
                );
                receive_host(
                    &self.socket,
                    self.map_key,
                    peers,
                    next_id,
                    now,
                    &mut shot_states,
                );
                peers.retain(|addr, peer| {
                    let keep = now.duration_since(peer.last_seen) <= PEER_TIMEOUT;
                    if !keep {
                        log::info!("multiplayer player '{}' ({addr}) timed out", peer.name);
                    }
                    keep
                });

                let host = player_from_local(1, &self.name, self.shots, local);
                let mut all = vec![host];
                all.extend(peers.values().filter_map(|peer| peer.state.clone()));
                all.sort_by_key(|player| player.id);
                if self.start_requested && !match_sync.started {
                    match_sync.started = true;
                    match_sync.round = 1;
                    match_sync.live = true;
                    match_sync.winner_id = 0;
                    match_sync.reset_pending = true;
                    match_sync.round_started_at = Some(now);
                }
                let spawn_grace_over = match_sync.round_started_at.is_some_and(|started| {
                    now.duration_since(started) >= Duration::from_millis(500)
                });
                if match_sync.live && spawn_grace_over && all.len() >= 2 {
                    if let Some(dead) = all.iter().find(|player| !player.alive) {
                        let winner = all
                            .iter()
                            .find(|player| player.alive && player.id != dead.id)
                            .map_or(0, |player| player.id);
                        match_sync.live = false;
                        match_sync.winner_id = winner;
                        match_sync.ended_at = Some(now);
                        if winner != 0 {
                            *match_sync.scores.entry(winner).or_default() += 1;
                        }
                    }
                } else if match_sync.started
                    && !match_sync.live
                    && match_sync.ended_at.is_some_and(|ended| {
                        now.duration_since(ended) >= Duration::from_secs_f32(3.5)
                    })
                {
                    match_sync.round = match_sync.round.saturating_add(1);
                    match_sync.live = true;
                    match_sync.winner_id = 0;
                    match_sync.ended_at = None;
                    match_sync.reset_pending = true;
                    match_sync.round_started_at = Some(now);
                }
                let packet = encode_snapshot(&all);
                let match_packet = encode_match_state(match_sync);
                for addr in peers.keys() {
                    let _ = self.socket.send_to(&packet, addr);
                    let _ = self.socket.send_to(&match_packet, addr);
                    if self.start_requested {
                        let _ = self.socket.send_to(&super_packet(START_MATCH), addr);
                    }
                }
                NetworkUpdate {
                    players: all.into_iter().filter(|player| player.id != 1).collect(),
                    shots: shot_states
                        .into_iter()
                        .map(|shooter| RemoteShot { shooter })
                        .collect(),
                    local_id: Some(1),
                    connected_players: peers.len() + 1,
                    start_match: match_sync.started,
                    round: match_sync.round,
                    round_live: match_sync.live,
                    winner_id: (match_sync.winner_id != 0).then_some(match_sync.winner_id),
                    reset_round: core::mem::take(&mut match_sync.reset_pending),
                    scores: sorted_scores(&match_sync.scores),
                }
            }
            Role::Client {
                server,
                id,
                last_hello,
                players,
                match_sync,
            } => {
                if id.is_none()
                    && last_hello.is_none_or(|sent| now.duration_since(sent) >= HELLO_INTERVAL)
                {
                    let _ = self
                        .socket
                        .send_to(&encode_hello(self.map_key, &self.name), *server);
                    *last_hello = Some(now);
                }

                receive_client(&self.socket, *server, id, players, match_sync);
                if let Some(player_id) = *id {
                    let state = player_from_local(player_id, &self.name, self.shots, local);
                    let _ = self
                        .socket
                        .send_to(&encode_state(self.sequence, &state), *server);
                }
                let local_id = *id;
                let visible: Vec<_> = players
                    .iter()
                    .filter(|player| Some(player.id) != local_id)
                    .cloned()
                    .collect();
                for player in &visible {
                    observe_shots(&mut self.observed_shots, player, &mut shot_states);
                }
                NetworkUpdate {
                    players: visible,
                    shots: shot_states
                        .into_iter()
                        .map(|shooter| RemoteShot { shooter })
                        .collect(),
                    local_id: *id,
                    connected_players: if id.is_some() { players.len() } else { 0 },
                    start_match: match_sync.started,
                    round: match_sync.round,
                    round_live: match_sync.live,
                    winner_id: (match_sync.winner_id != 0).then_some(match_sync.winner_id),
                    reset_round: core::mem::take(&mut match_sync.reset_pending),
                    scores: sorted_scores(&match_sync.scores),
                }
            }
        }
    }
}

/// Broadcast browser used by the in-game desktop lobby before a connection
/// exists. Hosts answer from the adjacent discovery port and advertise their
/// gameplay address, so joining needs no IP entry.
pub struct LobbyBrowser {
    socket: UdpSocket,
    discovery_port: u16,
    last_scan: Option<Instant>,
    hosts: HashMap<SocketAddr, (LobbyInfo, Instant)>,
}

impl LobbyBrowser {
    pub fn new(game_port: u16) -> Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.set_nonblocking(true)?;
        socket.set_broadcast(true)?;
        Ok(Self {
            socket,
            discovery_port: discovery_port(game_port),
            last_scan: None,
            hosts: HashMap::new(),
        })
    }

    pub fn tick(&mut self) -> Vec<LobbyInfo> {
        let now = Instant::now();
        if self
            .last_scan
            .is_none_or(|sent| now.duration_since(sent) >= Duration::from_secs(1))
        {
            let request = discovery_packet(1, &[]);
            let _ = self
                .socket
                .send_to(&request, ("255.255.255.255", self.discovery_port));
            // Also makes discovery deterministic for two local development
            // instances without changing the real LAN behavior.
            let _ = self
                .socket
                .send_to(&request, ("127.0.0.1", self.discovery_port));
            self.last_scan = Some(now);
        }
        let mut buf = [0u8; 512];
        loop {
            let (len, source) = match self.socket.recv_from(&mut buf) {
                Ok(value) => value,
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) => {
                    log::warn!("LAN lobby discovery receive failed: {e}");
                    break;
                }
            };
            if let Some(mut lobby) = decode_discovery_response(&buf[..len], source) {
                lobby.address.set_ip(source.ip());
                self.hosts.insert(lobby.address, (lobby, now));
            }
        }
        self.hosts
            .retain(|_, (_, seen)| now.duration_since(*seen) < Duration::from_secs(4));
        let mut result: Vec<_> = self
            .hosts
            .values()
            .map(|(lobby, _)| lobby.clone())
            .collect();
        result.sort_by(|a, b| a.name.cmp(&b.name).then(a.address.cmp(&b.address)));
        result.dedup_by(|a, b| {
            a.name == b.name && a.map == b.map && a.address.port() == b.address.port()
        });
        result
    }
}

const DISCOVERY_MAGIC: &[u8; 4] = b"OSLB";

fn discovery_port(game_port: u16) -> u16 {
    game_port.checked_add(1).unwrap_or(27016)
}

fn discovery_packet(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + body.len());
    out.extend_from_slice(DISCOVERY_MAGIC);
    out.push(VERSION);
    out.push(kind);
    out.extend_from_slice(body);
    out
}

fn respond_to_discovery(socket: &UdpSocket, game_port: u16, name: &str, players: usize, map: &str) {
    let mut buf = [0u8; 512];
    loop {
        let (len, source) = match socket.recv_from(&mut buf) {
            Ok(value) => value,
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) => {
                log::warn!("LAN lobby discovery failed: {e}");
                break;
            }
        };
        if len < 6 || &buf[..4] != DISCOVERY_MAGIC || buf[4] != VERSION || buf[5] != 1 {
            continue;
        }
        let mut body = game_port.to_le_bytes().to_vec();
        body.push(players.min(MAX_PLAYERS) as u8);
        put_string(&mut body, name);
        put_string(&mut body, map);
        let _ = socket.send_to(&discovery_packet(2, &body), source);
    }
}

fn decode_discovery_response(data: &[u8], source: SocketAddr) -> Option<LobbyInfo> {
    if data.len() < 9 || &data[..4] != DISCOVERY_MAGIC || data[4] != VERSION || data[5] != 2 {
        return None;
    }
    let mut at = 6;
    let game_port = u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?);
    at += 2;
    let players = take_u8(data, &mut at)? as usize;
    let name = take_string(data, &mut at)?;
    let map = take_string(data, &mut at)?;
    Some(LobbyInfo {
        name,
        address: SocketAddr::new(source.ip(), game_port),
        map,
        players,
    })
}

fn receive_host(
    socket: &UdpSocket,
    expected_map: u64,
    peers: &mut HashMap<SocketAddr, Peer>,
    next_id: &mut u32,
    now: Instant,
    shots: &mut Vec<PlayerState>,
) {
    let mut buf = [0u8; 1500];
    loop {
        let (len, addr) = match socket.recv_from(&mut buf) {
            Ok(received) => received,
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) => {
                log::warn!("multiplayer receive failed: {e}");
                break;
            }
        };
        let Some(kind) = header(&buf[..len]) else {
            continue;
        };
        match kind {
            HELLO => {
                let Some((key, name)) = decode_hello(&buf[..len]) else {
                    continue;
                };
                if key != expected_map {
                    let _ = socket.send_to(&packet(REJECT, &[1]), addr);
                    continue;
                }
                if !peers.contains_key(&addr) && peers.len() >= MAX_PLAYERS - 1 {
                    let _ = socket.send_to(&packet(REJECT, &[2]), addr);
                    continue;
                }
                let peer = peers.entry(addr).or_insert_with(|| {
                    let id = *next_id;
                    *next_id = next_id.wrapping_add(1).max(2);
                    log::info!("multiplayer player '{name}' joined as #{id} from {addr}");
                    Peer {
                        id,
                        name,
                        state: None,
                        last_seq: 0,
                        last_seen: now,
                    }
                });
                peer.last_seen = now;
                let _ = socket.send_to(&packet(WELCOME, &peer.id.to_le_bytes()), addr);
            }
            STATE => {
                let Some((seq, mut state)) = decode_state(&buf[..len]) else {
                    continue;
                };
                let Some(peer) = peers.get_mut(&addr) else {
                    continue;
                };
                if state.id != peer.id || !newer(seq, peer.last_seq) {
                    continue;
                }
                state.name = peer.name.clone();
                if let Some(previous) = &peer.state {
                    let count = state.shots.wrapping_sub(previous.shots).min(4);
                    shots.extend((0..count).map(|_| state.clone()));
                }
                peer.last_seq = seq;
                peer.last_seen = now;
                peer.state = Some(state);
            }
            _ => {}
        }
    }
}

fn receive_client(
    socket: &UdpSocket,
    server: SocketAddr,
    id: &mut Option<u32>,
    players: &mut Vec<PlayerState>,
    match_sync: &mut MatchSync,
) {
    let mut buf = [0u8; 1500];
    loop {
        let (len, addr) = match socket.recv_from(&mut buf) {
            Ok(received) => received,
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) => {
                log::warn!("multiplayer receive failed: {e}");
                break;
            }
        };
        if addr != server {
            continue;
        }
        match header(&buf[..len]) {
            Some(WELCOME) if len >= 10 => {
                let joined = u32::from_le_bytes(buf[6..10].try_into().unwrap());
                if id.is_none() {
                    log::info!("joined multiplayer as player #{joined}");
                }
                *id = Some(joined);
            }
            Some(SNAPSHOT) => {
                if let Some(snapshot) = decode_snapshot(&buf[..len]) {
                    *players = snapshot;
                }
            }
            Some(REJECT) if len >= 7 => match buf[6] {
                1 => log::error!("multiplayer host rejected this client: map does not match"),
                2 => log::error!("multiplayer host is full"),
                _ => log::error!("multiplayer host rejected this client"),
            },
            Some(START_MATCH) => match_sync.started = true,
            Some(MATCH_STATE) => decode_match_state(&buf[..len], match_sync),
            _ => {}
        }
    }
}

fn observe_shots(
    observed: &mut HashMap<u32, u32>,
    player: &PlayerState,
    shots: &mut Vec<PlayerState>,
) {
    match observed.insert(player.id, player.shots) {
        Some(previous) => {
            let count = player.shots.wrapping_sub(previous).min(4);
            shots.extend((0..count).map(|_| player.clone()));
        }
        None => {}
    }
}

fn player_from_local(id: u32, name: &str, shots: u32, state: LocalState) -> PlayerState {
    PlayerState {
        id,
        name: name.to_owned(),
        pos: state.pos,
        yaw: state.yaw,
        pitch: state.pitch,
        shot_direction: state.shot_direction,
        health: state.health,
        alive: state.alive,
        crouched: state.crouched,
        sliding: state.sliding,
        wall_running: state.wall_running,
        on_ground: state.on_ground,
        moving: state.moving,
        aiming: state.aiming,
        speed: state.speed,
        vertical_velocity: state.vertical_velocity,
        wall_roll: state.wall_roll,
        weapon: state.weapon,
        shots,
    }
}

fn encode_hello(map: u64, name: &str) -> Vec<u8> {
    let mut body = map.to_le_bytes().to_vec();
    put_string(&mut body, name);
    packet(HELLO, &body)
}

fn decode_hello(data: &[u8]) -> Option<(u64, String)> {
    let mut at = 6;
    let key = take_u64(data, &mut at)?;
    let name = take_string(data, &mut at)?;
    Some((key, clean_name(name)))
}

fn encode_state(sequence: u32, state: &PlayerState) -> Vec<u8> {
    let mut body = sequence.to_le_bytes().to_vec();
    put_player(&mut body, state);
    packet(STATE, &body)
}

fn decode_state(data: &[u8]) -> Option<(u32, PlayerState)> {
    let mut at = 6;
    let sequence = take_u32(data, &mut at)?;
    let state = take_player(data, &mut at)?;
    Some((sequence, state))
}

fn encode_snapshot(players: &[PlayerState]) -> Vec<u8> {
    let mut body = vec![players.len().min(MAX_PLAYERS) as u8];
    for player in players.iter().take(MAX_PLAYERS) {
        put_player(&mut body, player);
    }
    packet(SNAPSHOT, &body)
}

fn encode_match_state(state: &MatchSync) -> Vec<u8> {
    let mut body = vec![u8::from(state.started)];
    body.extend_from_slice(&state.round.to_le_bytes());
    body.push(u8::from(state.live));
    body.extend_from_slice(&state.winner_id.to_le_bytes());
    let scores = sorted_scores(&state.scores);
    body.push(scores.len().min(MAX_PLAYERS) as u8);
    for (id, score) in scores.into_iter().take(MAX_PLAYERS) {
        body.extend_from_slice(&id.to_le_bytes());
        body.extend_from_slice(&score.to_le_bytes());
    }
    packet(MATCH_STATE, &body)
}

fn decode_match_state(data: &[u8], state: &mut MatchSync) {
    let mut at = 6;
    let Some(started) = take_u8(data, &mut at) else {
        return;
    };
    let Some(round) = take_u32(data, &mut at) else {
        return;
    };
    let Some(live) = take_u8(data, &mut at) else {
        return;
    };
    let Some(winner_id) = take_u32(data, &mut at) else {
        return;
    };
    let Some(count) = take_u8(data, &mut at) else {
        return;
    };
    if count as usize > MAX_PLAYERS {
        return;
    }
    let mut scores = HashMap::new();
    for _ in 0..count {
        let Some(id) = take_u32(data, &mut at) else {
            return;
        };
        let Some(score) = take_u32(data, &mut at) else {
            return;
        };
        scores.insert(id, score);
    }
    if round > state.round && live != 0 {
        state.reset_pending = true;
    }
    state.started = started != 0;
    state.round = round;
    state.live = live != 0;
    state.winner_id = winner_id;
    state.scores = scores;
}

fn sorted_scores(scores: &HashMap<u32, u32>) -> Vec<(u32, u32)> {
    let mut values: Vec<_> = scores.iter().map(|(id, score)| (*id, *score)).collect();
    values.sort_by_key(|(id, _)| *id);
    values
}

fn decode_snapshot(data: &[u8]) -> Option<Vec<PlayerState>> {
    let count = *data.get(6)? as usize;
    if count > MAX_PLAYERS {
        return None;
    }
    let mut at = 7;
    (0..count).map(|_| take_player(data, &mut at)).collect()
}

fn put_player(out: &mut Vec<u8>, state: &PlayerState) {
    out.extend_from_slice(&state.id.to_le_bytes());
    for value in [
        state.pos.x,
        state.pos.y,
        state.pos.z,
        state.yaw,
        state.pitch,
        state.shot_direction.x,
        state.shot_direction.y,
        state.shot_direction.z,
    ] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&(state.health.clamp(0, i16::MAX as i32) as i16).to_le_bytes());
    let flags = u8::from(state.alive)
        | (u8::from(state.crouched) << 1)
        | (u8::from(state.moving) << 2)
        | (u8::from(state.sliding) << 3)
        | (u8::from(state.wall_running) << 4)
        | (u8::from(state.on_ground) << 5)
        | (u8::from(state.aiming) << 6);
    out.push(flags);
    out.push(state.weapon.min(255) as u8);
    out.extend_from_slice(&state.shots.to_le_bytes());
    out.extend_from_slice(&state.speed.to_le_bytes());
    out.extend_from_slice(&state.vertical_velocity.to_le_bytes());
    out.extend_from_slice(&state.wall_roll.to_le_bytes());
    put_string(out, &state.name);
}

fn take_player(data: &[u8], at: &mut usize) -> Option<PlayerState> {
    let id = take_u32(data, at)?;
    let pos = Vec3::new(
        take_f32(data, at)?,
        take_f32(data, at)?,
        take_f32(data, at)?,
    );
    let yaw = take_f32(data, at)?;
    let pitch = take_f32(data, at)?;
    let shot_direction = Vec3::new(
        take_f32(data, at)?,
        take_f32(data, at)?,
        take_f32(data, at)?,
    );
    let health = take_i16(data, at)? as i32;
    let flags = take_u8(data, at)?;
    let weapon = take_u8(data, at)? as usize;
    let shots = take_u32(data, at)?;
    let speed = take_f32(data, at)?;
    let vertical_velocity = take_f32(data, at)?;
    let wall_roll = take_f32(data, at)?;
    let name = take_string(data, at)?;
    if !pos.is_finite()
        || !yaw.is_finite()
        || !pitch.is_finite()
        || !shot_direction.is_finite()
        || !speed.is_finite()
        || !vertical_velocity.is_finite()
        || !wall_roll.is_finite()
    {
        return None;
    }
    Some(PlayerState {
        id,
        name,
        pos,
        yaw,
        pitch,
        shot_direction,
        health,
        alive: flags & 1 != 0,
        crouched: flags & 2 != 0,
        moving: flags & 4 != 0,
        aiming: flags & 64 != 0,
        sliding: flags & 8 != 0,
        wall_running: flags & 16 != 0,
        on_ground: flags & 32 != 0,
        speed,
        vertical_velocity,
        wall_roll,
        weapon,
        shots,
    })
}

fn packet(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + body.len());
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(kind);
    out.extend_from_slice(body);
    out
}

fn super_packet(kind: u8) -> Vec<u8> {
    packet(kind, &[])
}

fn header(data: &[u8]) -> Option<u8> {
    (data.len() >= 6 && &data[..4] == MAGIC && data[4] == VERSION).then_some(data[5])
}

fn put_string(out: &mut Vec<u8>, value: &str) {
    let bytes = value.as_bytes();
    let len = bytes.len().min(24);
    out.push(len as u8);
    out.extend_from_slice(&bytes[..len]);
}

fn take_string(data: &[u8], at: &mut usize) -> Option<String> {
    let len = take_u8(data, at)? as usize;
    let bytes = data.get(*at..*at + len)?;
    *at += len;
    Some(String::from_utf8_lossy(bytes).into_owned())
}

fn take_u8(data: &[u8], at: &mut usize) -> Option<u8> {
    let value = *data.get(*at)?;
    *at += 1;
    Some(value)
}

fn take_u32(data: &[u8], at: &mut usize) -> Option<u32> {
    let value = u32::from_le_bytes(data.get(*at..*at + 4)?.try_into().ok()?);
    *at += 4;
    Some(value)
}

fn take_u64(data: &[u8], at: &mut usize) -> Option<u64> {
    let value = u64::from_le_bytes(data.get(*at..*at + 8)?.try_into().ok()?);
    *at += 8;
    Some(value)
}

fn take_i16(data: &[u8], at: &mut usize) -> Option<i16> {
    let value = i16::from_le_bytes(data.get(*at..*at + 2)?.try_into().ok()?);
    *at += 2;
    Some(value)
}

fn take_f32(data: &[u8], at: &mut usize) -> Option<f32> {
    let value = f32::from_le_bytes(data.get(*at..*at + 4)?.try_into().ok()?);
    *at += 4;
    Some(value)
}

fn newer(sequence: u32, previous: u32) -> bool {
    sequence != previous && sequence.wrapping_sub(previous) < (u32::MAX / 2)
}

fn map_key(map: &str) -> u64 {
    map.as_bytes()
        .iter()
        .fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
}

fn clean_name(name: String) -> String {
    let trimmed: String = name.chars().filter(|c| !c.is_control()).take(24).collect();
    if trimmed.trim().is_empty() {
        "Player".into()
    } else {
        trimmed
    }
}

pub fn host_addr(port: u16) -> SocketAddr {
    SocketAddr::from(([0, 0, 0, 0], port))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> PlayerState {
        PlayerState {
            id: 7,
            name: "Mac Player".into(),
            pos: Vec3::new(1.25, -2.5, 99.0),
            yaw: 0.75,
            pitch: -0.2,
            shot_direction: Vec3::new(0.25, -0.5, -0.75).normalize(),
            health: 83,
            alive: true,
            crouched: false,
            sliding: false,
            wall_running: false,
            on_ground: true,
            moving: true,
            aiming: true,
            speed: 220.0,
            vertical_velocity: 0.0,
            wall_roll: 0.0,
            weapon: 2,
            shots: 42,
        }
    }

    fn local(pos: Vec3, fired: bool) -> LocalState {
        LocalState {
            pos,
            yaw: 0.0,
            pitch: 0.0,
            shot_direction: Vec3::NEG_Z,
            health: 100,
            alive: true,
            crouched: false,
            sliding: false,
            wall_running: false,
            on_ground: true,
            moving: true,
            aiming: false,
            speed: 220.0,
            vertical_velocity: 0.0,
            wall_roll: 0.0,
            weapon: 0,
            fired,
        }
    }

    #[test]
    fn player_snapshot_round_trips() {
        let packet = encode_snapshot(&[state()]);
        let decoded = decode_snapshot(&packet).unwrap();
        assert_eq!(decoded.len(), 1);
        let got = &decoded[0];
        assert_eq!(got.id, 7);
        assert_eq!(got.name, "Mac Player");
        assert_eq!(got.pos, Vec3::new(1.25, -2.5, 99.0));
        assert_eq!(got.shot_direction, state().shot_direction);
        assert_eq!(got.health, 83);
        assert!(got.alive && got.moving && !got.crouched);
        assert!(got.aiming);
        assert!(got.on_ground && !got.sliding && !got.wall_running);
        assert_eq!(got.speed, 220.0);
        assert_eq!(got.vertical_velocity, 0.0);
        assert_eq!(got.weapon, 2);
        assert_eq!(got.shots, 42);
    }

    #[test]
    fn rejects_truncated_snapshot() {
        let packet = encode_snapshot(&[state()]);
        assert!(decode_snapshot(&packet[..packet.len() - 1]).is_none());
    }

    #[test]
    fn sequence_comparison_handles_wrap() {
        assert!(newer(1, u32::MAX));
        assert!(!newer(9, 9));
        assert!(!newer(4, 10));
    }

    #[test]
    fn localhost_host_and_client_exchange_state_and_shots() {
        let mut host =
            NetworkSession::host("127.0.0.1:0".parse().unwrap(), "de_dust2", "Host".into())
                .unwrap();
        let port = host.socket.local_addr().unwrap().port();
        let mut browser = LobbyBrowser::new(port).unwrap();
        browser.tick();
        host.tick(local(Vec3::ZERO, false));
        std::thread::sleep(Duration::from_millis(2));
        let discovered = browser.tick();
        assert!(discovered.iter().any(|lobby| {
            lobby.name == "Host" && lobby.map == "de_dust2" && lobby.address.port() == port
        }));

        let mut client = NetworkSession::join(
            &format!("127.0.0.1:{port}"),
            port,
            "de_dust2",
            "Client".into(),
        )
        .unwrap();

        let mut host_update = NetworkUpdate::default();
        let mut client_update = NetworkUpdate::default();
        for step in 0..30 {
            client_update = client.tick(local(Vec3::new(20.0, 0.0, 0.0), step == 12));
            host_update = host.tick(local(Vec3::ZERO, false));
            std::thread::sleep(Duration::from_millis(2));
        }

        assert_eq!(host_update.players.len(), 1);
        assert_eq!(host_update.players[0].name, "Client");
        assert_eq!(client_update.players.len(), 1);
        assert_eq!(client_update.players[0].name, "Host");
        // The shot is edge-triggered and may have arrived in an earlier
        // update, so its monotonic counter is the durable assertion here.
        assert_eq!(host_update.players[0].shots, 1);

        host.request_start();
        host.tick(local(Vec3::ZERO, false));
        std::thread::sleep(Duration::from_millis(2));
        let mut dead = local(Vec3::new(20.0, 0.0, 0.0), false);
        dead.alive = false;
        dead.health = 0;
        let started = client.tick(dead);
        assert!(started.start_match && started.reset_round);
        assert_eq!(started.round, 1);
        if let Role::Host { match_sync, .. } = &mut host.role {
            match_sync.round_started_at = Some(Instant::now() - Duration::from_secs(1));
        }
        std::thread::sleep(Duration::from_millis(2));
        let ended = host.tick(local(Vec3::ZERO, false));
        assert!(!ended.round_live);
        assert_eq!(ended.winner_id, Some(1));
        assert_eq!(ended.scores, vec![(1, 1)]);

        if let Role::Host { match_sync, .. } = &mut host.role {
            match_sync.ended_at = Some(Instant::now() - Duration::from_secs(4));
        }
        let reset = host.tick(local(Vec3::ZERO, false));
        assert!(reset.reset_round && reset.round_live);
        assert_eq!(reset.round, 2);
    }
}
