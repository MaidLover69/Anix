//! One plugin's JavaScript context.
//!
//! Each plugin gets its own QuickJS runtime on its own thread: nothing is shared between
//! plugins, the runtime has a memory limit and a stack limit, and a script that runs too long
//! without yielding is interrupted. The context has no module loader, no file system and no
//! process access; the only way out is the host functions installed here (`fetch` through the
//! app's HTTP clients, timers, logging, the HTML parser and a few byte helpers).
//!
//! Calls are asynchronous on the JavaScript side: the thread drives the job queue and is woken
//! by completed requests and timers, so several calls into one plugin interleave.

use crate::plugins::dom::DomStore;
use crate::plugins::net::{NetContext, WireRequest, WireResponse};
use crate::plugins::transpile::typescript_to_javascript;
use crate::web_session::{SessionUse, SiteSession};
use aes::cipher::block_padding::{NoPadding, Pkcs7};
use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use base64::Engine as _;
use hmac::{Hmac, Mac};
use rquickjs::context::EvalOptions;
use rquickjs::{CatchResultExt, Context, Ctx, Function, Object, Runtime, TypedArray, Value};
use serde_json::Value as Json;
use sha2::Digest;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::Read;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::runtime::Handle;
use tokio::sync::oneshot;

const PRELUDE: &str = include_str!("prelude.js");

/// Limits of one plugin context.
#[derive(Debug, Clone, Copy)]
pub struct EngineLimits {
    /// Heap the context may use.
    pub memory_bytes: usize,
    pub stack_bytes: usize,
    /// Longest stretch of script that may run without returning to the event loop.
    pub slice: Duration,
    /// Requests a context may have in flight at once.
    pub concurrent_requests: usize,
}

impl Default for EngineLimits {
    fn default() -> Self {
        Self {
            memory_bytes: 96 * 1024 * 1024,
            stack_bytes: 1024 * 1024,
            slice: Duration::from_secs(4),
            concurrent_requests: 24,
        }
    }
}

/// Longest a single `$sleep` blocks, and how much sleeping one slice may add to its limit.
const MAX_SLEEP: Duration = Duration::from_secs(2);
const MAX_SLEEP_PER_SLICE: Duration = Duration::from_secs(10);
const MAX_TIMER: Duration = Duration::from_secs(60);
const MAX_GUNZIP_BYTES: u64 = 64 * 1024 * 1024;
const MAX_LOG_CHARS: usize = 4000;
const LOAD_TIMEOUT: Duration = Duration::from_secs(10);

pub type LogSink = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// Everything needed to start (and restart) a plugin's context.
#[derive(Clone)]
pub struct EngineSpec {
    pub id: String,
    /// The payload as downloaded.
    pub source: String,
    pub typescript: bool,
    /// Values `$getUserPreference` answers with.
    pub preferences: HashMap<String, String>,
    pub limits: EngineLimits,
    pub session: Arc<dyn SiteSession>,
    /// Lets `fetch` reach loopback and private addresses (tests and the dev runner only).
    pub allow_private_network: bool,
    /// Receives `console` output as (level, text); the default writes to the app log.
    pub log: Option<LogSink>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PluginError {
    /// The call did not finish in the time it was given.
    Timeout,
    /// The plugin threw, rejected or returned something unusable.
    Script(String),
    /// The context was stopped (script ran too long, engine failure).
    Stopped(String),
}

impl PluginError {
    pub fn reason(&self) -> String {
        match self {
            PluginError::Timeout => "timed out".to_string(),
            PluginError::Script(message) | PluginError::Stopped(message) => message.clone(),
        }
    }
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason())
    }
}

enum Message {
    Call {
        id: u64,
        method: String,
        args: String,
        create_session: bool,
        handle: Handle,
        reply: oneshot::Sender<Result<String, PluginError>>,
    },
    Abandon(u64),
    FetchDone {
        generation: u64,
        id: u32,
        result: Result<WireResponse, String>,
    },
    Timer {
        generation: u64,
        id: u32,
    },
    Stop,
}

struct Shared {
    net: Arc<NetContext>,
    alive: AtomicBool,
    next_call: AtomicU64,
}

/// Handle to a running plugin context. Dropping it stops the thread.
pub struct PluginEngine {
    tx: Mutex<mpsc::Sender<Message>>,
    shared: Arc<Shared>,
}

struct CallGuard<'a> {
    engine: &'a PluginEngine,
    id: u64,
    finished: bool,
}

impl Drop for CallGuard<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.engine.send(Message::Abandon(self.id));
        }
    }
}

impl PluginEngine {
    /// Starts the context and evaluates the payload. Fails when the payload cannot be
    /// transpiled or evaluated, or defines no `Provider` class.
    pub fn start(spec: EngineSpec) -> Result<Self, String> {
        let script = if spec.typescript {
            typescript_to_javascript(&spec.source)?
        } else {
            spec.source.clone()
        };
        let net = Arc::new(NetContext::new(&spec.id, spec.session.clone(), spec.allow_private_network));
        let shared = Arc::new(Shared {
            net,
            alive: AtomicBool::new(true),
            next_call: AtomicU64::new(1),
        });

        let (tx, rx) = mpsc::channel::<Message>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let thread_shared = shared.clone();
        let thread_tx = tx.clone();
        let name = format!("plugin-{}", spec.id);
        std::thread::Builder::new()
            .name(name.chars().take(15).collect())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let shared = thread_shared;
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_thread(spec, script, rx, thread_tx, shared.clone(), ready_tx);
                }));
                shared.alive.store(false, Ordering::SeqCst);
                if outcome.is_err() {
                    tracing::error!("plugin thread {} stopped after an internal error", name);
                }
            })
            .map_err(|e| format!("could not start the plugin thread: {}", e))?;

        match ready_rx.recv_timeout(LOAD_TIMEOUT) {
            Ok(Ok(())) => Ok(Self {
                tx: Mutex::new(tx),
                shared,
            }),
            Ok(Err(reason)) => {
                let _ = tx.send(Message::Stop);
                Err(reason)
            }
            Err(_) => {
                let _ = tx.send(Message::Stop);
                Err("the plugin did not finish loading".to_string())
            }
        }
    }

    fn send(&self, message: Message) -> bool {
        let guard = self.tx.lock().unwrap_or_else(|p| p.into_inner());
        guard.send(message).is_ok()
    }

    pub fn is_alive(&self) -> bool {
        self.shared.alive.load(Ordering::SeqCst)
    }

    pub fn net(&self) -> &Arc<NetContext> {
        &self.shared.net
    }

    /// Calls `method` on a fresh `Provider` instance with JSON arguments and waits for the
    /// (possibly asynchronous) result. `access` says how far the call's requests may go to
    /// reach a site through the in-app browser.
    pub async fn call(
        &self,
        method: &str,
        args: &[Json],
        timeout: Duration,
        access: SessionUse,
    ) -> Result<Json, PluginError> {
        let args = serde_json::to_string(args).map_err(|e| PluginError::Script(e.to_string()))?;
        let id = self.shared.next_call.fetch_add(1, Ordering::SeqCst);
        let (reply, answer) = oneshot::channel();
        let sent = self.send(Message::Call {
            id,
            method: method.to_string(),
            args,
            create_session: access == SessionUse::Create,
            handle: Handle::current(),
            reply,
        });
        if !sent {
            return Err(PluginError::Stopped("the plugin is not running".to_string()));
        }
        let mut guard = CallGuard {
            engine: self,
            id,
            finished: false,
        };
        let outcome = tokio::time::timeout(timeout, answer).await;
        match outcome {
            Err(_) => Err(PluginError::Timeout),
            Ok(Err(_)) => {
                guard.finished = true;
                Err(PluginError::Stopped("the plugin stopped".to_string()))
            }
            Ok(Ok(result)) => {
                guard.finished = true;
                let json = result?;
                serde_json::from_str(&json)
                    .map_err(|e| PluginError::Script(format!("{} returned unreadable data: {}", method, e)))
            }
        }
    }
}

impl Drop for PluginEngine {
    fn drop(&mut self) {
        self.send(Message::Stop);
    }
}

// ---------------------------------------------------------------------- the plugin thread

struct HostState {
    id: String,
    generation: u64,
    tx: mpsc::Sender<Message>,
    net: Arc<NetContext>,
    limits: EngineLimits,
    log: Option<LogSink>,
    handle: RefCell<Option<Handle>>,
    dom: RefCell<DomStore>,
    finished: RefCell<Vec<(u64, bool, String)>>,
    requests_in_flight: Cell<usize>,
    calls_allowed_to_create: Cell<usize>,
    slept_this_slice: Cell<Duration>,
    /// When the running slice must be over, in milliseconds since `epoch`.
    deadline_ms: Arc<AtomicU64>,
    interrupted: Arc<AtomicBool>,
    epoch: Instant,
}

impl HostState {
    fn start_slice(&self) {
        self.slept_this_slice.set(Duration::ZERO);
        let end = self.epoch.elapsed() + self.limits.slice;
        self.deadline_ms.store(end.as_millis() as u64, Ordering::SeqCst);
    }

    fn extend_slice(&self, by: Duration) {
        self.deadline_ms.fetch_add(by.as_millis() as u64, Ordering::SeqCst);
    }
}

fn js_error<'js>(ctx: &Ctx<'js>, message: impl AsRef<str>) -> rquickjs::Error {
    rquickjs::Exception::throw_message(ctx, message.as_ref())
}

fn bytes_of(array: &TypedArray<'_, u8>) -> Vec<u8> {
    // Copied at once; no script runs while the borrowed slice is alive.
    unsafe { array.as_bytes() }.map(<[u8]>::to_vec).unwrap_or_default()
}

fn to_array<'js>(ctx: &Ctx<'js>, bytes: Vec<u8>) -> rquickjs::Result<TypedArray<'js, u8>> {
    TypedArray::new(ctx.clone(), bytes)
}

/// Standard or URL-safe alphabet, padded or not, ignoring whitespace.
pub fn decode_base64_lenient(text: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '=')
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            other => other,
        })
        .collect();
    base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(cleaned.as_bytes())
        .map_err(|_| "invalid base64 data".to_string())
}

fn digest(algorithm: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    Ok(match algorithm {
        "md5" => md5::Md5::digest(data).to_vec(),
        "sha1" => sha1::Sha1::digest(data).to_vec(),
        "sha256" => sha2::Sha256::digest(data).to_vec(),
        "sha512" => sha2::Sha512::digest(data).to_vec(),
        other => return Err(format!("unsupported digest '{}'", other)),
    })
}

fn hmac(algorithm: &str, key: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
    macro_rules! mac {
        ($hash:ty) => {{
            let mut mac = <Hmac<$hash> as Mac>::new_from_slice(key).map_err(|e| e.to_string())?;
            mac.update(data);
            mac.finalize().into_bytes().to_vec()
        }};
    }
    Ok(match algorithm {
        "md5" => mac!(md5::Md5),
        "sha1" => mac!(sha1::Sha1),
        "sha256" => mac!(sha2::Sha256),
        "sha512" => mac!(sha2::Sha512),
        other => return Err(format!("unsupported digest '{}'", other)),
    })
}

fn aes_apply(encrypt: bool, mode: &str, key: &[u8], iv: &[u8], data: &[u8], padded: bool) -> Result<Vec<u8>, String> {
    let bad_key = || "invalid AES key size".to_string();
    let bad_data = || "the data could not be decrypted with this key".to_string();
    if !padded && data.len() % 16 != 0 {
        return Err("data length must be a multiple of 16 bytes without padding".to_string());
    }
    match mode {
        "cbc" => {
            if iv.len() != 16 {
                return Err("IV length must be 16 bytes".to_string());
            }
            macro_rules! cbc_mode {
                ($aes:ty) => {{
                    if encrypt {
                        let cipher = cbc::Encryptor::<$aes>::new_from_slices(key, iv).map_err(|_| bad_key())?;
                        Ok(if padded {
                            cipher.encrypt_padded_vec_mut::<Pkcs7>(data)
                        } else {
                            cipher.encrypt_padded_vec_mut::<NoPadding>(data)
                        })
                    } else {
                        let cipher = cbc::Decryptor::<$aes>::new_from_slices(key, iv).map_err(|_| bad_key())?;
                        if padded {
                            cipher.decrypt_padded_vec_mut::<Pkcs7>(data).map_err(|_| bad_data())
                        } else {
                            cipher.decrypt_padded_vec_mut::<NoPadding>(data).map_err(|_| bad_data())
                        }
                    }
                }};
            }
            match key.len() {
                16 => cbc_mode!(aes::Aes128),
                24 => cbc_mode!(aes::Aes192),
                32 => cbc_mode!(aes::Aes256),
                _ => Err(bad_key()),
            }
        }
        "ecb" => {
            // ECB is CBC with every block chained to zeroes, done block by block.
            let zero = [0u8; 16];
            let mut input = data.to_vec();
            if encrypt && padded {
                let pad = 16 - input.len() % 16;
                input.extend(std::iter::repeat(pad as u8).take(pad));
            }
            if input.len() % 16 != 0 {
                return Err(bad_data());
            }
            let mut out = Vec::with_capacity(input.len());
            for block in input.chunks(16) {
                out.extend(aes_apply(encrypt, "cbc", key, &zero, block, false)?);
            }
            if !encrypt && padded {
                let pad = *out.last().ok_or_else(bad_data)? as usize;
                if pad == 0 || pad > 16 || pad > out.len() {
                    return Err(bad_data());
                }
                out.truncate(out.len() - pad);
            }
            Ok(out)
        }
        other => Err(format!("unsupported cipher mode '{}'", other)),
    }
}

fn gunzip(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let read = if data.starts_with(&[0x1f, 0x8b]) {
        flate2::read::MultiGzDecoder::new(data)
            .take(MAX_GUNZIP_BYTES + 1)
            .read_to_end(&mut out)
    } else {
        flate2::read::ZlibDecoder::new(data)
            .take(MAX_GUNZIP_BYTES + 1)
            .read_to_end(&mut out)
    };
    read.map_err(|e| format!("the data is not valid gzip: {}", e))?;
    if out.len() as u64 > MAX_GUNZIP_BYTES {
        return Err("the decompressed data is too large".to_string());
    }
    Ok(out)
}

fn url_parts(url: &url::Url) -> String {
    use url::quirks;
    serde_json::json!({
        "href": quirks::href(url),
        "origin": quirks::origin(url),
        "protocol": quirks::protocol(url),
        "username": quirks::username(url),
        "password": quirks::password(url),
        "host": quirks::host(url),
        "hostname": quirks::hostname(url),
        "port": quirks::port(url),
        "pathname": quirks::pathname(url),
        "search": quirks::search(url),
        "hash": quirks::hash(url),
    })
    .to_string()
}

fn install_host<'js>(ctx: &Ctx<'js>, state: Rc<HostState>) -> rquickjs::Result<Object<'js>> {
    let host = Object::new(ctx.clone())?;

    let st = state.clone();
    host.set(
        "log",
        Function::new(ctx.clone(), move |level: String, text: String| {
            let text: String = if text.chars().count() > MAX_LOG_CHARS {
                text.chars().take(MAX_LOG_CHARS).collect::<String>() + "…"
            } else {
                text
            };
            match &st.log {
                Some(sink) => sink(&level, &text),
                None => match level.as_str() {
                    "warn" | "error" => tracing::info!("plugin {} [{}]: {}", st.id, level, text),
                    _ => tracing::debug!("plugin {} [{}]: {}", st.id, level, text),
                },
            }
        })?,
    )?;

    let st = state.clone();
    host.set(
        "done",
        Function::new(ctx.clone(), move |call: f64, ok: bool, payload: String| {
            st.finished.borrow_mut().push((call as u64, ok, payload));
        })?,
    )?;

    let st = state.clone();
    host.set(
        "fetch",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, id: u32, request: String, body: TypedArray<'js, u8>| -> rquickjs::Result<()> {
                let fail = |reason: String| {
                    let _ = st.tx.send(Message::FetchDone {
                        generation: st.generation,
                        id,
                        result: Err(reason),
                    });
                };
                let request: WireRequest = match serde_json::from_str(&request) {
                    Ok(request) => request,
                    Err(e) => return Err(js_error(&ctx, format!("fetch: {}", e))),
                };
                if st.requests_in_flight.get() >= st.limits.concurrent_requests {
                    fail("too many requests at once".to_string());
                    return Ok(());
                }
                let Some(handle) = st.handle.borrow().clone() else {
                    fail("the network is not available".to_string());
                    return Ok(());
                };
                st.requests_in_flight.set(st.requests_in_flight.get() + 1);
                let body = bytes_of(&body);
                let access = if st.calls_allowed_to_create.get() > 0 {
                    SessionUse::Create
                } else {
                    SessionUse::Existing
                };
                let (net, tx, generation) = (st.net.clone(), st.tx.clone(), st.generation);
                handle.spawn(async move {
                    let result = net.perform(request, body, access).await;
                    let _ = tx.send(Message::FetchDone { generation, id, result });
                });
                Ok(())
            },
        )?,
    )?;

    let st = state.clone();
    host.set(
        "timer",
        Function::new(ctx.clone(), move |id: u32, ms: f64| {
            let Some(handle) = st.handle.borrow().clone() else {
                return;
            };
            let delay = Duration::from_millis(ms.max(0.0) as u64).min(MAX_TIMER);
            let (tx, generation) = (st.tx.clone(), st.generation);
            handle.spawn(async move {
                tokio::time::sleep(delay).await;
                let _ = tx.send(Message::Timer { generation, id });
            });
        })?,
    )?;

    let st = state.clone();
    host.set(
        "sleep",
        Function::new(ctx.clone(), move |ms: f64| {
            let wanted = Duration::from_millis(ms.max(0.0) as u64).min(MAX_SLEEP);
            let room = MAX_SLEEP_PER_SLICE.saturating_sub(st.slept_this_slice.get());
            let pause = wanted.min(room);
            if pause.is_zero() {
                return;
            }
            st.slept_this_slice.set(st.slept_this_slice.get() + pause);
            st.extend_slice(pause);
            std::thread::sleep(pause);
        })?,
    )?;

    host.set(
        "utf8Encode",
        Function::new(ctx.clone(), |ctx: Ctx<'js>, text: String| to_array(&ctx, text.into_bytes()))?,
    )?;
    host.set(
        "utf8Decode",
        Function::new(ctx.clone(), |bytes: TypedArray<'js, u8>| {
            let bytes = bytes_of(&bytes);
            let text = String::from_utf8_lossy(&bytes);
            text.strip_prefix('\u{feff}').unwrap_or(&text).to_string()
        })?,
    )?;
    host.set(
        "b64Encode",
        Function::new(ctx.clone(), |bytes: TypedArray<'js, u8>| {
            base64::engine::general_purpose::STANDARD.encode(bytes_of(&bytes))
        })?,
    )?;
    host.set(
        "b64Decode",
        Function::new(ctx.clone(), |ctx: Ctx<'js>, text: String| {
            let bytes = decode_base64_lenient(&text).map_err(|e| js_error(&ctx, e))?;
            to_array(&ctx, bytes)
        })?,
    )?;
    host.set(
        "gunzip",
        Function::new(ctx.clone(), |ctx: Ctx<'js>, bytes: TypedArray<'js, u8>| {
            let out = gunzip(&bytes_of(&bytes)).map_err(|e| js_error(&ctx, e))?;
            to_array(&ctx, out)
        })?,
    )?;
    host.set(
        "digest",
        Function::new(ctx.clone(), |ctx: Ctx<'js>, algorithm: String, bytes: TypedArray<'js, u8>| {
            let out = digest(&algorithm, &bytes_of(&bytes)).map_err(|e| js_error(&ctx, e))?;
            to_array(&ctx, out)
        })?,
    )?;
    host.set(
        "hmac",
        Function::new(
            ctx.clone(),
            |ctx: Ctx<'js>, algorithm: String, key: TypedArray<'js, u8>, bytes: TypedArray<'js, u8>| {
                let out = hmac(&algorithm, &bytes_of(&key), &bytes_of(&bytes)).map_err(|e| js_error(&ctx, e))?;
                to_array(&ctx, out)
            },
        )?,
    )?;
    host.set(
        "aes",
        Function::new(
            ctx.clone(),
            |ctx: Ctx<'js>,
             op: String,
             mode: String,
             key: TypedArray<'js, u8>,
             iv: TypedArray<'js, u8>,
             data: TypedArray<'js, u8>,
             padded: bool| {
                let out = aes_apply(
                    op == "encrypt",
                    &mode,
                    &bytes_of(&key),
                    &bytes_of(&iv),
                    &bytes_of(&data),
                    padded,
                )
                .map_err(|e| js_error(&ctx, e))?;
                to_array(&ctx, out)
            },
        )?,
    )?;
    host.set(
        "randomBytes",
        Function::new(ctx.clone(), |ctx: Ctx<'js>, count: f64| {
            let count = (count.max(0.0) as usize).min(65536);
            let mut bytes = Vec::with_capacity(count);
            while bytes.len() < count {
                use std::hash::{BuildHasher, Hasher};
                let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
                hasher.write_usize(bytes.len());
                bytes.extend_from_slice(&hasher.finish().to_le_bytes());
            }
            bytes.truncate(count);
            to_array(&ctx, bytes)
        })?,
    )?;

    host.set(
        "scannerUtils",
        Function::new(ctx.clone(), |ctx: Ctx<'js>, name: String, args: String| -> rquickjs::Result<String> {
            let args: Json = serde_json::from_str(&args).map_err(|e| js_error(&ctx, e.to_string()))?;
            crate::plugins::scanner_utils::call(&name, &args)
                .map(|value| value.to_string())
                .map_err(|e| js_error(&ctx, e))
        })?,
    )?;

    host.set(
        "urlParse",
        Function::new(ctx.clone(), |input: String, base: String| -> Option<String> {
            let parsed = if base.is_empty() {
                url::Url::parse(&input)
            } else {
                url::Url::parse(&base).and_then(|base| base.join(&input))
            };
            parsed.ok().map(|url| url_parts(&url))
        })?,
    )?;
    host.set(
        "urlSet",
        Function::new(ctx.clone(), |href: String, part: String, value: String| -> Option<String> {
            use url::quirks;
            let mut url = url::Url::parse(&href).ok()?;
            match part.as_str() {
                "href" => quirks::set_href(&mut url, &value).ok()?,
                "protocol" => quirks::set_protocol(&mut url, &value).ok()?,
                "username" => quirks::set_username(&mut url, &value).ok()?,
                "password" => quirks::set_password(&mut url, &value).ok()?,
                "host" => quirks::set_host(&mut url, &value).ok()?,
                "hostname" => quirks::set_hostname(&mut url, &value).ok()?,
                "port" => quirks::set_port(&mut url, &value).ok()?,
                "pathname" => quirks::set_pathname(&mut url, &value),
                "search" => quirks::set_search(&mut url, &value),
                "hash" => quirks::set_hash(&mut url, &value),
                _ => return None,
            }
            Some(url_parts(&url))
        })?,
    )?;

    let st = state.clone();
    host.set(
        "docLoad",
        Function::new(ctx.clone(), move |html: String| st.dom.borrow_mut().load(&html))?,
    )?;
    let st = state.clone();
    host.set(
        "docSelect",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, doc: u32, op: String, nodes: Vec<u32>, arg: String| {
                st.dom
                    .borrow_mut()
                    .select(doc, &op, &nodes, &arg)
                    .map_err(|e| js_error(&ctx, e))
            },
        )?,
    )?;
    let st = state;
    host.set(
        "docText",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, doc: u32, op: String, nodes: Vec<u32>, arg: String| {
                st.dom
                    .borrow()
                    .read(doc, &op, &nodes, &arg)
                    .map_err(|e| js_error(&ctx, e))
            },
        )?,
    )?;

    Ok(host)
}

fn exception_text(ctx: &Ctx<'_>, error: rquickjs::Error) -> String {
    let caught: Result<(), rquickjs::CaughtError> = Err::<(), _>(error).catch(ctx);
    match caught {
        Err(rquickjs::CaughtError::Exception(exception)) => {
            let message = exception.message().unwrap_or_default();
            if message.is_empty() {
                "the script failed".to_string()
            } else {
                message
            }
        }
        Err(rquickjs::CaughtError::Value(value)) => value
            .as_string()
            .and_then(|s| s.to_string().ok())
            .unwrap_or_else(|| "the script failed".to_string()),
        Err(rquickjs::CaughtError::Error(error)) => error.to_string(),
        Ok(()) => "the script failed".to_string(),
    }
}

struct PendingCall {
    reply: oneshot::Sender<Result<String, PluginError>>,
    create_session: bool,
}

enum Exit {
    Stop,
    Rebuild,
}

struct Vm {
    runtime: Runtime,
    context: Context,
    host: Rc<HostState>,
    calls: HashMap<u64, PendingCall>,
    /// A call was abandoned or memory ran out; the context is replaced once it is idle.
    stale: bool,
}

const DRIVER: &str = "__anilili_driver";

impl Vm {
    fn build(
        spec: &EngineSpec,
        script: &str,
        tx: &mpsc::Sender<Message>,
        shared: &Arc<Shared>,
        generation: u64,
    ) -> Result<Self, String> {
        let runtime = Runtime::new().map_err(|e| format!("could not create the script runtime: {}", e))?;
        runtime.set_memory_limit(spec.limits.memory_bytes);
        runtime.set_max_stack_size(spec.limits.stack_bytes);

        let host = Rc::new(HostState {
            id: spec.id.clone(),
            generation,
            tx: tx.clone(),
            net: shared.net.clone(),
            limits: spec.limits,
            log: spec.log.clone(),
            handle: RefCell::new(None),
            dom: RefCell::new(DomStore::default()),
            finished: RefCell::new(Vec::new()),
            requests_in_flight: Cell::new(0),
            calls_allowed_to_create: Cell::new(0),
            slept_this_slice: Cell::new(Duration::ZERO),
            deadline_ms: Arc::new(AtomicU64::new(u64::MAX)),
            interrupted: Arc::new(AtomicBool::new(false)),
            epoch: Instant::now(),
        });

        let (deadline, interrupted, epoch) = (host.deadline_ms.clone(), host.interrupted.clone(), host.epoch);
        runtime.set_interrupt_handler(Some(Box::new(move || {
            if epoch.elapsed().as_millis() as u64 > deadline.load(Ordering::SeqCst) {
                interrupted.store(true, Ordering::SeqCst);
                true
            } else {
                false
            }
        })));

        let context = Context::full(&runtime).map_err(|e| format!("could not create the script context: {}", e))?;
        let vm = Self {
            runtime,
            context,
            host,
            calls: HashMap::new(),
            stale: false,
        };

        let config = serde_json::json!({
            "id": spec.id,
            "version": env!("CARGO_PKG_VERSION"),
            "preferences": spec.preferences,
        })
        .to_string();

        vm.host.start_slice();
        let host = vm.host.clone();
        let loaded: Result<(), String> = vm.context.with(|ctx| {
            let run = || -> rquickjs::Result<()> {
                let prelude: Function = ctx.eval(PRELUDE)?;
                let host_object = install_host(&ctx, host)?;
                let config_value: Value = ctx.json_parse(config)?;
                let driver: Object = prelude.call((host_object, config_value))?;
                ctx.globals().set(DRIVER, driver)?;
                Ok(())
            };
            run().map_err(|e| format!("could not prepare the plugin runtime: {}", exception_text(&ctx, e)))?;

            let mut options = EvalOptions::default();
            options.global = true;
            options.strict = false;
            options.filename = Some(format!("{}.js", spec.id));
            ctx.eval_with_options::<(), _>(script, options)
                .map_err(|e| exception_text(&ctx, e))?;
            let defined: bool = ctx
                .eval("typeof Provider === 'function'")
                .map_err(|e| exception_text(&ctx, e))?;
            if !defined {
                return Err("the plugin does not define a Provider class".to_string());
            }
            Ok(())
        });
        if vm.host.interrupted.load(Ordering::SeqCst) {
            return Err("the plugin took too long to load".to_string());
        }
        loaded?;
        vm.drain_jobs();
        Ok(vm)
    }

    fn drain_jobs(&self) {
        loop {
            match self.runtime.execute_pending_job() {
                Ok(true) => continue,
                Ok(false) => break,
                Err(_) => {
                    if self.host.interrupted.load(Ordering::SeqCst) {
                        break;
                    }
                }
            }
        }
    }

    fn enter(&self, entry: &str, run: impl for<'js> FnOnce(Ctx<'js>, Function<'js>) -> rquickjs::Result<()>) {
        self.host.start_slice();
        self.context.with(|ctx| {
            let outcome = (|| -> rquickjs::Result<()> {
                let driver: Object = ctx.globals().get(DRIVER)?;
                let function: Function = driver.get(entry)?;
                run(ctx.clone(), function)
            })();
            if let Err(error) = outcome {
                let text = exception_text(&ctx, error);
                tracing::debug!("plugin {}: {} failed: {}", self.host.id, entry, text);
            }
        });
        self.drain_jobs();
    }

    fn fail_all(&mut self, error: PluginError) {
        for (_, call) in self.calls.drain() {
            let _ = call.reply.send(Err(error.clone()));
        }
        self.host.calls_allowed_to_create.set(0);
    }

    fn finish_call(&mut self, id: u64, result: Result<String, PluginError>) {
        if let Some(call) = self.calls.remove(&id) {
            if call.create_session {
                let count = self.host.calls_allowed_to_create.get();
                self.host.calls_allowed_to_create.set(count.saturating_sub(1));
            }
            let _ = call.reply.send(result);
        }
    }

    fn serve(&mut self, rx: &mpsc::Receiver<Message>) -> Exit {
        loop {
            let Ok(message) = rx.recv() else {
                return Exit::Stop;
            };
            match message {
                Message::Stop => {
                    self.fail_all(PluginError::Stopped("the plugin was unloaded".to_string()));
                    return Exit::Stop;
                }
                Message::Call {
                    id,
                    method,
                    args,
                    create_session,
                    handle,
                    reply,
                } => {
                    *self.host.handle.borrow_mut() = Some(handle);
                    if create_session {
                        let count = self.host.calls_allowed_to_create.get();
                        self.host.calls_allowed_to_create.set(count + 1);
                    }
                    self.calls.insert(id, PendingCall { reply, create_session });
                    self.enter("invoke", |_, invoke| invoke.call::<_, ()>((id as f64, method, args)));
                }
                Message::Abandon(id) => {
                    if self.calls.contains_key(&id) {
                        self.finish_call(id, Err(PluginError::Timeout));
                        self.stale = true;
                    }
                }
                Message::FetchDone { generation, id, result } => {
                    if generation != self.host.generation {
                        continue;
                    }
                    let in_flight = self.host.requests_in_flight.get();
                    self.host.requests_in_flight.set(in_flight.saturating_sub(1));
                    self.enter("deliverFetch", |ctx, deliver| match result {
                        Ok(response) => {
                            let meta = serde_json::to_string(&response.meta).unwrap_or_else(|_| "{}".to_string());
                            let body = TypedArray::<u8>::new(ctx.clone(), response.body)?;
                            deliver.call::<_, ()>((id, Value::new_null(ctx.clone()), meta, body))
                        }
                        Err(reason) => deliver.call::<_, ()>((id, reason)),
                    });
                }
                Message::Timer { generation, id } => {
                    if generation != self.host.generation || self.calls.is_empty() {
                        continue;
                    }
                    self.enter("fireTimer", |_, fire| fire.call::<_, ()>((id,)));
                }
            }

            let finished: Vec<(u64, bool, String)> = self.host.finished.borrow_mut().drain(..).collect();
            for (id, ok, payload) in finished {
                if !ok && payload.contains("out of memory") {
                    self.stale = true;
                }
                let result = if ok { Ok(payload) } else { Err(PluginError::Script(payload)) };
                self.finish_call(id, result);
            }

            if self.host.interrupted.load(Ordering::SeqCst) {
                self.fail_all(PluginError::Stopped(format!(
                    "stopped: the script ran for more than {} seconds without pausing",
                    self.host.limits.slice.as_secs()
                )));
                return Exit::Rebuild;
            }
            if self.calls.is_empty() {
                if self.stale {
                    return Exit::Rebuild;
                }
                if !self.host.dom.borrow().is_empty() {
                    self.host.dom.borrow_mut().clear();
                }
            }
        }
    }
}

fn run_thread(
    spec: EngineSpec,
    script: String,
    rx: mpsc::Receiver<Message>,
    tx: mpsc::Sender<Message>,
    shared: Arc<Shared>,
    ready: mpsc::Sender<Result<(), String>>,
) {
    let mut ready = Some(ready);
    let mut generation = 0u64;
    loop {
        generation += 1;
        let mut vm = match Vm::build(&spec, &script, &tx, &shared, generation) {
            Ok(vm) => vm,
            Err(reason) => {
                match ready.take() {
                    Some(ready) => {
                        let _ = ready.send(Err(reason));
                    }
                    None => tracing::warn!("plugin {} could not be restarted: {}", spec.id, reason),
                }
                return;
            }
        };
        if let Some(ready) = ready.take() {
            let _ = ready.send(Ok(()));
        }
        match vm.serve(&rx) {
            Exit::Stop => return,
            Exit::Rebuild => {
                tracing::debug!("plugin {}: starting a fresh context", spec.id);
            }
        }
    }
}

// ---------------------------------------------------------------------- lazily started engine

/// A plugin's engine, started on first use and started again if its thread ever ends.
pub struct PluginRuntime {
    spec: EngineSpec,
    engine: Mutex<Option<Arc<PluginEngine>>>,
}

impl PluginRuntime {
    pub fn new(spec: EngineSpec) -> Self {
        Self {
            spec,
            engine: Mutex::new(None),
        }
    }

    pub fn id(&self) -> &str {
        &self.spec.id
    }

    pub fn engine(&self) -> Result<Arc<PluginEngine>, String> {
        let mut guard = self.engine.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(engine) = guard.as_ref() {
            if engine.is_alive() {
                return Ok(engine.clone());
            }
        }
        let engine = Arc::new(PluginEngine::start(self.spec.clone())?);
        *guard = Some(engine.clone());
        Ok(engine)
    }

    pub async fn call(
        &self,
        method: &str,
        args: &[Json],
        timeout: Duration,
        access: SessionUse,
    ) -> Result<Json, PluginError> {
        let engine = self.engine().map_err(PluginError::Stopped)?;
        engine.call(method, args, timeout, access).await
    }

    /// `call`, with the time its requests spend waiting for an in-app browser session added to
    /// the limit (up to `SESSION_CALL_ALLOWANCE`): starting a session takes longer than a
    /// plugin call is otherwise given, and is not the plugin being slow.
    pub async fn call_allowing_session(
        &self,
        method: &str,
        args: &[Json],
        timeout: Duration,
        access: SessionUse,
    ) -> Result<Json, PluginError> {
        use crate::providers::common::{with_session_allowance, SESSION_CALL_ALLOWANCE};
        let engine = self.engine().map_err(PluginError::Stopped)?;
        let net = engine.net().clone();
        let call = engine.call(method, args, timeout + SESSION_CALL_ALLOWANCE, access);
        with_session_allowance(call, timeout, SESSION_CALL_ALLOWANCE, |since| net.session_time_since(since))
            .await
            .unwrap_or(Err(PluginError::Timeout))
    }
}
