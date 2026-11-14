# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Re-hosting: lay a detected payload under the Linux build of the same
runtime and say how to start it.

rehost(finding, out, cache) returns a plan:

  ok        whether a launch command could be built
  argv/cwd/env   the launch command
  kind      gui | cli
  steps     what was done, in order, for the evidence
  blockers  what stands in the way, in words
  runtime   the Linux runtime used and where it came from

Nothing here runs the application; harness/run.py does that, in CI.
"""

import json
import os
import re
import shutil
import subprocess
import zipfile

from . import binfmt, fetch, natives
from .detect import Payload, electron_payload, resolve_nw_version

ELECTRON_RUNTIME_FILES = re.compile(
    r"(\.exe|\.dll|\.pak|\.bin|\.dat|\.ico|\.html|\.lib|\.pdb)$|^(license|licenses|version|vk_swiftshader_icd\.json|squirrel.*|uninstall.*|chrome-sandbox|chrome_crashpad_handler|.*\.so(\.\d+)*)$",
    re.I,
)
ELECTRON_RUNTIME_DIRS = {"locales", "resources", "swiftshader", "$PLUGINSDIR", "$TEMP", "$R0"}


def plan_base(finding):
    return {"ok": False, "runtime_family": finding["runtime"], "argv": [], "cwd": None, "env": {}, "kind": "gui", "steps": [], "blockers": [], "runtime": {}, "natives": []}


def safe_name(text, fallback="app"):
    name = re.sub(r"[^A-Za-z0-9._-]+", "-", (text or "").strip()).strip("-.")
    return name or fallback


def extract(archive, dest):
    os.makedirs(dest, exist_ok=True)
    done = subprocess.run(["bsdtar", "-xf", archive, "-C", dest], capture_output=True, text=True)
    if done.returncode != 0:
        raise RuntimeError(f"bsdtar could not extract {os.path.basename(archive)}: {done.stderr[-300:]}")


def copy_tree(source, dest):
    if os.path.isdir(source) and not os.path.islink(source):
        shutil.copytree(source, dest, symlinks=True, dirs_exist_ok=True, ignore_dangling_symlinks=True)
    else:
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        shutil.copy2(source, dest, follow_symlinks=False)


# ------------------------------------------------------------------ Electron


def install_shim(payload, plan):
    """Make the experiment's shim the payload's entry point."""
    package = payload.read_json("package.json")
    if not isinstance(package, dict):
        plan["blockers"].append("the payload has no readable package.json: the shim cannot be installed")
        return False
    main = package.get("main") or "index.js"
    esm = package.get("type") == "module" and not main.endswith(".cjs") or main.endswith(".mjs")
    here = os.path.join(os.path.dirname(os.path.abspath(__file__)), "shim", "tur-shim.cjs")
    with open(here, encoding="utf-8") as handle:
        shim = handle.read().replace("__TUR_MAIN__", main.replace("\\", "/").replace("'", "\\'")).replace("__TUR_ESM__", "true" if esm else "false")
    package["main"] = "tur-shim.cjs"
    if payload.asar:
        payload.asar.put_packed("tur-shim.cjs", shim.encode("utf-8"))
        payload.asar.put_packed("package.json", json.dumps(package).encode("utf-8"))
        payload.asar.save()
    else:
        with open(os.path.join(payload.directory, "tur-shim.cjs"), "w", encoding="utf-8") as handle:
            handle.write(shim)
        with open(os.path.join(payload.directory, "package.json"), "w", encoding="utf-8") as handle:
            json.dump(package, handle)
    plan["steps"].append(f"shim installed as the entry point (the real one is {main}{', an ES module' if esm else ''})")
    return True


def rehost_electron(finding, out, cache, log, spoof=None, resolve_natives=True):
    plan = plan_base(finding)
    version = (finding.get("version") or "").split("+")[0]
    if not version:
        plan["blockers"].append("the Electron version could not be read from the package")
        return plan
    archive, record = fetch.electron_linux(version, cache)
    plan["runtime"] = {"name": "Electron", "version": version, "source": record["url"], "sha256": record["sha256"], "publisher_sha256_matches": record.get("publisher_sha256_matches")}
    extract(archive, out)
    program = safe_name(os.path.splitext(finding["payload"].get("program") or "")[0] or finding["payload"].get("name"), "app").replace(" ", "-")
    if program.lower() == "electron":
        program = "app"
    os.replace(os.path.join(out, "electron"), os.path.join(out, program))
    os.chmod(os.path.join(out, program), 0o755)
    default_app = os.path.join(out, "resources", "default_app.asar")
    if os.path.exists(default_app):
        os.unlink(default_app)
    plan["steps"].append(f"Electron {version} for Linux unpacked; program renamed to {program}")

    resources = finding["payload"]["resources"]
    copied = []
    for name in sorted(os.listdir(resources)):
        if name.endswith(".lproj") or name.endswith(".icns") or name in ("default_app.asar", "electron.asar"):
            continue
        copy_tree(os.path.join(resources, name), os.path.join(out, "resources", name))
        copied.append(name)
    plan["steps"].append(f"payload copied into resources/: {', '.join(copied[:12])}{' ...' if len(copied) > 12 else ''}")
    if finding["platform"] in ("windows", "linux"):
        extra = []
        for name in sorted(os.listdir(finding["app_dir"])):
            source = os.path.join(finding["app_dir"], name)
            if name in ELECTRON_RUNTIME_DIRS or (os.path.isfile(source) and ELECTRON_RUNTIME_FILES.search(name)):
                continue
            if os.path.exists(os.path.join(out, name)):
                continue
            copy_tree(source, os.path.join(out, name))
            extra.append(name)
        if extra:
            plan["steps"].append(f"files beside the program copied: {', '.join(extra[:12])}")

    source_fuses = finding["payload"].get("fuses") or {}
    wanted = {name: state for name, state in source_fuses.items() if state in "01"}
    wanted["EnableEmbeddedAsarIntegrityValidation"] = "0"
    payload = electron_payload(os.path.join(out, "resources"))
    if payload is None:
        plan["blockers"].append("the payload did not survive the copy")
        return plan
    if payload.kind == "directory":
        wanted["OnlyLoadAppFromAsar"] = "0"
    changed = binfmt.write_fuses(os.path.join(out, program), wanted)
    plan["fuses"] = {"package": source_fuses, "set_on_linux_binary": changed}
    if changed:
        plan["steps"].append("fuses copied from the package's binary: " + ", ".join(f"{k} {v}" for k, v in changed.items()))

    if resolve_natives:
        from .detect import native_payload

        holder = native_payload(payload, os.path.join(out, "resources"))
        found = holder.natives()
        resolver = natives.Resolver(holder, version, cache, log)
        plan["electron_abi"] = resolver.abi
        plan["natives"] = resolver.resolve_all(found)
        counts = natives.summarise(plan["natives"])
        if plan["natives"]:
            plan["steps"].append("native modules: " + ", ".join(f"{count} {status}" for status, count in sorted(counts.items())))
        for result in plan["natives"]:
            if result["status"] in ("needs-build", "dead-end"):
                plan["blockers"].append(f"native module {result['package']}@{result['version']}: {result['status']}")
    foreign_helpers = [item["path"] for item in finding.get("sidecars", []) if item["format"] != "elf"]
    if foreign_helpers:
        plan["foreign_helpers"] = foreign_helpers[:40]

    env = {}
    if spoof:
        payload = electron_payload(os.path.join(out, "resources"))
        if not install_shim(payload, plan):
            return plan
        env["TUR_REHOST_PLATFORM"] = spoof
    plan.update({"ok": True, "argv": [os.path.join(out, program)], "cwd": out, "env": env, "kind": "gui", "chromium": True})
    return plan


def rehost_nwjs(finding, out, cache, log):
    plan = plan_base(finding)
    resolve_nw_version(finding, fetch.nwjs_versions(cache))
    version = finding.get("version")
    if not version:
        plan["blockers"].append("the NW.js version could not be worked out from the package")
        return plan
    layout, where = finding["payload"].get("layout"), finding["payload"].get("where")
    if not layout:
        plan["blockers"].append("the payload (package.nw) was not found")
        return plan
    archive, record = fetch.nwjs_linux(version, cache, sdk=bool(finding["payload"].get("sdk")))
    plan["runtime"] = {"name": "NW.js", "version": version, "source": record["url"], "sha256": record["sha256"]}
    holder = out + ".runtime"
    extract(archive, holder)
    inner = os.path.join(holder, os.listdir(holder)[0])
    os.replace(inner, out)
    shutil.rmtree(holder, ignore_errors=True)
    target = os.path.join(out, "package.nw")
    if layout == "zip":
        with zipfile.ZipFile(where) as bundle:
            bundle.extractall(target)
    elif layout == "directory":
        copy_tree(where, target)
    else:  # the payload's files sit beside the Windows program
        os.makedirs(target, exist_ok=True)
        for name in sorted(os.listdir(where)):
            source = os.path.join(where, name)
            if name in ELECTRON_RUNTIME_DIRS or name.lower() in ("pnacl", "lib") or (os.path.isfile(source) and ELECTRON_RUNTIME_FILES.search(name) and name != "package.json"):
                continue
            copy_tree(source, os.path.join(target, name))
    plan["steps"].append(f"NW.js {version} for Linux unpacked; payload ({layout}) placed in package.nw/")
    foreign = [item for item in Payload(directory=target).natives() if item["format"] != "elf"]
    for item in foreign:
        plan["blockers"].append(f"native module {item['package']}@{item['version']} ({item['path']}): built for another platform; NW.js modules are built per NW.js version, not resolved here")
    plan.update({"ok": True, "argv": [os.path.join(out, "nw")], "cwd": out, "kind": "gui", "chromium": True})
    return plan


# --------------------------------------------------------------------- games


def rehost_godot(finding, out, cache, log):
    plan = plan_base(finding)
    pack = finding["payload"].get("pack") or {}
    if finding["verdict"] == "not-rehostable":
        plan["blockers"].append("the pack is encrypted")
        return plan
    major, minor, patch = (int(part) for part in finding["version"].split("."))
    mono = "godot-dotnet" in finding["reasons"]
    if mono:
        plan["blockers"].append("a C# game: the .NET engine build and Linux assemblies are needed; not attempted")
        return plan
    template, record = fetch.godot_template(major, minor, patch, cache)
    plan["runtime"] = {"name": "Godot export template (release)", "version": finding["version"], "source": record["url"], "member": record.get("member"), "sha256": record["sha256"], "bytes_fetched": record.get("compressed"), "archive_bytes": record.get("of_archive")}
    os.makedirs(out, exist_ok=True)
    name = safe_name(os.path.splitext(finding["payload"].get("program") or "game")[0], "game")
    program = os.path.join(out, name + ".x86_64")
    shutil.copy2(template, program)
    os.chmod(program, 0o755)
    result = binfmt.pck_extract(finding["payload"]["carrier"], os.path.join(out, name + ".pck"), (pack["start"], pack["size"], pack["how"]))
    plan["steps"].append(f"Godot {finding['version']} Linux template placed as {name}.x86_64; pack written beside it ({pack['how']}; offsets: {result['fixed']})")
    for extension in finding["payload"].get("native_extensions", []):
        plan["blockers"].append(f"native extension {extension}: its Linux library is not in the package")
    plan.update({"ok": True, "argv": [program], "cwd": out, "kind": "gui"})
    return plan


def rehost_love(finding, out, cache, log):
    plan = plan_base(finding)
    version = finding.get("version") or ""
    wanted = finding["payload"].get("conf_version") or version
    major = wanted.split(".")[0]
    if major != "11":
        plan["blockers"].append(f"the game is written for LÖVE {wanted}; only 11.x has an official Linux build to fetch (11.5 AppImage)")
        return plan
    runtime_version = "11.5"
    appimage, record = fetch.love_linux(runtime_version, cache)
    plan["runtime"] = {"name": "LÖVE", "version": runtime_version, "wanted": wanted, "source": record["url"], "sha256": record["sha256"]}
    os.makedirs(out, exist_ok=True)
    root = os.path.join(out, "love")
    if not os.path.isdir(root):
        from .unpack import Unpacker

        if not Unpacker(log=log).appimage(appimage, root):
            plan["blockers"].append("LÖVE's AppImage could not be opened")
            return plan
    carrier = os.path.join(finding["app_dir"], finding["payload"]["carrier"]) if finding["platform"] != "macos" else os.path.join(finding["app_dir"], "Contents", "Resources", finding["payload"]["carrier"])
    game = os.path.join(out, "game.love")
    with open(carrier, "rb") as source, open(game, "wb") as dest:
        source.seek(finding["payload"]["zip_start"])
        dest.write(source.read(finding["payload"]["zip_end"] - finding["payload"]["zip_start"]))
    plan["steps"].append(f"LÖVE {runtime_version} unpacked from its AppImage; the game's archive cut out of {finding['payload']['carrier']} as game.love")
    plan.update({"ok": True, "argv": [os.path.join(root, "AppRun"), game], "cwd": out, "kind": "gui"})
    return plan


def rehost_renpy(finding, out, cache, log):
    plan = plan_base(finding)
    version = finding.get("version")
    if not version:
        plan["blockers"].append("the Ren'Py version could not be read")
        return plan
    archive, record = fetch.renpy_sdk(version, cache)
    plan["runtime"] = {"name": "Ren'Py SDK", "version": version, "source": record["url"], "sha256": record["sha256"]}
    sdk = os.path.join(cache, "runtimes", f"renpy-{'.'.join(version.split('.')[:3])}-sdk")
    if not os.path.isdir(sdk):
        holder = sdk + ".tmp"
        extract(archive, holder)
        os.replace(os.path.join(holder, os.listdir(holder)[0]), sdk)
        shutil.rmtree(holder, ignore_errors=True)
    os.makedirs(out, exist_ok=True)
    base = os.path.join(out, "game-base")
    os.makedirs(base, exist_ok=True)
    copy_tree(os.path.join(finding["app_dir"], "game"), os.path.join(base, "game"))
    plan["steps"].append(f"Ren'Py {version} SDK unpacked; the package's game/ directory is started with the SDK's engine")
    plan.update({"ok": True, "argv": [os.path.join(sdk, "renpy.sh"), base], "cwd": out, "kind": "gui"})
    return plan


# ----------------------------------------------------------------- .NET, JVM


def dotnet_shared(root="/usr/lib64/dotnet"):
    """{framework name: {major: (version, directory)}} of the installed runtime."""
    out = {}
    shared = os.path.join(root, "shared")
    if not os.path.isdir(shared):
        return out
    for framework in os.listdir(shared):
        for version in os.listdir(os.path.join(shared, framework)):
            major = version.split(".")[0]
            current = out.setdefault(framework, {}).get(major)
            if current is None or [int(x) for x in re.findall(r"\d+", version)] > [int(x) for x in re.findall(r"\d+", current[0])]:
                out[framework][major] = (version, os.path.join(shared, framework, version))
    return out


def nuget_linux_assets(deps, app, cache, log, plan):
    """Windows-specific files a win-x64 publish copied from NuGet packages:
    fetch the same package version and take its Linux assets instead."""
    libraries = {}
    for target in (deps.get("targets") or {}).values():
        for key, body in target.items():
            if not isinstance(body, dict) or "/" not in key:
                continue
            assets = list((body.get("runtime") or {})) + list((body.get("native") or {})) + list((body.get("runtimeTargets") or {}))
            if any("runtimes/win" in asset for asset in assets) or re.search(r"(?i)win32|win-x64|windows", key.split("/")[0]):
                libraries[key] = assets
    swapped = []
    for key in sorted(libraries):
        package, version = key.split("/", 1)
        tried = [package]
        for old, new in (("Win32", "Linux"), ("win-x64", "linux-x64"), ("Windows", "Linux")):
            if old in package:
                tried.append(package.replace(old, new))
        for candidate in tried[::-1]:
            nupkg = fetch.nuget_package(candidate, version, cache)
            if not nupkg:
                continue
            taken = 0
            try:
                with zipfile.ZipFile(nupkg) as archive:
                    names = archive.namelist()
                    for rid in ("linux-x64", "linux", "unix"):
                        for name in names:
                            if name.startswith(f"runtimes/{rid}/") and not name.endswith("/") and ("/native/" in name or "/lib/" in name):
                                target = os.path.join(app, os.path.basename(name))
                                if "/lib/" in name and not os.path.exists(target):
                                    continue  # a managed asset the application does not carry
                                if "/lib/" in name and name.count("/") >= 4:
                                    tfm = name.split("/")[3]
                                    best = sorted({n.split("/")[3] for n in names if n.startswith(f"runtimes/{rid}/lib/") and n.count("/") >= 4})
                                    if tfm != best[-1]:
                                        continue
                                with open(target, "wb") as handle:
                                    handle.write(archive.read(name))
                                taken += 1
                        if taken:
                            break
            except (zipfile.BadZipFile, OSError):
                continue
            if taken:
                swapped.append(f"{candidate} {version}: {taken} Linux file(s)")
                break
        else:
            plan["blockers"].append(f"NuGet package {key} has Windows-specific files and no Linux assets were found")
    if swapped:
        plan["steps"].append("Linux assets from NuGet: " + "; ".join(swapped[:12]))
    plan["nuget"] = {"windows_specific_packages": sorted(libraries), "swapped": swapped}


def rehost_dotnet(finding, out, cache, log):
    plan = plan_base(finding)
    if finding["verdict"] == "not-rehostable":
        plan["blockers"].append("Windows desktop framework (WPF or Windows Forms): goes to Wine")
        return plan
    payload = finding["payload"]
    app = os.path.join(out, "app")
    if payload.get("single_file"):
        carrier = os.path.join(finding["app_dir"], payload["single_file"])
        bundle = binfmt.dotnet_bundle_read(carrier)
        binfmt.dotnet_bundle_extract(carrier, bundle, app)
        for name in os.listdir(finding["app_dir"]):
            source = os.path.join(finding["app_dir"], name)
            if name != payload["single_file"] and not os.path.exists(os.path.join(app, name)):
                copy_tree(source, os.path.join(app, name))
        plan["steps"].append(f"single-file bundle unpacked ({len(bundle['files'])} files)")
        configs = [name for name in os.listdir(app) if name.endswith(".runtimeconfig.json")]
        if not configs:
            plan["blockers"].append("no runtimeconfig.json in the bundle")
            return plan
        payload = dict(payload, runtimeconfig=configs[0], assembly=configs[0][: -len(".runtimeconfig.json")] + ".dll", deps=configs[0][: -len(".runtimeconfig.json")] + ".deps.json")
    else:
        copy_tree(finding["app_dir"], app)
    if not payload.get("assembly") or not os.path.exists(os.path.join(app, payload["assembly"])):
        plan["blockers"].append("the application's main assembly was not found")
        return plan
    with open(os.path.join(app, payload["runtimeconfig"]), "rb") as handle:
        config = json.loads(handle.read().decode("utf-8-sig"))
    options = config.setdefault("runtimeOptions", {})
    included = options.pop("includedFrameworks", None)
    frameworks = options.get("frameworks") or ([options["framework"]] if options.get("framework") else [])
    if included:
        frameworks = [dict(item) for item in included]
        if any(name.lower().startswith("microsoft.aspnetcore.") for name in os.listdir(app)) and not any(item["name"] == "Microsoft.AspNetCore.App" for item in frameworks):
            core = next(item for item in frameworks if item["name"] == "Microsoft.NETCore.App")
            frameworks.append({"name": "Microsoft.AspNetCore.App", "version": core["version"]})
    installed = dotnet_shared()
    plan["runtime"] = {"name": ".NET (Fedora's packages)", "wanted": frameworks, "installed": {name: {major: value[0] for major, value in majors.items()} for name, majors in installed.items()}}
    removed = 0
    roll_forward = False
    for item in frameworks:
        majors = installed.get(item["name"], {})
        major = str(item.get("version", "0")).split(".")[0]
        if not majors:
            plan["blockers"].append(f"{item['name']} is not installed")
            return plan
        if major not in majors:
            roll_forward = True
        chosen = majors.get(major) or majors[sorted(majors, key=int)[-1]]
        if included:
            # The package carries the Windows build of the framework: use the
            # installed Linux one, and drop the carried files of the same name.
            for name in os.listdir(chosen[1]):
                for candidate in (name, name[3:-3] + ".dll" if name.startswith("lib") and name.endswith(".so") else None):
                    target = os.path.join(app, candidate) if candidate else None
                    if target and os.path.isfile(target):
                        os.unlink(target)
                        removed += 1
    if included:
        for name in ("coreclr.dll", "clrjit.dll", "hostfxr.dll", "hostpolicy.dll", "clrgc.dll", "clrgcexp.dll", "mscordaccore.dll", "mscordbi.dll", "mscorrc.dll", "msquic.dll", "createdump.exe", "Microsoft.DiaSymReader.Native.amd64.dll"):
            target = os.path.join(app, name)
            if os.path.isfile(target):
                os.unlink(target)
                removed += 1
        for name in os.listdir(app):
            if re.match(r"(?i)^(mscordaccore_|api-ms-win-|ucrtbase|vcruntime|msvcp)", name):
                os.unlink(os.path.join(app, name))
                removed += 1
        options["frameworks"] = frameworks
        options.pop("framework", None)
        with open(os.path.join(app, payload["runtimeconfig"]), "w", encoding="utf-8") as handle:
            json.dump(config, handle, indent=2)
        plan["steps"].append(f"self-contained package made framework-dependent: {removed} carried Windows framework files removed, runtimeconfig.json rewritten")
    deps_path = os.path.join(app, payload.get("deps") or "")
    if payload.get("deps") and os.path.isfile(deps_path):
        try:
            with open(deps_path, "rb") as handle:
                deps = json.loads(handle.read().decode("utf-8-sig"))
            nuget_linux_assets(deps, app, cache, log, plan)
        except ValueError:
            plan["steps"].append("deps.json could not be parsed")
        os.replace(deps_path, deps_path + ".windows")
        plan["steps"].append("the Windows deps.json set aside: the host then takes every assembly in the directory")
    env = {"DOTNET_ROLL_FORWARD": "LatestMajor"} if roll_forward else {}
    if roll_forward:
        plan["steps"].append("the wanted .NET major version is not installed: started with roll-forward to the newest installed")
    plan.update({"ok": True, "argv": ["dotnet", os.path.join(app, payload["assembly"])], "cwd": app, "env": env, "kind": "gui" if payload.get("avalonia") else "cli"})
    return plan


def java_home(needed):
    """An installed JDK at least as new as `needed`: (major, java path)."""
    found = []
    base = "/usr/lib/jvm"
    for name in os.listdir(base) if os.path.isdir(base) else []:
        release = os.path.join(base, name, "release")
        java = os.path.join(base, name, "bin", "java")
        if os.path.isfile(release) and os.path.isfile(java) and not os.path.islink(os.path.join(base, name)):
            match = re.search(r'JAVA_VERSION="(\d+)', open(release, encoding="utf-8", errors="replace").read())
            if match:
                found.append((int(match.group(1)), java))
    found.sort()
    for major, java in found:
        if major >= (needed or 0):
            return major, java, [item[0] for item in found]
    return (found[-1][0], found[-1][1], [item[0] for item in found]) if found else (None, None, [])


def swt_linux(swt, cache, log):
    """The GTK build of the SWT version an application carries, from Maven
    Central (EPL-2.0)."""
    wanted = (swt.get("version_txt") or "").replace("version", "").strip()
    library = swt.get("library") or ""
    match = re.search(r"swt-[a-z0-9]+-(\d{4,5})", library)
    number = match.group(1) if match else None
    versions = fetch.maven_versions("org.eclipse.platform", "org.eclipse.swt.gtk.linux.x86_64")
    tried = 0
    for version in versions[::-1]:
        if tried >= 30:
            break
        jar = fetch.maven_artifact("org.eclipse.platform", "org.eclipse.swt.gtk.linux.x86_64", version, cache)
        tried += 1
        if not jar:
            continue
        try:
            with zipfile.ZipFile(jar) as archive:
                names = archive.namelist()
                text = archive.read("version.txt").decode("utf-8", "replace") if "version.txt" in names else ""
                if (wanted and wanted in text) or (number and any(f"-{number}" in name for name in names)):
                    return jar, version, tried
        except (zipfile.BadZipFile, OSError):
            continue
    return None, None, tried


def rehost_jvm(finding, out, cache, log):
    plan = plan_base(finding)
    payload = finding["payload"]
    os.makedirs(out, exist_ok=True)
    needed = payload.get("needs_java") or 0
    bundled = (payload.get("bundled_runtime") or {}).get("version") or ""
    if re.match(r"\d+", bundled):
        needed = max(needed, int(re.match(r"\d+", bundled).group(0)) if not bundled.startswith("1.") else 8)
    major, java, available = java_home(needed)
    plan["runtime"] = {"name": "OpenJDK (Fedora's packages)", "wanted": bundled or f">= {needed}", "used": major, "installed": available}
    if not java:
        plan["blockers"].append("no Java runtime installed")
        return plan
    if major < needed:
        plan["blockers"].append(f"the application needs Java {needed}; the newest installed is {major}")
    classpath = []
    options = []
    main_class = payload.get("mainclass")
    app = out
    if payload.get("carrier"):
        carrier = os.path.join(finding["app_dir"], payload["carrier"])
        jar = os.path.join(out, safe_name(os.path.splitext(payload["carrier"])[0]) + ".jar")
        with open(carrier, "rb") as source, open(jar, "wb") as dest:
            source.seek(payload["jar_start"])
            shutil.copyfileobj(source, dest)
        classpath = [jar]
        plan["steps"].append(f"the jar cut out of {payload['carrier']} ({payload['wrapper']})")
    else:
        app = os.path.join(out, "app")
        copy_tree(payload["app"], app)

        def translate(value):
            value = value.replace("\\", "/")
            for token in ("$APPDIR", "$ROOTDIR/app", "${APPDIR}"):
                value = value.replace(token, app)
            return value.replace("$ROOTDIR", out).replace("$BINDIR", out)

        classpath = [translate(item) for item in payload.get("classpath") or []]
        if payload.get("wrapper") == "packr":
            classpath = [os.path.join(app, item) for item in payload.get("classpath") or []]
        options = [translate(item) for item in payload.get("options") or [] if not re.search(r"(?i)-splash:|-Xdock|-XstartOnFirstThread|apple\.", item)]
        if payload.get("mainjar"):
            classpath.insert(0, os.path.join(app, payload["mainjar"]))
        if payload.get("mainmodule") and not classpath:
            plan["blockers"].append("the application is inside the bundled runtime image (jlink): extraction with jimage is not implemented")
            return plan
    swt = payload.get("swt")
    if swt and swt.get("port") != "gtk":
        jar, version, tried = swt_linux(swt, cache, log)
        if jar:
            classpath.insert(0, jar)
            plan["steps"].append(f"SWT for GTK {version} from Maven Central put first on the class path ({tried} versions examined)")
            plan["swt"] = {"maven_version": version, "examined": tried}
        else:
            plan["blockers"].append(f"no GTK build of this SWT version found on Maven Central ({tried} versions examined)")
    for name in payload.get("javafx_platform_jars") or []:
        match = re.match(r"(javafx-[a-z]+)-([0-9][0-9a-z.+-]*?)-(win|mac)(-[a-z0-9]+)?\.jar$", name)
        if not match:
            continue
        artifact, version = match.group(1), match.group(2)
        jar = fetch.maven_artifact("org.openjfx", artifact, version, cache, classifier="linux")
        if jar:
            classpath = [jar if os.path.basename(item) == name else item for item in classpath]
            plan["steps"].append(f"{name} replaced by {artifact} {version} for Linux from Maven Central")
        else:
            plan["blockers"].append(f"no Linux build of {name} on Maven Central")
    classpath = list(dict.fromkeys(classpath))
    if not classpath:
        plan["blockers"].append("no class path could be built")
        return plan
    if not main_class:
        facts_jar = classpath[-1] if (swt and len(classpath) > 1) else classpath[0]
        from .detect import jar_facts

        facts = jar_facts(facts_jar)
        main_class = facts["main_class"] if facts else None
    if not main_class:
        plan["blockers"].append("no main class found")
        return plan
    argv = [java, *options, "-cp", ":".join(classpath), main_class]
    plan["steps"].append(f"started with Java {major}: main class {main_class}, {len(classpath)} class path entries")
    plan.update({"ok": True, "argv": argv, "cwd": app, "kind": "gui"})
    return plan


# ----------------------------------------------------------------- PyInstaller


def rehost_pyinstaller(finding, out, cache, log):
    plan = plan_base(finding)
    plan["kind"] = "cli"
    version = finding["version"]
    python = shutil.which(f"python{version}")
    plan["runtime"] = {"name": "CPython (Fedora's packages)", "wanted": version, "found": python}
    if not python:
        plan["blockers"].append(f"python{version} is not installed: the frozen bytecode only loads in that minor version")
        return plan
    os.makedirs(out, exist_ok=True)
    carrier = os.path.join(finding["app_dir"], finding["payload"]["carrier"])
    archive = binfmt.pyinstaller_read(carrier)
    scripts = []
    data_dir = os.path.join(out, "data")
    os.makedirs(data_dir, exist_ok=True)
    for entry in archive["entries"]:
        blob = None
        if entry["type"] == "z":
            blob = binfmt.pyinstaller_entry(carrier, entry)
            with open(os.path.join(out, "payload.pyz"), "wb") as handle:
                handle.write(blob)
        elif entry["type"] == "s" and not entry["name"].startswith(("pyi", "_pyi")):
            blob = binfmt.pyinstaller_entry(carrier, entry)
            name = safe_name(entry["name"]) + ".code"
            with open(os.path.join(out, name), "wb") as handle:
                handle.write(blob)
            scripts.append(name)
        elif entry["type"] in ("x", "m") or (entry["type"] == "b" and not entry["name"].lower().endswith((".dll", ".pyd", ".so", ".dylib", ".exe"))):
            target = os.path.normpath(os.path.join(data_dir, entry["name"].replace("\\", "/")))
            if target.startswith(data_dir + os.sep):
                os.makedirs(os.path.dirname(target), exist_ok=True)
                with open(target, "wb") as handle:
                    handle.write(binfmt.pyinstaller_entry(carrier, entry))
    internal = os.path.join(finding["app_dir"], "_internal")
    if os.path.isdir(internal):
        for root, _, names in os.walk(internal):
            for name in names:
                if name.lower().endswith((".dll", ".pyd", ".exe")):
                    continue
                source = os.path.join(root, name)
                copy_tree(source, os.path.join(data_dir, os.path.relpath(source, internal)))
    if not scripts or not os.path.exists(os.path.join(out, "payload.pyz")):
        plan["blockers"].append("no entry script or no PYZ archive in the program")
        return plan
    # Extension modules: the top-level names of what the Windows build froze.
    wanted = set()
    for name in finding["payload"].get("extension_modules", []):
        top = name.replace("\\", "/").split("/")[0].split(".")[0]
        if top and not top.startswith(("_", "python", "vcruntime", "api-ms", "libcrypto", "libssl", "libffi", "select", "unicodedata", "pyexpat", "sqlite3", "winsound", "msvcrt", "ucrtbase", "zlib", "liblzma", "libbz2", "pywintypes", "win32", "pythoncom")):
            wanted.add(top)
    plan["extension_packages"] = sorted(wanted)
    here = os.path.join(os.path.dirname(os.path.abspath(__file__)), "pyi_loader.py")
    plan["steps"].append(f"PYZ archive and entry script {scripts[-1]} taken out of the program; started under {python} with the experiment's loader")
    plan.update({"ok": True, "argv": [python, "-I", here, out, scripts[-1]], "cwd": out, "kind": "cli", "pip": sorted(wanted), "python": python})
    return plan


REHOSTERS = {
    "electron": rehost_electron,
    "nwjs": rehost_nwjs,
    "godot": rehost_godot,
    "love": rehost_love,
    "renpy": rehost_renpy,
    "dotnet": rehost_dotnet,
    "jvm": rehost_jvm,
    "pyinstaller": rehost_pyinstaller,
}


def rehost(finding, out, cache, log=print, **options):
    function = REHOSTERS.get(finding["runtime"])
    if function is None:
        plan = plan_base(finding)
        plan["blockers"].append(f"{finding['runtime']}: not a re-hostable runtime")
        return plan
    try:
        return function(finding, out, cache, log, **options)
    except (fetch.FetchError, OSError, RuntimeError, ValueError, KeyError, zipfile.BadZipFile) as problem:
        plan = plan_base(finding)
        plan["blockers"].append(f"re-hosting failed: {type(problem).__name__}: {problem}")
        return plan
