# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# TUR dualrun test agent. Test instrumentation, written by TUR, installed in
# the evaluation Windows by Setup's first-logon command (guest/setup.cmd).
# It runs in the signed-in user's session, started by a scheduled task at
# logon, and speaks only on the first serial port: one JSON object per line,
# each line starting with @@DR@@. The host sends a PowerShell script
# (base64) and gets its output back (base64). It listens on no network port.
#
# A serial port is used because Windows has its driver built in. On a
# computer without that port the agent finds no host and exits at once: it
# does nothing when Windows runs by itself.

$prefix = '@@DR@@'
try {
    $port = New-Object System.IO.Ports.SerialPort 'COM1', 115200, ([System.IO.Ports.Parity]::None), 8, ([System.IO.Ports.StopBits]::One)
    $port.ReadBufferSize = 4194304
    $port.WriteBufferSize = 1048576
    $port.ReadTimeout = 3000
    $port.WriteTimeout = 120000
    $port.NewLine = "`n"
    $port.Encoding = [System.Text.Encoding]::ASCII
    $port.DtrEnable = $true
    $port.RtsEnable = $true
    $port.Open()
} catch {
    exit 0
}
$ErrorActionPreference = 'Continue'

function Send-Line($object) {
    $port.WriteLine($prefix + ($object | ConvertTo-Json -Compress -Depth 8))
}

function To-Base64([string]$text) {
    [Convert]::ToBase64String([System.Text.Encoding]::UTF8.GetBytes($text))
}

$os = Get-CimInstance Win32_OperatingSystem
$hello = @{
    ev      = 'hello'
    boot    = $os.LastBootUpTime.ToUniversalTime().ToString('o')
    started = (Get-Date).ToUniversalTime().ToString('o')
    user    = $env:USERNAME
    session = [System.Diagnostics.Process]::GetCurrentProcess().SessionId
    pid     = $PID
}
$heard = $false
while ($true) {
    if (-not $heard) {
        $hello.now = (Get-Date).ToUniversalTime().ToString('o')
        try { Send-Line $hello } catch { }
    }
    try {
        $line = $port.ReadLine()
    } catch [System.TimeoutException] {
        continue
    } catch {
        Start-Sleep -Seconds 1
        continue
    }
    $at = $line.IndexOf($prefix)
    if ($at -lt 0) { continue }
    try { $request = $line.Substring($at + $prefix.Length) | ConvertFrom-Json } catch { continue }
    $heard = $true
    $reply = @{ id = $request.id; ok = $true; out = '' }
    try {
        $script = [System.Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($request.b64))
        $items = & ([scriptblock]::Create($script)) 2>&1
        $text = ($items | ForEach-Object {
                if ($_ -is [string]) { $_ } else { ($_ | Out-String -Width 4000).TrimEnd() }
            }) -join "`n"
        $reply.out = To-Base64 $text
    } catch {
        $reply.ok = $false
        $reply.out = To-Base64 (($_ | Out-String -Width 4000))
    }
    try { Send-Line $reply } catch { }
}
