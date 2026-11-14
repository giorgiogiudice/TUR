#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""What a seamless desktop needs from the Windows side, measured
(docs/reports/exp-inwin-1.md, section 4): the list of top-level windows
with rectangle, title, icon and z-order; what one full listing costs; and
how soon a change is known when Windows reports it (WinEvents) rather than
when the agent asks again (polling).

    desk.py --out DIR [--target PROGRAM]

The changes are made to a window of TUR's own (tw-controls.exe of the
winsee experiment, or Notepad when it was not built) from this process;
the listener is the same out-of-context hook an agent would hold.
"""

import argparse
import ctypes
import ctypes.wintypes as wt
import json
import os
import pathlib
import statistics
import subprocess
import threading
import time

import win

user32 = win.user32
gdi32 = win.gdi32
WINEVENTPROC = ctypes.WINFUNCTYPE(None, ctypes.c_void_p, wt.DWORD, ctypes.c_void_p, wt.LONG, wt.LONG, wt.DWORD, wt.DWORD)
user32.SetWinEventHook.restype = ctypes.c_void_p
user32.SetWinEventHook.argtypes = [wt.DWORD, wt.DWORD, ctypes.c_void_p, WINEVENTPROC, wt.DWORD, wt.DWORD, wt.DWORD]
user32.UnhookWinEvent.argtypes = [ctypes.c_void_p]
user32.GetMessageW.argtypes = [ctypes.POINTER(wt.MSG), ctypes.c_void_p, wt.UINT, wt.UINT]
user32.PostThreadMessageW.argtypes = [wt.DWORD, wt.UINT, wt.WPARAM, wt.LPARAM]
user32.SetWindowTextW.argtypes = [ctypes.c_void_p, wt.LPCWSTR]
user32.GetIconInfo.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
gdi32.GetObjectW.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p]

EVENTS = {
    0x0003: "foreground", 0x000A: "move-size-start", 0x000B: "move-size-end", 0x0016: "minimise-start", 0x0017: "minimise-end",
    0x8000: "create", 0x8001: "destroy", 0x8002: "show", 0x8003: "hide", 0x8004: "reorder", 0x800B: "location",
    0x800C: "name", 0x8017: "cloaked", 0x8018: "uncloaked",
}


class ICONINFO(ctypes.Structure):
    _fields_ = [("icon", wt.BOOL), ("x", wt.DWORD), ("y", wt.DWORD), ("mask", ctypes.c_void_p), ("colour", ctypes.c_void_p)]


class BITMAP(ctypes.Structure):
    _fields_ = [("type", wt.LONG), ("width", wt.LONG), ("height", wt.LONG), ("stride", wt.LONG), ("planes", wt.WORD), ("bits_per_pixel", wt.WORD), ("bits", ctypes.c_void_p)]


def icon_of(hwnd):
    """The window's icon: where it came from and its size in pixels."""
    for label, wparam in (("WM_GETICON big", 1), ("WM_GETICON small", 2)):
        result = ctypes.c_size_t(0)
        win.SendMessageTimeoutW(hwnd, win.WM_GETICON, wparam, None, win.SMTO_ABORTIFHUNG, 200, ctypes.byref(result))
        if result.value:
            return _icon_size(result.value, label)
    for label, index in (("class icon", -14), ("class small icon", -34)):
        handle = win.GetClassLongPtrW(hwnd, index)
        if handle:
            return _icon_size(handle, label)
    return None


def _icon_size(handle, source):
    info = ICONINFO()
    if not user32.GetIconInfo(ctypes.c_void_p(handle), ctypes.byref(info)):
        return {"source": source}
    bitmap = BITMAP()
    size = None
    if info.colour and gdi32.GetObjectW(ctypes.c_void_p(info.colour), ctypes.sizeof(BITMAP), ctypes.byref(bitmap)):
        size = [bitmap.width, bitmap.height, bitmap.bits_per_pixel]
    for part in (info.colour, info.mask):
        if part:
            win.DeleteObject(part)
    return {"source": source, "size": size}


def listing(icons=True):
    """Every top-level window a desktop would show, top of the z-order first."""
    rows = []
    for window in win.top_windows():
        hwnd = window["hwnd"]
        style = win.GetWindowLongPtrW(hwnd, win.GWL_EXSTYLE)
        owner = win.GetWindow(hwnd, win.GW_OWNER)
        row = {
            "z": window["z"], "class": window["class"], "exe": window["exe"], "title_length": len(window["title"]),
            "rect": window["screen"], "frame": win.frame_of(hwnd), "cloaked": win.cloaked(hwnd),
            "minimised": bool(win.IsIconic(hwnd)), "maximised": bool(win.IsZoomed(hwnd)),
            "tool_window": bool(style & win.WS_EX_TOOLWINDOW), "app_window": bool(style & win.WS_EX_APPWINDOW),
            "no_activate": bool(style & win.WS_EX_NOACTIVATE), "topmost": bool(style & win.WS_EX_TOPMOST), "owned": bool(owner),
        }
        # What Alt+Tab and the taskbar show: unowned, not a tool window, not hidden by the compositor.
        row["on_taskbar"] = (not row["cloaked"]) and ((not row["owned"] and not row["tool_window"]) or row["app_window"]) and bool(window["title"])
        if icons:
            row["icon"] = icon_of(hwnd)
        rows.append(row)
    return rows


class Listener(threading.Thread):
    """An out-of-context WinEvent hook with its message loop."""

    def __init__(self):
        super().__init__(daemon=True)
        self.events = []
        self.ready = threading.Event()
        self.thread_id = 0

    def run(self):
        def on_event(_hook, event, hwnd, id_object, id_child, _thread, _time):
            self.events.append((time.perf_counter(), event, hwnd or 0, id_object, id_child))

        self.callback = WINEVENTPROC(on_event)
        self.thread_id = win.kernel32.GetCurrentThreadId()
        # Flag 0: out of context, and not skipping this process, whose own calls make the changes that are timed.
        hooks = [user32.SetWinEventHook(low, high, None, self.callback, 0, 0, 0) for low, high in ((0x0003, 0x0017), (0x8000, 0x8018))]
        self.hooked = all(hooks)
        self.ready.set()
        message = wt.MSG()
        while user32.GetMessageW(ctypes.byref(message), None, 0, 0) > 0:
            user32.TranslateMessage(ctypes.byref(message))
            user32.DispatchMessageW(ctypes.byref(message))
        for hook in hooks:
            if hook:
                user32.UnhookWinEvent(hook)

    def stop(self):
        user32.PostThreadMessageW(self.thread_id, 0x0012, 0, 0)

    def first(self, since, hwnd, event):
        for stamp, kind, window, id_object, id_child in list(self.events):
            if stamp >= since and kind == event and window == hwnd and id_object == 0 and id_child == 0:
                return stamp
        return None


def covered_content(out):
    """Can the content of a window that another window covers be had from
    inside Windows, without Remote Desktop? Each kind of window is put in
    front and pictured from the screen (the reference), then covered by a
    Notepad window and asked for its own content (PrintWindow)."""
    rows = []
    cover_process = subprocess.Popen(["notepad.exe"])
    time.sleep(3)
    cover = None
    kinds = []
    data = pathlib.Path.home()
    subprocess.Popen(["explorer.exe", str(data)])
    subprocess.Popen(["cmd.exe", "/c", "start", "ms-settings:"])
    edge = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
    edge_process = None
    if pathlib.Path(edge).exists():
        page = (pathlib.Path(__file__).resolve().parent / "form.html").as_uri()
        edge_process = subprocess.Popen([edge, f"--user-data-dir={pathlib.Path(os.environ.get('TEMP', '.')) / 'inwin-edge-profile'}", "--no-first-run", "--no-default-browser-check", "--new-window", page])
    time.sleep(8)
    for window in win.top_windows():
        if window["exe"] == "notepad.exe" and cover is None and window["title"]:
            cover = window
        elif window["class"] == "CabinetWClass":
            kinds.append(("Explorer (classic frame, modern content)", window))
        elif window["class"] == "ApplicationFrameWindow" and "settings" in window["title"].lower():
            kinds.append(("Settings (XAML)", window))
        elif "tur inwin form" in window["title"].lower():
            kinds.append(("Edge (Chromium, composited)", window))
        elif window["exe"] == "tw-controls.exe":
            kinds.append(("TUR's test window (classic Win32)", window))
    if cover is None:
        return [{"error": "no covering window"}]
    for label, window in kinds:
        hwnd = window["hwnd"]
        row = {"window": label}
        try:
            win.ShowWindow(hwnd, 9)
            win.SetWindowPos(hwnd, None, 80, 60, 760, 560, 0x0004)
            win.SetForegroundWindow(hwnd)
            time.sleep(1.5)
            reference = win.grab(win.rect_of(hwnd))
            ok, front = win.print_window(hwnd)
            row["in_front_differs_from_screen"] = round(win.differing(win.crop(reference, 16), win.crop(front, 16)), 4)
            win.ShowWindow(cover["hwnd"], 9)
            win.SetWindowPos(cover["hwnd"], None, 60, 40, 820, 620, 0x0004)
            win.SetForegroundWindow(cover["hwnd"])
            time.sleep(1.2)
            on_screen = win.grab(win.rect_of(hwnd))
            row["screen_shows_the_cover"] = round(win.differing(win.crop(reference, 16), win.crop(on_screen, 16)), 4)
            times = []
            for _ in range(8):
                began = time.perf_counter()
                ok, covered = win.print_window(hwnd)
                times.append((time.perf_counter() - began) * 1000)
            row["print_window_ok"] = ok
            row["covered_differs_from_reference"] = round(win.differing(win.crop(reference, 16), win.crop(covered, 16)), 4)
            row["covered_black_share"] = round(win.black_share(win.crop(covered, 16)), 4)
            row["capture"] = quantiles(times)
            row["pixels"] = [covered[2], covered[3]]
            win.ShowWindow(hwnd, 6)
            time.sleep(0.8)
            ok, minimised = win.print_window(hwnd)
            row["minimised_size"] = [minimised[2], minimised[3]]
            win.ShowWindow(hwnd, 9)
        except Exception as error:
            row["error"] = f"{type(error).__name__}: {str(error)[:200]}"
        rows.append(row)
    for label, window in kinds:
        if "test window" not in label:
            win.PostMessageW(window["hwnd"], win.WM_CLOSE, 0, 0)
    cover_process.kill()
    if edge_process is not None:
        subprocess.run(["taskkill.exe", "/f", "/t", "/pid", str(edge_process.pid)], capture_output=True)
    return rows


def quantiles(values):
    if not values:
        return None
    ordered = sorted(values)
    return {"n": len(ordered), "median_ms": round(statistics.median(ordered), 2), "p95_ms": round(ordered[min(len(ordered) - 1, int(len(ordered) * 0.95))], 2), "max_ms": round(ordered[-1], 2)}


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", required=True)
    parser.add_argument("--target", default="")
    arguments = parser.parse_args()
    out = pathlib.Path(arguments.out)
    out.mkdir(parents=True, exist_ok=True)
    win.dpi_aware()
    result = {"screen": win.screen_box()}

    # Some windows to list: the target, Notepad and an Explorer folder.
    target_program = arguments.target if arguments.target and pathlib.Path(arguments.target).exists() else "notepad.exe"
    processes = [subprocess.Popen([target_program]), subprocess.Popen(["notepad.exe"]) if target_program != "notepad.exe" else None]
    subprocess.Popen(["explorer.exe", str(pathlib.Path.home())])
    name = pathlib.Path(target_program).name.lower()
    target = win.wait_window(lambda w: w["exe"] == name and w["title"], 40)
    if target is None and name != "notepad.exe":
        processes.append(subprocess.Popen(["notepad.exe"]))
        name = "notepad.exe"
        target = win.wait_window(lambda w: w["exe"] == name and w["title"], 40)
    result["target"] = name if target is not None else None
    time.sleep(4)

    rows = listing()
    result["windows"] = rows
    result["counts"] = {
        "visible_top_level": len(rows), "on_taskbar": sum(1 for r in rows if r["on_taskbar"]), "cloaked": sum(1 for r in rows if r["cloaked"]),
        "with_icon": sum(1 for r in rows if r.get("icon")), "taskbar_with_icon": sum(1 for r in rows if r["on_taskbar"] and r.get("icon")),
        "frame_differs_from_rect": sum(1 for r in rows if r["frame"] and r["frame"] != r["rect"]),
    }
    result["shell"] = {cls: any(r["class"] == cls for r in win.top_windows(visible_only=False)) for cls in ("Progman", "Shell_TrayWnd", "WorkerW")}

    for label, icons in (("listing_without_icons", False), ("listing_with_icons", True)):
        times = []
        for _ in range(40):
            began = time.perf_counter()
            listing(icons)
            times.append((time.perf_counter() - began) * 1000)
        result[label] = quantiles(times)

    try:
        result["covered_content"] = covered_content(out)
    except Exception as error:
        result["covered_content"] = [{"error": f"{type(error).__name__}: {str(error)[:300]}"}]

    # Events against polling.
    listener = Listener()
    listener.start()
    listener.ready.wait(10)
    result["hooked"] = getattr(listener, "hooked", False)
    latencies = {"location": [], "name": [], "minimise-start": [], "minimise-end": [], "foreground": []}
    codes = {name: code for code, name in EVENTS.items()}
    if target is not None:
        hwnd = target["hwnd"]
        time.sleep(1)
        idle_from = len(listener.events)
        time.sleep(3)
        result["events_in_3_idle_seconds"] = len(listener.events) - idle_from
        for round_number in range(25):
            for kind, action in (
                ("location", lambda n=round_number: win.SetWindowPos(hwnd, None, 60 + 7 * n, 50 + 5 * n, 0, 0, 0x0001 | 0x0004 | 0x0010)),
                ("name", lambda n=round_number: user32.SetWindowTextW(hwnd, f"inwin {n}")),
                ("minimise-start", lambda: win.ShowWindow(hwnd, 6)),
                ("minimise-end", lambda: win.ShowWindow(hwnd, 9)),
            ):
                since = time.perf_counter()
                action()
                deadline = since + 1.0
                stamp = None
                while time.perf_counter() < deadline and stamp is None:
                    stamp = listener.first(since, hwnd, codes[kind])
                    if stamp is None:
                        time.sleep(0.001)
                if stamp is not None:
                    latencies[kind].append((stamp - since) * 1000)
                time.sleep(0.15)
        burst_from = len(listener.events)
        began = time.perf_counter()
        for step in range(120):
            win.SetWindowPos(hwnd, None, 40 + step * 3, 40 + step * 2, 0, 0, 0x0001 | 0x0004 | 0x0010)
            time.sleep(0.008)
        time.sleep(0.5)
        burst = listener.events[burst_from:]
        result["drag_of_120_steps"] = {
            "seconds": round(time.perf_counter() - began, 2), "events": len(burst),
            "for_the_window_itself": sum(1 for e in burst if e[2] == hwnd and e[3] == 0 and e[1] == 0x800B),
            "by_kind": {EVENTS.get(kind, hex(kind)): sum(1 for e in burst if e[1] == kind) for kind in sorted({e[1] for e in burst})},
        }
        since = time.perf_counter()
        win.PostMessageW(hwnd, win.WM_CLOSE, 0, 0)
        time.sleep(1.5)
        stamp = listener.first(since, hwnd, 0x8001) or listener.first(since, hwnd, 0x8003)
        result["close_seen_after_ms"] = round((stamp - since) * 1000, 2) if stamp else None
    result["event_latency"] = {kind: quantiles(values) for kind, values in latencies.items() if kind != "foreground"}
    result["events_total"] = len(listener.events)
    listener.stop()
    for process in processes:
        if process is not None and process.poll() is None:
            process.kill()
    for other in [w for w in win.top_windows() if w["class"] == "CabinetWClass"]:
        win.PostMessageW(other["hwnd"], win.WM_CLOSE, 0, 0)
    subprocess.run(["taskkill.exe", "/f", "/im", "notepad.exe"], capture_output=True)

    (out / "desk.json").write_text(json.dumps(result, indent=1), encoding="utf-8")
    lines = ["### The window list a seamless desktop needs", "", f"Screen: {result['screen']}; shell windows present: {result['shell']}", "",
             "| Measure | Value |", "|---|---|"]
    for key, value in result["counts"].items():
        lines.append(f"| {key.replace('_', ' ')} | {value} |")
    for key in ("listing_without_icons", "listing_with_icons"):
        lines.append(f"| one full {key.replace('_', ' ')} | {json.dumps(result[key])} |")
    lines.append(f"| events in 3 idle seconds | {result.get('events_in_3_idle_seconds')} |")
    for kind, value in result["event_latency"].items():
        lines.append(f"| change to event: {kind} | {json.dumps(value)} |")
    lines.append(f"| a drag of 120 steps | {json.dumps(result.get('drag_of_120_steps'))} |")
    lines.append(f"| close seen after (ms) | {result.get('close_seen_after_ms')} |")
    lines += ["", "| z | class | program | on taskbar | cloaked | rect | frame | icon |", "|---|---|---|---|---|---|---|---|"]
    for row in rows[:40]:
        lines.append(f"| {row['z']} | {row['class'][:28]} | {row['exe']} | {row['on_taskbar']} | {row['cloaked']} | {row['rect']} | {row['frame']} | {json.dumps(row.get('icon'))} |")
    lines += ["", "### The content of a covered window, asked of the window itself (PrintWindow)", "",
              "Share of pixels that differ from the picture of the same window when it was in front (0 is identical).", "",
              "| Window | In front: differs from the screen | Covered: the screen differs (the cover is there) | Covered: PrintWindow differs from the reference | Black | One capture | Pixels |", "|---|---|---|---|---|---|---|"]
    for row in result.get("covered_content", []):
        lines.append(f"| {row.get('window', '-')} | {row.get('in_front_differs_from_screen')} | {row.get('screen_shows_the_cover')} | {row.get('covered_differs_from_reference')} | {row.get('covered_black_share')} | {json.dumps(row.get('capture'))} | {row.get('pixels')} {row.get('error', '')} |")
    (out / "desk.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("\n".join(lines))


if __name__ == "__main__":
    main()
