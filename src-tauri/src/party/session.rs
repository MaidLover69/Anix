//! A hosted party and a joined one, over TCP.
//!
//! Frames are a 2-byte big-endian length and the bytes. The guest opens with its SPAKE2
//! message; the host answers with a status byte (ok, full, locked) and its own; then every
//! frame is sealed (`secure.rs`) and holds one message (`protocol.rs`). Only one join is
//! handled at a time, so wrong codes are counted before the next attempt, and after
//! [`MAX_FAILED_JOINS`] the party takes nobody new.
//!
//! The host decides what plays and where. Play (and a seek while playing) goes in two steps:
//! `Prepare` asks every guest to seek there paused, the host waits until all answer `Ready`
//! (at most [`READY_WAIT`]), then a `State` starts everyone, including the host's own player,
//! [`START_LEAD_MS`] later, so the message has arrived everywhere by then. Pauses go out at
//! once, and while playing the host repeats its position so guests can correct drift.
//! Nothing is written to disk.

use super::lan::{self, Advertiser};
use super::protocol::{self, Load, Message, RateLimit, MAX_MESSAGE_BYTES, PROTOCOL_VERSION};
use super::secure::{self, Channel, Pake, SecureError, Side, PAKE_MESSAGE_BYTES, TAG_BYTES};
use super::sync::{self, Clock, ClockSample};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{sleep_until, timeout};

/// Guests a party takes.
pub const MAX_GUESTS: usize = 8;
/// Wrong codes before the party locks.
pub const MAX_FAILED_JOINS: u32 = 5;
/// Time for connecting and the whole join exchange.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// A peer silent for this long is gone (guests ping every [`PING_INTERVAL`]).
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(10);
pub const PING_INTERVAL: Duration = Duration::from_secs(2);
/// How long the host waits for guests to buffer before it plays anyway.
pub const READY_WAIT: Duration = Duration::from_secs(15);
/// How far ahead a start is scheduled.
pub const START_LEAD_MS: u64 = 1000;
/// Position repeats closer together than this are dropped.
const HEARTBEAT_MIN_MS: u64 = 500;
/// Reports closer together than this are merged into the latest one, so a held seek key
/// stays well under the guests' inbound [`RateLimit`].
const REPORT_GAP_MS: u64 = 200;
/// Connections still in the join exchange, in all and from one address.
const MAX_PRE_AUTH: usize = 32;
const MAX_PRE_AUTH_PER_IP: usize = 4;
/// Pause after a failed `accept` (out of file descriptors, a network blip) before the next.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(250);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const OUTBOX: usize = 64;
const WIRE_MAX: usize = MAX_MESSAGE_BYTES + TAG_BYTES;

const STATUS_OK: u8 = 0;
const STATUS_FULL: u8 = 1;
const STATUS_LOCKED: u8 = 2;

/// Why starting, joining or a request failed. The page shows `party.errors.<kind>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PartyError {
    #[error("No local network was found")]
    NoNetwork,
    #[error("No free port for the party")]
    PortsBusy,
    #[error("That is not a local network address")]
    BadAddress,
    #[error("The code has 6 digits")]
    BadCode,
    #[error("The host could not be reached")]
    Unreachable,
    #[error("Wrong code")]
    WrongCode,
    #[error("The party is full")]
    Full,
    #[error("The party is locked after too many wrong codes")]
    Locked,
    #[error("Not in a party")]
    NotInParty,
    #[error("Invalid value")]
    Invalid,
}

impl PartyError {
    /// Stable name, used as the UI's translation key.
    pub fn kind(&self) -> &'static str {
        match self {
            PartyError::NoNetwork => "no_network",
            PartyError::PortsBusy => "ports_busy",
            PartyError::BadAddress => "bad_address",
            PartyError::BadCode => "bad_code",
            PartyError::Unreachable => "unreachable",
            PartyError::WrongCode => "wrong_code",
            PartyError::Full => "full",
            PartyError::Locked => "locked",
            PartyError::NotInParty => "not_in_party",
            PartyError::Invalid => "invalid",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    #[default]
    None,
    Host,
    Guest,
}

/// What the party chip shows.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct PartyStatus {
    pub role: Role,
    /// The join code (host only).
    pub code: Option<String>,
    /// The host's addresses (`ip`), or the one a guest joined (`ip:port`).
    pub addresses: Vec<String>,
    pub port: u16,
    /// Guests connected (host only).
    pub guests: usize,
    pub locked: bool,
    /// The host waits for guests to buffer.
    pub waiting: bool,
    /// The fastest recent round trip to the host (guest only).
    pub rtt_ms: Option<u64>,
}

/// Where the host is, as the page applies it: `age_ms` ago the position was `position_ms`
/// (negative age: that moment is still ahead).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlayState {
    pub seq: u64,
    pub paused: bool,
    pub position_ms: u64,
    pub rate: f64,
    pub age_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    /// This side left.
    Left,
    /// The host ended the party.
    HostLeft,
    /// The connection was lost or the other side broke the protocol.
    Lost,
}

/// Sent to the page as the `party-event` event.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PartyEvent {
    Status(PartyStatus),
    Load(Load),
    Prepare { seq: u64, position_ms: u64 },
    State(PlayState),
    Ended { reason: EndReason },
}

pub type Events = mpsc::UnboundedSender<PartyEvent>;

async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, bytes: &[u8]) -> std::io::Result<()> {
    let len = u16::try_from(bytes.len()).map_err(|_| std::io::Error::other("frame too large"))?;
    let mut buf = Vec::with_capacity(bytes.len() + 2);
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(bytes);
    timeout(WRITE_TIMEOUT, writer.write_all(&buf)).await.map_err(|_| std::io::Error::other("write timed out"))?
}

async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R, max: usize) -> std::io::Result<Vec<u8>> {
    let mut len = [0u8; 2];
    reader.read_exact(&mut len).await?;
    let len = u16::from_be_bytes(len) as usize;
    if len > max {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    Ok(buf)
}

/// Reads frames into a channel, so the connection loop can wait on frames, its outbox and a
/// timer together without losing half a frame.
fn spawn_reader(mut reader: tokio::net::tcp::OwnedReadHalf) -> (mpsc::Receiver<Vec<u8>>, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(16);
    let task = tokio::spawn(async move {
        while let Ok(frame) = read_frame(&mut reader, WIRE_MAX).await {
            if tx.send(frame).await.is_err() {
                break;
            }
        }
    });
    (rx, task)
}

async fn send_sealed<W: AsyncWrite + Unpin>(writer: &mut W, channel: &mut Channel, message: &Message) -> bool {
    write_frame(writer, &channel.seal(&protocol::encode(message))).await.is_ok()
}

/// Opens and decodes one frame; None for anything that is not a valid message.
fn open_message(channel: &mut Channel, frame: &[u8]) -> Option<Message> {
    let plain = channel.open(frame).ok()?;
    protocol::decode(&plain).ok()
}

// ---------------------------------------------------------------------------------------
// Host

struct Pending {
    seq: u64,
    position_ms: u64,
    rate: f64,
    waiting: HashSet<u32>,
}

#[derive(Default)]
struct HostState {
    guests: HashMap<u32, mpsc::Sender<Message>>,
    next_id: u32,
    failures: u32,
    locked: bool,
    stopped: bool,
    load: Option<Load>,
    last: Option<Message>,
    seq: u64,
    pending: Option<Pending>,
    last_heartbeat_ms: u64,
    last_report_ms: Option<u64>,
    /// A report waiting for [`REPORT_GAP_MS`] to pass: (paused, position_ms, rate).
    queued_report: Option<(bool, u64, f64)>,
    /// Connections in the join exchange, by address.
    pre_auth: HashMap<IpAddr, usize>,
}

/// One connection's place among those in the join exchange; given back when dropped.
struct PreAuthSlot {
    shared: Arc<HostShared>,
    ip: IpAddr,
}

impl PreAuthSlot {
    fn take(shared: &Arc<HostShared>, ip: IpAddr) -> Option<PreAuthSlot> {
        let mut s = shared.lock();
        let total: usize = s.pre_auth.values().sum();
        let mine = s.pre_auth.get(&ip).copied().unwrap_or(0);
        if total >= MAX_PRE_AUTH || mine >= MAX_PRE_AUTH_PER_IP {
            return None;
        }
        *s.pre_auth.entry(ip).or_insert(0) += 1;
        Some(PreAuthSlot { shared: shared.clone(), ip })
    }
}

impl Drop for PreAuthSlot {
    fn drop(&mut self) {
        let mut s = self.shared.lock();
        if let Some(count) = s.pre_auth.get_mut(&self.ip) {
            *count -= 1;
            if *count == 0 {
                s.pre_auth.remove(&self.ip);
            }
        }
    }
}

struct HostShared {
    code: String,
    port: u16,
    addresses: Vec<String>,
    events: Events,
    state: Mutex<HostState>,
    /// Held during one join exchange.
    joining: tokio::sync::Mutex<()>,
}

impl HostShared {
    fn lock(&self) -> std::sync::MutexGuard<'_, HostState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn status_of(&self, s: &HostState) -> PartyStatus {
        PartyStatus {
            role: Role::Host,
            code: Some(self.code.clone()),
            addresses: self.addresses.clone(),
            port: self.port,
            guests: s.guests.len(),
            locked: s.locked,
            waiting: s.pending.as_ref().is_some_and(|p| !p.waiting.is_empty()),
            rtt_ms: None,
        }
    }

    fn emit_status(&self, s: &HostState) {
        let _ = self.events.send(PartyEvent::Status(self.status_of(s)));
    }

    /// Queues `message` for every guest; one whose outbox is full is dropped.
    fn broadcast(s: &mut HostState, message: &Message) {
        s.guests.retain(|_, tx| tx.try_send(message.clone()).is_ok());
    }

    fn remove_guest(&self, id: u32) {
        let mut s = self.lock();
        if s.guests.remove(&id).is_none() {
            return;
        }
        if let Some(p) = s.pending.as_mut() {
            p.waiting.remove(&id);
        }
        self.emit_status(&s);
    }

    fn mark_ready(&self, id: u32, seq: u64) {
        let mut s = self.lock();
        if let Some(p) = s.pending.as_mut().filter(|p| p.seq == seq) {
            p.waiting.remove(&id);
        }
    }

    /// Starts the prepared play `seq`, unless something newer replaced it.
    fn start_playing(&self, seq: u64) {
        let mut s = self.lock();
        if s.pending.as_ref().map(|p| p.seq) != Some(seq) {
            return;
        }
        let p = s.pending.take().expect("checked above");
        let lead = if s.guests.is_empty() { 0 } else { START_LEAD_MS };
        let state = Message::State { seq, paused: false, position_ms: p.position_ms, rate: p.rate, at_ms: sync::now_ms() + lead };
        HostShared::broadcast(&mut s, &state);
        s.last = Some(state);
        s.last_heartbeat_ms = sync::now_ms() + lead;
        let _ = self.events.send(PartyEvent::State(PlayState {
            seq,
            paused: false,
            position_ms: p.position_ms,
            rate: p.rate,
            age_ms: -(lead as i64),
        }));
        self.emit_status(&s);
    }
}

/// A party this machine hosts. Dropping it ends the party.
pub struct Host {
    shared: Arc<HostShared>,
    accept_tasks: Vec<JoinHandle<()>>,
    _advertiser: Option<Advertiser>,
}

impl Host {
    /// Hosts on the local network: listens on every LAN address (port [`lan::DEFAULT_PORT`]
    /// or the next free one) and announces the party over mDNS.
    pub async fn start(events: Events) -> Result<Host, PartyError> {
        let addresses = lan::lan_addresses();
        if addresses.is_empty() {
            return Err(PartyError::NoNetwork);
        }
        let ips: Vec<IpAddr> = addresses.iter().map(|(_, ip)| IpAddr::V4(*ip)).collect();
        let listeners = bind_all(&ips, lan::DEFAULT_PORT, lan::PORT_TRIES).await?;
        let port = listeners[0].local_addr().map_err(|_| PartyError::PortsBusy)?.port();
        let advertiser = Advertiser::start(port, &addresses)
            .map_err(|e| tracing::warn!("Watch party: mDNS announcement failed: {}", e))
            .ok();
        Ok(Host::launch(listeners, ips, port, advertiser, events))
    }

    /// Hosts on 127.0.0.1 with a port the system picks, without mDNS. For tests only: the app
    /// itself never listens on loopback.
    #[doc(hidden)]
    pub async fn start_allowing_loopback(events: Events) -> Result<Host, PartyError> {
        let ips = vec![IpAddr::V4(Ipv4Addr::LOCALHOST)];
        let listeners = bind_all(&ips, 0, 1).await?;
        let port = listeners[0].local_addr().map_err(|_| PartyError::PortsBusy)?.port();
        Ok(Host::launch(listeners, ips, port, None, events))
    }

    fn launch(listeners: Vec<TcpListener>, ips: Vec<IpAddr>, port: u16, advertiser: Option<Advertiser>, events: Events) -> Host {
        let shared = Arc::new(HostShared {
            code: secure::new_code(),
            port,
            addresses: ips.iter().map(IpAddr::to_string).collect(),
            events,
            state: Mutex::new(HostState::default()),
            joining: tokio::sync::Mutex::new(()),
        });
        let accept_tasks = listeners
            .into_iter()
            .map(|listener| {
                let shared = shared.clone();
                tokio::spawn(async move {
                    loop {
                        match listener.accept().await {
                            Ok((stream, peer)) => {
                                // Over the limit: dropping the stream closes it at once.
                                let Some(slot) = PreAuthSlot::take(&shared, peer.ip()) else { continue };
                                let _ = stream.set_nodelay(true);
                                tokio::spawn(handle_guest(shared.clone(), stream, slot));
                            }
                            // The listener stays: the error passes (fds freed, network back).
                            Err(e) => {
                                tracing::debug!("Watch party: accept failed: {}", e);
                                tokio::time::sleep(ACCEPT_BACKOFF).await;
                            }
                        }
                    }
                })
            })
            .collect();
        Host { shared, accept_tasks, _advertiser: advertiser }
    }

    pub fn code(&self) -> &str {
        &self.shared.code
    }

    pub fn port(&self) -> u16 {
        self.shared.port
    }

    pub fn status(&self) -> PartyStatus {
        let s = self.shared.lock();
        self.shared.status_of(&s)
    }

    pub fn load(&self) -> Option<Load> {
        self.shared.lock().load.clone()
    }

    /// A new episode (or what is known about it changed). Guests follow it.
    pub fn set_load(&self, load: Load) -> Result<(), PartyError> {
        protocol::validate_load(&load).map_err(|_| PartyError::Invalid)?;
        let mut s = self.shared.lock();
        if s.load.as_ref() == Some(&load) {
            return Ok(());
        }
        if !s.load.as_ref().is_some_and(|old| old.same_episode(&load)) {
            s.seq += 1;
            s.last = None;
            s.pending = None;
            s.queued_report = None;
        }
        HostShared::broadcast(&mut s, &Message::Load(load.clone()));
        s.load = Some(load);
        self.shared.emit_status(&s);
        Ok(())
    }

    /// The host's player was paused, or asked to play, at `position_ms`. Play is prepared
    /// first and starts with a `State` event (also sent to this host's page).
    pub fn report(&self, paused: bool, position_ms: u64, rate: f64) -> Result<(), PartyError> {
        if position_ms > protocol::MAX_POSITION_MS || !protocol::valid_rate(rate) {
            return Err(PartyError::Invalid);
        }
        let mut s = self.shared.lock();
        if s.stopped {
            return Ok(());
        }
        let now = sync::now_ms();
        // The clock starts near 0, so the first report must not wait on it.
        let due = s.last_report_ms.map_or(0, |at| at + REPORT_GAP_MS);
        if now < due || s.queued_report.is_some() {
            // Only the latest of a quick run goes out (a held seek key repeats ~25 times a second).
            if s.queued_report.replace((paused, position_ms, rate)).is_none() {
                let shared = self.shared.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(due.saturating_sub(now))).await;
                    let mut s = shared.lock();
                    if let Some((paused, position_ms, rate)) = s.queued_report.take() {
                        apply_report(&shared, &mut s, paused, position_ms, rate);
                    }
                });
            }
            return Ok(());
        }
        apply_report(&self.shared, &mut s, paused, position_ms, rate);
        Ok(())
    }

    /// Where the host's player is: repeated while it plays so guests can correct drift, and
    /// the first report when nothing was reported for this episode yet (hosting started
    /// mid-episode, or a new episode opened without playing).
    pub fn heartbeat(&self, paused: bool, position_ms: u64, rate: f64) -> Result<(), PartyError> {
        if position_ms > protocol::MAX_POSITION_MS || !protocol::valid_rate(rate) {
            return Err(PartyError::Invalid);
        }
        let mut s = self.shared.lock();
        let now = sync::now_ms();
        if s.pending.is_some() || s.stopped {
            return Ok(());
        }
        let seq = match s.last {
            None => {
                s.seq += 1;
                s.seq
            }
            Some(Message::State { seq, paused: false, rate: old_rate, .. }) if !paused => {
                let gap = if old_rate == rate { HEARTBEAT_MIN_MS } else { REPORT_GAP_MS };
                if now < s.last_heartbeat_ms + gap {
                    return Ok(());
                }
                seq
            }
            _ => return Ok(()),
        };
        let state = Message::State { seq, paused, position_ms, rate, at_ms: now };
        HostShared::broadcast(&mut s, &state);
        s.last = Some(state);
        s.last_heartbeat_ms = now;
        Ok(())
    }

    /// Ends the party: guests are told and disconnected.
    pub fn stop(&self) {
        let mut s = self.shared.lock();
        if s.stopped {
            return;
        }
        s.stopped = true;
        HostShared::broadcast(&mut s, &Message::Bye {});
        s.guests.clear();
        s.pending = None;
        s.queued_report = None;
        drop(s);
        for task in &self.accept_tasks {
            task.abort();
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Sends one report of the host's player (see [`Host::report`]).
fn apply_report(shared: &Arc<HostShared>, s: &mut HostState, paused: bool, position_ms: u64, rate: f64) {
    if s.stopped {
        return;
    }
    s.last_report_ms = Some(sync::now_ms());
    s.seq += 1;
    let seq = s.seq;
    if paused {
        s.pending = None;
        let state = Message::State { seq, paused: true, position_ms, rate, at_ms: sync::now_ms() };
        HostShared::broadcast(s, &state);
        s.last = Some(state);
        shared.emit_status(s);
        return;
    }
    let waiting: HashSet<u32> = s.guests.keys().copied().collect();
    s.pending = Some(Pending { seq, position_ms, rate, waiting });
    HostShared::broadcast(s, &Message::Prepare { seq, position_ms });
    shared.emit_status(s);
    let shared = shared.clone();
    tokio::spawn(async move {
        let deadline = tokio::time::Instant::now() + READY_WAIT;
        loop {
            let done = match shared.lock().pending.as_ref() {
                Some(p) if p.seq == seq => p.waiting.is_empty(),
                _ => return,
            };
            if done || tokio::time::Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        shared.start_playing(seq);
    });
}

async fn bind_all(ips: &[IpAddr], first_port: u16, tries: u16) -> Result<Vec<TcpListener>, PartyError> {
    for offset in 0..tries {
        let port = first_port.saturating_add(offset);
        let mut listeners = Vec::new();
        for ip in ips {
            // Every address gets the same port, picked by the first when it is 0.
            let port = listeners.first().and_then(|l: &TcpListener| l.local_addr().ok()).map_or(port, |a| a.port());
            match TcpListener::bind(SocketAddr::new(*ip, port)).await {
                Ok(listener) => listeners.push(listener),
                Err(_) => break,
            }
        }
        if listeners.len() == ips.len() {
            return Ok(listeners);
        }
    }
    Err(PartyError::PortsBusy)
}

/// The join exchange on the host side, after the guest's key-exchange message `inbound`.
/// `replied` turns true once the host has answered with its own: from then on a failure
/// counts as a wrong code.
async fn host_handshake(code: &str, inbound: &[u8], stream: &mut TcpStream, replied: &mut bool) -> Option<(Channel, u64)> {
    let (pake, outbound) = Pake::start(Side::Host, code);
    let mut reply = vec![STATUS_OK];
    reply.extend_from_slice(&outbound);
    write_frame(stream, &reply).await.ok()?;
    *replied = true;
    let mut channel = pake.finish(inbound).ok()?;
    let frame = read_frame(stream, WIRE_MAX).await.ok()?;
    match open_message(&mut channel, &frame)? {
        Message::Hello { proto: PROTOCOL_VERSION, t1 } => Some((channel, t1)),
        _ => None,
    }
}

async fn handle_guest(shared: Arc<HostShared>, mut stream: TcpStream, slot: PreAuthSlot) {
    let refusal = |s: &HostState| {
        if s.locked || s.stopped {
            Some(STATUS_LOCKED)
        } else if s.guests.len() >= MAX_GUESTS {
            Some(STATUS_FULL)
        } else {
            None
        }
    };
    let refused = refusal(&shared.lock());
    if let Some(status) = refused {
        let _ = write_frame(&mut stream, &[status]).await;
        return;
    }
    // Read before taking the join lock, so a connection that says nothing cannot hold it.
    let Ok(Ok(inbound)) = timeout(HANDSHAKE_TIMEOUT, read_frame(&mut stream, PAKE_MESSAGE_BYTES)).await else { return };
    if inbound.len() != PAKE_MESSAGE_BYTES {
        return;
    }
    let Ok(joining) = timeout(HANDSHAKE_TIMEOUT, shared.joining.lock()).await else { return };
    let refused = refusal(&shared.lock());
    if let Some(status) = refused {
        let _ = write_frame(&mut stream, &[status]).await;
        return;
    }
    let mut replied = false;
    let result = timeout(HANDSHAKE_TIMEOUT, host_handshake(&shared.code, &inbound, &mut stream, &mut replied)).await.ok().flatten();
    let Some((mut channel, t1)) = result else {
        if replied {
            let mut s = shared.lock();
            s.failures += 1;
            if s.failures >= MAX_FAILED_JOINS && !s.locked {
                s.locked = true;
                tracing::warn!("Watch party locked after {} failed joins", s.failures);
            }
            shared.emit_status(&s);
        }
        return;
    };

    let (tx, mut outbox) = mpsc::channel::<Message>(OUTBOX);
    let id = {
        let mut s = shared.lock();
        if s.stopped {
            return;
        }
        s.next_id += 1;
        let id = s.next_id;
        let _ = tx.try_send(Message::Welcome { t1, t2: sync::now_ms() });
        if let Some(load) = &s.load {
            let _ = tx.try_send(Message::Load(load.clone()));
        }
        if let Some(p) = &s.pending {
            let _ = tx.try_send(Message::Prepare { seq: p.seq, position_ms: p.position_ms });
        } else if let Some(last) = &s.last {
            let _ = tx.try_send(last.clone());
        }
        s.guests.insert(id, tx);
        shared.emit_status(&s);
        id
    };
    drop(joining);
    drop(slot);

    let (reader, mut writer) = stream.into_split();
    let (mut frames, reader_task) = spawn_reader(reader);
    let mut limit = RateLimit::new(Instant::now());
    let mut idle = tokio::time::Instant::now() + IDLE_TIMEOUT;
    loop {
        tokio::select! {
            frame = frames.recv() => {
                let Some(frame) = frame else { break };
                idle = tokio::time::Instant::now() + IDLE_TIMEOUT;
                if !limit.allow(Instant::now()) {
                    tracing::warn!("Watch party: a guest sent too many messages");
                    break;
                }
                match open_message(&mut channel, &frame) {
                    Some(Message::Ping { t1 }) => {
                        if !send_sealed(&mut writer, &mut channel, &Message::Pong { t1, t2: sync::now_ms() }).await {
                            break;
                        }
                    }
                    Some(Message::Ready { seq }) => shared.mark_ready(id, seq),
                    // Bye, a second Hello, a host-only kind, or a frame that did not open.
                    _ => break,
                }
            }
            message = outbox.recv() => {
                let Some(message) = message else { break };
                let bye = message == Message::Bye {};
                if !send_sealed(&mut writer, &mut channel, &message).await || bye {
                    break;
                }
            }
            _ = sleep_until(idle) => break,
        }
    }
    reader_task.abort();
    let _ = writer.shutdown().await;
    shared.remove_guest(id);
}

// ---------------------------------------------------------------------------------------
// Guest

#[derive(Default)]
struct GuestState {
    clock: Clock,
    load: Option<Load>,
    /// The last Prepare or State.
    last: Option<Message>,
}

struct GuestShared {
    address: String,
    state: Mutex<GuestState>,
}

impl GuestShared {
    fn lock(&self) -> std::sync::MutexGuard<'_, GuestState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// A party this machine joined. Dropping it leaves.
pub struct Guest {
    shared: Arc<GuestShared>,
    outbox: mpsc::Sender<Message>,
    task: JoinHandle<()>,
}

impl Guest {
    /// Joins the party at `address` (a private IPv4 address, with or without port).
    pub async fn join(address: &str, code: &str, events: Events) -> Result<Guest, PartyError> {
        let addr = lan::parse_host_address(address).ok_or(PartyError::BadAddress)?;
        Guest::connect(addr, code, events).await
    }

    /// Joins at any address, loopback included. For tests only.
    #[doc(hidden)]
    pub async fn join_allowing_loopback(addr: SocketAddr, code: &str, events: Events) -> Result<Guest, PartyError> {
        Guest::connect(addr, code, events).await
    }

    async fn connect(addr: SocketAddr, code: &str, events: Events) -> Result<Guest, PartyError> {
        let code = secure::normalize_code(code).ok_or(PartyError::BadCode)?;
        let mut stream = timeout(HANDSHAKE_TIMEOUT, TcpStream::connect(addr))
            .await
            .map_err(|_| PartyError::Unreachable)?
            .map_err(|_| PartyError::Unreachable)?;
        let _ = stream.set_nodelay(true);
        let (channel, sample) = timeout(HANDSHAKE_TIMEOUT, guest_handshake(&code, &mut stream))
            .await
            .map_err(|_| PartyError::Unreachable)??;

        let shared = Arc::new(GuestShared { address: addr.to_string(), state: Mutex::new(GuestState::default()) });
        shared.lock().clock.add(sample);
        let (outbox, rx) = mpsc::channel(OUTBOX);
        let task = tokio::spawn(run_guest(shared.clone(), stream, channel, rx, events));
        Ok(Guest { shared, outbox, task })
    }

    pub fn status(&self) -> PartyStatus {
        let s = self.shared.lock();
        PartyStatus {
            role: Role::Guest,
            addresses: vec![self.shared.address.clone()],
            rtt_ms: s.clock.rtt_ms(),
            ..PartyStatus::default()
        }
    }

    /// The host's clock minus this one, from the fastest recent ping.
    pub fn offset_ms(&self) -> Option<i64> {
        self.shared.lock().clock.offset_ms()
    }

    pub fn load(&self) -> Option<Load> {
        self.shared.lock().load.clone()
    }

    /// The host's last Prepare or State as an event, aged to now; for a player that just opened.
    pub fn last_play(&self) -> Option<PartyEvent> {
        let s = self.shared.lock();
        play_event(s.last.as_ref()?, s.clock.offset_ms().unwrap_or(0), sync::now_ms())
    }

    /// This player is at the position of Prepare `seq` and has buffered.
    pub fn ready(&self, seq: u64) {
        let _ = self.outbox.try_send(Message::Ready { seq });
    }

    /// Leaves the party.
    pub fn leave(&self) {
        let _ = self.outbox.try_send(Message::Bye {});
    }
}

impl Drop for Guest {
    fn drop(&mut self) {
        // A guest dropped without `leave` just closes the connection.
        let outbox = self.outbox.clone();
        if outbox.try_send(Message::Bye {}).is_err() {
            self.task.abort();
        }
    }
}

async fn guest_handshake(code: &str, stream: &mut TcpStream) -> Result<(Channel, ClockSample), PartyError> {
    let (pake, outbound) = Pake::start(Side::Guest, code);
    write_frame(stream, &outbound).await.map_err(|_| PartyError::Unreachable)?;
    let reply = read_frame(stream, PAKE_MESSAGE_BYTES + 1).await.map_err(|_| PartyError::Unreachable)?;
    match reply.first() {
        Some(&STATUS_FULL) => return Err(PartyError::Full),
        Some(&STATUS_LOCKED) => return Err(PartyError::Locked),
        Some(&STATUS_OK) if reply.len() == PAKE_MESSAGE_BYTES + 1 => {}
        _ => return Err(PartyError::Unreachable),
    }
    let mut channel = pake.finish(&reply[1..]).map_err(|_: SecureError| PartyError::WrongCode)?;
    let t1 = sync::now_ms();
    if !send_sealed(stream, &mut channel, &Message::Hello { proto: PROTOCOL_VERSION, t1 }).await {
        return Err(PartyError::Unreachable);
    }
    // With a wrong code the host cannot open the Hello and hangs up.
    let frame = read_frame(stream, WIRE_MAX).await.map_err(|_| PartyError::WrongCode)?;
    match open_message(&mut channel, &frame) {
        Some(Message::Welcome { t1: echoed, t2 }) if echoed == t1 => Ok((channel, ClockSample::new(t1, t2, sync::now_ms()))),
        _ => Err(PartyError::WrongCode),
    }
}

/// A host report as the page applies it.
fn play_event(message: &Message, offset_ms: i64, now: u64) -> Option<PartyEvent> {
    match *message {
        Message::Prepare { seq, position_ms } => Some(PartyEvent::Prepare { seq, position_ms }),
        Message::State { seq, paused, position_ms, rate, at_ms } => Some(PartyEvent::State(PlayState {
            seq,
            paused,
            position_ms,
            rate,
            age_ms: sync::age_ms(at_ms, offset_ms, now),
        })),
        _ => None,
    }
}

async fn run_guest(shared: Arc<GuestShared>, stream: TcpStream, mut channel: Channel, mut outbox: mpsc::Receiver<Message>, events: Events) {
    let (reader, mut writer) = stream.into_split();
    let (mut frames, reader_task) = spawn_reader(reader);
    let mut limit = RateLimit::new(Instant::now());
    let mut idle = tokio::time::Instant::now() + IDLE_TIMEOUT;
    // A few quick pings first, so the clock offset is good before the first play.
    let mut quick_pings = 4;
    let mut next_ping = tokio::time::Instant::now() + Duration::from_millis(100);
    let reason = loop {
        tokio::select! {
            frame = frames.recv() => {
                let Some(frame) = frame else { break EndReason::Lost };
                idle = tokio::time::Instant::now() + IDLE_TIMEOUT;
                if !limit.allow(Instant::now()) {
                    break EndReason::Lost;
                }
                let Some(message) = open_message(&mut channel, &frame).filter(Message::is_from_host) else { break EndReason::Lost };
                let now = sync::now_ms();
                let mut s = shared.lock();
                match message {
                    Message::Pong { t1, t2 } if t1 <= now => s.clock.add(ClockSample::new(t1, t2, now)),
                    Message::Load(load) => {
                        // Only a new episode drops the host's last report; a load that just
                        // adds the length keeps it, as on the host.
                        if !s.load.as_ref().is_some_and(|old| old.same_episode(&load)) {
                            s.last = None;
                        }
                        s.load = Some(load.clone());
                        let _ = events.send(PartyEvent::Load(load));
                    }
                    Message::Bye {} => break EndReason::HostLeft,
                    message @ (Message::Prepare { .. } | Message::State { .. }) => {
                        if let Some(event) = play_event(&message, s.clock.offset_ms().unwrap_or(0), now) {
                            let _ = events.send(event);
                        }
                        s.last = Some(message);
                    }
                    _ => break EndReason::Lost,
                }
            }
            message = outbox.recv() => {
                let Some(message) = message else { break EndReason::Left };
                let bye = message == Message::Bye {};
                if !send_sealed(&mut writer, &mut channel, &message).await {
                    break EndReason::Lost;
                }
                if bye {
                    break EndReason::Left;
                }
            }
            _ = sleep_until(next_ping) => {
                if !send_sealed(&mut writer, &mut channel, &Message::Ping { t1: sync::now_ms() }).await {
                    break EndReason::Lost;
                }
                let gap = if quick_pings > 0 { quick_pings -= 1; Duration::from_millis(250) } else { PING_INTERVAL };
                next_ping = tokio::time::Instant::now() + gap;
            }
            _ = sleep_until(idle) => break EndReason::Lost,
        }
    };
    reader_task.abort();
    let _ = writer.shutdown().await;
    let _ = events.send(PartyEvent::Ended { reason });
}
