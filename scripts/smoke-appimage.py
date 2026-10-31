#!/usr/bin/env python3
"""Smoke test for a built Anix AppImage, run by CI in a clean container (see release.yml › smoke).

    smoke-appimage.py <Anix.AppImage> <out-dir> [--webdriver] [--clip <video.mp4>]

Needs an X server on $DISPLAY with a window manager, xdotool, and a D-Bus session.
Without --webdriver: start, real clicks and keys, stay alive 30 s, quit on SIGTERM, leave nothing behind.
With --webdriver (only where WebKitWebDriver comes from the same WebKit build the AppImage bundles,
i.e. Ubuntu 22.04): also link the clip as a local episode, play it 30 s through the stream proxy, and
check a real click and the fullscreen key while it plays.
Fails on an exit, a stopped web process (the app logs "web process stopped"), or leftover processes.
Writes <out-dir>/result.json and screenshots; exit code 1 on any failure."""
import json, os, shutil, signal, subprocess, sys, tempfile, time, urllib.request

APPIMAGE = os.path.abspath(sys.argv[1])
OUT = os.path.abspath(sys.argv[2])
WEBDRIVER = "--webdriver" in sys.argv
CLIP = sys.argv[sys.argv.index("--clip") + 1] if "--clip" in sys.argv else None
os.makedirs(OUT, exist_ok=True)
HOME = tempfile.mkdtemp(prefix="anix-smoke-")
ENV = {k: v for k, v in os.environ.items() if not k.startswith("XDG_") or k == "XDG_RUNTIME_DIR"}
ENV.update(HOME=HOME, APPIMAGE_EXTRACT_AND_RUN="1", ANIX_PLAYER_ENGINE="web", NO_AT_BRIDGE="1")
LOG = os.path.join(HOME, ".local", "share", "anix", "anix.log")
res = {"appimage": os.path.basename(APPIMAGE), "webdriver": WEBDRIVER, "failures": []}


def fail(msg):
    res["failures"].append(msg)
    print("FAIL:", msg, flush=True)


def sh(*cmd, timeout=20):
    p = subprocess.run(cmd, env=ENV, capture_output=True, text=True, timeout=timeout)
    return p.stdout.strip()


def ours():
    """Processes started with this scratch HOME (the app, WebKit helpers, the AppImage runtime)."""
    found = []
    for p in os.listdir("/proc"):
        if p.isdigit():
            try:
                env = open(f"/proc/{p}/environ", "rb").read().split(b"\0")
                if f"HOME={HOME}".encode() in env and "WebKitWebDriver" not in open(f"/proc/{p}/comm").read():
                    found.append((int(p), open(f"/proc/{p}/comm").read().strip()))
            except OSError:
                pass
    return found


def web_process_ok():
    names = [n for _, n in ours()]
    return any(n.startswith("anix") for n in names) and any(n.startswith("WebKitWebProces") for n in names)


def log_text():
    try:
        return open(LOG, errors="replace").read()
    except OSError:
        return ""


def window():
    for _ in range(60):
        ids = sh("xdotool", "search", "--onlyvisible", "--name", "Anix").split()
        if ids:
            return ids[-1]
        time.sleep(1)
    return None


def geometry(win):
    g = dict(line.split("=", 1) for line in sh("xdotool", "getwindowgeometry", "--shell", win).splitlines() if "=" in line)
    return int(g.get("WIDTH", 0)), int(g.get("HEIGHT", 0))


def screen():
    w, h = sh("xdotool", "getdisplaygeometry").split()
    return int(w), int(h)


def key(win, name):
    # Through XTEST on the focused window: GTK ignores the synthetic events `xdotool key --window` sends.
    sh("xdotool", "windowactivate", "--sync", win)
    sh("xdotool", "key", name)


def click_center(win):
    w, h = geometry(win)
    sh("xdotool", "mousemove", "--window", win, str(w // 2), str(h // 2), "click", "1")


def shot(name):
    if shutil.which("import"):
        subprocess.run(["import", "-window", "root", os.path.join(OUT, name)], env=ENV, timeout=20, check=False)


class Driver:
    def __init__(self, port=4444):
        self.proc = subprocess.Popen(["WebKitWebDriver", f"--port={port}"], env={**ENV, "TAURI_WEBVIEW_AUTOMATION": "true"},
                                     stdout=open(os.path.join(OUT, "app-stdout.log"), "w"), stderr=subprocess.STDOUT)
        self.url = f"http://127.0.0.1:{port}"
        time.sleep(2)
        caps = {"capabilities": {"alwaysMatch": {"webkitgtk:browserOptions": {"binary": APPIMAGE, "args": []}}}}
        self.sid = self.call("POST", "/session", caps, timeout=120)["sessionId"]

    def call(self, method, path, body=None, timeout=60):
        data = json.dumps(body).encode() if body is not None else None
        req = urllib.request.Request(self.url + path, data=data, method=method, headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return json.loads(r.read() or b"{}").get("value")

    def run(self, js, *args):
        return self.call("POST", f"/session/{self.sid}/execute/sync", {"script": js, "args": list(args)})

    def invoke(self, cmd, args=None):
        js = "const done=arguments[arguments.length-1]; window.__TAURI_INTERNALS__.invoke(arguments[0], arguments[1]||{}).then(v=>done({ok:v}), e=>done({err:String(e)}))"
        return self.call("POST", f"/session/{self.sid}/execute/async", {"script": js, "args": [cmd, args or {}]}, timeout=90)

    def wait(self, js, budget):
        t0 = time.time()
        while time.time() - t0 < budget:
            try:
                if self.run(js) is True:
                    return round(time.time() - t0, 1)
            except Exception:
                pass
            time.sleep(0.5)
        return None


SEEK = "const s=document.querySelector('[role=slider][aria-label=Seek]'); const t=s&&s.getAttribute('aria-valuenow'); return t==null?null:Number(t)"
DIALOGS = "const b=[...document.querySelectorAll('[role=dialog] button')].find(x=>/^\\s*(Keep|Skip|Save|Continue|Done)\\b/.test(x.textContent)); if(b){b.click(); return true} return false"

# The runtime part of the file: a static one needs no libfuse2 on the host.
needed = sh("sh", "-c", f"readelf -d '{APPIMAGE}' 2>/dev/null | grep NEEDED || true")
res["runtime_needs_libfuse2"] = "libfuse.so.2" in needed

app = None
driver = None
try:
    if WEBDRIVER:
        driver = Driver()
        res["shell_ready_s"] = driver.wait("return !!window.__TAURI_INTERNALS__ && !!document.querySelector('nav, aside')", 90)
        for _ in range(4):
            time.sleep(1.5)
            if not driver.run(DIALOGS):
                break
    else:
        app = subprocess.Popen([APPIMAGE], env=ENV, stdout=open(os.path.join(OUT, "app-stdout.log"), "w"), stderr=subprocess.STDOUT)
    win = window()
    res["window"] = bool(win)
    if not win:
        fail("no Anix window appeared within 60 s")
    else:
        time.sleep(8)
        shot("started.png")
        # Real input: a click on whatever the page shows, then a key, while the page is live.
        click_center(win)
        time.sleep(2)
        key(win, "Escape")
        time.sleep(3)
        if not web_process_ok():
            fail("the app or its web process is gone after a real click")

    if driver and win and CLIP:
        folder = os.path.join(HOME, "Videos", "Smoke")
        os.makedirs(folder, exist_ok=True)
        shutil.copy(CLIP, os.path.join(folder, "Smoke - 01.mp4"))
        linked = driver.invoke("link_local_folder", {"anilistId": 154587, "folderPath": folder})
        res["linked"] = "ok" in (linked or {})
        driver.run("location.hash = '#/watch/154587/1?cat=sub&prov=local'")
        t0 = time.time(); first = None
        while time.time() - t0 < 60:
            v = driver.run(SEEK)
            if isinstance(v, (int, float)) and v > 0:
                first = round(time.time() - t0, 1)
                break
            time.sleep(0.5)
        res["first_frame_s"] = first
        if first is None:
            fail("the local clip did not start within 60 s")
        else:
            time.sleep(15)
            # Real input while playing: fullscreen on and off with the key, a click on the video.
            key(win, "f")
            time.sleep(3)
            res["fullscreen_size"] = geometry(win)
            res["screen_size"] = screen()
            if res["fullscreen_size"] != res["screen_size"]:
                fail(f"the f key did not make the window fullscreen: {res['fullscreen_size']} vs screen {res['screen_size']}")
            shot("fullscreen.png")
            key(win, "f")
            time.sleep(2)
            res["after_second_f"] = geometry(win)
            click_center(win)
            time.sleep(2)
            res["paused_by_click"] = driver.run("const v=document.querySelector('video'); return v ? v.paused : null")
            click_center(win)
            time.sleep(15)
            res["position_after_play_s"] = driver.run(SEEK)
            if not isinstance(res["position_after_play_s"], (int, float)) or res["position_after_play_s"] < 25:
                fail(f"playback did not reach 25 s: {res['position_after_play_s']}")
            shot("playing.png")

    if win:
        time.sleep(17)  # with the 13 s of clicks above: still alive 30 s after the window appeared
        if not web_process_ok():
            fail("the app or its web process stopped within 30 s")
except Exception as e:
    fail(f"{type(e).__name__}: {e}")
finally:
    # Quit the way a desktop session does, then make sure nothing is left.
    for pid, name in ours():
        if name.startswith("anix"):
            os.kill(pid, signal.SIGTERM)
    t0 = time.time()
    while ours() and time.time() - t0 < 15:
        time.sleep(0.5)
    res["quit_s"] = round(time.time() - t0, 1)
    left = ours()
    res["leftover"] = [n for _, n in left]
    if left:
        fail(f"processes left 15 s after SIGTERM: {res['leftover']}")
        for pid, _ in left:
            try:
                os.kill(pid, signal.SIGKILL)
            except OSError:
                pass
    if driver:
        driver.proc.terminate()
    if app and app.poll() is None:
        app.kill()
    text = log_text()
    shutil.copy(LOG, os.path.join(OUT, "anix.log")) if os.path.exists(LOG) else None
    if "web process stopped" in text:
        fail("the log reports a stopped web process")
    res["log_errors"] = [l[:200] for l in text.splitlines() if " ERROR " in l][:10]
    json.dump(res, open(os.path.join(OUT, "result.json"), "w"), indent=1)
    print(json.dumps(res, indent=1))
    sys.exit(1 if res["failures"] else 0)
