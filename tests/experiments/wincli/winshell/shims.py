# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""TUR-aware commands: Windows command names whose honest meaning on TUR is
something TUR does or knows.

Each shim takes the arguments a Windows user would type and prints what
that command prints on Windows, in the same shape, from this computer's
real state (network, processes, hardware), or, where the Windows command
has no literal meaning here (winget, sfc, chkdsk, shutdown), says what it
does on Windows and what the TUR action is. Explanations go to standard
error, behind the marker `tur-winshell: answer:`, so a script that reads
the output is not fed prose.

Prototype quality: enough switches to measure with.
"""

from __future__ import annotations

import base64
import ipaddress
import json
import os
import pathlib
import re
import shutil
import signal
import socket
import subprocess
import sys
import time

from winpath import WinEnv

HERE = pathlib.Path(__file__).resolve().parent


def out(text: str = "") -> None:
    sys.stdout.write(text + "\r\n")


def answer(text: str) -> None:
    sys.stderr.write(f"tur-winshell: answer: {text}\n")


def read(path: str) -> str:
    try:
        return pathlib.Path(path).read_text(errors="replace").strip()
    except OSError:
        return ""


def ip_json(*args: str) -> list:
    try:
        done = subprocess.run(["ip", "-j", *args], capture_output=True, text=True, timeout=10, check=False)
        return json.loads(done.stdout) if done.returncode == 0 and done.stdout.strip() else []
    except (OSError, ValueError, subprocess.SubprocessError):
        return []


def switches(argv: list[str]) -> list[str]:
    return [a.lower() for a in argv if a.startswith(("/", "-"))]


# ------------------------------------------------------------ network ----

def dns_servers() -> list[str]:
    servers = []
    for line in read("/etc/resolv.conf").splitlines():
        match = re.match(r"\s*nameserver\s+(\S+)", line)
        if match:
            servers.append(match.group(1))
    return servers


def dns_suffix() -> str:
    for line in read("/etc/resolv.conf").splitlines():
        match = re.match(r"\s*(?:search|domain)\s+(\S+)", line)
        if match:
            return match.group(1)
    return ""


def adapters() -> list[dict]:
    gateways: dict[str, list[str]] = {}
    for route in ip_json("route", "show", "default") + ip_json("-6", "route", "show", "default"):
        if route.get("gateway"):
            gateways.setdefault(route.get("dev", ""), []).append(route["gateway"])
    found = []
    for link in ip_json("addr", "show"):
        if link.get("link_type") == "loopback":
            continue
        name = link.get("ifname", "?")
        kind = "Wireless LAN adapter" if name.startswith("wl") else "Ethernet adapter"
        found.append({
            "name": name, "kind": kind, "mac": (link.get("address") or "").upper().replace(":", "-"),
            "up": link.get("operstate") != "DOWN" and "NO-CARRIER" not in link.get("flags", []),
            "v4": [a for a in link.get("addr_info", []) if a.get("family") == "inet"],
            "v6": [a for a in link.get("addr_info", []) if a.get("family") == "inet6"],
            "gateways": gateways.get(name, []),
        })
    return found


def field(label: str, value: str) -> None:
    """One `Label . . . . : value` line, dotted out to the usual column."""
    text = label + ("  ." if label == "Connection-specific DNS Suffix" else "")
    while len(text) < 33:
        text += "." if len(text) % 2 == 0 else " "
    out(f"   {text} : {value}".rstrip())


def ipconfig(argv: list[str], env: WinEnv) -> int:
    sw = switches(argv)
    if any(s in ("/flushdns",) for s in sw):
        tool = shutil.which("resolvectl")
        if tool and subprocess.run([tool, "flush-caches"], capture_output=True, check=False).returncode == 0:
            out()
            out("Windows IP Configuration")
            out()
            out("Successfully flushed the DNS Resolver Cache.")
            return 0
        answer("'ipconfig /flushdns' empties the DNS cache. This computer's name-resolution service is not "
               "running a cache that can be flushed, so there was nothing to do.")
        return 0
    if any(s in ("/release", "/renew", "/registerdns") for s in sw):
        answer("'ipconfig /release' and '/renew' give up and re-request the network address. On TUR the network "
               "is managed in Settings > Network; reconnect there. Nothing was changed.")
        return 0
    everything = "/all" in sw
    out()
    out("Windows IP Configuration")
    out()
    if everything:
        field("Host Name", socket.gethostname())
        field("Primary Dns Suffix", "")
        field("Node Type", "Hybrid")
        field("IP Routing Enabled", "Yes" if read("/proc/sys/net/ipv4/ip_forward") == "1" else "No")
        field("WINS Proxy Enabled", "No")
        out()
    for adapter in adapters():
        out(f"{adapter['kind']} {adapter['name']}:")
        out()
        if not adapter["up"] and not adapter["v4"]:
            field("Media State", "Media disconnected")
            field("Connection-specific DNS Suffix", "")
            if everything:
                field("Physical Address", adapter["mac"])
            out()
            continue
        field("Connection-specific DNS Suffix", dns_suffix())
        if everything:
            driver = os.path.basename(os.path.realpath(f"/sys/class/net/{adapter['name']}/device/driver"))
            field("Description", f"{adapter['name']} ({driver})" if driver and driver != "driver" else adapter["name"])
            field("Physical Address", adapter["mac"])
            field("DHCP Enabled", "Yes" if any(a.get("dynamic") for a in adapter["v4"]) else "No")
        for addr in adapter["v6"]:
            label = "Link-local IPv6 Address" if addr.get("scope") == "link" else "IPv6 Address"
            field(label, addr["local"] + ("(Preferred)" if everything else ""))
        for addr in adapter["v4"]:
            field("IPv4 Address", addr["local"] + ("(Preferred)" if everything else ""))
            field("Subnet Mask", str(ipaddress.ip_network(f"0.0.0.0/{addr.get('prefixlen', 32)}").netmask))
        gateways = adapter["gateways"]
        field("Default Gateway", gateways[0] if gateways else "")
        for extra in gateways[1:]:
            out(" " * 39 + extra)
        if everything:
            servers = dns_servers()
            field("DNS Servers", servers[0] if servers else "")
            for extra in servers[1:]:
                out(" " * 39 + extra)
        out()
    return 0


def getmac(argv: list[str], env: WinEnv) -> int:
    out()
    out("Physical Address    Transport Name")
    out("=================== ==========================================================")
    for adapter in adapters():
        out(f"{adapter['mac']:<19} {adapter['name'] if adapter['up'] else 'Media disconnected'}")
    return 0


def route(argv: list[str], env: WinEnv) -> int:
    if not argv or argv[0].lower() != "print":
        answer("'route add/delete/change' edits the routing table. On TUR, routes belong to the network "
               "connection (Settings > Network). Nothing was changed.")
        return 0
    own = {a["name"]: (a["v4"][0]["local"] if a["v4"] else "") for a in adapters()}
    out("=" * 75)
    out("Interface List")
    for link in ip_json("link", "show"):
        mac = (link.get("address") or "").replace(":", " ")
        out(f"{link.get('ifindex', 0):>3}...{mac:<18}......{link.get('ifname', '')}")
    out("=" * 75)
    out()
    out("IPv4 Route Table")
    out("=" * 75)
    out("Active Routes:")
    out("Network Destination        Netmask          Gateway       Interface  Metric")
    for entry in ip_json("route", "show"):
        dst = entry.get("dst", "")
        if dst == "default":
            network = ipaddress.ip_network("0.0.0.0/0")
        else:
            try:
                network = ipaddress.ip_network(dst, strict=False)
            except ValueError:
                continue
        gateway = entry.get("gateway", "On-link")
        iface = entry.get("prefsrc") or own.get(entry.get("dev", ""), "")
        out(f"{str(network.network_address):>17} {str(network.netmask):>16} {gateway:>16} {iface:>15} "
            f"{entry.get('metric', 0):>6}")
    out("=" * 75)
    return 0


def tracert(argv: list[str], env: WinEnv) -> int:
    hops, numeric, target, n = 30, False, None, 0
    while n < len(argv):
        low = argv[n].lower()
        if low in ("-d", "/d"):
            numeric = True
        elif low in ("-h", "/h") and n + 1 < len(argv):
            n += 1
            hops = int(argv[n])
        elif low in ("-w", "/w", "-j", "/j", "-s", "/s") and n + 1 < len(argv):
            n += 1
        elif not low.startswith(("-", "/")):
            target = argv[n]
        n += 1
    if not target:
        out("Usage: tracert [-d] [-h maximum_hops] [-w timeout] target_name")
        return 1
    try:
        address = socket.getaddrinfo(target, None, socket.AF_INET)[0][4][0]
    except OSError:
        out(f"Unable to resolve target system name {target}.")
        return 1
    tool = shutil.which("traceroute")
    if not tool:
        answer("'tracert' shows the route packets take. This computer has no traceroute program installed.")
        return 1
    shown = address if target == address else f"{target} [{address}]"
    out()
    out(f"Tracing route to {shown} over a maximum of {hops} hops")
    out()
    cmd = [tool, "-m", str(hops), "-w", "2", "-q", "3"] + (["-n"] if numeric else []) + [address]
    done = subprocess.run(cmd, capture_output=True, text=True, timeout=hops * 8 + 20, check=False)
    for line in done.stdout.splitlines():
        match = re.match(r"\s*(\d+)\s+(.*)$", line)
        if not match:
            continue
        rest = match.group(2)
        times = re.findall(r"([\d.]+) ms|(\*)", rest)
        cells = []
        for value, star in times[:3]:
            if star:
                cells.append("    *   ")
            else:
                ms = float(value)
                cells.append(f"{'<1' if ms < 1 else str(int(round(ms))):>5} ms")
        while len(cells) < 3:
            cells.append("    *   ")
        host = re.match(r"(\S+)(?: \(([\d.]+)\))?", rest)
        if host and host.group(1) != "*":
            name = host.group(1) if not host.group(2) or host.group(1) == host.group(2) else \
                f"{host.group(1)} [{host.group(2)}]"
        else:
            name = "Request timed out."
        out(f"{int(match.group(1)):>3}  {'  '.join(cells)}  {name}")
    out()
    out("Trace complete.")
    return 0


def nslookup(argv: list[str], env: WinEnv) -> int:
    qtype, names = "A", []
    for arg in argv:
        match = re.match(r"(?i)^-(?:type|q|querytype)=(\w+)$", arg)
        if match:
            qtype = match.group(1).upper()
        elif not arg.startswith("-"):
            names.append(arg)
    if not names:
        answer("'nslookup' without a name starts an interactive session, which this prototype does not offer. "
               "Give it a name: nslookup example.com")
        return 1
    name = names[0]
    servers = [names[1]] if len(names) > 1 else dns_servers()
    out("Server:  UnKnown")
    out(f"Address:  {servers[0] if servers else ''}")
    out()
    if qtype in ("A", "AAAA", "ANY"):
        try:
            infos = socket.getaddrinfo(name, None, type=socket.SOCK_STREAM)
        except OSError:
            sys.stderr.write(f"*** UnKnown can't find {name}: Non-existent domain\r\n")
            return 1
        found = []
        for info in infos:
            if info[4][0] not in found:
                found.append(info[4][0])
        sys.stderr.write("Non-authoritative answer:\r\n")
        out(f"Name:    {name}")
        if len(found) == 1:
            out(f"Address:  {found[0]}")
        else:
            out(f"Addresses:  {found[0]}")
            for extra in found[1:]:
                out(f"\t  {extra}")
        out()
        return 0
    dig = shutil.which("dig")
    if not dig:
        answer(f"'nslookup -type={qtype}' needs a DNS query tool; none is installed on this computer.")
        return 1
    done = subprocess.run([dig, "+short", "-t", qtype, name] + ([f"@{servers[0]}"] if len(names) > 1 else []),
                          capture_output=True, text=True, timeout=30, check=False)
    lines = [line.strip() for line in done.stdout.splitlines() if line.strip()]
    if not lines:
        sys.stderr.write(f"*** UnKnown can't find {name}: Non-existent domain\r\n")
        return 1
    sys.stderr.write("Non-authoritative answer:\r\n")
    for line in lines:
        if qtype == "MX":
            pref, _, host = line.partition(" ")
            out(f"{name}\tMX preference = {pref}, mail exchanger = {host.rstrip('.')}")
        elif qtype == "TXT":
            out(f"{name}\ttext =\r\n\r\n\t{line}")
        elif qtype == "NS":
            out(f"{name}\tnameserver = {line.rstrip('.')}")
        elif qtype == "CNAME":
            out(f"{name}\tcanonical name = {line.rstrip('.')}")
        else:
            out(f"{name}\t{qtype} = {line}")
    out()
    return 0


# ---------------------------------------------------------- processes ----

def processes() -> list[dict]:
    found = []
    for entry in os.scandir("/proc"):
        if not entry.name.isdigit():
            continue
        pid = int(entry.name)
        try:
            raw = pathlib.Path(f"/proc/{pid}/cmdline").read_bytes()
            status = pathlib.Path(f"/proc/{pid}/status").read_text(errors="replace")
        except OSError:
            continue
        first = raw.split(b"\0")[0].decode(errors="replace")
        comm = read(f"/proc/{pid}/comm")
        # A Windows program's first argument is its Windows path.
        match = re.search(r"([^\\/]+\.exe)$", first, re.I)
        image = match.group(1) if match else comm
        rss = re.search(r"VmRSS:\s+(\d+)", status)
        uid = re.search(r"Uid:\s+(\d+)", status)
        stat = read(f"/proc/{pid}/stat")
        tty = stat.rsplit(")", 1)[-1].split()[4:5] if stat else []
        loginuid = read(f"/proc/{pid}/loginuid")
        interactive = (tty and tty[0] != "0") or (loginuid not in ("", "4294967295"))
        found.append({"image": image, "pid": pid, "session": "Console" if interactive else "Services",
                      "num": 1 if interactive else 0, "kb": int(rss.group(1)) if rss else 0,
                      "uid": int(uid.group(1)) if uid else -1, "windows": bool(match)})
    return sorted(found, key=lambda p: p["pid"])


def _filters(argv: list[str]) -> list[tuple[str, str, str]]:
    found = []
    for n, arg in enumerate(argv):
        if arg.lower() == "/fi" and n + 1 < len(argv):
            match = re.match(r"(?i)\s*(\w+)\s+(eq|ne|gt|lt|ge|le)\s+(.*)$", argv[n + 1].strip('"'))
            if match:
                found.append((match.group(1).lower(), match.group(2).lower(), match.group(3).strip()))
    return found


def _matches(proc: dict, filters: list[tuple[str, str, str]]) -> bool:
    for name, op, value in filters:
        if name == "imagename":
            pattern = "^" + re.escape(value).replace(r"\*", ".*") + "$"
            hit = re.match(pattern, proc["image"], re.I) is not None
            if hit != (op == "eq"):
                return False
        elif name == "pid":
            ok = {"eq": proc["pid"] == int(value), "ne": proc["pid"] != int(value), "gt": proc["pid"] > int(value),
                  "lt": proc["pid"] < int(value), "ge": proc["pid"] >= int(value), "le": proc["pid"] <= int(value)}
            if not ok[op]:
                return False
        elif name == "memusage":
            ok = {"eq": proc["kb"] == int(value), "ne": proc["kb"] != int(value), "gt": proc["kb"] > int(value),
                  "lt": proc["kb"] < int(value), "ge": proc["kb"] >= int(value), "le": proc["kb"] <= int(value)}
            if not ok[op]:
                return False
        elif name == "sessionname":
            if (proc["session"].lower() == value.lower()) != (op == "eq"):
                return False
    return True


def tasklist(argv: list[str], env: WinEnv) -> int:
    low = [a.lower() for a in argv]
    fmt = low[low.index("/fo") + 1] if "/fo" in low and low.index("/fo") + 1 < len(low) else "table"
    header = "/nh" not in low
    rows = [p for p in processes() if _matches(p, _filters(argv))]
    if not rows:
        out("INFO: No tasks are running which match the specified criteria.")
        return 0
    if fmt == "csv":
        if header:
            out('"Image Name","PID","Session Name","Session#","Mem Usage"')
        for p in rows:
            out(f'"{p["image"]}","{p["pid"]}","{p["session"]}","{p["num"]}","{p["kb"]:,} K"')
        return 0
    if fmt == "list":
        for p in rows:
            out()
            out(f"Image Name:   {p['image']}")
            out(f"PID:          {p['pid']}")
            out(f"Session Name: {p['session']}")
            out(f"Session#:     {p['num']}")
            out(f"Mem Usage:    {p['kb']:,} K")
        return 0
    out()
    if header:
        out("Image Name                     PID Session Name        Session#    Mem Usage")
        out("========================= ======== ================ =========== ============")
    for p in rows:
        out(f"{p['image'][:25]:<25} {p['pid']:>8} {p['session']:<16} {p['num']:>11} {p['kb']:>10,} K")
    return 0


def _other_environment(pid: int, env: WinEnv) -> bool:
    """A Windows program that belongs to a different Windows environment:
    taskkill by name leaves those alone."""
    try:
        raw = pathlib.Path(f"/proc/{pid}/environ").read_bytes()
    except OSError:
        return True
    for item in raw.split(b"\0"):
        if item.startswith(b"WINEPREFIX="):
            return os.path.realpath(item[11:].decode(errors="replace")) != os.path.realpath(env.prefix)
    return False


def taskkill(argv: list[str], env: WinEnv) -> int:
    low = [a.lower() for a in argv]
    force = "/f" in low
    images = [argv[n + 1] for n, a in enumerate(low) if a == "/im" and n + 1 < len(argv)]
    pids = [argv[n + 1] for n, a in enumerate(low) if a == "/pid" and n + 1 < len(argv)]
    if not images and not pids:
        sys.stderr.write("ERROR: Invalid syntax. Neither /FI nor /PID nor /IM were specified.\r\n")
        return 1
    me = os.getuid()
    code = 0
    everything = processes()
    for image in images:
        pattern = "^" + re.escape(image.strip('"')).replace(r"\*", ".*") + "$"
        hits = [p for p in everything if re.match(pattern, p["image"], re.I) and p["pid"] not in (os.getpid(), 1)
                and not (p["windows"] and _other_environment(p["pid"], env))]
        if not hits:
            sys.stderr.write(f'ERROR: The process "{image}" not found.\r\n')
            code = 128
        for p in hits:
            if p["uid"] != me and me != 0:
                sys.stderr.write(f'ERROR: The process "{p["image"]}" with PID {p["pid"]} could not be terminated.\r\n'
                                 "Reason: Access is denied.\r\n")
                code = 1
                continue
            try:
                os.kill(p["pid"], signal.SIGKILL if force else signal.SIGTERM)
                kind = "SUCCESS: The process" if force else "SUCCESS: Sent termination signal to the process"
                out(f'{kind} "{p["image"]}" with PID {p["pid"]}' + (" has been terminated." if force else "."))
            except OSError:
                code = 1
    for pid in pids:
        try:
            os.kill(int(pid), signal.SIGKILL if force else signal.SIGTERM)
            out(f"SUCCESS: The process with PID {pid} has been terminated.")
        except (OSError, ValueError):
            sys.stderr.write(f'ERROR: The process "{pid}" not found.\r\n')
            code = 128
    return code


# ------------------------------------------------------------- system ----

def os_release() -> dict:
    values = {}
    for line in read("/etc/os-release").splitlines():
        key, _, value = line.partition("=")
        values[key] = value.strip('"')
    return values


def systeminfo(argv: list[str], env: WinEnv) -> int:
    release = os_release()
    mem = {m.group(1): int(m.group(2)) for m in re.finditer(r"(\w+):\s+(\d+)", read("/proc/meminfo"))}
    models = re.findall(r"model name\s*:\s*(.*)", read("/proc/cpuinfo"))
    speeds = re.findall(r"cpu MHz\s*:\s*([\d.]+)", read("/proc/cpuinfo"))
    sockets = sorted(set(re.findall(r"physical id\s*:\s*(\d+)", read("/proc/cpuinfo")))) or ["0"]
    boot = time.time() - float((read("/proc/uptime") or "0").split()[0])
    rows = [
        ("Host Name", socket.gethostname()),
        ("OS Name", release.get("PRETTY_NAME", "Linux")),
        ("OS Version", read("/proc/sys/kernel/osrelease")),
        ("OS Manufacturer", release.get("NAME", "")),
        ("OS Configuration", "Standalone Workstation"),
        ("System Boot Time", time.strftime("%m/%d/%Y, %I:%M:%S %p", time.localtime(boot))),
        ("System Manufacturer", read("/sys/class/dmi/id/sys_vendor")),
        ("System Model", read("/sys/class/dmi/id/product_name")),
        ("System Type", "x64-based PC"),
        ("Processor(s)", f"{len(sockets)} Processor(s) Installed."),
    ]
    for n, _socket in enumerate(sockets, 1):
        model = models[0] if models else "?"
        speed = f" ~{int(float(speeds[0]))} Mhz" if speeds else ""
        rows.append(("", f"[{n:02}]: {model}{speed}"))
    rows += [
        ("BIOS Version", f"{read('/sys/class/dmi/id/bios_vendor')} {read('/sys/class/dmi/id/bios_version')}, "
                         f"{read('/sys/class/dmi/id/bios_date')}"),
        ("Windows Directory", env.vars["WINDIR"]),
        ("System Directory", env.vars["WINDIR"] + "\\system32"),
        ("Time Zone", time.strftime("(UTC%z) %Z")),
        ("Total Physical Memory", f"{mem.get('MemTotal', 0) // 1024:,} MB"),
        ("Available Physical Memory", f"{mem.get('MemAvailable', 0) // 1024:,} MB"),
        ("Virtual Memory: Max Size", f"{(mem.get('MemTotal', 0) + mem.get('SwapTotal', 0)) // 1024:,} MB"),
        ("Virtual Memory: Available", f"{(mem.get('MemAvailable', 0) + mem.get('SwapFree', 0)) // 1024:,} MB"),
    ]
    cards = adapters()
    rows.append(("Network Card(s)", f"{len(cards)} NIC(s) Installed."))
    for n, card in enumerate(cards, 1):
        rows.append(("", f"[{n:02}]: {card['name']}"))
        for addr in card["v4"]:
            rows.append(("", f"      IP address: {addr['local']}"))
    out()
    for label, value in rows:
        out(f"{label + ':' if label else '':<27}{value}")
    return 0


def tree(argv: list[str], env: WinEnv) -> int:
    files = any(a.lower() == "/f" for a in argv)
    ascii_only = any(a.lower() == "/a" for a in argv)
    paths = [a for a in argv if not a.startswith("/")]
    root = env.win_to_unix(paths[0]) if paths else os.getcwd()
    if not root or not os.path.isdir(root):
        out("Invalid path - " + (paths[0] if paths else ""))
        return 1
    tee, last, bar = ("+---", "\\---", "|   ") if ascii_only else ("├───", "└───", "│   ")
    out("Folder PATH listing")
    out("C:." if not paths else paths[0].upper())

    def walk(folder: str, indent: str) -> None:
        try:
            entries = sorted(os.scandir(folder), key=lambda e: e.name.lower())
        except OSError:
            return
        dirs = [e for e in entries if e.is_dir(follow_symlinks=False)]
        names = [e.name for e in entries if not e.is_dir(follow_symlinks=False)]
        lead = indent + (bar if dirs else "    ")
        if files and names:
            for name in names:
                out(lead + name)
            out(lead.rstrip())
        for n, sub in enumerate(dirs):
            final = n == len(dirs) - 1
            out(indent + (last if final else tee) + sub.name)
            walk(sub.path, indent + ("    " if final else bar))

    walk(root, "")
    return 0


# ----------------------------------------------- native tools, renamed ----

def native(tool: str):
    def run(argv: list[str], env: WinEnv) -> int:
        program = shutil.which(tool)
        if not program:
            answer(f"'{tool}' is not installed on this computer.")
            return 9009
        cwd = os.getcwd()
        args = [env.translate_arg(a, cwd) for a in argv]
        return subprocess.run([program, *args], check=False).returncode
    return run


def powershell(argv: list[str], env: WinEnv) -> int:
    """`powershell ...` typed in cmd: the script goes to TUR's PowerShell."""
    import turwinshell

    script, n = None, 0
    while n < len(argv):
        low = argv[n].lower()
        if low in ("-command", "-c"):
            script = " ".join(argv[n + 1:])
            break
        if low in ("-encodedcommand", "-enc", "-ec", "-e") and n + 1 < len(argv):
            script = base64.b64decode(argv[n + 1]).decode("utf-16-le", errors="replace")
            break
        if low in ("-file", "-f") and n + 1 < len(argv):
            target = env.win_to_unix(argv[n + 1], os.getcwd()) or argv[n + 1]
            rest = " ".join(turwinshell.ps_arg(a) for a in argv[n + 2:])
            script = f"& '{target}' {rest}"
            break
        if low in ("-executionpolicy", "-ep", "-windowstyle", "-w", "-workingdirectory", "-wd"):
            n += 1
        elif not low.startswith("-"):
            script = " ".join(argv[n:])
            break
        n += 1
    if script is None:
        answer("'powershell' without a command starts an interactive shell; start TUR's terminal in PowerShell "
               "mode for that.")
        return 0
    return turwinshell.run_ps(script, env)


# ----------------------------------------------------------- answers ----

def catalogue_match(wanted: str) -> tuple[str, str] | None:
    """A catalogue entry for a winget name or identifier."""
    roots = [os.environ.get("TUR_CATALOGUE", ""), "/usr/share/tur/catalogue",
             str(HERE.parent.parent.parent.parent / "catalogue")]
    tokens = [t for t in re.split(r"[^a-z0-9]+", wanted.lower()) if t]
    best = None
    for root in roots:
        if not root or not os.path.isdir(root):
            continue
        for path in sorted(pathlib.Path(root).glob("*.toml")):
            text = path.read_text(errors="replace")
            ident = re.search(r'^id\s*=\s*"([^"]+)"', text, re.M)
            name = re.search(r'^name\s*=\s*"([^"]+)"', text, re.M)
            if not ident:
                continue
            hay = set(re.split(r"[^a-z0-9]+", (ident.group(1) + " " + (name.group(1) if name else "")).lower()))
            score = sum(1 for t in tokens if t in hay)
            if score and (best is None or score > best[0]):
                best = (score, ident.group(1), name.group(1) if name else ident.group(1))
        if best:
            break
    return (best[1], best[2]) if best else None


def winget(argv: list[str], env: WinEnv) -> int:
    verb = next((a.lower() for a in argv if not a.startswith("-")), "")
    values = [a for n, a in enumerate(argv) if not a.startswith("-") and a.lower() != verb
              and not (n and argv[n - 1].lower() in ("--source", "-s", "--scope", "--version", "-v"))]
    wanted = values[0] if values else ""
    intro = "'winget' is Windows' package manager. On TUR, applications are installed by TUR's application centre"
    if verb in ("install", "add", "upgrade", "update", "uninstall", "remove", "show", "search", "find") and wanted:
        hit = catalogue_match(wanted)
        action = {"install": "plan", "add": "plan", "upgrade": "update", "update": "update",
                  "uninstall": "remove", "remove": "remove"}.get(verb, "plan")
        if hit:
            answer(f"{intro}. '{wanted}' is '{hit[1]}' in TUR's catalogue ({hit[0]}).")
            sys.stderr.write(f"  TUR's equivalent: tur-apps {action} {hit[0]}\n")
            program = shutil.which("tur-apps")
            if program and action == "plan":
                return subprocess.run([program, "plan", hit[0]], check=False).returncode
            sys.stderr.write("  Nothing was installed or removed: the application centre shows the plan and asks "
                             "first.\n")
            return 0
        answer(f"{intro}. '{wanted}' is not in TUR's catalogue; search the application centre, or ask P@x.")
        return 0
    answer(f"{intro} (tur-apps list, tur-apps plan <application>). Nothing was changed.")
    return 0


def repair(name: str, what: str):
    def run(argv: list[str], env: WinEnv) -> int:
        answer(f"'{name}' {what} TUR's system files are a read-only image that is verified at every start and "
               "replaced as a whole by updates, so there is nothing for it to repair. To check this computer: "
               "tur-diag collect (TUR Diagnostics), or roll back to the previous image from the boot menu. "
               "Nothing was changed.")
        return 0
    return run


def shutdown(argv: list[str], env: WinEnv) -> int:
    sw = switches(argv)
    if not argv or "/?" in sw or "-?" in sw:
        out("Usage: shutdown [/s | /r | /h | /l | /a] [/f] [/t seconds]")
        out()
        for flag, text in (("/s", "Turn the computer off."), ("/r", "Restart the computer."),
                           ("/h", "Hibernate."), ("/l", "Log off."), ("/a", "Cancel a pending shutdown."),
                           ("/f", "Close running applications without asking."),
                           ("/t", "Seconds to wait before turning off.")):
            out(f"    {flag}         {text}")
        answer("on TUR, 'shutdown' explains itself and does not turn the computer off: use the power menu.")
        return 0
    meaning = "restarts" if "/r" in sw or "-r" in sw else "logs off" if "/l" in sw else "turns off"
    answer(f"'shutdown {' '.join(argv)}' {meaning} a Windows PC. On TUR, use the power menu; a pasted command "
           "does not turn this computer off. Nothing was done.")
    return 0


def refused(argv: list[str], env: WinEnv, name: str = "") -> int:
    import rules

    text = rules.REFUSE.get(name) or f"'{name}' is not run on TUR."
    sys.stderr.write(f"tur-winshell: refused: {text}\n")
    return 5


SHIMS = {
    "ipconfig": ipconfig, "getmac": getmac, "route": route, "tracert": tracert, "nslookup": nslookup,
    "tasklist": tasklist, "taskkill": taskkill, "systeminfo": systeminfo, "tree": tree,
    "tar": native("tar"), "curl": native("curl"),
    "powershell": powershell, "pwsh": powershell,
    "winget": winget, "shutdown": shutdown,
    "sfc": repair("sfc", "checks Windows' system files and replaces damaged ones."),
    "chkdsk": repair("chkdsk", "checks a Windows volume's file system and repairs it."),
    "dism": repair("dism", "services and repairs the Windows system image."),
}
for _name in ("format", "diskpart", "bcdedit", "bcdboot", "bootrec", "bootsect"):
    SHIMS[_name] = (lambda argv, env, _n=_name: refused(argv, env, _n))
