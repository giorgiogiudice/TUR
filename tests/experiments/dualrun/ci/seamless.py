# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""H5: Windows applications as separate windows on the host. Two routes, both measured.

`run_agent_route`: nothing asked of Windows' Remote Desktop. TUR's helper in
the guest lists the top-level windows and captures each window's own content
(PrintWindow with full-content rendering, a public API); the host shows them
as separate windows, routes a click back through the virtual tablet, closes
and minimises from outside. It also produces what the simpler idea gives,
cutting each window out of the guest's one screen, so that the difference
(what is hidden behind another window) is in pictures.

`run_remoteapp`: the edition has Remote Desktop (the evaluation is
Enterprise). Remote Desktop and RemoteApp are switched on with Windows' own
documented settings, inside a protected session that is thrown away, and
FreeRDP shows applications as host windows. Nothing of Windows is patched.

The host desktop here is Xvfb with openbox, not KWin on Wayland: what it
shows is window count, titles, icons and pictures, not how KWin behaves.
"""

from __future__ import annotations

import base64
import os
import pathlib
import shutil
import socket
import subprocess
import time

import guestscripts as gs
from winvm import AgentError

DISPLAY = ":77"
SHELL_CLASSES = {"Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd", "Windows.UI.Core.CoreWindow",
                 "ApplicationFrameWindow", "XamlExplorerHostIslandWindow", "TopLevelWindowForOverflowXamlIsland",
                 "Xaml_WindowedPopupClass", "SystemTray_Main", "DummyDWMListenerWindow", "EdgeUiInputTopWndClass"}
VIEWER = r'''
import sys, tkinter
from PIL import Image, ImageTk
root = tkinter.Tk(); root.withdraw(); keep = []
for spec in sys.argv[1:]:
    path, x, y, title = spec.split("|", 3)
    image = Image.open(path).convert("RGB")
    top = tkinter.Toplevel(root); top.title(title); top.geometry(f"{image.width}x{image.height}+{x}+{y}")
    photo = ImageTk.PhotoImage(image); keep.append(photo)
    tkinter.Label(top, image=photo, borderwidth=0).pack()
root.after(600000, root.destroy); root.mainloop()
'''


def parse_windows(text: str) -> list[dict]:
    rows = []
    for line in text.splitlines():
        parts = line.rstrip("\r").split("\t")
        if len(parts) < 14 or not parts[0].strip().isdigit():
            continue
        z, hwnd, pid, left, top, right, bottom, cloaked, iconic, ex, owner, foreground = (int(v) for v in parts[:12])
        rows.append({"z": z, "hwnd": hwnd, "pid": pid, "rect": [left, top, right, bottom], "cloaked": bool(cloaked),
                     "minimised": bool(iconic), "tool_window": bool(ex & 0x80), "owner": owner,
                     "foreground": bool(foreground), "class": parts[12], "title": parts[13]})
    return rows


def find(rows: list[dict], word: str) -> dict | None:
    for row in rows:
        if not row["cloaked"] and word.lower() in (row["title"] + " " + row["class"]).lower() \
                and row["rect"][2] - row["rect"][0] > 120:
            return row
    return None


def start_display() -> list[subprocess.Popen]:
    environment = dict(os.environ, DISPLAY=DISPLAY)
    processes = [subprocess.Popen(["Xvfb", DISPLAY, "-screen", "0", "1600x1000x24", "-nolisten", "tcp"],
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)]
    time.sleep(2)
    processes.append(subprocess.Popen(["openbox"], env=environment, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
    time.sleep(2)
    return processes


def stop(processes: list[subprocess.Popen]) -> None:
    for process in reversed(processes):
        try:
            process.kill()
            process.wait(timeout=10)
        except (OSError, subprocess.TimeoutExpired):
            pass


def host_windows() -> list[dict]:
    """The host desktop's windows: id, geometry, title, whether each has an icon."""
    environment = dict(os.environ, DISPLAY=DISPLAY)
    listing = subprocess.run(["wmctrl", "-lG"], env=environment, capture_output=True, text=True).stdout
    windows = []
    for line in listing.splitlines():
        parts = line.split(None, 7)
        if len(parts) < 7:
            continue
        properties = subprocess.run(["xprop", "-id", parts[0], "_NET_WM_ICON", "WM_CLASS", "_NET_WM_PID"],
                                    env=environment, capture_output=True, text=True).stdout
        icon = [l for l in properties.splitlines() if l.startswith("_NET_WM_ICON")]
        windows.append({"id": parts[0], "x": int(parts[2]), "y": int(parts[3]), "w": int(parts[4]), "h": int(parts[5]),
                        "title": parts[7] if len(parts) > 7 else "",
                        "has_icon": bool(icon) and "not found" not in icon[0],
                        "class": next((l.split("=", 1)[1].strip() for l in properties.splitlines()
                                       if l.startswith("WM_CLASS")), "")})
    return windows


def host_screenshot(path: pathlib.Path) -> bool:
    done = subprocess.run(f"xwd -root -silent -display {DISPLAY} | xwdtopnm 2>/dev/null | pnmtopng > '{path}'",
                          shell=True, capture_output=True)
    return done.returncode == 0 and path.exists() and path.stat().st_size > 1000


def run_agent_route(exp, out: dict) -> None:
    from PIL import Image, ImageDraw
    pictures = exp.out / "seamless"
    pictures.mkdir(exist_ok=True)
    g = exp.start_guest("seamless", mode="protected", identity="carried")
    if not (g.machine and g.ready):
        out["error"] = g.info.get("no_agent") or g.info.get("attach")
        if g.machine:
            exp.end(g, "kill")
            exp.finish_layer(g, "discard")
            exp.close_layer_session(g)
        else:
            exp.results["sessions"].append(g.info)
        return
    m, agent = g.machine, g.machine.agent
    display = []
    try:
        time.sleep(20)
        agent.ps("Start-Process notepad.exe; Start-Process charmap.exe; Start-Sleep -Seconds 8; 'ok'", timeout=120)
        rows = parse_windows(agent.ps(gs.LIST_WINDOWS, timeout=180))
        note, chars = find(rows, "notepad"), find(rows, "character map")
        out["windows_listed"] = len(rows)
        out["found"] = {"notepad": bool(note), "character_map": bool(chars)}
        if not (note and chars):
            out["all_titles"] = [f"{r['class']}: {r['title']}" for r in rows if not r["cloaked"]][:40]
            raise AgentError("the two test applications did not both show a window")
        agent.ps(gs.WINDOW_HELPER + f"[TurWin]::Place({note['hwnd']}, 60, 60, 780, 540); "
                 f"[TurWin]::Place({chars['hwnd']}, 430, 250, 560, 480); [TurWin]::Front({chars['hwnd']}); "
                 "Start-Sleep -Seconds 3; 'ok'", timeout=120)
        rows = parse_windows(agent.ps(gs.LIST_WINDOWS, timeout=120))
        note, chars = find(rows, "notepad"), find(rows, "character map")
        screen_path, _ = m.screenshot("two-windows")
        screen = Image.open(screen_path).convert("RGB")
        screen.save(pictures / "1-guest-screen.png")
        width, height = screen.size
        out["guest_screen"] = [width, height]
        out["shell_windows_the_host_must_hide"] = sorted({r["class"] for r in rows if not r["cloaked"]
                                                          and (r["class"] in SHELL_CLASSES or r["tool_window"])})
        report = {}
        specs = []
        for name, row in (("notepad", note), ("character-map", chars)):
            left, top, right, bottom = row["rect"]
            item: dict = {"title": row["title"], "class": row["class"], "rect": row["rect"], "z": row["z"]}
            # (i) cut out of the one guest screen: what is behind another window is not there.
            clipped = screen.crop((max(0, left), max(0, top), min(width, right), min(height, bottom)))
            above = [r for r in rows if r["z"] < row["z"] and not r["cloaked"] and not r["minimised"]
                     and r["class"] not in SHELL_CLASSES and r["hwnd"] != row["hwnd"]]
            draw = ImageDraw.Draw(clipped)
            hidden = 0
            for other in above:
                ol, ot, orr, ob = other["rect"]
                il, it, ir, ib = max(left, ol), max(top, ot), min(right, orr), min(bottom, ob)
                if ir > il and ib > it:
                    hidden += (ir - il) * (ib - it)
                    draw.rectangle((il - left, it - top, ir - left - 1, ib - top - 1), outline=(255, 0, 0), width=4)
            clipped.save(pictures / f"2-cut-from-screen-{name}.png")
            item["cut_from_screen_hidden_fraction"] = round(hidden / max(1, (right - left) * (bottom - top)), 3)
            # (ii) the window's own content, asked from Windows.
            answer = agent.ps(gs.capture_window(row["hwnd"]), timeout=600).strip().splitlines()
            stats = answer[0].split("\t")
            picture = pictures / f"3-own-content-{name}.png"
            picture.write_bytes(base64.b64decode("".join(answer[1:])))
            item["own_content"] = {"api_said_ok": stats[0] == "1", "size": [int(stats[1]), int(stats[2])],
                                   "milliseconds": int(stats[3]),
                                   "sample_points_not_flat": f"{stats[4]}/{stats[5]}", "png_bytes": picture.stat().st_size}
            times = [int(v) for v in agent.ps(gs.time_captures(row["hwnd"]), timeout=300).strip().split(",") if v.strip().isdigit()]
            item["capture_ms_10_runs"] = times
            report[name] = item
            specs.append(f"{picture}|{max(0, left) + 40}|{max(0, top) + 40}|{row['title']} (Windows)")
        out["windows"] = report

        # Two host windows showing the two guest windows.
        display = start_display()
        viewer = exp.vm / "viewer.py"
        viewer.write_text(VIEWER)
        display.append(subprocess.Popen(["python3", str(viewer), *specs], env=dict(os.environ, DISPLAY=DISPLAY),
                                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
        time.sleep(6)
        out["host_windows"] = host_windows()
        out["host_picture"] = host_screenshot(pictures / "4-host-desktop-two-windows.png")

        # A click on the host's picture of the lower window, routed back to the guest.
        left, top, right, bottom = note["rect"]
        click = (left + 200, top + 14)
        started = time.time()
        m.click(click[0], click[1], width, height)
        front = None
        while time.time() - started < 15:
            rows = parse_windows(agent.ps(gs.LIST_WINDOWS, timeout=60))
            front = next((r for r in rows if r["foreground"]), None)
            if front and front["hwnd"] == note["hwnd"]:
                break
            time.sleep(0.3)
        out["click_routed_back"] = {"guest_point": list(click), "lower_window_came_to_front": bool(front and front["hwnd"] == note["hwnd"]),
                                    "seconds_until_seen": round(time.time() - started, 2),
                                    "note": "includes the agent's own answer time over a serial line"}
        # A menu: is it a window of its own?
        before = {r["hwnd"] for r in rows if not r["cloaked"]}
        m.click(left + 28, top + 50, width, height)
        time.sleep(2)
        rows = parse_windows(agent.ps(gs.LIST_WINDOWS, timeout=60))
        out["menu_opens_as"] = [f"{r['class']} {r['rect']}" for r in rows if not r["cloaked"] and r["hwnd"] not in before][:6]
        m.key("esc")
        # Close and minimise from outside.
        agent.ps(gs.WINDOW_HELPER + f"[TurWin]::Close({chars['hwnd']}); [TurWin]::Minimise({note['hwnd']}); Start-Sleep -Seconds 3; 'ok'", timeout=60)
        rows = parse_windows(agent.ps(gs.LIST_WINDOWS, timeout=60))
        gone = not any(r["hwnd"] == chars["hwnd"] for r in rows)
        small = next((r for r in rows if r["hwnd"] == note["hwnd"]), None)
        out["from_outside"] = {"closed": gone, "minimised": bool(small and small["minimised"])}
        answer = agent.ps(gs.capture_window(note["hwnd"]), timeout=300).strip().splitlines()
        stats = answer[0].split("\t")
        out["minimised_window_content"] = {"api_said_ok": stats[0] == "1", "size": [int(stats[1]), int(stats[2])],
                                           "sample_points_not_flat": f"{stats[4]}/{stats[5]}"}
    except Exception as error:  # noqa: BLE001
        out["error"] = f"{type(error).__name__}: {error}"[:400]
    finally:
        stop(display)
        exp.end(g, "shutdown")
        exp.finish_layer(g, "discard")
        exp.close_layer_session(g)


ENABLE_REMOTE_DESKTOP = r'''
$ErrorActionPreference = 'SilentlyContinue'
Set-ItemProperty 'HKLM:\System\CurrentControlSet\Control\Terminal Server' -Name fDenyTSConnections -Value 0
Enable-NetFirewallRule -DisplayGroup 'Remote Desktop'
New-Item 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Terminal Server\TSAppAllowList' -Force | Out-Null
Set-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Terminal Server\TSAppAllowList' -Name fDisabledAllowList -Value 1 -Type DWord
Start-Service TermService
New-Item -ItemType Directory -Force C:\dualrun\share | Out-Null
New-SmbShare -Name tur -Path C:\dualrun\share -FullAccess tester | Out-Null
Enable-NetFirewallRule -DisplayGroup 'File and Printer Sharing'
Start-Sleep -Seconds 6
@{ service="$((Get-Service TermService).Status)"; listening=[bool](Get-NetTCPConnection -LocalPort 3389 -State Listen); addresses=@(Get-NetIPAddress -AddressFamily IPv4 | ForEach-Object { $_.IPAddress }); network="$((Get-NetConnectionProfile).NetworkCategory)"; share=[bool](Get-SmbShare -Name tur) } | ConvertTo-Json -Compress
'''


def wait_port(port: int, seconds: float) -> bool:
    deadline = time.time() + seconds
    while time.time() < deadline:
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=3):
                return True
        except OSError:
            time.sleep(2)
    return False


def run_remoteapp(exp, out: dict) -> None:
    client = shutil.which("xfreerdp3") or shutil.which("xfreerdp")
    out["client"] = client
    if client:
        out["client_version"] = subprocess.run([client, "/version"], capture_output=True, text=True).stdout.strip()[:120]
    else:
        out["not_run"] = "no FreeRDP client on the runner"
        return
    version3 = "xfreerdp3" in client or "version 3" in out.get("client_version", "")
    pictures = exp.out / "seamless"
    pictures.mkdir(exist_ok=True)
    g = exp.start_guest("remoteapp", mode="protected", identity="carried",
                        net={"hostfwd": ["tcp:127.0.0.1:13389-:3389", "tcp:127.0.0.1:14445-:445"]})
    if not (g.machine and g.ready):
        out["error"] = g.info.get("no_agent") or g.info.get("attach")
        if g.machine:
            exp.end(g, "kill")
            exp.finish_layer(g, "discard")
            exp.close_layer_session(g)
        else:
            exp.results["sessions"].append(g.info)
        return
    agent = g.machine.agent
    display: list[subprocess.Popen] = []
    share = exp.vm / "share"
    share.mkdir(exist_ok=True)
    (share / "from-tur.txt").write_text("a file in a folder TUR shares with Windows\n")
    environment = dict(os.environ, DISPLAY=DISPLAY)
    try:
        out["windows_settings_changed"] = agent.json(ENABLE_REMOTE_DESKTOP, timeout=240)
        out["port_reachable"] = wait_port(13389, 90)
        display = start_display()

        def launch(program: str, tag: str) -> dict:
            app = f"/app:program:{program}" if version3 else f"/app:{program}"
            command = [client, "/v:127.0.0.1:13389", "/u:tester", f"/p:{exp.password}", "/cert:ignore",
                       "+clipboard", f"/drive:tur,{share}", app]
            before = {w["id"] for w in host_windows()}
            log = open(exp.out / "sessions" / "remoteapp" / f"freerdp-{tag}.log", "ab")
            started = time.time()
            display.append(subprocess.Popen(command, env=environment, stdout=log, stderr=log))
            new: list[dict] = []
            while time.time() - started < 120 and not new:
                time.sleep(1)
                new = [w for w in host_windows() if w["id"] not in before]
            return {"program": program, "seconds_to_first_window": round(time.time() - started, 1) if new else None,
                    "client_still_running": display[-1].poll() is None, "windows": new}

        first = launch("charmap.exe" if version3 else r"C:\Windows\System32\charmap.exe", "charmap")
        out["first_application"] = first
        time.sleep(5)
        second = launch("notepad.exe" if version3 else r"C:\Windows\System32\notepad.exe", "notepad")
        out["second_application"] = second
        time.sleep(8)
        windows = host_windows()
        out["host_windows"] = windows
        out["host_picture"] = host_screenshot(pictures / "5-remoteapp-host-desktop.png")
        try:
            rows = parse_windows(agent.ps(gs.LIST_WINDOWS, timeout=120))
            out["guest_windows_now"] = [f"{r['class']}: {r['title']} {r['rect']}" for r in rows
                                        if not r["cloaked"] and r["class"] not in SHELL_CLASSES and r["title"]][:12]
        except AgentError as error:
            out["guest_windows_now"] = f"agent: {error}"
        # Resize from the host; does the guest's window follow?
        target = next((w for w in windows if "notepad" in w["title"].lower()), windows[-1] if windows else None)
        if target:
            started = time.time()
            subprocess.run(["wmctrl", "-i", "-r", target["id"], "-e", "0,120,90,900,620"], env=environment)
            followed = None
            while time.time() - started < 20 and followed is None:
                time.sleep(0.4)
                try:
                    rows = parse_windows(agent.ps(gs.LIST_WINDOWS, timeout=60))
                except AgentError:
                    break
                for r in rows:
                    if "notepad" in (r["title"] + r["class"]).lower() and abs((r["rect"][2] - r["rect"][0]) - 900) < 60:
                        followed = round(time.time() - started, 2)
            out["resize_from_host"] = {"guest_window_followed_after_s": followed,
                                       "note": "includes the agent's answer time over a serial line"}
            host_screenshot(pictures / "6-remoteapp-after-resize.png")
        # Clipboard, both ways.
        clip: dict = {}
        subprocess.run(["xclip", "-selection", "clipboard"], input="from TUR 1234", text=True, env=environment, timeout=20)
        time.sleep(3)
        try:
            clip["guest_got"] = agent.ps("Get-Clipboard", timeout=60).strip()
            agent.ps("Set-Clipboard 'from Windows 5678'; 'ok'", timeout=60)
            time.sleep(3)
            clip["host_got"] = subprocess.run(["xclip", "-o", "-selection", "clipboard"], env=environment,
                                              capture_output=True, text=True, timeout=20).stdout.strip()
        except (AgentError, subprocess.TimeoutExpired) as error:
            clip["error"] = str(error)[:200]
        out["clipboard"] = clip
        # The folder redirected by the client.
        folder: dict = {}
        try:
            folder["guest_reads"] = agent.ps(r"Get-Content \\tsclient\tur\from-tur.txt", timeout=60).strip()[:80]
            agent.ps(r"Set-Content \\tsclient\tur\from-windows.txt 'written by Windows'; 'ok'", timeout=60)
            folder["host_sees_guest_file"] = (share / "from-windows.txt").exists()
        except AgentError as error:
            folder["error"] = str(error)[:200]
        out["redirected_folder"] = folder
        # Windows' own file sharing, reached from the host.
        smb: dict = {"port_reachable": wait_port(14445, 20)}
        if smb["port_reachable"] and shutil.which("smbclient"):
            local = exp.vm / "via-smb.txt"
            local.write_text("written by TUR through Windows' file sharing\n")
            done = subprocess.run(["smbclient", "//127.0.0.1/tur", "-p", "14445", "-U", f"tester%{exp.password}",
                                   "-c", f"put {local} via-smb.txt; ls"], capture_output=True, text=True, timeout=120)
            smb["client_exit"] = done.returncode
            smb["client_says"] = (done.stdout + done.stderr).replace(exp.password, "***").strip()[-300:]
            try:
                smb["guest_sees_file"] = agent.ps(r"Get-Content C:\dualrun\share\via-smb.txt", timeout=60).strip()[:80]
            except AgentError as error:
                smb["guest_sees_file"] = f"error: {error}"[:160]
        out["windows_file_sharing"] = smb
    except Exception as error:  # noqa: BLE001
        out["error"] = f"{type(error).__name__}: {error}"[:400]
    finally:
        stop(display)
        for log in (exp.out / "sessions" / "remoteapp").glob("freerdp-*.log"):
            try:
                log.write_text(log.read_text(errors="replace").replace(exp.password, "***")[-20000:])
            except OSError:
                pass
        shutil.rmtree(share, ignore_errors=True)
        exp.end(g, "shutdown")
        exp.finish_layer(g, "discard")
        exp.close_layer_session(g)
