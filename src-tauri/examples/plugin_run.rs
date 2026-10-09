//! Runs a streaming plugin against the app's real plugin runtime from the command line.
//! See `usage()` below, or run it without arguments.

use anilili_lib::models::{AnimeQuery, Category};
use anilili_lib::plugins::engine::{EngineLimits, EngineSpec, PluginRuntime};
use anilili_lib::plugins::provider::{PluginProvider, ProviderSettings};
use anilili_lib::plugins::registry::{PluginRegistry, RegistryOptions};
use anilili_lib::providers::AnimeProvider;
use anilili_lib::web_session::{app_session, SessionUse};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

fn usage() -> ! {
    eprintln!(
        "usage: plugin_run <manifest.json | payload.js | payload.ts> <command> [options]

commands (raw plugin calls, printed as JSON):
  settings                           getSettings()
  search <title>                     search({{ media, query, dub, year }})
  episodes <id>                      findEpisodes(id)
  server <episode json> [server]     findEpisodeServer(episode, server); server defaults to
                                     the first of getSettings().episodeServers

commands (the path the app takes, through title matching and stream verification):
  list <title>                       episode lists the app would show (sub and dub)
  play <title> <episode number>      sources for one episode, streams verified

  repo <link>                        (no plugin file) installs a repository into a scratch
                                     directory the way Settings does and prints what it holds

options:
  --dub                 ask for dubbed audio (search, play)
  --year <n>            release year passed to search and used for matching
  --episodes <n>        episode count used for matching (list, play)
  --anilist <id>        AniList id put into media.id (default 0)
  --alt <title>         a further known title (English title, synonym); may repeat
  --media <json>        the whole media object for search, instead of the one built from flags
  --payload <file>      payload file to use with a manifest (default: inline payload, then a
                        local file named like the end of payloadURI, then download payloadURI)
  --timeout <seconds>   limit for each plugin call (default 30)
  --allow-local         let fetch reach localhost and private addresses
  --quiet               do not print the plugin's console output

Console output of the plugin goes to stderr, results to stdout. The exit status is 1 when the
call fails; the reason is printed to stderr."
    );
    std::process::exit(2);
}

struct Options {
    positional: Vec<String>,
    dub: bool,
    year: Option<i32>,
    episodes: Option<i32>,
    anilist: i64,
    alt: Vec<String>,
    media: Option<Value>,
    payload: Option<PathBuf>,
    timeout: Duration,
    allow_local: bool,
    quiet: bool,
}

fn parse_args() -> Options {
    let mut options = Options {
        positional: Vec::new(),
        dub: false,
        year: None,
        episodes: None,
        anilist: 0,
        alt: Vec::new(),
        media: None,
        payload: None,
        timeout: Duration::from_secs(30),
        allow_local: false,
        quiet: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next().unwrap_or_else(|| {
                eprintln!("{} needs a value", name);
                std::process::exit(2)
            })
        };
        match arg.as_str() {
            "--dub" => options.dub = true,
            "--allow-local" => options.allow_local = true,
            "--quiet" => options.quiet = true,
            "--year" => options.year = value("--year").parse().ok(),
            "--episodes" => options.episodes = value("--episodes").parse().ok(),
            "--anilist" => options.anilist = value("--anilist").parse().unwrap_or(0),
            "--alt" => options.alt.push(value("--alt")),
            "--media" => {
                let text = value("--media");
                options.media = Some(serde_json::from_str(&text).unwrap_or_else(|e| fail(format!("--media is not JSON: {}", e))));
            }
            "--payload" => options.payload = Some(PathBuf::from(value("--payload"))),
            "--timeout" => {
                options.timeout = Duration::from_secs_f64(value("--timeout").parse().unwrap_or(30.0));
            }
            "-h" | "--help" => usage(),
            _ => options.positional.push(arg),
        }
    }
    options
}

fn fail(reason: impl std::fmt::Display) -> ! {
    eprintln!("error: {}", reason);
    std::process::exit(1);
}

struct Loaded {
    id: String,
    source: String,
    typescript: bool,
    preferences: HashMap<String, String>,
}

fn is_typescript(name: &str) -> bool {
    name.split(['?', '#']).next().unwrap_or(name).to_ascii_lowercase().ends_with(".ts")
}

async fn load(path: &Path, payload_override: Option<&Path>) -> Result<Loaded, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    if !name.to_ascii_lowercase().ends_with(".json") {
        let id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("plugin").to_string();
        return Ok(Loaded {
            id,
            typescript: is_typescript(&name),
            source: text,
            preferences: HashMap::new(),
        });
    }

    let manifest: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {}", path.display(), e))?;
    let id = manifest["id"].as_str().unwrap_or("plugin").to_string();
    let language = manifest["language"].as_str().unwrap_or_default().to_ascii_lowercase();
    let payload_uri = manifest["payloadURI"].as_str().unwrap_or_default().trim().to_string();

    let (source, origin) = if let Some(file) = payload_override {
        let source = std::fs::read_to_string(file).map_err(|e| format!("{}: {}", file.display(), e))?;
        (source, file.display().to_string())
    } else if let Some(inline) = manifest["payload"].as_str().filter(|p| !p.trim().is_empty()) {
        (inline.to_string(), String::new())
    } else {
        let local = payload_uri
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .and_then(|name| path.parent().map(|dir| dir.join(name.split(['?', '#']).next().unwrap_or(name))))
            .filter(|candidate| candidate.is_file());
        match local {
            Some(file) => {
                eprintln!("using the local payload {}", file.display());
                let source = std::fs::read_to_string(&file).map_err(|e| format!("{}: {}", file.display(), e))?;
                (source, file.display().to_string())
            }
            None if payload_uri.starts_with("http://") || payload_uri.starts_with("https://") => {
                eprintln!("downloading {}", payload_uri);
                let client = anilili_lib::clients::http::http_client().map_err(|e| e.reason())?;
                let response = client.get(&payload_uri).send().await.map_err(|e| e.to_string())?;
                if !response.status().is_success() {
                    return Err(format!("{} answered HTTP {}", payload_uri, response.status().as_u16()));
                }
                (response.text().await.map_err(|e| e.to_string())?, payload_uri.clone())
            }
            None => return Err("the manifest has no payload and no usable payloadURI".to_string()),
        }
    };
    let typescript = match language.as_str() {
        "typescript" => true,
        "javascript" => false,
        _ => is_typescript(&origin) || is_typescript(&payload_uri),
    };
    // The manifest's settings at their defaults, as the app runs a plugin nobody configured.
    let config = anilili_lib::plugins::userconfig::UserConfig::from_manifest(&manifest);
    let (source, preferences) = anilili_lib::plugins::userconfig::apply(&source, config.as_ref(), None)
        .map_err(|e| e.message().to_string())?;
    Ok(Loaded {
        id,
        source,
        typescript,
        preferences,
    })
}

fn print(value: &impl serde::Serialize) {
    println!("{}", serde_json::to_string_pretty(value).unwrap_or_else(|e| e.to_string()));
}

fn main() {
    let options = parse_args();
    if options.positional.len() < 2 {
        usage();
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|e| fail(e));
    let code = runtime.block_on(run(options));
    // Plugin threads and pending requests are not waited for.
    std::process::exit(code);
}

async fn run(options: Options) -> i32 {
    if options.positional[0] == "repo" {
        let dir = std::env::temp_dir().join(format!("anilili-plugin-run-{}", std::process::id()));
        let registry = PluginRegistry::open(dir.clone(), RegistryOptions::default());
        let outcome = registry.add_repo(&options.positional[1]).await;
        let _ = match &outcome {
            Ok(repo) => {
                print(&json!({ "repo": repo, "plugins": registry.list_plugins() }));
                std::fs::remove_dir_all(&dir)
            }
            Err(_) => std::fs::remove_dir_all(&dir),
        };
        return match outcome {
            Ok(_) => 0,
            Err(reason) => fail(reason),
        };
    }
    let path = PathBuf::from(&options.positional[0]);
    let command = options.positional[1].as_str();
    let arg = |index: usize| options.positional.get(index + 2).cloned();

    let loaded = load(&path, options.payload.as_deref()).await.unwrap_or_else(|e| fail(e));
    let quiet = options.quiet;
    let spec = EngineSpec {
        id: loaded.id.clone(),
        source: loaded.source,
        typescript: loaded.typescript,
        preferences: loaded.preferences,
        limits: EngineLimits::default(),
        session: app_session(),
        allow_private_network: options.allow_local,
        log: Some(Arc::new(move |level: &str, text: &str| {
            if !quiet {
                eprintln!("[{}] {}", level, text);
            }
        })),
    };
    let plugin = Arc::new(PluginRuntime::new(spec));
    if let Err(reason) = plugin.engine() {
        fail(format!("the plugin did not load: {}", reason));
    }
    let timeout = options.timeout;
    let call = |method: &'static str, args: Vec<Value>| {
        let plugin = plugin.clone();
        async move {
            plugin
                .call(method, &args, timeout, SessionUse::Existing)
                .await
                .unwrap_or_else(|e| fail(format!("{} failed: {}", method, e.reason())))
        }
    };

    let title = || arg(0).unwrap_or_else(|| usage());
    let query_for = |title: &str| {
        let mut query = AnimeQuery::new(options.anilist, None, Some(title));
        for alt in &options.alt {
            query.add_title(alt);
        }
        query.year = options.year;
        query.episodes = options.episodes;
        query
    };

    match command {
        "settings" => print(&call("getSettings", vec![]).await),
        "search" => {
            let title = title();
            let media = options.media.clone().unwrap_or_else(|| {
                json!({
                    "id": options.anilist,
                    "romajiTitle": title,
                    "englishTitle": options.alt.first().cloned().unwrap_or_else(|| title.clone()),
                    "synonyms": options.alt.iter().skip(1).collect::<Vec<_>>(),
                    "isAdult": false,
                    "format": "TV",
                    "status": "FINISHED",
                    "episodeCount": options.episodes,
                    "absoluteSeasonOffset": 0,
                    "startDate": options.year.map(|year| json!({ "year": year })),
                })
            });
            let opts = json!({ "media": media, "query": title, "dub": options.dub, "year": options.year });
            print(&call("search", vec![opts]).await);
        }
        "episodes" => {
            let id = arg(0).unwrap_or_else(|| usage());
            print(&call("findEpisodes", vec![Value::String(id)]).await);
        }
        "server" => {
            let episode: Value = serde_json::from_str(&arg(0).unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(format!("the episode is not JSON: {}", e)));
            let server = match arg(1) {
                Some(server) => server,
                None => call("getSettings", vec![]).await["episodeServers"][0]
                    .as_str()
                    .unwrap_or("default")
                    .to_string(),
            };
            print(&call("findEpisodeServer", vec![episode, Value::String(server)]).await);
        }
        "list" | "play" => {
            let settings = ProviderSettings::from_json(&call("getSettings", vec![]).await);
            let provider = PluginProvider::new(&loaded.id, plugin.clone(), settings, "dev");
            let query = query_for(&title());
            let listing = provider
                .get_episodes_with(&query, SessionUse::Create)
                .await
                .unwrap_or_else(|e| fail(format!("episode list failed: {}", e.reason())));
            if command == "list" {
                print(&listing);
                return 0;
            }
            let number: f64 = arg(1)
                .and_then(|n| n.parse().ok())
                .unwrap_or_else(|| fail("play needs an episode number"));
            let category = if options.dub { Category::Dub } else { Category::Sub };
            let list = if options.dub { &listing.dub } else { &listing.sub };
            let episode = list
                .iter()
                .find(|episode| (episode.number - number).abs() < 0.01)
                .unwrap_or_else(|| {
                    fail(format!(
                        "episode {} is not in the {} list ({} episodes)",
                        number,
                        category.as_str(),
                        list.len()
                    ))
                });
            let sources = provider
                .get_sources_with(episode, category, SessionUse::Create)
                .await
                .unwrap_or_else(|e| fail(format!("sources failed: {}", e.reason())));
            eprintln!("verified: the first stream below answered with a playlist and a segment (or the file's first bytes)");
            print(&sources);
        }
        _ => usage(),
    }
    0
}
