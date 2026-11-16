@echo off
rem SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
rem
rem Run once by Windows Setup's first-logon command, from the answer medium.
rem Installs TUR's test agent (agent.ps1) as a task that starts when the test
rem user signs in, and sets three things a test rig needs: the screen and the
rem machine do not go to sleep by themselves, and the test user is signed in
rem automatically. Nothing else in Windows is changed: no driver, no service,
rem no boot file.
mkdir C:\dualrun 2>nul
copy /y "%~dp0agent.ps1" C:\dualrun\agent.ps1
schtasks /create /tn "TUR dualrun test agent" /tr "powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File C:\dualrun\agent.ps1" /sc onlogon /rl highest /f
powercfg /change monitor-timeout-ac 0
powercfg /change standby-timeout-ac 0
powercfg /change hibernate-timeout-ac 0
reg add "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon" /v AutoAdminLogon /t REG_SZ /d 1 /f
reg add "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon" /v DefaultUserName /t REG_SZ /d tester /f
reg add "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon" /v DefaultPassword /t REG_SZ /d @PASSWORD@ /f
schtasks /run /tn "TUR dualrun test agent"
