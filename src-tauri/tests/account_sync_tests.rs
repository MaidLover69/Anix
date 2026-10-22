//! What a watched episode sends to AniList and MyAnimeList, the queue that holds it until it
//! can be sent, and editing a list entry by hand. The two services are a loopback server.

mod account_support;

use anix_lib::accounts;

use accounts::oauth::Service;
use accounts::session::Notice;
use accounts::store::Profile;
use accounts::sync::{
    self, anilist_status_from_mal, mal_status, plan_progress_update, whole_episode, MalMirror,
    PendingKind, ProgressUpdate, SkipReason, MAX_ATTEMPTS,
};
use account_support::{closed_address, far_future, FakeServer, Recorded, World};
use anix_lib::clients::anilist::ListEntryState;
use anix_lib::db;
use anix_lib::error::AppError;
use anix_lib::models::HistoryEntry;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// ---- A small AniList + MyAnimeList ----------------------------------------------------------

#[derive(Clone)]
struct Anime {
    id_mal: Option<i32>,
    episodes: Option<i32>,
    status: &'static str,
    /// The viewer's AniList entry: (status, progress).
    anilist_entry: Option<(String, i32)>,
    /// The viewer's MAL entry: (status, watched, rewatching).
    mal_entry: Option<(String, i32, bool)>,
}

impl Anime {
    fn new(id_mal: Option<i32>, episodes: Option<i32>, status: &'static str) -> Self {
        Self { id_mal, episodes, status, anilist_entry: None, mal_entry: None }
    }

    fn on_anilist(mut self, status: &str, progress: i32) -> Self {
        self.anilist_entry = Some((status.to_string(), progress));
        self
    }

    fn on_mal(mut self, status: &str, watched: i32) -> Self {
        self.mal_entry = Some((status.to_string(), watched, false));
        self
    }
}

#[derive(Default)]
struct Remote {
    /// Keyed by AniList id.
    anime: HashMap<i32, Anime>,
    /// Status every request is answered with instead, while set.
    broken: Option<u16>,
    /// The same for MyAnimeList requests only.
    mal_broken: Option<u16>,
}

type Shared = Arc<Mutex<Remote>>;

fn respond(remote: &Shared, request: &Recorded) -> (u16, String) {
    let mut remote = remote.lock().unwrap();
    if let Some(status) = remote.broken {
        return (status, r#"{"errors":[{"message":"broken","status":500}],"error":"broken"}"#.to_string());
    }

    if request.url == "/graphql" {
        let body = request.json();
        let query = body["query"].as_str().unwrap_or_default();
        let variables = &body["variables"];
        let signed_in = request.authorization.is_some();

        if query.contains("SaveMediaListEntry") {
            if !signed_in {
                return (400, r#"{"errors":[{"message":"Invalid token","status":400}],"data":null}"#.to_string());
            }
            let id = variables["mediaId"].as_i64().unwrap_or(0) as i32;
            let Some(anime) = remote.anime.get_mut(&id) else {
                return (404, r#"{"errors":[{"message":"Not Found.","status":404}],"data":{"SaveMediaListEntry":null}}"#.to_string());
            };
            let (mut status, mut progress) = anime.anilist_entry.clone().unwrap_or(("CURRENT".to_string(), 0));
            if let Some(new_status) = variables["status"].as_str() {
                status = new_status.to_string();
            }
            if let Some(new_progress) = variables["progress"].as_i64() {
                progress = new_progress as i32;
            }
            anime.anilist_entry = Some((status.clone(), progress));
            return (
                200,
                json!({ "data": { "SaveMediaListEntry": { "id": 1, "mediaId": id, "status": status, "progress": progress, "score": 0 } } })
                    .to_string(),
            );
        }

        if query.contains("MediaListCollection") {
            let entries: Vec<Value> = remote
                .anime
                .iter()
                .filter_map(|(id, anime)| anime.anilist_entry.as_ref().map(|(status, progress)| (id, status, progress)))
                .map(|(id, status, progress)| json!({ "id": id + 10_000, "mediaId": id, "status": status, "progress": progress, "score": 0, "media": { "id": id } }))
                .collect();
            return (200, json!({ "data": { "MediaListCollection": { "lists": [{ "name": "all", "entries": entries }] } } }).to_string());
        }

        let id = variables["id"].as_i64().unwrap_or(0) as i32;
        return match remote.anime.get(&id) {
            None => (404, r#"{"errors":[{"message":"Not Found.","status":404}],"data":{"Media":null}}"#.to_string()),
            Some(anime) => {
                let entry = match (&anime.anilist_entry, signed_in) {
                    (Some((status, progress)), true) => json!({ "id": 1, "status": status, "progress": progress }),
                    _ => Value::Null,
                };
                (
                    200,
                    json!({ "data": { "Media": {
                        "idMal": anime.id_mal,
                        "episodes": anime.episodes,
                        "status": anime.status,
                        "mediaListEntry": entry,
                    } } })
                    .to_string(),
                )
            }
        };
    }

    if let Some(rest) = request.url.strip_prefix("/v2/anime/") {
        if let Some(status) = remote.mal_broken {
            return (status, r#"{"error":"broken","message":"MAL is down"}"#.to_string());
        }
        let mal_id: i32 = rest
            .split(|c| c == '/' || c == '?')
            .next()
            .and_then(|id| id.parse().ok())
            .unwrap_or(0);
        let Some(anime) = remote.anime.values_mut().find(|anime| anime.id_mal == Some(mal_id)) else {
            return (404, r#"{"error":"not_found"}"#.to_string());
        };
        let entry_json = |entry: &(String, i32, bool)| {
            json!({ "status": entry.0, "num_episodes_watched": entry.1, "is_rewatching": entry.2, "score": 0 })
        };

        if request.method == "PUT" && rest.contains("/my_list_status") {
            let mut entry = anime.mal_entry.clone().unwrap_or(("watching".to_string(), 0, false));
            if let Some(status) = request.form("status") {
                entry.0 = status;
            }
            if let Some(watched) = request.form("num_watched_episodes") {
                entry.1 = watched.parse().unwrap_or(0);
            }
            anime.mal_entry = Some(entry.clone());
            return (200, entry_json(&entry).to_string());
        }
        if request.method == "GET" {
            let mut body = json!({ "id": mal_id, "title": "x" });
            if let Some(entry) = &anime.mal_entry {
                body["my_list_status"] = entry_json(entry);
            }
            return (200, body.to_string());
        }
    }

    (404, r#"{"error":"not_found"}"#.to_string())
}

fn remote_with(anime: Vec<(i32, Anime)>) -> (Shared, FakeServer) {
    let shared: Shared = Arc::new(Mutex::new(Remote { anime: anime.into_iter().collect(), ..Default::default() }));
    let for_server = Arc::clone(&shared);
    let server = FakeServer::start(move |request| respond(&for_server, request));
    (shared, server)
}

fn anilist_entry(remote: &Shared, id: i32) -> Option<(String, i32)> {
    remote.lock().unwrap().anime[&id].anilist_entry.clone()
}

fn mal_entry(remote: &Shared, id: i32) -> Option<(String, i32)> {
    remote.lock().unwrap().anime[&id].mal_entry.clone().map(|(status, watched, _)| (status, watched))
}

fn some(status: &str, progress: i32) -> Option<(String, i32)> {
    Some((status.to_string(), progress))
}

fn profile() -> Profile {
    Profile { id: 42, name: "mehul".to_string(), avatar_url: None }
}

fn connect_anilist(world: &World) {
    world.store.save_anilist("ani-token", Some(far_future()), &profile()).unwrap();
}

fn connect_mal(world: &World) {
    world.store.save_mal("mal-access", "mal-refresh", far_future(), &profile()).unwrap();
}

fn mutations(server: &FakeServer) -> Vec<Value> {
    server
        .requests_to("/graphql")
        .iter()
        .map(Recorded::json)
        .filter(|body| body["query"].as_str().unwrap_or_default().contains("SaveMediaListEntry"))
        .map(|body| body["variables"].clone())
        .collect()
}

fn mal_updates(server: &FakeServer) -> Vec<Recorded> {
    server.requests().into_iter().filter(|r| r.method == "PUT").collect()
}

async fn queued(world: &World, service: Service) -> Vec<(i32, PendingKind, i32, i64)> {
    sync::pending(&world.pool, service)
        .await
        .unwrap()
        .into_iter()
        .map(|item| (item.anilist_id, item.kind, item.progress, item.attempts))
        .collect()
}

// ---- What to send -------------------------------------------------------------------------

#[test]
fn test_account_only_whole_episodes_are_list_progress() {
    assert_eq!(whole_episode(1.0), Some(1));
    assert_eq!(whole_episode(1070.0), Some(1070));
    for not_progress in [0.0, 0.5, 5.5, 12.01, -1.0, f64::NAN, f64::INFINITY, 1e12] {
        assert_eq!(whole_episode(not_progress), None, "{}", not_progress);
    }
}

fn entry(status: &str, progress: i32) -> ListEntryState {
    ListEntryState { status: status.to_string(), progress }
}

#[test]
fn test_account_plan_never_lowers_progress() {
    for (current, watched) in [(5, 5), (5, 4), (12, 1)] {
        assert_eq!(
            plan_progress_update(Some(&entry("CURRENT", current)), watched, Some(24), true),
            Err(SkipReason::AlreadyAtOrAhead)
        );
    }
    assert_eq!(
        plan_progress_update(Some(&entry("CURRENT", 5)), 6, Some(24), true),
        Ok(ProgressUpdate { progress: 6, status: None })
    );
    // A completed entry is not reopened by watching an episode again.
    assert_eq!(
        plan_progress_update(Some(&entry("COMPLETED", 12)), 3, Some(12), true),
        Err(SkipReason::RemoteCompleted)
    );
    assert_eq!(
        plan_progress_update(Some(&entry("COMPLETED", 0)), 3, Some(12), true),
        Err(SkipReason::RemoteCompleted)
    );
    for invalid in [0, -3] {
        assert_eq!(plan_progress_update(None, invalid, Some(12), true), Err(SkipReason::InvalidProgress));
    }
}

#[test]
fn test_account_plan_status_transitions() {
    let plan = |current: Option<ListEntryState>, watched| {
        plan_progress_update(current.as_ref(), watched, Some(12), true).map(|update| update.status)
    };
    // Not on the list yet, or only planned: now being watched.
    assert_eq!(plan(None, 1), Ok(Some("CURRENT")));
    assert_eq!(plan(Some(entry("PLANNING", 0)), 1), Ok(Some("CURRENT")));
    // Already watching: only the progress moves.
    assert_eq!(plan(Some(entry("CURRENT", 3)), 4), Ok(None));
    // Statuses the user chose are kept, mid-show and at the end.
    for kept in ["REPEATING", "PAUSED", "DROPPED"] {
        assert_eq!(plan(Some(entry(kept, 3)), 4), Ok(None), "{}", kept);
        assert_eq!(plan(Some(entry(kept, 3)), 12), Ok(None), "{}", kept);
    }
}

#[test]
fn test_account_plan_completes_on_the_final_episode_of_a_finished_show() {
    let done = Ok(ProgressUpdate { progress: 12, status: Some("COMPLETED") });
    assert_eq!(plan_progress_update(Some(&entry("CURRENT", 11)), 12, Some(12), true), done);
    assert_eq!(plan_progress_update(Some(&entry("PLANNING", 0)), 12, Some(12), true), done);
    assert_eq!(plan_progress_update(None, 12, Some(12), true), done);

    // Still airing: the last listed episode does not complete it.
    assert_eq!(
        plan_progress_update(Some(&entry("CURRENT", 11)), 12, Some(12), false),
        Ok(ProgressUpdate { progress: 12, status: None })
    );
    assert_eq!(
        plan_progress_update(None, 12, Some(12), false),
        Ok(ProgressUpdate { progress: 12, status: Some("CURRENT") })
    );
    // Episode count unknown: never completed, never clamped.
    for unknown in [None, Some(0)] {
        assert_eq!(
            plan_progress_update(Some(&entry("CURRENT", 11)), 500, unknown, true),
            Ok(ProgressUpdate { progress: 500, status: None })
        );
    }
    // An episode number past the count is the final episode, not more.
    assert_eq!(plan_progress_update(Some(&entry("CURRENT", 11)), 13, Some(12), true), done);
    assert_eq!(
        plan_progress_update(Some(&entry("CURRENT", 12)), 13, Some(12), true),
        Err(SkipReason::AlreadyAtOrAhead)
    );
}

#[test]
fn test_account_status_vocabularies() {
    let pairs = [
        ("CURRENT", "watching"),
        ("COMPLETED", "completed"),
        ("PAUSED", "on_hold"),
        ("DROPPED", "dropped"),
        ("PLANNING", "plan_to_watch"),
    ];
    for (anilist, mal) in pairs {
        assert_eq!(mal_status(anilist), mal);
        assert_eq!(anilist_status_from_mal(Some(mal), false), anilist);
    }
    assert_eq!(mal_status("REPEATING"), "watching");
    assert_eq!(anilist_status_from_mal(Some("completed"), true), "REPEATING");
    assert_eq!(anilist_status_from_mal(None, false), "PLANNING");
}

// ---- The queue ----------------------------------------------------------------------------

#[tokio::test]
async fn test_account_queue_keeps_the_latest_episode_per_anime() {
    let world = World::new("http://127.0.0.1:9").await;
    assert!(queued(&world, Service::AniList).await.is_empty());

    assert!(sync::queue_watched_episode(&world.pool, 100, 3.0).await.unwrap());
    assert!(sync::queue_watched_episode(&world.pool, 100, 5.0).await.unwrap());
    // An earlier episode watched afterwards does not replace the later one.
    assert!(sync::queue_watched_episode(&world.pool, 100, 4.0).await.unwrap());
    assert!(sync::queue_watched_episode(&world.pool, 200, 1.0).await.unwrap());
    // Not list progress: nothing is queued.
    assert!(!sync::queue_watched_episode(&world.pool, 100, 5.5).await.unwrap());
    assert!(!sync::queue_watched_episode(&world.pool, 100, 0.0).await.unwrap());
    assert!(!sync::queue_watched_episode(&world.pool, 0, 1.0).await.unwrap());

    // Every service this build can sign in to gets it, connected or not.
    for service in Service::ALL.into_iter().filter(|service| service.available()) {
        let mut rows = queued(&world, service).await;
        rows.sort_by_key(|row| row.0);
        assert_eq!(
            rows,
            vec![(100, PendingKind::Progress, 5, 0), (200, PendingKind::Progress, 1, 0)],
            "{:?}",
            service
        );
    }
}

#[tokio::test]
async fn test_account_watchlist_additions_are_queued_for_connected_services_only() {
    let world = World::new("http://127.0.0.1:9").await;
    assert!(!sync::queue_watchlist_addition(&world.ctx(), 100).await.unwrap());
    assert!(queued(&world, Service::AniList).await.is_empty());

    connect_mal(&world);
    assert!(sync::queue_watchlist_addition(&world.ctx(), 100).await.unwrap());
    assert!(sync::queue_watchlist_addition(&world.ctx(), 100).await.unwrap());
    assert!(queued(&world, Service::AniList).await.is_empty());
    assert_eq!(queued(&world, Service::Mal).await, vec![(100, PendingKind::Planning, 0, 0)]);

    // Taking it off the watchlist before it was pushed forgets the addition, not the progress.
    sync::queue_watched_episode(&world.pool, 100, 2.0).await.unwrap();
    sync::cancel_watchlist_addition(&world.pool, 100).await.unwrap();
    assert_eq!(queued(&world, Service::Mal).await, vec![(100, PendingKind::Progress, 2, 0)]);
}

#[tokio::test]
async fn test_account_save_watch_progress_reports_the_moment_an_episode_is_watched() {
    let world = World::new("http://127.0.0.1:9").await;
    let at = |position_ms: i64, duration_ms: i64| HistoryEntry {
        anilist_id: 100,
        title: "Show".to_string(),
        episode_number: 3.0,
        provider: "auto".to_string(),
        category: "sub".to_string(),
        position_ms,
        duration_ms,
        updated_at: db::current_unix_millis(),
        ..Default::default()
    };

    assert!(!sync::save_watch_progress(&world.pool, &at(60_000, 1_440_000)).await.unwrap());
    assert!(!sync::save_watch_progress(&world.pool, &at(1_200_000, 1_440_000)).await.unwrap());
    // Duration not known yet: never "watched".
    assert!(!sync::save_watch_progress(&world.pool, &at(1_400_000, 0)).await.unwrap());
    // 85%: this save is the one.
    assert!(sync::save_watch_progress(&world.pool, &at(1_224_000, 1_440_000)).await.unwrap());
    assert_eq!(db::get_watched_episodes(&world.pool, 100).await.unwrap(), vec![3.0]);
    // Later saves of the same episode, and a replay from the start, are not.
    assert!(!sync::save_watch_progress(&world.pool, &at(1_400_000, 1_440_000)).await.unwrap());
    assert!(!sync::save_watch_progress(&world.pool, &at(5_000, 1_440_000)).await.unwrap());
    assert!(!sync::save_watch_progress(&world.pool, &at(1_300_000, 1_440_000)).await.unwrap());

    // Another episode has its own moment.
    let mut next = at(1_440_000, 1_440_000);
    next.episode_number = 4.0;
    assert!(sync::save_watch_progress(&world.pool, &next).await.unwrap());
}

// ---- Pushing ------------------------------------------------------------------------------

#[tokio::test]
async fn test_account_watched_episode_updates_both_lists() {
    let (remote, server) = remote_with(vec![(100, Anime::new(Some(5100), Some(24), "FINISHED"))]);
    let world = World::new(&server.base).await;
    connect_anilist(&world);
    connect_mal(&world);

    sync::queue_watched_episode(&world.pool, 100, 3.0).await.unwrap();
    let report = sync::drain(&world.ctx()).await;
    assert_eq!((report.updated, report.skipped, report.failed), (2, 0, 0));

    assert_eq!(anilist_entry(&remote, 100), some("CURRENT", 3));
    assert_eq!(mutations(&server), vec![json!({ "mediaId": 100, "status": "CURRENT", "progress": 3 })]);
    assert!(server.requests_to("/graphql").iter().all(|r| r.authorization.as_deref() == Some("Bearer ani-token")));

    assert_eq!(mal_entry(&remote, 100), some("watching", 3));
    let update = &mal_updates(&server)[0];
    assert_eq!(update.url, "/v2/anime/5100/my_list_status");
    assert_eq!(update.authorization.as_deref(), Some("Bearer mal-access"));
    assert_eq!(update.form("num_watched_episodes").as_deref(), Some("3"));
    assert_eq!(update.form("status").as_deref(), Some("watching"));
    // The MAL id came from the AniList record that was already fetched: one lookup, one save.
    assert_eq!(server.requests_to("/graphql").len(), 2);

    assert!(queued(&world, Service::AniList).await.is_empty());
    assert!(queued(&world, Service::Mal).await.is_empty());
    assert_eq!(
        world.recorder.notices(),
        vec![
            Notice::Synced { service: Service::AniList, anilist_id: 100 },
            Notice::Synced { service: Service::Mal, anilist_id: 100 },
        ]
    );

    // The next episode only moves the progress.
    sync::queue_watched_episode(&world.pool, 100, 4.0).await.unwrap();
    sync::drain(&world.ctx()).await;
    assert_eq!(mutations(&server).last().unwrap(), &json!({ "mediaId": 100, "progress": 4 }));
    let update = mal_updates(&server).pop().unwrap();
    assert_eq!(update.form("num_watched_episodes").as_deref(), Some("4"));
    assert_eq!(update.form("status"), None);
}

#[tokio::test]
async fn test_account_remote_progress_is_never_lowered() {
    let (remote, server) = remote_with(vec![
        (100, Anime::new(Some(5100), Some(24), "FINISHED").on_anilist("CURRENT", 10).on_mal("watching", 12)),
        (200, Anime::new(Some(5200), Some(12), "FINISHED").on_anilist("COMPLETED", 12).on_mal("completed", 12)),
    ]);
    let world = World::new(&server.base).await;
    connect_anilist(&world);
    connect_mal(&world);

    sync::queue_watched_episode(&world.pool, 100, 4.0).await.unwrap();
    sync::queue_watched_episode(&world.pool, 200, 2.0).await.unwrap();
    let report = sync::drain(&world.ctx()).await;
    assert_eq!((report.updated, report.skipped, report.failed), (0, 4, 0));

    assert!(mutations(&server).is_empty());
    assert!(mal_updates(&server).is_empty());
    assert_eq!(anilist_entry(&remote, 100), some("CURRENT", 10));
    assert_eq!(mal_entry(&remote, 100), some("watching", 12));
    assert_eq!(anilist_entry(&remote, 200), some("COMPLETED", 12));
    // Decided: nothing stays queued, nothing is announced.
    assert!(queued(&world, Service::AniList).await.is_empty());
    assert!(queued(&world, Service::Mal).await.is_empty());
    assert!(world.recorder.notices().is_empty());

    // Each list is judged on its own: AniList is behind episode 11, MAL is not.
    sync::queue_watched_episode(&world.pool, 100, 11.0).await.unwrap();
    sync::drain(&world.ctx()).await;
    assert_eq!(anilist_entry(&remote, 100), some("CURRENT", 11));
    assert_eq!(mal_entry(&remote, 100), some("watching", 12));
}

#[tokio::test]
async fn test_account_final_episode_completes_and_chosen_statuses_stay() {
    let (remote, server) = remote_with(vec![
        (100, Anime::new(Some(5100), Some(12), "FINISHED").on_anilist("CURRENT", 11).on_mal("plan_to_watch", 0)),
        (200, Anime::new(Some(5200), Some(12), "RELEASING").on_anilist("CURRENT", 11)),
        (300, Anime::new(Some(5300), Some(12), "FINISHED").on_anilist("REPEATING", 3).on_mal("on_hold", 3)),
    ]);
    let world = World::new(&server.base).await;
    connect_anilist(&world);
    connect_mal(&world);

    for id in [100, 200, 300] {
        sync::queue_watched_episode(&world.pool, id, 12.0).await.unwrap();
    }
    sync::drain(&world.ctx()).await;

    assert_eq!(anilist_entry(&remote, 100), some("COMPLETED", 12));
    assert_eq!(mal_entry(&remote, 100), some("completed", 12));
    // Still airing: watching the last listed episode keeps it current.
    assert_eq!(anilist_entry(&remote, 200), some("CURRENT", 12));
    assert_eq!(mal_entry(&remote, 200), some("watching", 12));
    // A rewatch and a paused entry keep their status; the progress still moves.
    assert_eq!(anilist_entry(&remote, 300), some("REPEATING", 12));
    assert_eq!(mal_entry(&remote, 300), some("on_hold", 12));

    let sent = mutations(&server);
    assert!(sent.contains(&json!({ "mediaId": 100, "status": "COMPLETED", "progress": 12 })));
    assert!(sent.contains(&json!({ "mediaId": 300, "progress": 12 })));
}

#[tokio::test]
async fn test_account_anime_without_mal_id_or_unknown_to_anilist_is_skipped() {
    let (remote, server) = remote_with(vec![(100, Anime::new(None, Some(12), "FINISHED"))]);
    let world = World::new(&server.base).await;
    connect_anilist(&world);
    connect_mal(&world);

    sync::queue_watched_episode(&world.pool, 100, 1.0).await.unwrap();
    sync::queue_watched_episode(&world.pool, 999, 1.0).await.unwrap();
    let report = sync::drain(&world.ctx()).await;
    assert_eq!((report.updated, report.skipped, report.failed), (1, 3, 0));

    assert_eq!(anilist_entry(&remote, 100), some("CURRENT", 1));
    assert!(server.requests_to("/v2/").is_empty());
    assert!(queued(&world, Service::AniList).await.is_empty());
    assert!(queued(&world, Service::Mal).await.is_empty());
}

#[tokio::test]
async fn test_account_only_mal_connected_looks_the_anime_up_without_a_token() {
    let (remote, server) = remote_with(vec![(100, Anime::new(Some(5100), Some(12), "FINISHED").on_anilist("CURRENT", 9))]);
    let world = World::new(&server.base).await;
    connect_mal(&world);

    sync::queue_watched_episode(&world.pool, 100, 2.0).await.unwrap();
    let report = sync::drain(&world.ctx()).await;
    assert_eq!((report.updated, report.failed), (1, 0));

    assert_eq!(mal_entry(&remote, 100), some("watching", 2));
    assert_eq!(anilist_entry(&remote, 100), some("CURRENT", 9));
    let lookups = server.requests_to("/graphql");
    assert_eq!(lookups.len(), 1);
    assert_eq!(lookups[0].authorization, None);
    // AniList is not connected: its copy waits for a sign-in.
    assert_eq!(queued(&world, Service::AniList).await, vec![(100, PendingKind::Progress, 2, 0)]);
    assert!(queued(&world, Service::Mal).await.is_empty());
}

#[tokio::test]
async fn test_account_progress_made_signed_out_or_offline_is_pushed_later() {
    let (remote, server) = remote_with(vec![
        (100, Anime::new(Some(5100), Some(24), "FINISHED")),
        (200, Anime::new(Some(5200), Some(24), "FINISHED")),
    ]);

    // Signed out: queued, nothing sent.
    let world = World::new(&server.base).await;
    sync::queue_watched_episode(&world.pool, 100, 1.0).await.unwrap();
    sync::queue_watched_episode(&world.pool, 100, 2.0).await.unwrap();
    let report = sync::drain(&world.ctx()).await;
    assert_eq!(report, sync::DrainReport::default());
    assert!(server.requests().is_empty());
    assert_eq!(queued(&world, Service::AniList).await.len(), 1);

    // Signed in: the next drain (run after a login and at startup) sends the latest episode.
    connect_anilist(&world);
    sync::drain(&world.ctx()).await;
    assert_eq!(anilist_entry(&remote, 100), some("CURRENT", 2));
    assert!(queued(&world, Service::AniList).await.is_empty());
    assert_eq!(queued(&world, Service::Mal).await, vec![(100, PendingKind::Progress, 2, 0)]);

    // Offline: the push fails, stays queued and is not counted as a refusal.
    let offline = World::new(&closed_address()).await;
    connect_anilist(&offline);
    connect_mal(&offline);
    sync::queue_watched_episode(&offline.pool, 200, 7.0).await.unwrap();
    for _ in 0..3 {
        let report = sync::drain(&offline.ctx()).await;
        assert_eq!((report.updated, report.failed), (0, 2));
    }
    assert_eq!(queued(&offline, Service::AniList).await, vec![(200, PendingKind::Progress, 7, 0)]);
    assert_eq!(queued(&offline, Service::Mal).await, vec![(200, PendingKind::Progress, 7, 0)]);
    assert!(offline.store.is_connected(Service::AniList) && offline.store.is_connected(Service::Mal));
    assert!(offline.recorder.notices().is_empty());
}

#[tokio::test]
async fn test_account_refused_pushes_are_retried_then_dropped() {
    let (remote, server) = remote_with(vec![(100, Anime::new(Some(5100), Some(24), "FINISHED"))]);
    let world = World::new(&server.base).await;
    connect_anilist(&world);
    sync::queue_watched_episode(&world.pool, 100, 3.0).await.unwrap();

    remote.lock().unwrap().broken = Some(500);
    let report = sync::drain(&world.ctx()).await;
    assert_eq!((report.updated, report.failed), (0, 1));
    assert_eq!(queued(&world, Service::AniList).await, vec![(100, PendingKind::Progress, 3, 1)]);
    let item = &sync::pending(&world.pool, Service::AniList).await.unwrap()[0];
    assert!(item.last_error.as_deref().unwrap_or_default().contains("500"), "{:?}", item.last_error);
    // A server error is not a sign-out.
    assert!(world.store.is_connected(Service::AniList));

    // A later episode starts counting again.
    sync::queue_watched_episode(&world.pool, 100, 4.0).await.unwrap();
    assert_eq!(queued(&world, Service::AniList).await, vec![(100, PendingKind::Progress, 4, 0)]);

    for attempt in 1..MAX_ATTEMPTS {
        sync::drain(&world.ctx()).await;
        assert_eq!(queued(&world, Service::AniList).await, vec![(100, PendingKind::Progress, 4, attempt)]);
    }
    sync::drain(&world.ctx()).await;
    assert!(queued(&world, Service::AniList).await.is_empty());

    // Working again: a new episode goes through.
    remote.lock().unwrap().broken = None;
    sync::queue_watched_episode(&world.pool, 100, 5.0).await.unwrap();
    sync::drain(&world.ctx()).await;
    assert_eq!(anilist_entry(&remote, 100), some("CURRENT", 5));
}

#[tokio::test]
async fn test_account_rejected_token_during_sync_signs_out_and_keeps_the_queue() {
    let (remote, server) = remote_with(vec![(100, Anime::new(Some(5100), Some(24), "FINISHED"))]);
    let world = World::new(&server.base).await;
    connect_anilist(&world);
    sync::queue_watched_episode(&world.pool, 100, 3.0).await.unwrap();
    sync::queue_watched_episode(&world.pool, 200, 1.0).await.unwrap();

    remote.lock().unwrap().broken = Some(401);
    let report = sync::drain(&world.ctx()).await;
    assert_eq!((report.updated, report.failed), (0, 1));
    assert!(!world.store.is_connected(Service::AniList));
    assert!(matches!(
        world.recorder.notices().as_slice(),
        [Notice::SignedOut { service: Service::AniList, .. }]
    ));
    // One request, then it stopped; both pushes wait for the next sign-in.
    assert_eq!(server.requests().len(), 1);
    assert_eq!(queued(&world, Service::AniList).await.len(), 2);
    assert!(queued(&world, Service::AniList).await.iter().all(|row| row.3 == 0));
}

#[tokio::test]
async fn test_account_watchlist_addition_adds_as_planning_only_when_absent() {
    let (remote, server) = remote_with(vec![
        (100, Anime::new(Some(5100), Some(24), "FINISHED")),
        (200, Anime::new(Some(5200), Some(24), "FINISHED").on_anilist("CURRENT", 7).on_mal("completed", 24)),
    ]);
    let world = World::new(&server.base).await;
    connect_anilist(&world);
    connect_mal(&world);

    sync::queue_watchlist_addition(&world.ctx(), 100).await.unwrap();
    sync::queue_watchlist_addition(&world.ctx(), 200).await.unwrap();
    let report = sync::drain(&world.ctx()).await;
    assert_eq!((report.updated, report.skipped, report.failed), (2, 2, 0));

    assert_eq!(anilist_entry(&remote, 100), some("PLANNING", 0));
    assert_eq!(mal_entry(&remote, 100), some("plan_to_watch", 0));
    assert_eq!(mutations(&server), vec![json!({ "mediaId": 100, "status": "PLANNING" })]);
    let update = &mal_updates(&server)[0];
    assert_eq!(update.form("status").as_deref(), Some("plan_to_watch"));
    assert_eq!(update.form("num_watched_episodes"), None);
    // Already on the lists: left exactly as it was.
    assert_eq!(anilist_entry(&remote, 200), some("CURRENT", 7));
    assert_eq!(mal_entry(&remote, 200), some("completed", 24));

    // Removing from the in-app watchlist sends nothing.
    let before = server.requests().len();
    sync::cancel_watchlist_addition(&world.pool, 100).await.unwrap();
    sync::drain(&world.ctx()).await;
    assert_eq!(server.requests().len(), before);
    assert_eq!(anilist_entry(&remote, 100), some("PLANNING", 0));
    assert_eq!(mal_entry(&remote, 100), some("plan_to_watch", 0));
}

// ---- Editing an entry by hand -------------------------------------------------------------

#[tokio::test]
async fn test_account_save_entry_writes_what_was_chosen_to_both_lists() {
    let (remote, server) = remote_with(vec![
        (100, Anime::new(Some(5100), Some(12), "FINISHED").on_anilist("CURRENT", 9).on_mal("watching", 9)),
        (200, Anime::new(None, Some(12), "FINISHED").on_anilist("CURRENT", 9)),
    ]);
    let world = World::new(&server.base).await;
    connect_anilist(&world);
    connect_mal(&world);
    // A push that failed earlier must not undo the edit afterwards.
    sync::queue_watched_episode(&world.pool, 100, 9.0).await.unwrap();

    // Unlike a watched episode, an edit may lower the progress.
    let saved = sync::save_entry(&world.ctx(), 100, Some("PAUSED".to_string()), Some(4)).await.unwrap();
    assert_eq!(saved.entry.id, 100);
    assert_eq!(saved.entry.progress, 4);
    assert_eq!(saved.entry.status.as_deref(), Some("PAUSED"));
    assert_eq!(saved.mal, MalMirror::Saved);
    assert_eq!(anilist_entry(&remote, 100), some("PAUSED", 4));
    assert_eq!(mal_entry(&remote, 100), some("on_hold", 4));
    assert!(queued(&world, Service::AniList).await.is_empty());
    assert!(queued(&world, Service::Mal).await.is_empty());

    // Progress is clamped to the episode count.
    let saved = sync::save_entry(&world.ctx(), 100, Some("COMPLETED".to_string()), Some(40)).await.unwrap();
    assert_eq!(saved.entry.progress, 12);
    assert_eq!(mal_entry(&remote, 100), some("completed", 12));
    let saved = sync::save_entry(&world.ctx(), 100, None, Some(-3)).await.unwrap();
    assert_eq!(saved.entry.progress, 0);

    // No MAL id: AniList only.
    let saved = sync::save_entry(&world.ctx(), 200, Some("DROPPED".to_string()), Some(2)).await.unwrap();
    assert_eq!(saved.mal, MalMirror::NoMalId);
    assert_eq!(anilist_entry(&remote, 200), some("DROPPED", 2));

    let result = sync::save_entry(&world.ctx(), 100, Some("WATCHING".to_string()), Some(1)).await;
    assert!(matches!(result, Err(AppError::Other(_))), "{:?}", result);
    let result = sync::save_entry(&world.ctx(), 999, Some("CURRENT".to_string()), Some(1)).await;
    assert!(matches!(result, Err(AppError::Other(_))), "{:?}", result);

    assert_eq!(
        serde_json::to_value(&saved).unwrap()["mal"],
        json!({ "state": "no_mal_id" })
    );
    assert_eq!(
        serde_json::to_value(MalMirror::Failed { message: "x".to_string() }).unwrap(),
        json!({ "state": "failed", "message": "x" })
    );
}

#[tokio::test]
async fn test_account_save_entry_without_mal_or_anilist() {
    let (remote, server) = remote_with(vec![(100, Anime::new(Some(5100), Some(12), "FINISHED").on_mal("watching", 2))]);
    let world = World::new(&server.base).await;

    // AniList not connected: nothing is sent.
    let result = sync::save_entry(&world.ctx(), 100, Some("CURRENT".to_string()), Some(3)).await;
    assert!(matches!(result, Err(AppError::AuthRequired)));
    assert!(server.requests().is_empty());

    // MAL not connected: AniList only.
    connect_anilist(&world);
    let saved = sync::save_entry(&world.ctx(), 100, Some("CURRENT".to_string()), Some(3)).await.unwrap();
    assert_eq!(saved.mal, MalMirror::NotConnected);
    assert_eq!(anilist_entry(&remote, 100), some("CURRENT", 3));
    assert_eq!(mal_entry(&remote, 100), some("watching", 2));
    assert!(server.requests_to("/v2/").is_empty());
}

#[tokio::test]
async fn test_account_save_entry_reports_a_failed_mal_copy() {
    let (remote, server) = remote_with(vec![(100, Anime::new(Some(5100), Some(12), "FINISHED").on_mal("watching", 2))]);
    let world = World::new(&server.base).await;
    connect_anilist(&world);
    connect_mal(&world);
    remote.lock().unwrap().mal_broken = Some(503);

    // AniList is saved; the failure of the copy is reported, not returned as an error.
    let saved = sync::save_entry(&world.ctx(), 100, Some("CURRENT".to_string()), Some(5)).await.unwrap();
    assert_eq!(anilist_entry(&remote, 100), some("CURRENT", 5));
    assert_eq!(mal_entry(&remote, 100), some("watching", 2));
    match saved.mal {
        MalMirror::Failed { message } => assert!(message.contains("503"), "{}", message),
        other => panic!("{:?}", other),
    }
    assert!(world.store.is_connected(Service::Mal));
    assert_eq!(server.requests_to("/v2/").len(), 1);
}

// ---- List statuses ------------------------------------------------------------------------

async fn local_status(world: &World, id: i32) -> Option<(String, bool)> {
    db::get_watchlist_status(&world.pool, id).await.unwrap()
}

#[tokio::test]
async fn test_account_connected_tracker_status_is_shown_and_local_change_pushed() {
    let (remote, server) = remote_with(vec![
        (100, Anime::new(Some(5100), Some(24), "FINISHED").on_anilist("PAUSED", 4).on_mal("on_hold", 4)),
        (200, Anime::new(Some(5200), Some(12), "FINISHED").on_anilist("REPEATING", 2)),
    ]);
    let world = World::new(&server.base).await;
    connect_anilist(&world);
    connect_mal(&world);
    db::add_to_watchlist(&world.pool, 100, "Show", None, None, None).await.unwrap();
    db::add_to_watchlist(&world.pool, 200, "Other", None, None, None).await.unwrap();

    // The tracker wins: its status replaces the watchlist's.
    let shown = sync::list_status(&world.ctx(), 100).await.unwrap();
    assert_eq!(shown, sync::ListStatus { status: Some("PAUSED".to_string()), synced: true, source: "anilist" });
    assert_eq!(local_status(&world, 100).await, Some(("PAUSED".to_string(), true)));
    // The whole list is read back at once too; a rewatch is "Watching" here.
    assert_eq!(sync::pull_statuses(&world.ctx()).await.unwrap(), 1);
    assert_eq!(local_status(&world, 200).await, Some(("CURRENT".to_string(), true)));

    // A status chosen here is sent to both lists and then counts as in sync.
    sync::set_status(&world.pool, 100, "Show", None, None, None, "DROPPED").await.unwrap();
    assert_eq!(local_status(&world, 100).await, Some(("DROPPED".to_string(), false)));
    let report = sync::drain(&world.ctx()).await;
    assert_eq!((report.updated, report.failed), (2, 0));
    assert_eq!(anilist_entry(&remote, 100), some("DROPPED", 4));
    assert_eq!(mal_entry(&remote, 100), some("dropped", 4));
    assert_eq!(mutations(&server).last().unwrap(), &json!({ "mediaId": 100, "status": "DROPPED" }));
    assert_eq!(local_status(&world, 100).await, Some(("DROPPED".to_string(), true)));

    // Unknown statuses are refused.
    assert!(sync::set_status(&world.pool, 100, "Show", None, None, None, "WATCHING").await.is_err());
}

#[tokio::test]
async fn test_account_status_chosen_while_anilist_is_down_is_pushed_on_recovery() {
    let (remote, server) = remote_with(vec![(100, Anime::new(Some(5100), Some(24), "FINISHED").on_anilist("PLANNING", 0))]);
    let world = World::new(&server.base).await;
    connect_anilist(&world);
    db::add_to_watchlist(&world.pool, 100, "Show", None, None, None).await.unwrap();

    remote.lock().unwrap().broken = Some(503);
    sync::set_status(&world.pool, 100, "Show", None, None, None, "CURRENT").await.unwrap();
    let report = sync::drain(&world.ctx()).await;
    assert_eq!((report.updated, report.failed), (0, 1));
    assert_eq!(local_status(&world, 100).await, Some(("CURRENT".to_string(), false)));
    // The watchlist's status is shown meanwhile, and a pull does not overwrite it.
    let shown = sync::list_status(&world.ctx(), 100).await.unwrap();
    assert_eq!(shown, sync::ListStatus { status: Some("CURRENT".to_string()), synced: false, source: "local" });
    assert!(world.store.is_connected(Service::AniList));

    remote.lock().unwrap().broken = None;
    assert_eq!(sync::pull_statuses(&world.ctx()).await.unwrap(), 0);
    assert_eq!(local_status(&world, 100).await, Some(("CURRENT".to_string(), false)));
    let report = sync::drain(&world.ctx()).await;
    assert_eq!(report.updated, 1);
    assert_eq!(anilist_entry(&remote, 100), some("CURRENT", 0));
    assert_eq!(local_status(&world, 100).await, Some(("CURRENT".to_string(), true)));
}

#[tokio::test]
async fn test_account_status_without_account_stays_local_until_connected() {
    let (remote, server) = remote_with(vec![(100, Anime::new(Some(5100), Some(24), "FINISHED"))]);
    let world = World::new(&server.base).await;

    // Not on the watchlist yet: choosing a status puts it there.
    sync::set_status(&world.pool, 100, "Show", Some("c.jpg"), Some("TV"), Some(80), "COMPLETED").await.unwrap();
    let report = sync::drain(&world.ctx()).await;
    assert_eq!(report, sync::DrainReport::default());
    assert!(server.requests().is_empty());
    let shown = sync::list_status(&world.ctx(), 100).await.unwrap();
    assert_eq!(shown, sync::ListStatus { status: Some("COMPLETED".to_string()), synced: false, source: "local" });
    assert_eq!(sync::pull_statuses(&world.ctx()).await.unwrap(), 0);
    let row = &db::get_watchlist(&world.pool).await.unwrap()[0];
    assert_eq!((row.title.as_str(), row.status.as_str(), row.synced), ("Show", "COMPLETED", false));

    // Connected later: the waiting status is sent.
    connect_anilist(&world);
    sync::drain(&world.ctx()).await;
    assert_eq!(anilist_entry(&remote, 100), some("COMPLETED", 0));
    assert_eq!(local_status(&world, 100).await, Some(("COMPLETED".to_string(), true)));
}

// ---- Stand-in ids ---------------------------------------------------------------------------

#[tokio::test]
async fn test_account_stand_in_with_mal_id_syncs_to_mal_only() {
    // -(1 × 100 000 000 + 5300): an anime known by its MyAnimeList id 5300 only.
    let stand_in = -100_005_300;
    assert_eq!(sync::stand_in_mal_id(stand_in), Some(5300));
    let (remote, server) = remote_with(vec![(1, Anime::new(Some(5300), Some(12), "FINISHED"))]);
    let world = World::new(&server.base).await;
    connect_anilist(&world);
    connect_mal(&world);

    assert!(sync::queue_watched_episode(&world.pool, stand_in, 3.0).await.unwrap());
    assert!(queued(&world, Service::AniList).await.is_empty());
    assert_eq!(queued(&world, Service::Mal).await, vec![(stand_in, PendingKind::Progress, 3, 0)]);
    let report = sync::drain(&world.ctx()).await;
    assert_eq!((report.updated, report.failed), (1, 0));
    assert_eq!(mal_entry(&remote, 1), some("watching", 3));
    assert_eq!(mal_updates(&server)[0].url, "/v2/anime/5300/my_list_status");

    // A status goes to MAL as well; AniList is never asked about a stand-in.
    sync::set_status(&world.pool, stand_in, "Stand-in", None, None, None, "PAUSED").await.unwrap();
    sync::drain(&world.ctx()).await;
    assert_eq!(mal_entry(&remote, 1), some("on_hold", 3));
    assert!(server.requests_to("/graphql").is_empty());
    assert_eq!(local_status(&world, stand_in).await, Some(("PAUSED".to_string(), true)));

    // Stand-ins without a MyAnimeList id (Kitsu 77 here), and id 0, are still skipped.
    for no_mal in [-200_000_077, 0, -5] {
        assert_eq!(sync::stand_in_mal_id(no_mal), None);
        assert!(!sync::queue_watched_episode(&world.pool, no_mal, 1.0).await.unwrap());
        assert!(!sync::queue_watchlist_addition(&world.ctx(), no_mal).await.unwrap());
    }
}
