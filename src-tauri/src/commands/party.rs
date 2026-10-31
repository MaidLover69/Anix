//! Tauri commands of the LAN watch party (player › Watch together).
//!
//! At most one party at a time: hosting or joining ends the previous one. Events reach the
//! page as `party-event`; the backend's errors as `party:<kind>`, translated by the page.

use crate::party::lan::{self, FoundParty};
use crate::party::protocol::Load;
use crate::party::{Guest, Host, PartyError, PartyEvent, PartyStatus};
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{mpsc, Mutex};

/// How long Join looks for parties on the network.
const DISCOVER_WAIT: Duration = Duration::from_secs(3);

enum Active {
    Host(Host),
    Guest(Guest),
}

/// The party this app is in, if any, with the number of the session it belongs to.
#[derive(Default)]
pub struct PartyState {
    active: Arc<Mutex<Option<(u64, Active)>>>,
    sessions: AtomicU64,
}

/// What a player that just opened needs: the status, the episode and the host's last report.
#[derive(Serialize)]
pub struct PartySnapshot {
    status: PartyStatus,
    load: Option<Load>,
    play: Option<PartyEvent>,
}

fn err(e: PartyError) -> String {
    format!("party:{}", e.kind())
}

fn status_of(active: &Active) -> PartyStatus {
    match active {
        Active::Host(host) => host.status(),
        Active::Guest(guest) => guest.status(),
    }
}

/// Passes a session's events to the page; an ended session is forgotten.
fn forward(app: AppHandle, active: Arc<Mutex<Option<(u64, Active)>>>, session: u64, mut rx: mpsc::UnboundedReceiver<PartyEvent>) {
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            let ended = matches!(event, PartyEvent::Ended { .. });
            let _ = app.emit("party-event", &event);
            if ended {
                let mut slot = active.lock().await;
                if slot.as_ref().is_some_and(|(id, _)| *id == session) {
                    *slot = None;
                }
                break;
            }
        }
    });
}

async fn end_current(slot: &mut Option<(u64, Active)>) {
    match slot.take() {
        Some((_, Active::Host(host))) => host.stop(),
        Some((_, Active::Guest(guest))) => guest.leave(),
        None => {}
    }
}

/// Hosts a party on the local network and returns its code and addresses.
#[tauri::command]
pub async fn party_host(app: AppHandle, party: State<'_, PartyState>) -> Result<PartyStatus, String> {
    let mut slot = party.active.lock().await;
    end_current(&mut slot).await;
    let (tx, rx) = mpsc::unbounded_channel();
    let host = Host::start(tx).await.map_err(err)?;
    let status = host.status();
    let session = party.sessions.fetch_add(1, Ordering::Relaxed) + 1;
    *slot = Some((session, Active::Host(host)));
    forward(app, party.active.clone(), session, rx);
    Ok(status)
}

/// Joins the party at `address` with the host's code.
#[tauri::command]
pub async fn party_join(app: AppHandle, party: State<'_, PartyState>, address: String, code: String) -> Result<PartyStatus, String> {
    let mut slot = party.active.lock().await;
    end_current(&mut slot).await;
    let (tx, rx) = mpsc::unbounded_channel();
    let guest = Guest::join(&address, &code, tx).await.map_err(err)?;
    let status = guest.status();
    let session = party.sessions.fetch_add(1, Ordering::Relaxed) + 1;
    *slot = Some((session, Active::Guest(guest)));
    forward(app, party.active.clone(), session, rx);
    Ok(status)
}

/// Leaves the party (as host: ends it for everyone).
#[tauri::command]
pub async fn party_leave(party: State<'_, PartyState>) -> Result<(), String> {
    end_current(&mut *party.active.lock().await).await;
    Ok(())
}

/// The current party, for a player that just opened.
#[tauri::command]
pub async fn party_status(party: State<'_, PartyState>) -> Result<PartySnapshot, String> {
    let slot = party.active.lock().await;
    Ok(match slot.as_ref() {
        None => PartySnapshot { status: PartyStatus::default(), load: None, play: None },
        Some((_, active @ Active::Host(host))) => PartySnapshot { status: status_of(active), load: host.load(), play: None },
        Some((_, active @ Active::Guest(guest))) => {
            PartySnapshot { status: status_of(active), load: guest.load(), play: guest.last_play() }
        }
    })
}

/// Parties announced on the local network.
#[tauri::command]
pub async fn party_discover() -> Result<Vec<FoundParty>, String> {
    tauri::async_runtime::spawn_blocking(|| lan::discover(DISCOVER_WAIT))
        .await
        .map_err(|_| err(PartyError::NoNetwork))?
        .map_err(|e| {
            tracing::warn!("Watch party: mDNS search failed: {}", e);
            err(PartyError::NoNetwork)
        })
}

/// Host: the episode now playing (or what is known about it changed).
#[tauri::command]
pub async fn party_load(party: State<'_, PartyState>, load: Load) -> Result<(), String> {
    match party.active.lock().await.as_ref() {
        Some((_, Active::Host(host))) => host.set_load(load).map_err(err),
        _ => Err(err(PartyError::NotInParty)),
    }
}

/// Host: the player paused or wants to play (`heartbeat`: just where it is while playing).
#[tauri::command]
pub async fn party_report(party: State<'_, PartyState>, paused: bool, position_ms: u64, rate: f64, heartbeat: bool) -> Result<(), String> {
    match party.active.lock().await.as_ref() {
        Some((_, Active::Host(host))) if heartbeat => host.heartbeat(position_ms, rate).map_err(err),
        Some((_, Active::Host(host))) => host.report(paused, position_ms, rate).map_err(err),
        _ => Err(err(PartyError::NotInParty)),
    }
}

/// Guest: the player is at the prepared position and has buffered.
#[tauri::command]
pub async fn party_ready(party: State<'_, PartyState>, seq: u64) -> Result<(), String> {
    match party.active.lock().await.as_ref() {
        Some((_, Active::Guest(guest))) => {
            guest.ready(seq);
            Ok(())
        }
        _ => Err(err(PartyError::NotInParty)),
    }
}
