# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Commands that would damage a real computer if taken literally, and what
tur-winshell must decide about each (rules.py states the rules).

None of these is ever run on the Windows judge. On Linux they are judged
without being run (`tur-winshell --verdict`), and the ones marked `run`
are then run for real against a throwaway Windows environment in which
C:\\users\\<name>\\Documents is a mapped folder with canary files, to
check that the canary survives the router, and to show what happens to it
without the router (plain Wine).

    (shell, line, expected action, rule, run)
"""

CASES = [
    # R1: disks, partitions, boot.
    ("cmd", "format D: /q", "refuse", "R1", True),
    ("cmd", "format C: /fs:ntfs /q /y", "refuse", "R1", False),
    ("cmd", "diskpart /s wipe.txt", "refuse", "R1", True),
    ("cmd", "bcdedit /set {default} safeboot minimal", "refuse", "R1", True),
    ("cmd", "bcdedit /delete {current}", "refuse", "R1", False),
    ("cmd", "bootrec /fixmbr", "refuse", "R1", False),
    ("cmd", "cipher /w:C:\\", "refuse", "R1", False),
    ("cmd", "echo y | format E:", "refuse", "R1", False),
    ("cmd", "if exist D:\\ format D: /q", "refuse", "R1", False),
    ("cmd", 'cmd /c "diskpart /s x.txt"', "refuse", "R1", False),
    ("cmd", 'start "" /wait bcdedit /set testsigning on', "refuse", "R1", False),
    ("cmd", 'powershell -Command "Format-Volume -DriveLetter D -Force"', "refuse", "R1", False),
    ("ps", "Format-Volume -DriveLetter D -FileSystem NTFS", "refuse", "R1", True),
    ("ps", "Clear-Disk -Number 0 -RemoveData -Confirm:$false", "refuse", "R1", True),
    ("ps", "Get-Disk | Initialize-Disk -PartitionStyle GPT", "refuse", "R1", False),
    ("ps", "Remove-Partition -DriveLetter D", "refuse", "R1", False),
    ("ps", "bcdedit /set testsigning on", "refuse", "R1", False),
    # R2: power.
    ("ps", "Stop-Computer -Force", "refuse", "R2", True),
    ("ps", "Restart-Computer", "refuse", "R2", False),
    # R3: recursive deletes that leave the Windows environment.
    ("cmd", "rd /s /q C:\\Users\\%USERNAME%\\Documents", "refuse", "R3", False),
    ("cmd", "rd /s /q %USERPROFILE%\\Documents", "refuse", "R3", True),
    ("cmd", "del /s /q %USERPROFILE%\\Documents\\*", "refuse", "R3", True),
    ("cmd", "del /s /q Z:\\home\\*", "refuse", "R3", False),
    ("cmd", "rd /s /q C:\\", "refuse", "R3", False),
    ("cmd", "del /s /q C:\\*.*", "refuse", "R3", False),
    ("cmd", "for /d %i in (*) do rd /s /q %i", "refuse", "R3", False),
    ("cmd", "robocopy empty %USERPROFILE%\\Documents /mir", "refuse", "R3", True),
    ("cmd", 'cmd /c "rd /s /q %USERPROFILE%\\Documents"', "refuse", "R3", False),
    ("ps", "Remove-Item $env:USERPROFILE\\Documents -Recurse -Force", "refuse", "R3", True),
    ("ps", "Remove-Item C:\\ -Recurse -Force", "refuse", "R3", False),
    ("ps", "rm -r $target", "refuse", "R3", False),
    ("ps", 'cmd /c "rd /s /q %USERPROFILE%\\Documents"', "refuse", "R3", False),
    # Allowed: the same verbs inside the Windows environment.
    ("cmd", "rd /s /q docs", "allow", "", True),
    ("cmd", "del /s /q *.txt", "allow", "", True),
    ("cmd", "del notes.txt", "allow", "", False),
    ("cmd", "del %USERPROFILE%\\Documents\\canary.txt", "allow", "", False),
    ("cmd", "robocopy docs mirror /mir", "allow", "", False),
    ("cmd", "echo format D:", "allow", "", False),
    ("cmd", "reg add HKCU\\Software\\TurWincliTest /v X /d 1 /f", "allow", "", True),
    ("cmd", "reg delete HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run /va /f", "allow", "", True),
    ("ps", "Remove-Item docs -Recurse -Force", "allow", "", True),
    ("ps", "Remove-Item notes.txt", "allow", "", False),
    ("ps", "Set-ItemProperty -Path HKCU:\\Software\\TurWincliTest -Name X -Value 1", "allow", "", False),
]
