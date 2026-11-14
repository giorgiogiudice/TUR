#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The re-hosting experiment's measurement (docs/reports/exp-rehost-1.md).

    run.py --corpus corpus.json --shard NAME --work DIR --cache DIR --out DIR

For every application of the shard, and for each of its packages:

  linux     the project's official Linux build: the ground truth. It is
            opened, the detector is run on it, and it is started as it is.
  windows,  the project's Windows and macOS packages: opened without being
  macos     run, the runtime detected, the payload re-hosted under the Linux
            build of that runtime, and started.
  +pretend  (Electron only) the same payload again, told that it runs on
            the platform it was built for (rehostlib/shim/tur-shim.cjs).

Each start is measured the same way: time to the first window, a
screenshot, sway's window list (Wayland or Xwayland), the page as the
application's own Chromium reports it (DevTools protocol), the
accessibility tree (AT-SPI), memory (PSS of the processes it started), the
files it wrote to a fresh home directory, and whether text typed into it
arrives. Results go to OUT as JSON per application, screenshots, logs and a
contact sheet per application; summary.py turns them into tables.

Runs in CI only, as an unprivileged user inside a Fedora container. A
failure of one application is a result, not a failure of the run.
"""

import argparse
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import time
import traceback

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
sys.path.insert(0, HERE)

from rehostlib import detect, fetch, rehost, unpack  # noqa: E402
from session import DevTools, Session, run  # noqa: E402

PAGE_PROBE = """JSON.stringify({title: document.title, ready: document.readyState,
 nodes: document.getElementsByTagName('*').length,
 text: document.body ? document.body.innerText.length : 0,
 url: location.href.slice(0, 200), ua: navigator.userAgent, platform: navigator.platform,
 sample: document.body ? document.body.innerText.replace(/\\s+/g, ' ').slice(0, 300) : ''})"""
TYPED = "turprobe7391"
TYPE_CHECK = f"""(function() {{ var a = document.activeElement; var v = a ? ((a.value || '') + ' ' + (a.innerText || '')) : '';
 return v.indexOf('{TYPED}') >= 0 || (document.body && document.body.innerText.indexOf('{TYPED}') >= 0); }})()"""
ERROR_WORDS = re.compile(r"A JavaScript error occurred|Uncaught Exception|Cannot find module|invalid ELF header|not a valid Win32|was compiled against a different Node\.js version|Error: The module|dlopen|MODULE_NOT_FOUND|Unhandled exception|DllNotFoundException|UnsatisfiedLinkError|ClassNotFoundException|NoClassDefFoundError|Traceback \(most recent call last\)|ModuleNotFoundError|ImportError|SCRIPT ERROR|Segmentation fault|core dumped")


def log(text):
    print(text, flush=True)


def pids():
    return {int(name) for name in os.listdir("/proc") if name.isdigit()}


def pss_kb(pid):
    try:
        with open(f"/proc/{pid}/smaps_rollup", encoding="ascii", errors="replace") as handle:
            for line in handle:
                if line.startswith("Pss:"):
                    return int(line.split()[1])
    except (OSError, ValueError):
        pass
    return 0


def image_stats(path):
    try:
        from PIL import Image, ImageStat

        with Image.open(path) as image:
            small = image.convert("RGB").resize((320, 200))
            deviation = sum(ImageStat.Stat(small).stddev) / 3
            colours = len(small.getcolors(maxcolors=70000) or []) or 70000
        return {"stddev": round(deviation, 1), "colours": colours}
    except Exception as problem:  # evidence only
        return {"error": str(problem)[:100]}


def profile_files(home):
    listed = []
    for root, dirs, names in os.walk(home):
        dirs[:] = [name for name in dirs if name not in ("Cache", "Code Cache", "GPUCache", "DawnCache", "DawnGraphiteCache", "DawnWebGPUCache", "blob_storage", "Crashpad", "Service Worker", "ShaderCache", "GrShaderCache", "shader_cache", "fontconfig", "mesa_shader_cache", "Dictionaries", "component_crx_cache", "extensions_crx_cache")]
        for name in names:
            listed.append(os.path.relpath(os.path.join(root, name), home))
            if len(listed) >= 600:
                return listed
    return sorted(listed)


class Bench:
    def __init__(self, options):
        self.options = options
        self.work = options.work
        self.cache = options.cache
        self.out = options.out
        for path in (self.work, self.cache, self.out):
            os.makedirs(path, exist_ok=True)
        self.session = Session(self.work, os.path.join(self.out, "session-logs"))
        self.port = 9300
        code, _, _ = run(["unshare", "-Ur", "true"])
        self.userns = code == 0
        self.environment = {"user_namespaces": self.userns}

    # ---------------------------------------------------------------- start

    def measure(self, label, argv, cwd, env_extra, shots, chromium=False, kind="gui", function=None, wait=40):
        """Start a program in the session and record what it does."""
        home = os.path.join(self.work, "home", label)
        shutil.rmtree(home, ignore_errors=True)
        os.makedirs(home)
        env = self.session.env(HOME=home, XDG_CONFIG_HOME=os.path.join(home, ".config"), XDG_DATA_HOME=os.path.join(home, ".local/share"), XDG_CACHE_HOME=os.path.join(home, ".cache"))
        env.update(env_extra or {})
        argv = list(argv)
        if os.path.isfile(argv[0]) and not os.access(argv[0], os.X_OK):
            os.chmod(argv[0], 0o755)
        result = {"label": label, "argv": [os.path.basename(argv[0])] + argv[1:], "chromium": chromium, "kind": kind}
        devtools = None
        if chromium:
            self.port += 1
            devtools = DevTools(self.port)
            # --disable-gpu: the runner has no GPU, and in run 43544136201
            # Chromium's GPU process died in this container ("GPU process
            # isn't usable"), which kept several official Linux builds from
            # ever showing their window. Given to official and re-hosted
            # builds alike.
            argv += [f"--remote-debugging-port={self.port}", "--ozone-platform-hint=auto", "--disable-gpu"]
            env["ELECTRON_OZONE_PLATFORM_HINT"] = "auto"
            if not self.userns:
                argv.append("--no-sandbox")
            result["sandbox"] = "chromium's own (user namespaces)" if self.userns else "off (--no-sandbox): no user namespaces in this container"
        shot_log = os.path.join(shots, label + ".log")
        before = pids()
        self.session.close_windows()
        started = time.time()
        try:
            with open(shot_log, "wb") as output:
                process = subprocess.Popen(argv, cwd=cwd, env=env, stdout=output, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL, start_new_session=True)
        except OSError as problem:
            result.update({"started": False, "error": f"could not start: {problem}"})
            return result
        result["started"] = True
        first_window = None
        page_ready = None
        windows = []
        http = None
        while time.time() - started < wait:
            now = time.time() - started
            if kind == "gui":
                windows = self.session.windows()
                if windows and first_window is None:
                    first_window = now
            if devtools and page_ready is None and first_window is not None:
                for target in devtools.targets() or []:
                    if target.get("type") == "page" and target.get("webSocketDebuggerUrl"):
                        state = devtools.evaluate(target["webSocketDebuggerUrl"], "document.readyState", timeout=4)
                        if state == "complete":
                            page_ready = time.time() - started
                            break
            if function and function.get("http") and http is None:
                try:
                    import urllib.request

                    with urllib.request.urlopen(function["http"], timeout=3) as response:
                        http = {"status": response.status, "seconds": round(time.time() - started, 1), "bytes": len(response.read(200000))}
                except Exception as problem:
                    status = getattr(problem, "code", None)
                    if status:
                        http = {"status": status, "seconds": round(time.time() - started, 1)}
            exited = process.poll() is not None
            if kind == "gui":
                settle = 8 if devtools else 14  # Java and game engines paint later than they map
                if first_window is not None and now - first_window >= settle and (not devtools or page_ready is not None or now - first_window >= 22):
                    if not function or not function.get("http") or http is not None or now > wait - 2:
                        break
                if exited and first_window is None and now > 4 and not (pids() - before):
                    break
            else:
                if exited or (function and function.get("http") and http is not None):
                    break
            time.sleep(0.4)
        result["seconds_to_first_window"] = round(first_window, 1) if first_window is not None else None
        result["seconds_to_page_ready"] = round(page_ready, 1) if page_ready is not None else None
        result["exit_code"] = process.poll()
        mine = sorted(pids() - before)
        result["processes"] = len(mine)
        result["alive"] = bool(mine) and (process.poll() is None or len(mine) > 0)
        result["pss_mb"] = round(sum(pss_kb(pid) for pid in mine) / 1024, 1)
        if http is not None:
            result["http"] = http
        if kind == "gui":
            windows = self.session.windows()
            result["windows"] = windows
            shot = os.path.join(shots, label + ".png")
            if self.session.screenshot(shot):
                result["screenshot"] = os.path.basename(shot)
                result["image"] = image_stats(shot)
            code, out, err = run([sys.executable, os.path.join(HERE, "atspi_probe.py")], env=self.session.env(), timeout=45)
            try:
                tree = json.loads(out)
                apps = [app for app in tree["applications"] if app.get("nodes", 0) > 1]
                result["atspi"] = {
                    "applications": len(apps),
                    "nodes": sum(app["nodes"] for app in apps),
                    "named": sum(app["named"] for app in apps),
                    "actionable": sum(app["actionable"] for app in apps),
                    "toolkits": sorted({app.get("toolkit", "?") for app in apps}),
                    "sample": [item for app in apps for item in app["sample"]][:20],
                    "windows": [item for app in apps for item in app["windows"]][:6],
                    "roles": apps[0]["roles"] if apps else {},
                }
            except (ValueError, KeyError):
                result["atspi"] = {"error": (err or out)[-300:]}
        if devtools:
            version = devtools.version()
            result["devtools"] = bool(version)
            if version:
                result["browser"] = version.get("Browser")
                result["user_agent"] = version.get("User-Agent")
                match = re.search(r"Electron/([0-9][^ ]*)", version.get("User-Agent") or "")
                result["electron_running"] = match.group(1) if match else None
            pages = []
            for target in devtools.targets() or []:
                if target.get("type") not in ("page", "webview") or not target.get("webSocketDebuggerUrl"):
                    continue
                raw = devtools.evaluate(target["webSocketDebuggerUrl"], PAGE_PROBE)
                try:
                    page = json.loads(raw) if raw else None
                except ValueError:
                    page = None
                if page:
                    page["type"] = target.get("type")
                    page["socket"] = target["webSocketDebuggerUrl"]
                    pages.append(page)
            pages.sort(key=lambda page: -page.get("nodes", 0))
            if pages:
                main = pages[0]
                socket_url = main["socket"]
                devtools.call(socket_url, "Input.dispatchMouseEvent", {"type": "mousePressed", "x": 640, "y": 420, "button": "left", "clickCount": 1})
                devtools.call(socket_url, "Input.dispatchMouseEvent", {"type": "mouseReleased", "x": 640, "y": 420, "button": "left", "clickCount": 1})
                devtools.call(socket_url, "Input.insertText", {"text": TYPED})
                time.sleep(0.8)
                result["typed_text_arrived"] = bool(devtools.evaluate(socket_url, TYPE_CHECK))
                if kind == "gui" and self.session.screenshot(os.path.join(shots, label + "-typed.png")):
                    result["screenshot_typed"] = label + "-typed.png"
            for page in pages:
                page.pop("socket", None)
            result["pages"] = pages[:4]
        result["profile_files"] = profile_files(home)
        # stop everything it started
        for sig in (signal.SIGTERM, signal.SIGKILL):
            for pid in sorted(pids() - before):
                try:
                    os.kill(pid, sig)
                except OSError:
                    pass
            deadline = time.time() + (6 if sig == signal.SIGTERM else 2)
            while time.time() < deadline and (pids() - before):
                time.sleep(0.3)
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            pass
        try:
            with open(shot_log, "rb") as handle:
                text = handle.read().decode("utf-8", "replace")
        except OSError:
            text = ""
        result["output_bytes"] = len(text)
        result["output_head"] = text[:1500] if kind == "cli" else ""
        result["error_lines"] = [line.strip()[:240] for line in text.splitlines() if ERROR_WORDS.search(line)][:12]
        result["verdict"] = self.verdict(result, function)
        shutil.rmtree(home, ignore_errors=True)
        return result

    @staticmethod
    def verdict(result, function):
        """The automatic part of the judgement; screenshots are looked at by
        a person before anything is claimed."""
        if not result.get("started"):
            return "did-not-start"
        if result["kind"] == "cli":
            if function and function.get("http"):
                return "works" if (result.get("http") or {}).get("status") in (200, 301, 302, 307, 401) else "no-answer"
            if function and function.get("stdout"):
                return "works" if re.search(function["stdout"], result.get("output_head", "")) else "wrong-output"
            return "ran" if result.get("exit_code") == 0 else "failed"
        windows = [window for window in result.get("windows") or [] if window["width"] * window["height"] > 20000]
        if not windows:
            return "no-window"
        titles = " ".join(window["title"] for window in windows) + " ".join((result.get("atspi") or {}).get("windows", []))
        if re.search(r"(?i)\berror\b|exception|crash", titles) and len(windows) == 1:
            return "error-dialog"
        image = result.get("image") or {}
        if image.get("colours", 99) <= 3:
            return "blank-window"
        if result.get("chromium") and result.get("devtools"):
            pages = result.get("pages") or []
            if not pages or max(page.get("nodes", 0) for page in pages) < 15:
                return "empty-page"
        if function and function.get("http") and (result.get("http") or {}).get("status") not in (200, 301, 302, 307, 401):
            return "window-but-no-answer"
        return "window"

    # ------------------------------------------------------- one application

    def official_linux_plan(self, app, tree, finding):
        """How to start the project's own Linux build."""
        override = app.get("linux_run")
        if override:
            import glob

            matches = sorted(glob.glob(os.path.join(tree, override[0]), recursive=True))
            if matches:
                os.chmod(matches[0], 0o755)
                return {"argv": [matches[0]] + override[1:], "cwd": os.path.dirname(matches[0]), "chromium": app["family"] in ("electron", "nwjs")}
        if not finding:
            return None
        runtime, directory, payload = finding["runtime"], finding["app_dir"], finding["payload"]
        if runtime in ("electron", "nwjs") and payload.get("program"):
            return {"argv": [os.path.join(directory, payload["program"])], "cwd": directory, "chromium": True}
        if runtime == "godot" and payload.get("program"):
            return {"argv": [os.path.join(directory, payload["program"])], "cwd": directory}
        if runtime == "renpy":
            scripts = [name for name in os.listdir(directory) if name.endswith(".sh")]
            if scripts:
                return {"argv": ["bash", os.path.join(directory, scripts[0])], "cwd": directory}
        if runtime == "dotnet" and payload.get("single_file"):
            return {"argv": [os.path.join(directory, payload["single_file"])], "cwd": directory, "kind": "gui" if payload.get("avalonia") else "cli"}
        if runtime == "dotnet" and payload.get("assembly"):
            program = os.path.join(directory, payload["assembly"][:-4])
            if not os.path.isfile(program):
                # The launcher may have another name; it names its assembly.
                for name in sorted(os.listdir(directory)):
                    path = os.path.join(directory, name)
                    if os.path.isfile(path) and os.path.getsize(path) < (8 << 20) and "." not in name and detect.binfmt.binary_kind(path) and detect.binfmt.contains(path, payload["assembly"].encode()):
                        program = path
                        break
            if os.path.isfile(program):
                return {"argv": [program], "cwd": directory, "kind": "gui" if payload.get("avalonia") else "cli"}
        if runtime == "jvm":
            binaries = os.path.join(directory, "bin")
            if os.path.isdir(binaries):
                for name in sorted(os.listdir(binaries)):
                    if detect.binfmt.binary_kind(os.path.join(binaries, name)):
                        return {"argv": [os.path.join(binaries, name)], "cwd": directory}
            plan = rehost.rehost_jvm(finding, os.path.join(self.work, "r", app["id"] + "-linux-official"), self.cache, log)
            if plan["ok"]:
                return {"argv": plan["argv"], "cwd": plan["cwd"]}
        if runtime == "pyinstaller":
            return {"argv": [os.path.join(directory, payload["carrier"])], "cwd": directory, "kind": "cli"}
        if runtime == "love":
            plan = rehost.rehost_love(finding, os.path.join(self.work, "r", app["id"] + "-linux-official"), self.cache, log)
            if plan["ok"]:
                return {"argv": plan["argv"], "cwd": plan["cwd"]}
        return None

    def package(self, app, variant, record, shots):
        spec = app["packages"][variant]
        entry = {"asset": spec.get("asset") or spec.get("url")}
        record["variants"][variant] = entry
        files = []
        entry["asset"] = spec.get("asset") or spec.get("local") or spec.get("url")
        for item in [spec] + list(spec.get("with", [])):
            if item.get("local"):
                import glob

                matches = sorted(glob.glob(os.path.join(self.work, item["local"])))
                if not matches:
                    entry["error"] = f"the package built in this job was not found: {item['local']}"
                    return
                files.append(matches[0])
                entry.setdefault("downloads", []).append({"url": "built in this job: " + os.path.basename(matches[0]), "size": os.path.getsize(matches[0]), "sha256": fetch.sha256_file(matches[0]), "pinned": False})
                continue
            url = item.get("url") or fetch.github_asset(app["repo"], app["tag"], item["asset"])
            name = item.get("asset") or url.rsplit("/", 1)[1]
            cached = "github.com" not in url
            destination = os.path.join(self.cache if cached else self.work, "dl", app["id"], name)
            try:
                got = fetch.download(url, destination, sha256=item.get("sha256"))
            except fetch.FetchError as problem:
                entry["error"] = f"download: {problem}"
                return
            files.append(destination)
            entry.setdefault("downloads", []).append({"url": url, "size": got["size"], "sha256": got["sha256"], "pinned": got["pinned"]})
        tree = os.path.join(self.work, "x", f"{app['id']}-{variant}")
        shutil.rmtree(tree, ignore_errors=True)
        unpacker = unpack.Unpacker(log=log)
        started = time.time()
        try:
            if len(files) == 1:
                kind = unpacker.unpack(files[0], tree)
            else:  # files that belong side by side (a program and its .pck)
                os.makedirs(tree)
                for path in files:
                    shutil.copy2(path, os.path.join(tree, os.path.basename(path)))
                kind = "files"
                unpacker.layers.append("files side by side")
        except Exception as problem:
            entry["error"] = f"unpack: {type(problem).__name__}: {problem}"
            return
        entry["container"] = {"kind": kind, "layers": unpacker.layers, "problems": unpacker.problems, "seconds": round(time.time() - started, 1)}
        for path in files:
            if os.path.join(self.work, "dl") in path:  # not cached, not built here
                try:
                    os.unlink(path)
                except OSError:
                    pass
        try:
            findings = detect.detect(tree)
            for finding in findings:
                if finding["runtime"] == "nwjs":
                    detect.resolve_nw_version(finding, fetch.nwjs_versions(self.cache))
        except Exception as problem:
            entry["error"] = f"detect: {type(problem).__name__}: {problem}"
            log(traceback.format_exc())
            return
        primary = findings[0] if findings else None
        entry["detected"] = detect.summary(primary)
        entry["finding"] = {key: value for key, value in (primary or {}).items() if key not in ("natives", "sidecars")}
        if primary:
            entry["finding"]["natives"] = [{k: item.get(k) for k in ("path", "format", "archs", "package", "version", "where")} for item in primary.get("natives", [])][:120]
            entry["finding"]["sidecars"] = [item["path"] + " (" + item["format"] + ")" for item in primary.get("sidecars", []) if item["format"] != "elf"][:40]
        entry["other_findings"] = [detect.summary(f) for f in findings[1:6]]
        log(f"  {variant}: {' > '.join(unpacker.layers)} -> {entry['detected']}")
        if app.get("detect_only"):
            shutil.rmtree(tree, ignore_errors=True)
            return
        function = app.get("function")
        label = f"{app['id']}-{variant}"
        if variant == "linux":
            plan = self.official_linux_plan(app, tree, primary)
            if not plan:
                entry["error"] = "no way found to start the official Linux build"
            else:
                arguments = (function or {}).get("args", [])
                entry["run"] = self.measure(label, plan["argv"] + arguments, plan["cwd"], {}, shots, chromium=plan.get("chromium", False), kind=plan.get("kind", app.get("kind", "gui")), function=function)
        elif not primary:
            entry["error"] = "nothing recognised in the package"
        else:
            out = os.path.join(self.work, "r", label)
            shutil.rmtree(out, ignore_errors=True)
            started = time.time()
            plan = rehost.rehost(primary, out, self.cache, log)
            entry["plan"] = {key: value for key, value in plan.items() if key not in ("argv", "env", "cwd")}
            entry["plan"]["seconds"] = round(time.time() - started, 1)
            if plan["ok"]:
                if plan.get("pip") is not None:
                    self.python_packages(plan, primary, out, entry)
                arguments = (function or {}).get("args", [])
                entry["run"] = self.measure(label, plan["argv"] + arguments, plan["cwd"], plan["env"], shots, chromium=plan.get("chromium", False), kind=app.get("kind", plan["kind"]), function=function)
                log(f"    {label}: {entry['run']['verdict']}")
                if primary["runtime"] == "electron" and not self.options.no_pretend:
                    spoof = "win32" if variant == "windows" else "darwin"
                    out2 = out + "-pretend"
                    shutil.rmtree(out2, ignore_errors=True)
                    plan2 = rehost.rehost(primary, out2, self.cache, log, spoof=spoof)
                    if plan2["ok"]:
                        spoof_log = os.path.join(shots, label + "-pretend.shim.txt")
                        run2 = self.measure(label + "-pretend", plan2["argv"], plan2["cwd"], dict(plan2["env"], TUR_REHOST_LOG=spoof_log), shots, chromium=True, kind="gui", wait=32)
                        try:
                            run2["shim"] = open(spoof_log, encoding="utf-8", errors="replace").read().splitlines()[:60]
                        except OSError:
                            run2["shim"] = []
                        entry["pretend"] = run2
                        log(f"    {label}-pretend ({spoof}): {run2['verdict']}")
                    shutil.rmtree(out2, ignore_errors=True)
            else:
                log(f"    {label}: not re-hosted: {plan['blockers']}")
            shutil.rmtree(out, ignore_errors=True)
        if "run" in entry:
            log(f"    {label}: {entry['run']['verdict']}")
        shutil.rmtree(tree, ignore_errors=True)

    def python_packages(self, plan, finding, out, entry):
        """A virtual environment of the matching Python with the Linux
        builds of the extension packages the frozen program carries."""
        names = {"Cryptodome": "pycryptodomex", "Crypto": "pycryptodome", "yaml": "PyYAML", "PIL": "pillow", "nacl": "pynacl", "curl_cffi": "curl_cffi", "brotli": "brotli", "websockets": "websockets", "charset_normalizer": "charset-normalizer", "markupsafe": "markupsafe", "yt_dlp_ejs": "yt-dlp-ejs"}
        metadata = {}
        for name in finding["payload"].get("metadata", []):
            match = re.match(r"(.+)-([0-9][^-]*)\.dist-info$", name)
            if match:
                metadata[re.sub(r"[-_.]+", "-", match.group(1)).lower()] = match.group(2)
        wanted = []
        for top in plan["pip"]:
            if re.match(r"^[0-9a-f]{12,}", top):
                continue
            dist = names.get(top, top)
            key = re.sub(r"[-_.]+", "-", dist).lower()
            wanted.append(f"{dist}=={metadata[key]}" if key in metadata else dist)
        for key, version in metadata.items():
            if any(top.lower().replace("_", "-") == key for top in plan["pip"]) is False and key in ("black", "click", "mypy-extensions", "packaging", "pathspec", "platformdirs", "pytokens"):
                wanted.append(f"{key}=={version}")
        venv = os.path.join(out, "venv")
        code, _, err = run([plan["python"], "-m", "venv", venv], timeout=120)
        report = {"venv": code == 0, "installed": [], "failed": []}
        if code == 0:
            for spec in sorted(set(wanted)):
                code, _, err = run([os.path.join(venv, "bin", "python"), "-m", "pip", "install", "--quiet", "--disable-pip-version-check", "--only-binary", ":all:", spec], timeout=240)
                (report["installed"] if code == 0 else report["failed"]).append(spec if code == 0 else f"{spec}: {err.strip().splitlines()[-1][:160] if err.strip() else 'failed'}")
            plan["argv"][0] = os.path.join(venv, "bin", "python")
        entry["plan"]["python_packages"] = report

    def application(self, app):
        log(f"== {app['id']} ({app['family']})")
        record = {"id": app["id"], "name": app["name"], "family": app["family"], "licence": app.get("licence"), "repo": app.get("repo"), "tag": app.get("tag"), "note": app.get("note"), "variants": {}}
        shots = os.path.join(self.out, "shots")
        os.makedirs(shots, exist_ok=True)
        for variant in ("linux", "windows", "macos"):
            if variant not in app["packages"]:
                continue
            started = time.time()
            try:
                self.package(app, variant, record, shots)
            except Exception as problem:
                record["variants"].setdefault(variant, {})["error"] = f"{type(problem).__name__}: {problem}"
                log(traceback.format_exc())
            record["variants"][variant]["seconds"] = round(time.time() - started, 1)
            shutil.rmtree(os.path.join(self.work, "x"), ignore_errors=True)
            shutil.rmtree(os.path.join(self.work, "r"), ignore_errors=True)
        with open(os.path.join(self.out, f"app-{app['id']}.json"), "w", encoding="utf-8") as handle:
            json.dump(record, handle, indent=1, default=str)
        self.sheet(record, shots)
        return record

    def sheet(self, record, shots):
        """One image per application: its screenshots side by side."""
        try:
            from PIL import Image, ImageDraw

            cells = []
            for variant in ("linux", "windows", "macos"):
                entry = record["variants"].get(variant)
                if not entry:
                    continue
                for key, suffix in (("run", ""), ("pretend", "-pretend")):
                    data = entry.get(key)
                    if not data:
                        if key == "run":
                            cells.append((f"{variant}: " + (entry.get("error") or "; ".join((entry.get("plan") or {}).get("blockers", [])) or "not started")[:70], None))
                        continue
                    name = "official Linux build" if variant == "linux" else f"re-hosted from {variant}" + (" (pretend)" if suffix else "")
                    cells.append((f"{name}: {data.get('verdict')}", os.path.join(shots, data["screenshot"]) if data.get("screenshot") else None))
            if not cells:
                return
            width, height = 512, 320
            sheet = Image.new("RGB", (width * len(cells), height + 24), (24, 24, 24))
            draw = ImageDraw.Draw(sheet)
            for index, (caption, path) in enumerate(cells):
                if path and os.path.exists(path):
                    with Image.open(path) as image:
                        sheet.paste(image.convert("RGB").resize((width - 4, height)), (index * width + 2, 24))
                draw.text((index * width + 6, 6), caption[:84], fill=(255, 255, 255))
            os.makedirs(os.path.join(self.out, "sheets"), exist_ok=True)
            sheet.save(os.path.join(self.out, "sheets", record["id"] + ".png"), optimize=True)
        except Exception as problem:
            log(f"  contact sheet failed: {problem}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--corpus", required=True)
    parser.add_argument("--shard", required=True)
    parser.add_argument("--only", default="")
    parser.add_argument("--work", required=True)
    parser.add_argument("--cache", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--no-pretend", action="store_true")
    options = parser.parse_args()
    with open(options.corpus, encoding="utf-8") as handle:
        corpus = json.load(handle)
    only = {name for name in options.only.split(",") if name}
    apps = [app for app in corpus["applications"] if (app["id"] in only if only else app.get("shard") == options.shard)]
    bench = Bench(options)
    bench.session.up()
    bench.environment.update({"display": bench.session.display, "wayland": bench.session.base.get("WAYLAND_DISPLAY"), "a11y": bench.session.a11y})
    log(f"session: {bench.environment}")
    started = time.time()
    try:
        for app in apps:
            if time.time() - started > options_budget():
                log(f"== {app['id']}: NOT RUN (the shard's time budget is used up)")
                with open(os.path.join(options.out, f"app-{app['id']}.json"), "w", encoding="utf-8") as handle:
                    json.dump({"id": app["id"], "name": app["name"], "family": app["family"], "variants": {}, "not_run": "time budget"}, handle)
                continue
            bench.application(app)
    finally:
        bench.session.down()
        with open(os.path.join(options.out, f"environment-{options.shard}.json"), "w", encoding="utf-8") as handle:
            json.dump(bench.environment, handle, indent=1, default=str)
        with open(os.path.join(options.out, f"fetched-{options.shard}.json"), "w", encoding="utf-8") as handle:
            json.dump(fetch.FETCHED, handle, indent=1, default=str)
    return 0


def options_budget():
    return int(os.environ.get("REHOST_BUDGET_SECONDS", "2700"))


if __name__ == "__main__":
    sys.exit(main())
