# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The corpus: everyday Windows commands as people type them.

Three groups:

    cmd     cmd.exe built-ins and batch constructs, typed at a cmd prompt
    tools   the classic command-line tools, typed at a cmd prompt
    ps      PowerShell cmdlets and idioms, typed at a PowerShell prompt

Every command runs with `C:\\turfix` (fixture.py) as its current directory.
`rw=True` marks a state-changing command: it changes only the fixture tree,
the registry key HKCU\\Software\\TurWincliTest, the user variable TURTEST or
the scheduled task TurWincliTest, on a throwaway machine or a throwaway
Windows environment. Commands that would change anything else are not in
the corpus (see safety_cases.py for those).

HYPOTHESIS (written before the first measurement): with the right
combination of existing parts, a large majority of everyday Windows commands
run on TUR with the same meaning as on Windows, and the rest can be answered
honestly instead of "command not found".

    Supported   the best architecture passes at least 80 % of the corpus,
                and at least 60 % of each group, with PASS meaning the same
                exit class and the same shape as real Windows; and every
                command it does not pass gets an explanation.
    Killed      the best architecture passes less than 50 % overall, or no
                candidate passes half of some group.
    Otherwise   partly supported; the report says where.

`truth=False` marks the few repair commands that are too slow or too
invasive to run even on the judge; they are scored only for whether TUR
answers them.
"""

from __future__ import annotations

import base64
import re

import fixture

CORPUS: list[dict] = []
_ids: set[str] = set()

DATE = r"\d{1,4}[-/.]\d{1,2}[-/.]\d{2,4}"
DT = [DATE + r"\s+\d{1,2}:\d{2}(:\d{2})?(\s?[AP]M)?", "<DT>"]
DIR_DROP = [r"(?i)^\s*Volume "]
DIR_MASK = [DT, [r"[\d,.\u00a0]+ bytes free", "N bytes free"]]
IPV4 = r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}"
MAC = r"(?:[0-9A-Fa-f]{2}-){5}[0-9A-Fa-f]{2}"
HOME = r"(?i)^C:\\Users\\[^\\%]+"


def text(*flags, drop=(), mask=(), empty_ok=False):
    return {"k": "text", "flags": list(flags), "drop": list(drop), "mask": [list(m) for m in mask],
            "empty_ok": empty_ok}


def probes(*rx):
    for pattern in rx:
        re.compile(pattern)
    return {"k": "probes", "rx": list(rx)}


def columns():
    return {"k": "columns"}


def keys():
    return {"k": "keys"}


def js(vals=True):
    return {"k": "json", "vals": vals}


def _add(group, shell, ident, cmd, *cmp, rw=False, pre=None, files="hash", timeout=90,
         helptext=False, truth=True, exact=False):
    ident = f"{group}.{ident}"
    assert ident not in _ids, ident
    _ids.add(ident)
    CORPUS.append({
        "id": ident, "group": group, "shell": shell, "cmd": cmd, "cmp": list(cmp), "rw": rw,
        "pre": pre, "files": files, "timeout": timeout, "help": helptext, "truth": truth,
        "exact": exact,
    })


def c(ident, cmd, *cmp, **kw):
    _add("cmd", "cmd", ident, cmd, *cmp, **kw)


def t(ident, cmd, *cmp, **kw):
    _add("tools", "cmd", ident, cmd, *cmp, **kw)


def p(ident, cmd, *cmp, **kw):
    _add("ps", "ps", ident, cmd, *cmp, **kw)


# ---------------------------------------------------------------- cmd ----

c("dir", "dir", text("ws", drop=DIR_DROP, mask=DIR_MASK))
c("dir-b", "dir /b", text())
c("dir-bs", "dir /b /s", text())
c("dir-ad", "dir /a:d /b", text())
c("dir-wild", "dir *.txt", text("ws", drop=DIR_DROP, mask=DIR_MASK))
c("dir-on", "dir /o:-n /b docs", text())
c("dir-s-wild", "dir /s /b *.txt", text())
c("dir-missing", "dir nosuchfile.xyz")
c("dir-abs", r"dir /b C:\turfix\src", text())
c("dir-count", 'dir /b | find /c /v ""', text("ws"))
c("cd", "cd", text())
c("cd-chain", "cd docs && cd", text())
c("cd-d", r"cd /d C:\turfix\docs\sub & dir /b", text())
c("cd-up", r"cd docs\sub & cd .. & cd", text())
c("pushd", "pushd docs & cd & popd & cd", text())
c("echo", "echo Hello, World!", text())
c("echo-quote", 'echo "quoted text"', text())
c("echo-escape", "echo a ^& b ^| c", text())
c("echo-dot", "echo. & echo after", text())
c("echo-chain", "echo a & echo b", text("ws"))
c("var-username", "echo %USERNAME%", probes(r"^[^%\s]+$"))
c("var-cd", "echo %CD%", text())
c("var-os", "echo %OS%", text())
c("var-userprofile", "echo %USERPROFILE%", probes(HOME + r"$"))
c("var-appdata", "echo %APPDATA%", probes(HOME + r"\\AppData\\Roaming$"))
c("var-temp", "echo %TEMP%", probes(r"(?i)^C:\\.*\\Temp$"))
c("var-computername", "echo %COMPUTERNAME%", probes(r"^[A-Za-z0-9_-]+$"))
c("var-path", "echo %PATH%", probes(r"(?i)C:\\Windows\\system32"))
c("var-errorlevel", "echo %ERRORLEVEL%", text())
c("var-random", "echo %RANDOM%", probes(r"^\d{1,5}$"))
c("var-time", "echo %TIME%", probes(r"^\s?\d{1,2}:\d{2}:\d{2}[.,]\d{2}$"))
c("var-date", "echo %DATE%", probes(DATE))
c("var-undefined", "echo %NOTDEFINED_TUR%", text())
c("var-substr", "echo %OS:~0,7%", text())
c("var-replace", "echo %OS:_NT=%", text())
c("var-arch", "echo %PROCESSOR_ARCHITECTURE%", text())
c("var-systemroot", "echo %SystemRoot%", text("ci"))
c("var-programfiles", "echo %ProgramFiles%", text("ci"))
c("var-home", "echo %HOMEDRIVE%%HOMEPATH%", probes(HOME + r"$"))
c("var-pathext", "echo %PATHEXT%", probes(r"(?i)\.COM;\.EXE;\.BAT;\.CMD"))
c("var-comspec", "echo %COMSPEC%", probes(r"(?i)^C:\\Windows\\system32\\cmd\.exe$"))
c("set-list", "set", probes(r"(?i)^PATH=", r"(?i)^SystemRoot=", r"(?i)^USERPROFILE=", r"(?i)^COMSPEC=",
                            r"(?i)^PATHEXT=", r"(?i)^TEMP=", r"(?i)^windir="))
c("set-prefix", "set PATH", probes(r"(?i)^PATH=", r"(?i)^PATHEXT="))
c("set-assign", "set FOO=bar& set FOO", text())
c("set-early", "set FOO=bar& echo %FOO%", text())
c("set-quoted", 'set "A=hello world" & set A', text())
c("set-a", "set /a 7*6", text())
c("set-a-expr", 'set /a "X=(10+5)*2"', text())
c("set-delayed", 'cmd /v:on /c "set V=1& echo !V!"', text())
c("if-exist", "if exist notes.txt (echo yes) else (echo no)", text())
c("if-not-exist", "if not exist missing.txt echo missing", text())
c("if-exist-dir", r"if exist docs\ echo isdir", text())
c("if-exist-abs", r"if exist C:\turfix\docs\readme.md echo found", text())
c("if-equal", 'if "abc"=="abc" echo same', text())
c("if-i", 'if /i "ABC"=="abc" echo same', text())
c("if-gtr", "if 5 gtr 3 echo bigger", text())
c("if-defined", "if defined PATH echo defined", text())
c("if-errorlevel", "if errorlevel 1 (echo fail) else (echo ok)", text())
c("for-list", "for %i in (1 2 3) do @echo %i", text())
c("for-files", "for %f in (*.txt) do @echo %f", text("sort"))
c("for-l", "for /l %i in (1,1,5) do @echo %i", text())
c("for-f-csv", 'for /f "tokens=1,2 delims=," %a in (data.csv) do @echo %a-%b', text())
c("for-f-cmd", "for /f \"delims=\" %i in ('dir /b docs') do @echo [%i]", text())
c("for-f-skip", 'for /f "skip=1 tokens=2 delims=," %a in (data.csv) do @echo %a', text())
c("for-r", "for /r %f in (*.c) do @echo %~nxf", text("sort"))
c("for-d", "for /d %d in (*) do @echo %d", text("sort"))
c("for-mod", "for %f in (notes.txt) do @echo %~nf %~xf %~zf", text())
c("for-nested", "for %i in (a b) do @for %j in (1 2) do @echo %i%j", text())
c("for-fullpath", "for %f in (notes.txt) do @echo %~ff", text())
c("type", "type notes.txt", text())
c("type-space", 'type "hello world.txt"', text())
c("type-missing", "type nosuch.txt")
c("pipe-find", 'type notes.txt | find "beta"', text())
c("pipe-count", 'type notes.txt | find /c /v ""', text("ws"))
c("pipe-sort", "type notes.txt | sort", text())
c("pipe-sort-r", "type notes.txt | sort /r", text())
c("pipe-more", "type notes.txt | more", text())
c("pipe-findstr", 'echo hello| findstr /r "^h"', text())
c("redir-out", "echo one > out.txt & type out.txt", text(), rw=True)
c("redir-append", "echo two >> notes.txt & type notes.txt", text(), rw=True)
c("redir-dir", "dir /b > list.txt & type list.txt", text(), rw=True)
c("redir-in", "sort < notes.txt", text())
c("redir-inout", "sort < notes.txt > sorted.txt & type sorted.txt", text(), rw=True)
c("redir-nul", "dir /b notes.txt >nul && echo found", text())
c("redir-2nul", "dir nosuch.xyz 2>nul || echo failed", text())
c("redir-group", "(echo x & echo y) > xy.txt & type xy.txt", text("ws"), rw=True)
c("redir-2to1", "dir nosuch.xyz > err.txt 2>&1 & if exist err.txt echo captured", text(), rw=True,
  files="names")
c("copy", "copy notes.txt copy.txt", probes(r"1 file\(s\) copied"), rw=True)
c("copy-concat", "copy notes.txt + data.csv merged.txt", rw=True)
c("copy-y-dir", "copy /y notes.txt docs\\", rw=True)
c("copy-missing", "copy nosuch.txt x.txt", rw=True)
c("copy-wild", r"copy src\*.c docs", probes(r"2 file\(s\) copied"), rw=True)
c("move", r"move report.log docs\report.log", rw=True)
c("move-dir", "move docs documents", rw=True)
c("ren", "ren notes.txt notes.bak", rw=True)
c("ren-wild", r"ren src\*.c *.cpp", rw=True)
c("del", "del data.csv", rw=True)
c("del-q-wild", r"del /q docs\*.txt", rw=True)
c("del-s", "del /s /q *.txt", rw=True)
c("del-missing", "del nosuch.txt", rw=True)
c("mkdir-deep", r"mkdir newdir\sub\leaf", rw=True)
c("md-space", 'md "My Folder"', rw=True)
c("rmdir", "rmdir empty", rw=True)
c("rd-s", "rd /s /q docs", rw=True)
c("rmdir-nonempty", "rmdir docs", rw=True)
c("mklink", "mklink /d doclink docs & dir /b doclink", text(drop=[r"<<===>>"]), rw=True)
c("ver", "ver", probes(r"Microsoft Windows \[Version \d+\.\d+\.\d+"))
c("vol", "vol")
c("title", "title Test & echo ok", text())
c("date-t", "date /t", probes(DATE))
c("time-t", "time /t", probes(r"^\d{1,2}:\d{2}"))
c("path", "path", probes(r"(?i)^PATH=.*system32"))
c("assoc", "assoc .txt", probes(r"(?i)^\.txt=txtfile"))
c("ftype", "ftype txtfile", probes(r"(?i)^txtfile=.*notepad"))
c("bat-rel", r"scripts\hello.bat", text())
c("bat-call", r"call scripts\hello.bat", text())
c("bat-noext", "cd scripts & hello", text())
c("cmd-args", r'scripts\args.cmd one "two three"', text(), exact=True)
c("cmd-c-exit", "cmd /c exit 3", exact=True)
c("exit-b", "exit /b 7", exact=True)
c("cmd-c-nested", 'cmd /c "echo inner & echo done"', text("ws"))
c("start-wait", "start /b /wait cmd /c echo started", text())
c("help-dir", "dir /?", probes(r"(?i)/B\b", r"(?i)/S\b", r"(?s).{400}"), helptext=True)
c("help-copy", "copy /?", probes(r"(?i)/Y\b", r"(?s).{200}"), helptext=True)
c("rem", "rem this is a comment", text(empty_ok=True))

# -------------------------------------------------------------- tools ----

t("ipconfig", "ipconfig", probes(r"^\s+IPv4 Address[. ]*: " + IPV4, r"^\s+Subnet Mask[. ]*: " + IPV4,
                                 r"^\s+Default Gateway[. ]*:", r"^\S.* adapter .+:$"))
t("ipconfig-all", "ipconfig /all", probes(r"^\s+Host Name[. ]*: \S+", r"^\s+Physical Address[. ]*: " + MAC,
                                          r"^\s+DHCP Enabled[. ]*: (Yes|No)", r"^\s+DNS Servers[. ]*: \S+",
                                          r"^\s+IPv4 Address[. ]*: " + IPV4))
t("ipconfig-findstr", 'ipconfig | findstr /i "IPv4"', probes(r"IPv4 Address[. ]*: " + IPV4))
t("ipconfig-flushdns", "ipconfig /flushdns", probes(r"(?i)flushed"), rw=True)
t("ping", "ping -n 2 127.0.0.1", probes(r"Reply from 127\.0\.0\.1: bytes=32", r"Sent = 2, Received = 2, Lost = 0",
                                        r"Minimum = \d+ms, Maximum = \d+ms, Average = \d+ms"))
t("ping-localhost", "ping -n 1 localhost", probes(r"Reply from (127\.0\.0\.1|::1)", r"Sent = 1, Received = 1"))
t("ping-fail", "ping -n 1 -w 500 192.0.2.1", probes(r"Sent = 1, Received = 0"))
t("ping-badhost", "ping -n 1 nosuchhost.invalid")
t("ping-quiet", "ping -n 2 127.0.0.1 >nul & echo waited", text())
t("tracert", "tracert -d -h 2 127.0.0.1", probes(r"Tracing route to 127\.0\.0\.1", r"^\s+1\s+.*127\.0\.0\.1",
                                                 r"Trace complete"))
t("nslookup", "nslookup example.com", probes(r"^Server:\s+\S", r"^Name:\s+example\.com", r"^Address(es)?:\s+\S"))
t("nslookup-mx", "nslookup -type=mx gmail.com", probes(r"MX preference = \d+, mail exchanger = \S+"))
t("hostname", "hostname", probes(r"^\S+$"))
t("whoami", "whoami", probes(r"^[^\\\s]+\\[^\\\s]+$"))
t("whoami-user", "whoami /user", probes(r"S-1-5-21-\d+-\d+-\d+-\d+"))
t("tasklist", "tasklist", columns(), probes(r"^\S.*\s+\d+ \S+\s+\d+\s+[\d,.]+ K$"))
t("tasklist-fi", 'tasklist /fi "imagename eq cmd.exe"', probes(r"(?i)^cmd\.exe\s+\d+"))
t("tasklist-csv", "tasklist /fo csv /nh", probes(r'^"[^"]+","\d+","[^"]*","\d+","[\d,.]+ K"$'))
t("tasklist-pipe", 'tasklist | findstr /i "cmd.exe"', probes(r"(?i)^cmd\.exe\s+\d+"))
t("tasklist-count", 'tasklist /nh | find /c /v ""', probes(r"^\s*[1-9]\d*\s*$"))
t("taskkill-missing", "taskkill /f /im nosuchproc_tur.exe")
t("taskkill", "start /b ping -n 30 127.0.0.1 >nul 2>&1 & taskkill /f /im ping.exe", probes(r"(?i)SUCCESS"), rw=True)
t("systeminfo", "systeminfo", probes(r"^Host Name:\s+\S", r"^OS Name:\s+\S", r"^OS Version:\s+\d+\.\d+",
                                     r"^System Type:\s+\S", r"^Total Physical Memory:\s+[\d,.]+ MB",
                                     r"^Processor\(s\):\s+\d+"), timeout=240)
t("systeminfo-findstr", 'systeminfo | findstr /B /C:"OS Name" /C:"OS Version"',
  probes(r"^OS Name:", r"^OS Version:"), timeout=240)
t("where-cmd", "where cmd", probes(r"(?i)\\system32\\cmd\.exe"))
t("where-notepad", "where notepad", probes(r"(?i)\\notepad\.exe"))
t("where-r", "where /r . *.c", text("sort"))
t("where-missing", "where nosuchprogram_tur")
t("findstr", "findstr beta notes.txt", text())
t("findstr-i", 'findstr /i "ALPHA" notes.txt', text())
t("findstr-n", "findstr /n /i alpha notes.txt", text())
t("findstr-s", 'findstr /s /i /m "alpha" *.txt *.md', text("sort"))
t("findstr-r", 'findstr /r /c:"^g.*a$" notes.txt', text())
t("findstr-v", "findstr /v beta notes.txt", text())
t("findstr-c", 'findstr /c:"Alpha Beta" notes.txt', text())
t("findstr-miss", "findstr nosuch notes.txt")
t("findstr-multi", 'findstr "alpha gamma" notes.txt', text())
t("find", 'find "alpha" notes.txt', text("ci"))
t("find-c", 'find /i /c "alpha" notes.txt', text("ci"))
t("find-n", 'find /n "a" notes.txt', text("ci"))
t("sort-file", "sort notes.txt", text())
t("sort-o", "sort /r notes.txt /o sorted.txt & type sorted.txt", text(), rw=True)
t("more-file", "more notes.txt", text())
t("fc-diff", "fc notes.txt data.csv", exact=True)
t("fc-same", "fc notes.txt notes.txt", exact=True)
t("tree", "tree /f /a", text(drop=[r"(?i)^Folder PATH listing", r"(?i)^Volume serial number"]))
t("xcopy-e", "xcopy docs backup /e /i /y", probes(r"\d+ File\(s\) copied"), rw=True)
t("xcopy-wild", "xcopy src\\*.c csrc\\ /y", probes(r"2 File\(s\) copied"), rw=True)
t("xcopy-s-q", "xcopy docs backup /s /i /q", probes(r"3 File\(s\) copied"), rw=True)
t("xcopy-missing", "xcopy nosuchdir backup /e /i", rw=True)
t("robocopy-e", "robocopy docs mirror /e", rw=True, exact=True)
t("robocopy-mir", "robocopy docs mirror /mir /njh /njs /ndl /nfl", rw=True, exact=True)
t("robocopy-filter", "robocopy src dst *.c /njh /njs", rw=True, exact=True)
t("robocopy-nochange", "robocopy docs mirror /e", rw=True, exact=True, pre="robocopy docs mirror /e")
t("attrib-r", "attrib +r notes.txt & attrib notes.txt", text("ws"), rw=True)
t("attrib-h", "attrib +h report.log & dir /b", text(), rw=True)
t("attrib-list", "attrib", text("ws", "sort"))
t("attrib-ro-del", "attrib +r data.csv & del data.csv", rw=True)
t("sc-query", "sc query", probes(r"^SERVICE_NAME: \S+", r"^\s+STATE\s+: 4\s+RUNNING", r"^\s+TYPE\s+: [0-9a-f]+"))
t("sc-query-one", "sc query Winmgmt", probes(r"(?i)^SERVICE_NAME: winmgmt", r"^\s+STATE\s+: \d\s+\w+"))
t("sc-query-missing", "sc query nosuchsvc_tur", exact=True)
t("sc-qc", "sc qc Winmgmt", probes(r"^\s+BINARY_PATH_NAME\s+: \S", r"^\s+START_TYPE\s+: \d"))
t("sc-count", 'sc query state= all | find /c "SERVICE_NAME"', probes(r"^\s*[1-9]\d*\s*$"))
t("net-user", "net user", probes(r"(?i)Administrator"))
t("net-start", "net start", probes(r"^\s{3}\S"))
t("net-localgroup", "net localgroup", probes(r"^\*Administrators"))
t("net-use", "net use")
t("reg-query-value", r'reg query "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion" /v ProductName',
  probes(r"^\s+ProductName\s+REG_SZ\s+\S"))
t("reg-query-key", r"reg query HKCU\Environment",
  probes(r"(?i)^HKEY_CURRENT_USER\\Environment", r"(?i)^\s+TEMP\s+REG_(EXPAND_)?SZ"))
t("reg-query-pfdir", r"reg query HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion /v ProgramFilesDir",
  probes(r"(?i)^\s+ProgramFilesDir\s+REG_SZ\s+C:\\Program Files$"))
t("reg-query-missing", r"reg query HKCU\Software\NoSuchKeyTur")
t("reg-add-sz",
  r"reg add HKCU\Software\TurWincliTest /v Name /t REG_SZ /d hello /f & reg query HKCU\Software\TurWincliTest /v Name",
  probes(r"^\s+Name\s+REG_SZ\s+hello$"), rw=True)
t("reg-add-dword",
  r"reg add HKCU\Software\TurWincliTest /v Count /t REG_DWORD /d 42 /f & reg query HKCU\Software\TurWincliTest /v Count",
  probes(r"^\s+Count\s+REG_DWORD\s+0x2a$"), rw=True)
t("reg-delete", r"reg delete HKCU\Software\TurWincliTest /f & reg query HKCU\Software\TurWincliTest", rw=True,
  pre=r"reg add HKCU\Software\TurWincliTest /v Name /d hello /f")
t("reg-query-s", r"reg query HKCU\Software\TurWincliTest /s",
  probes(r"(?i)^HKEY_CURRENT_USER\\Software\\TurWincliTest\\Sub$", r"^\s+Deep\s+REG_SZ\s+yes$"), rw=True,
  pre=r"reg add HKCU\Software\TurWincliTest\Sub /v Deep /d yes /f")
t("reg-export", r"reg export HKCU\Software\TurWincliTest out.reg /y & if exist out.reg echo exported",
  probes(r"^exported$"), rw=True, files="names", pre=r"reg add HKCU\Software\TurWincliTest /v Name /d hello /f")
t("certutil-sha256", "certutil -hashfile notes.txt SHA256",
  probes(r"(?i)^" + r"\s?".join(re.findall("..", fixture.SHA256["notes.txt"])) + r"$"))
t("certutil-md5", "certutil -hashfile notes.txt MD5",
  probes(r"(?i)^" + r"\s?".join(re.findall("..", fixture.MD5["notes.txt"])) + r"$"))
t("certutil-missing", "certutil -hashfile nosuch.txt SHA256")
t("certutil-encode", "certutil -encode notes.txt notes.b64 & type notes.b64",
  probes(r"^-----BEGIN CERTIFICATE-----$",
         "^" + re.escape(base64.b64encode(fixture.FILES["notes.txt"].encode()).decode()) + "$"), rw=True)
t("tar-list", "tar -cf docs.tar docs & tar -tf docs.tar", text("sort"), rw=True, files="names")
t("tar-extract", "tar -cf docs.tar docs & mkdir out & tar -xf docs.tar -C out & dir /b /s out", text("sort"),
  rw=True, files="names")
t("tar-gz", "tar -czf src.tgz src & tar -tzf src.tgz", text("sort"), rw=True, files="names")
t("curl-version", "curl --version", probes(r"^curl \d+\.\d+", r"(?i)^Protocols: .*https"))
t("curl-status", 'curl -s -o NUL -w "%{http_code}" https://example.com', text())
t("curl-head", "curl -sI https://example.com", probes(r"^HTTP/\S+ 200"))
t("curl-save", "curl -s -L -o page.html https://example.com & if exist page.html echo saved", text(), rw=True,
  files="names")
t("schtasks-query", "schtasks /query /fo LIST", probes(r"^TaskName:\s+\\"))
t("schtasks-create", 'schtasks /create /tn TurWincliTest /tr "cmd /c echo hi" /sc once /st 23:59 /f',
  probes(r"(?i)SUCCESS"), rw=True)
t("schtasks-delete", "schtasks /delete /tn TurWincliTest /f", probes(r"(?i)SUCCESS"), rw=True,
  pre='schtasks /create /tn TurWincliTest /tr "cmd /c echo hi" /sc once /st 23:59 /f')
t("shutdown-help", "shutdown /?", probes(r"^\s+/s\s", r"^\s+/r\s", r"^\s+/t\s", r"^\s+/a\s"), helptext=True)
t("winget-version", "winget --version", probes(r"^v\d+\.\d+"))
t("winget-search", "winget search --id 7zip.7zip -e --accept-source-agreements --disable-interactivity",
  probes(r"7zip\.7zip"), timeout=240)
t("winget-install",
  "winget install --id 7zip.7zip -e --accept-source-agreements --accept-package-agreements "
  "--disable-interactivity --silent", rw=True, timeout=420, files="skip")
t("getmac", "getmac", probes(MAC))
t("arp", "arp -a", probes(r"^Interface: " + IPV4))
t("netstat", "netstat -an", probes(r"^\s+TCP\s+\S+:\d+\s+\S+\s+LISTENING"))
t("netstat-pipe", "netstat -ano | findstr LISTENING", probes(r"^\s+TCP\s+\S+:\d+\s+\S+\s+LISTENING\s+\d+"))
t("route-print", "route print", probes(r"^\s+0\.0\.0\.0\s+0\.0\.0\.0\s+" + IPV4))
t("wmic-os", "wmic os get Caption /value", probes(r"^Caption=\S"))
t("setx", "setx TURTEST hello", probes(r"(?i)SUCCESS"), rw=True)
t("icacls", "icacls notes.txt", probes(r"^notes\.txt \S.*:\("))
t("fsutil-drives", "fsutil fsinfo drives", probes(r"(?i)Drives:.*C:\\"))
t("chcp", "chcp", probes(r"\d{3,5}\s*$"))
t("chcp-utf8", "chcp 65001", probes(r"65001"))
t("subst", r"subst X: C:\turfix\docs & dir /b X:\ & subst X: /d", text(), rw=True)
t("ps-from-cmd", 'powershell -NoProfile -Command "Write-Output hello"', text())
t("ps-file-from-cmd", r"powershell -NoProfile -ExecutionPolicy Bypass -File scripts\hello.ps1", text())
t("ps-year-from-cmd", 'powershell -NoProfile -Command "(Get-Date).Year"', probes(r"^20\d\d\s*$"))
# Repair commands: not run on the judge (minutes long, and they change the
# machine). Scored only for whether TUR answers them.
t("sfc-scannow", "sfc /scannow", truth=False)
t("chkdsk", "chkdsk C: /f", truth=False)
t("dism-restorehealth", "DISM /Online /Cleanup-Image /RestoreHealth", truth=False)

# ---------------------------------------------------------- PowerShell ----

TOP = sorted(set(n.split("/")[0] for n in fixture.FILES) | set(fixture.DIRS))

p("gci", "Get-ChildItem", columns(), probes(r"^d-{4}\s.*\sdocs$",
                                            rf"^-a-{{3}}\s.*\s{fixture.SIZE['data.csv']} data\.csv$"))
p("gci-name", "Get-ChildItem -Name", text())
p("gci-recurse-filter", "Get-ChildItem -Recurse -Filter *.txt | Select-Object -ExpandProperty Name", text("sort"))
p("gci-count", r"(Get-ChildItem -Path C:\turfix\docs -File).Count", text())
p("gci-dir-alias", "dir", columns())
p("gci-ls", "ls | Select-Object -ExpandProperty Name", text("sort"))
p("gci-sort", "Get-ChildItem -File | Sort-Object Length -Descending | Select-Object -First 1 -ExpandProperty Name",
  text())
p("gci-where-json",
  "Get-ChildItem -File | Where-Object { $_.Length -gt 40 } | Select-Object Name, Length | ConvertTo-Json", js())
p("gci-sum", "(Get-ChildItem -Recurse -File | Measure-Object -Property Length -Sum).Sum", text())
p("gci-directory", "Get-ChildItem -Directory -Name", text())
p("gci-basename", "Get-ChildItem *.txt | ForEach-Object { $_.BaseName }", text("sort"))
p("gci-group",
  'Get-ChildItem -File -Recurse | Group-Object Extension | Sort-Object Name | ForEach-Object { "$($_.Name)=$($_.Count)" }',
  text())
p("gci-fullname", "(Get-Item notes.txt).FullName", text())
p("gci-recurse-name", r"Get-ChildItem C:\turfix\docs -Recurse -Name", text("sort"))
p("gc", "Get-Content notes.txt", text())
p("gc-cat", "cat notes.txt", text())
p("gc-total", "Get-Content notes.txt -TotalCount 2", text())
p("gc-last", "Get-Content notes.txt | Select-Object -Last 1", text())
p("gc-count", "(Get-Content notes.txt).Count", text())
p("gc-type", "type notes.txt", text())
p("set-content", 'Set-Content -Path out.txt -Value "hello"; Get-Content out.txt', text(), rw=True)
p("add-content", 'Add-Content notes.txt "delta"; (Get-Content notes.txt).Count', text(), rw=True)
p("out-file", '"text" | Out-File out.txt; (Get-Item out.txt).Length', text(), rw=True)
p("redirect", 'echo "hello" > out.txt; Get-Content out.txt', text(), rw=True)
p("copy-item", "Copy-Item notes.txt copy.txt", rw=True)
p("copy-recurse", "Copy-Item docs backup -Recurse", rw=True)
p("move-item", "Move-Item report.log docs\\", rw=True)
p("rename-item", "Rename-Item notes.txt notes.bak", rw=True)
p("remove-item", "Remove-Item data.csv", rw=True)
p("remove-recurse", "Remove-Item docs -Recurse -Force", rw=True)
p("new-dir", r"New-Item -ItemType Directory -Path newdir\sub | Out-Null", rw=True)
p("new-file", 'New-Item -Path . -Name "new.txt" -ItemType File -Value "x" | Out-Null', rw=True)
p("mkdir", "mkdir newdir2 | Out-Null", rw=True)
p("test-path", "Test-Path notes.txt", text())
p("test-path-abs", r"Test-Path C:\turfix\docs\readme.md", text())
p("test-path-sys", r"Test-Path C:\Windows\System32", text())
p("test-path-profile", "Test-Path $env:USERPROFILE", text())
p("dotnet-file-exists", r"[System.IO.File]::Exists('C:\turfix\notes.txt')", text())
p("get-location", "(Get-Location).Path", text())
p("set-location", "Set-Location docs; (Get-Location).Path", text())
p("cd-abs", r"cd C:\turfix\src; Get-ChildItem -Name", text())
p("resolve-path", r"(Resolve-Path .\docs\..\notes.txt).Path", text())
p("join-path", "Join-Path $env:USERPROFILE Desktop", probes(HOME + r"\\Desktop$"))
p("split-leaf", r"Split-Path C:\turfix\docs\readme.md -Leaf", text())
p("split-parent", r"Split-Path C:\turfix\docs\readme.md", text())
p("temp-path", "[System.IO.Path]::GetTempPath()", probes(r"(?i)^C:\\.*\\Temp\\$"))
p("file-hash", "(Get-FileHash notes.txt -Algorithm SHA256).Hash", text())
p("file-hash-list", "Get-FileHash notes.txt | Format-List", keys())
p("select-string", "Select-String -Path notes.txt -Pattern beta", text())
p("select-string-recurse",
  "Get-ChildItem -Recurse -Include *.txt,*.md | Select-String alpha | Select-Object -ExpandProperty Filename",
  text("sort"))
p("item-attributes", "(Get-Item notes.txt).Attributes", text())
p("item-list", "Get-Item notes.txt | Select-Object Name, Length | Format-List", text("ws"))
p("compress", "Compress-Archive -Path docs -DestinationPath docs.zip; Test-Path docs.zip", text(), rw=True,
  files="names")
p("expand",
  "Compress-Archive -Path docs -DestinationPath docs.zip; Expand-Archive docs.zip -DestinationPath out; "
  "Get-ChildItem out -Recurse -Name", text("sort"), rw=True, files="names")
p("csv-where", "Import-Csv data.csv | Where-Object { [int]$_.size -gt 10 } | Select-Object -ExpandProperty name",
  text())
p("csv-sort",
  "Import-Csv data.csv | Sort-Object { [int]$_.size } -Descending | Select-Object -First 1 -ExpandProperty name",
  text())
p("csv-json", "Import-Csv data.csv | ConvertTo-Json", js())
p("csv-sum", "(Import-Csv data.csv | Measure-Object -Property size -Sum).Sum", text())
p("csv-table", "Import-Csv data.csv | Format-Table -AutoSize", text("ws"))
p("csv-export",
  "Import-Csv data.csv | Where-Object colour -eq red | Export-Csv out.csv -NoTypeInformation; Get-Content out.csv",
  text(), rw=True)
p("select-table", "Get-ChildItem -File | Select-Object Name, Length | Sort-Object Name | Format-Table -AutoSize",
  text("ws"))
p("range-foreach", "1..5 | ForEach-Object { $_ * 2 }", text())
p("where-sum", "(1..10 | Where-Object { $_ % 2 -eq 0 } | Measure-Object -Sum).Sum", text())
p("split-upper", '"a,b,c".Split(",") | ForEach-Object { $_.ToUpper() }', text())
p("date-format", 'Get-Date -Format "yyyy-MM-dd"', probes(r"^\d{4}-\d{2}-\d{2}$"))
p("round", "[math]::Round(3.14159, 2)", text())
p("write-host", 'Write-Host "Hello"', text())
p("try-catch", 'try { throw "boom" } catch { "caught: $_" }', text())
p("error-stop", "$ErrorActionPreference = 'Stop'; Get-Item nosuch.txt")
p("ordered-json", "[ordered]@{ name = 'tur'; n = 3 } | ConvertTo-Json -Compress", text())
p("replace", '"Hello World" -replace "World", "TUR"', text())
p("iex-plain", "'Write-Output piped' | Invoke-Expression", text())
p("sleep", 'Start-Sleep -Seconds 1; "done"', text())
p("get-help", "Get-Help Get-ChildItem", probes(r"^NAME", r"^SYNTAX"), helptext=True)
p("psversion", "$PSVersionTable.PSVersion.Major", text())
p("platform", "$PSVersionTable.Platform", text())
p("iswindows", "$IsWindows", text())
p("os-platform", "[Environment]::OSVersion.Platform", text())
p("os-major", "[Environment]::OSVersion.Version.Major", text())
p("newline-length", "[Environment]::NewLine.Length", text())
p("dir-separator", "[IO.Path]::DirectorySeparatorChar", text())
p("host-name", "(Get-Host).Name", text())
p("env-username", "$env:USERNAME", probes(r"^\S+$"))
p("env-userprofile", "$env:USERPROFILE", probes(HOME + r"$"))
p("env-computername", "$env:COMPUTERNAME", probes(r"^[A-Za-z0-9_-]+$"))
p("env-temp", "$env:TEMP", probes(r"(?i)^C:\\.*\\Temp$"))
p("env-path-first", "($env:Path -split ';')[0]", probes(r"^[A-Za-z]:\\"))
p("env-os", "$env:OS", text())
p("env-set", '$env:TURTEST = "abc"; $env:TURTEST', text())
p("env-gci", "Get-ChildItem Env: | Where-Object Name -eq 'USERPROFILE' | Select-Object -ExpandProperty Value",
  probes(HOME + r"$"))
p("env-getvar", '[Environment]::GetEnvironmentVariable("windir")', text("ci"))
p("env-folder", '[Environment]::GetFolderPath("Desktop")', probes(HOME + r"\\Desktop$"))
p("env-setuser",
  '[Environment]::SetEnvironmentVariable("TURTEST", "x", "User"); [Environment]::GetEnvironmentVariable("TURTEST", "User")',
  text(), rw=True)
p("env-appdata", "$env:APPDATA", probes(HOME + r"\\AppData\\Roaming$"))
p("env-test", r"Test-Path Env:\SystemRoot", text())
p("env-programfiles", "$env:ProgramFiles", text("ci"))
p("env-child", '$env:TURCHILD = "seen"; cmd /c "echo %TURCHILD%"', text())
p("machine-name", "[System.Environment]::MachineName", probes(r"^[A-Za-z0-9_-]+$"))
CV = r"'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'"
TK = r"HKCU:\Software\TurWincliTest"
RESET = rf"Remove-Item {TK} -Recurse -Force -ErrorAction SilentlyContinue; New-Item -Path {TK} -Force | Out-Null"
p("reg-productname", f"(Get-ItemProperty -Path {CV}).ProductName", probes(r"\S"))
p("reg-build", f"(Get-ItemProperty {CV}).CurrentBuild", probes(r"^\d+\s*$"))
p("reg-value", r"Get-ItemPropertyValue 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion' -Name ProgramFilesDir",
  text("ci"))
p("reg-test", r"Test-Path HKCU:\Software", text())
p("reg-test-missing", r"Test-Path HKLM:\SOFTWARE\NoSuchKeyTur", text())
p("reg-children", r"Get-ChildItem HKCU:\ | Select-Object -ExpandProperty PSChildName",
  probes(r"(?i)^Software$", r"(?i)^Environment$"))
p("reg-new",
  rf"New-Item -Path {TK} -Force | Out-Null; New-ItemProperty -Path {TK} -Name Colour -Value blue "
  rf"-PropertyType String -Force | Out-Null; (Get-ItemProperty {TK}).Colour", text(), rw=True)
p("reg-set-dword", rf"Set-ItemProperty -Path {TK} -Name Count -Value 7 -Type DWord; (Get-ItemProperty {TK}).Count",
  text(), rw=True, pre=RESET)
p("reg-remove", rf"Remove-Item {TK} -Recurse; Test-Path {TK}", text(), rw=True, pre=RESET)
p("reg-drives", "Get-PSDrive -PSProvider Registry | Select-Object -ExpandProperty Name", text("sort"))
p("reg-cd", r"Set-Location HKLM:\SOFTWARE; (Get-Location).Path", text())
p("reg-env", r"(Get-ItemProperty 'HKCU:\Environment').TEMP", probes(r"\S"))
p("reg-get-item", f"(Get-Item {CV}).GetValue('CurrentVersion')", probes(r"^\d+\.\d+"))
p("reg-exe", r'reg query "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion" /v ProductName',
  probes(r"^\s+ProductName\s+REG_SZ\s+\S"))
p("cim-os", "Get-CimInstance Win32_OperatingSystem | Select-Object Caption, Version, OSArchitecture | Format-List",
  keys(), probes(r"^Caption\s+: \S", r"^Version\s+: \d+\.\d+", r"^OSArchitecture\s+: 64"))
p("cim-memory", "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory", probes(r"^\d{9,}\s*$"))
p("cim-cpu", "(Get-CimInstance Win32_Processor).Name", probes(r"\S"))
p("cim-cpu-count",
  "(Get-CimInstance Win32_Processor | Measure-Object -Property NumberOfLogicalProcessors -Sum).Sum",
  probes(r"^[1-9]\d*\s*$"))
p("cim-disk",
  'Get-CimInstance Win32_LogicalDisk -Filter "DriveType=3" | Select-Object DeviceID, Size, FreeSpace | Format-List',
  keys(), probes(r"^DeviceID\s+: C:", r"^Size\s+: \d{9,}", r"^FreeSpace\s+: \d+"))
p("cim-bios", "Get-CimInstance Win32_BIOS | Select-Object Manufacturer, SMBIOSBIOSVersion | Format-List", keys())
p("cim-net",
  'Get-CimInstance Win32_NetworkAdapterConfiguration -Filter "IPEnabled=True" | Select-Object -ExpandProperty IPAddress',
  probes(r"^" + IPV4 + r"$"))
p("cim-services", "(Get-CimInstance -ClassName Win32_Service -Filter \"State='Running'\" | Measure-Object).Count",
  probes(r"^[1-9]\d*\s*$"))
p("cim-process", "Get-CimInstance Win32_Process | Select-Object -First 3 Name, ProcessId | Format-Table", columns())
p("cim-computersystem",
  "Get-CimInstance Win32_ComputerSystem | Select-Object Manufacturer, Model, Name | Format-List", keys())
p("cim-boot", "(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.Year", probes(r"^20\d\d\s*$"))
p("cim-query",
  "Get-CimInstance -Query \"SELECT Name FROM Win32_Process WHERE Name='pwsh.exe'\" | "
  "Select-Object -First 1 -ExpandProperty Name", text())
p("cim-video", "(Get-CimInstance Win32_VideoController | Select-Object -First 1).Name", probes(r"\S"))
p("cim-freemem", "[math]::Round((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB, 1) -gt 0", text())
p("computer-info", "Get-ComputerInfo -Property OsName, CsName | Format-List", keys(), timeout=240)
p("svc-table", "Get-Service | Select-Object -First 3 | Format-Table", columns())
p("svc-running", "(Get-Service | Where-Object Status -eq 'Running').Count -gt 0", text())
p("svc-one", "(Get-Service -Name Winmgmt).Status", probes(r"^(Running|Stopped)\s*$"))
p("svc-missing", "Get-Service -Name NoSuchSvcTur")
p("svc-starttype", "(Get-Service Winmgmt).StartType", probes(r"^(Automatic|Manual|Disabled)"))
p("svc-list", "Get-Service | Select-Object -First 1 Name, Status, StartType | Format-List", keys())
p("svc-wild", "Get-Service -Name Win* | Select-Object -ExpandProperty Name", probes(r"(?i)^winmgmt$"))
p("svc-group", "Get-Service | Group-Object Status | Select-Object -ExpandProperty Name", probes(r"^Running$"))
p("proc-table", "Get-Process | Select-Object -First 3", columns())
p("proc-top", "Get-Process | Sort-Object CPU -Descending | Select-Object -First 5 Name, Id, CPU | Format-Table",
  columns())
p("proc-self", "(Get-Process -Id $PID).ProcessName", text())
p("proc-count", "(Get-Process | Where-Object { $_.WorkingSet64 -gt 1MB }).Count -gt 0", text())
p("proc-missing", "Get-Process -Name nosuchproc_tur")
p("proc-start-wait",
  "(Start-Process -FilePath cmd.exe -ArgumentList '/c','exit 5' -Wait -PassThru -WindowStyle Hidden).ExitCode",
  text())
p("proc-stop",
  "$p = Start-Process ping -ArgumentList '-n','30','127.0.0.1' -PassThru -WindowStyle Hidden; "
  "Start-Sleep -Milliseconds 500; Stop-Process -Id $p.Id -Force; $p.WaitForExit(5000)", text(), rw=True)
p("proc-path", "(Get-Process -Id $PID).Path", probes(r"(?i)pwsh\.exe$"))
p("proc-name", "Get-Process -Name pwsh | Select-Object -First 1 -ExpandProperty Name", text())
p("iwr-status", "(Invoke-WebRequest -Uri https://example.com -UseBasicParsing).StatusCode", text())
p("iwr-content", "(Invoke-WebRequest https://example.com).Content -match 'Example Domain'", text())
p("iwr-outfile", "Invoke-WebRequest -Uri https://example.com -OutFile page.html; Test-Path page.html", text(),
  rw=True, files="names")
p("irm", "(Invoke-RestMethod https://example.com) -match 'Example Domain'", text())
p("webclient", "(New-Object System.Net.WebClient).DownloadString('https://example.com') -match 'Example Domain'",
  text())
p("test-connection", "Test-Connection -ComputerName 127.0.0.1 -Count 1 -Quiet", text())
p("test-netconnection",
  "(Test-NetConnection -ComputerName example.com -Port 443 -WarningAction SilentlyContinue).TcpTestSucceeded",
  text())
p("resolve-dns", "(Resolve-DnsName example.com -Type A | Select-Object -First 1).Type", text())
p("net-ipaddress", "Get-NetIPAddress -AddressFamily IPv4 | Select-Object -ExpandProperty IPAddress",
  probes(r"^127\.0\.0\.1$"))
p("net-adapter", "Get-NetAdapter | Select-Object -First 1 -ExpandProperty Status",
  probes(r"^(Up|Disconnected|Disabled)"))
p("dns-hostname", "[System.Net.Dns]::GetHostName()", probes(r"^\S+$"))
p("get-command", "(Get-Command Get-ChildItem).CommandType", text())
p("get-alias", "(Get-Alias ls).Definition", text())
p("execution-policy",
  "Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass -Force; Get-ExecutionPolicy -Scope Process", text())
p("modules", "Get-Module -ListAvailable | Select-Object -ExpandProperty Name | Sort-Object -Unique",
  probes(r"^Microsoft\.PowerShell\.Management$", r"^Microsoft\.PowerShell\.Utility$"))
p("psdrive-c", "(Get-PSDrive C).Root", text())
p("psdrive-fs", "Get-PSDrive -PSProvider FileSystem | Select-Object -ExpandProperty Name", probes(r"^C$"))
p("providers", "Get-PSProvider | Select-Object -ExpandProperty Name",
  probes(r"^Registry$", r"^Environment$", r"^FileSystem$"))
p("local-user", "Get-LocalUser | Select-Object -ExpandProperty Name", probes(r"\S"))
p("win-event", "(Get-WinEvent -LogName System -MaxEvents 1).LogName", text())
p("volume", "(Get-Volume -DriveLetter C).DriveLetter", text())
p("timezone", "(Get-TimeZone).Id", probes(r"\S"))
p("culture", "(Get-Culture).Name", probes(r"^[a-z]{2}-[A-Z]{2}"))
p("tool-whoami", "whoami", probes(r"^[^\\\s]+\\[^\\\s]+$"))
p("tool-ipconfig", "ipconfig | Select-String IPv4", probes(r"IPv4 Address[. ]*: " + IPV4))
p("tool-ping", 'ping -n 1 127.0.0.1 | Select-String "Reply"', probes(r"Reply from 127\.0\.0\.1"))
p("tool-hostname", "hostname", probes(r"^\S+$"))
p("tool-cmd", "cmd /c ver", probes(r"Microsoft Windows \[Version \d+\.\d+\.\d+"))
p("tool-where", "where.exe cmd", probes(r"(?i)\\system32\\cmd\.exe"))
p("tool-findstr", "Get-Content notes.txt | findstr /i alpha", text())
p("tool-tasklist", 'tasklist /fi "imagename eq pwsh.exe" /fo csv /nh', probes(r'(?i)^"pwsh\.exe","\d+"'))
p("lastexitcode", "cmd /c exit 4; $LASTEXITCODE", text())
p("ps1-run", r".\scripts\hello.ps1", text())
p("ps1-args", r"& .\scripts\hello.ps1 -Name World", text())
p("bat-from-ps", r".\scripts\hello.bat", text())


def by_id() -> dict[str, dict]:
    return {entry["id"]: entry for entry in CORPUS}


if __name__ == "__main__":
    import collections

    counts = collections.Counter(entry["group"] for entry in CORPUS)
    changing = sum(1 for entry in CORPUS if entry["rw"])
    print(f"{len(CORPUS)} commands: {dict(counts)}; {changing} state-changing, {len(CORPUS) - changing} read-only")
