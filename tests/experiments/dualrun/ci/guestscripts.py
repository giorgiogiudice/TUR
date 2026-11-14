# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""PowerShell that the experiment sends to TUR's test agent inside Windows.

Each script only asks Windows what it reports, or does one named thing. The
answers are text and JSON that these scripts produce; no file of Microsoft's
is copied out of the guest.
"""

# What Windows reports about itself. One JSON object on the last line.
FACTS = r'''
$ErrorActionPreference = 'SilentlyContinue'
$os = Get-CimInstance Win32_OperatingSystem
$cs = Get-CimInstance Win32_ComputerSystem
$prod = Get-CimInstance Win32_ComputerSystemProduct
$bios = Get-CimInstance Win32_BIOS
$bb = Get-CimInstance Win32_BaseBoard
$boot = $os.LastBootUpTime
$lic = Get-CimInstance SoftwareLicensingProduct -Filter "ApplicationID='55c92734-d682-4d71-983e-d6ec3f16059f' AND PartialProductKey IS NOT NULL" | Select-Object -First 1
$svc = Get-CimInstance SoftwareLicensingService
$ev = @(Get-WinEvent -FilterHashtable @{LogName='System'; StartTime=$boot} -MaxEvents 5000 | Group-Object ProviderName, Id | Sort-Object Count -Descending | Select-Object -First 90 | ForEach-Object { @{ k=$_.Name; n=$_.Count } })
$spp = @(Get-WinEvent -FilterHashtable @{LogName='Application'; ProviderName='Microsoft-Windows-Security-SPP'; StartTime=$boot} -MaxEvents 300 | Group-Object Id | ForEach-Object { @{ id=$_.Name; n=$_.Count } })
$bt = Get-WinEvent -FilterHashtable @{LogName='System'; ProviderName='Microsoft-Windows-Kernel-Boot'; Id=27; StartTime=$boot} -MaxEvents 1
$bad = @(Get-PnpDevice -PresentOnly | Where-Object { $_.Status -ne 'OK' } | ForEach-Object { @{ class="$($_.Class)"; name="$($_.FriendlyName)"; id="$($_.InstanceId)"; problem="$($_.Problem)" } })
$tpm = Get-Tpm
$r = [ordered]@{
  caption = $os.Caption; version = $os.Version; build = $os.BuildNumber
  boot = $boot.ToUniversalTime().ToString('o'); now = (Get-Date).ToUniversalTime().ToString('o')
  boot_type = "$($bt.Message)"
  computer = @{ manufacturer=$cs.Manufacturer; model=$cs.Model; hypervisor=$cs.HypervisorPresent; memory_mb=[int]($cs.TotalPhysicalMemory/1MB); cpus=$cs.NumberOfLogicalProcessors }
  product = @{ vendor=$prod.Vendor; name=$prod.Name; uuid=$prod.UUID; serial=$prod.IdentifyingNumber }
  bios = @{ vendor=$bios.Manufacturer; version=$bios.SMBIOSBIOSVersion; serial=$bios.SerialNumber }
  board = @{ manufacturer=$bb.Manufacturer; product=$bb.Product; serial=$bb.SerialNumber }
  licence = @{ name="$($lic.Name)"; description="$($lic.Description)"; status=$lic.LicenseStatus; reason=('0x{0:X8}' -f [uint32]$lic.LicenseStatusReason); grace_minutes=$lic.GracePeriodRemaining; rearms_left=$svc.RemainingWindowsReArmCount; firmware_key_present=[bool]$svc.OA3xOriginalProductKey }
  machine_guid = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Cryptography').MachineGuid
  volumes = @(Get-Volume | ForEach-Object { @{ letter="$($_.DriveLetter)"; label="$($_.FileSystemLabel)"; fs="$($_.FileSystem)"; type="$($_.DriveType)"; size=$_.Size; health="$($_.HealthStatus)" } })
  partitions = @(Get-Partition | ForEach-Object { @{ disk=$_.DiskNumber; number=$_.PartitionNumber; letter="$($_.DriveLetter)".Trim([char]0); type="$($_.Type)"; gpt_type="$($_.GptType)"; guid="$($_.Guid)"; size=$_.Size; offset=$_.Offset } })
  disks = @(Get-Disk | ForEach-Object { @{ number=$_.Number; name="$($_.FriendlyName)"; serial="$($_.SerialNumber)".Trim(); bus="$($_.BusType)"; guid="$($_.Guid)"; style="$($_.PartitionStyle)"; size=$_.Size; health="$($_.HealthStatus)" } })
  boot_drivers = @(Get-CimInstance Win32_SystemDriver -Filter "StartMode='Boot'" | ForEach-Object { $_.Name } | Sort-Object)
  devices_present = @(Get-PnpDevice -PresentOnly).Count
  devices_known = @(Get-PnpDevice).Count
  devices_with_problems = $bad
  system_events = $ev
  licensing_events = $spp
  tpm = @{ present=$tpm.TpmPresent; ready=$tpm.TpmReady; owned=$tpm.TpmOwned; maker="$($tpm.ManufacturerIdTxt)"; version="$($tpm.ManufacturerVersion)" }
  secure_boot = "$(try { Confirm-SecureBootUEFI -ErrorAction Stop } catch { 'not available' })"
  power = @{ sleep_states=((powercfg /a) -join ' | '); fast_startup=(Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power').HiberbootEnabled; hibernate_enabled=(Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Power').HibernateEnabled }
  firmware = @{ boot_device="$((Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control').FirmwareBootDevice)"; system_boot_device="$((Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control').SystemBootDevice)" }
  memory = @{ total_visible_kb=$os.TotalVisibleMemorySize; free_kb=$os.FreePhysicalMemory }
}
$r | ConvertTo-Json -Depth 6 -Compress
'''

# Windows' own boot timing records, newest first.
BOOT_TIMES = r'''
$ErrorActionPreference = 'SilentlyContinue'
$rows = @(Get-WinEvent -LogName 'Microsoft-Windows-Diagnostics-Performance/Operational' -FilterXPath '*[System[EventID=100]]' -MaxEvents 60 | ForEach-Object {
  $x = [xml]$_.ToXml(); $d = @{}
  foreach ($n in $x.Event.EventData.Data) { $d[$n.Name] = $n.'#text' }
  @{ time=$_.TimeCreated.ToUniversalTime().ToString('o'); boot_ms=$d.BootTime; main_ms=$d.MainPathBootTime; post_ms=$d.BootPostBootTime; start=$d.BootStartTime }
})
ConvertTo-Json -InputObject $rows -Depth 4 -Compress
'''

# Windows' own disk check of C:, online, changing nothing.
DISK_CHECK = r'''
$ErrorActionPreference = 'SilentlyContinue'
$scan = "$(Repair-Volume -DriveLetter C -Scan)"
$dirty = "$(fsutil dirty query C:)"
$boot = (Get-CimInstance Win32_OperatingSystem).LastBootUpTime
$ntfs = @(Get-WinEvent -FilterHashtable @{LogName='System'; StartTime=$boot} -MaxEvents 5000 | Where-Object { $_.ProviderName -match 'Ntfs|volmgr|disk|stornvme|storahci|volsnap|Chkdsk|Wininit' -and $_.Level -le 3 } | Group-Object ProviderName, Id | ForEach-Object { @{ k=$_.Name; n=$_.Count } })
$unexpected = @(Get-WinEvent -FilterHashtable @{LogName='System'; Id=6008,41,1001; StartTime=$boot} -MaxEvents 20 | ForEach-Object { "$($_.ProviderName)/$($_.Id)" })
@{ scan=$scan; dirty=$dirty; storage_warnings=$ntfs; unexpected_shutdown_events=$unexpected } | ConvertTo-Json -Depth 4 -Compress
'''

# The full chkdsk scan (minutes). Returns its last lines.
CHKDSK = r'''
$out = (chkdsk C: /scan 2>&1 | Out-String)
$lines = $out -split "`r?`n" | Where-Object { $_.Trim() }
@{ exit=$LASTEXITCODE; tail=($lines | Select-Object -Last 14) } | ConvertTo-Json -Depth 3 -Compress
'''

# Twenty small documents with known content, and their hashes.
MAKE_DOCS = r'''
New-Item -ItemType Directory -Force C:\dualrun\docs | Out-Null
1..20 | ForEach-Object { Set-Content -Path ("C:\dualrun\docs\doc{0:D2}.txt" -f $_) -Value ("document $_ " * 200) }
'ok'
'''

DOC_HASHES = r'''
$ErrorActionPreference = 'SilentlyContinue'
$rows = @(Get-ChildItem C:\dualrun\docs -File | Sort-Object Name | ForEach-Object { @{ name=$_.Name; sha=(Get-FileHash $_.FullName -Algorithm SHA256).Hash } })
@{ count=$rows.Count; files=$rows; marker=(Test-Path C:\dualrun\marker.txt); marker_text="$(Get-Content C:\dualrun\marker.txt -ErrorAction SilentlyContinue)"; public=(Test-Path C:\Users\Public\Documents) } | ConvertTo-Json -Depth 4 -Compress
'''

# A ransomware-style change, confined to the test's own folders plus one public folder.
DESTROY = r'''
$ErrorActionPreference = 'SilentlyContinue'
Get-ChildItem C:\dualrun\docs -File | ForEach-Object {
  $bytes = New-Object byte[] 4096; (New-Object Random).NextBytes($bytes)
  [IO.File]::WriteAllBytes($_.FullName, $bytes); Rename-Item $_.FullName ($_.Name + '.locked')
}
Remove-Item C:\Users\Public\Documents -Recurse -Force
Set-Content C:\dualrun\RANSOM-NOTE.txt 'all your files'
$big = New-Object byte[] (64MB); (New-Object Random).NextBytes($big)
1..4 | ForEach-Object { [IO.File]::WriteAllBytes("C:\dualrun\junk$($_).bin", $big) }
'destroyed'
'''

# A background writer for the power-cut tests: file data and metadata, without end.
START_LOAD = r'''
$code = @'
$i = 0
$buf = New-Object byte[] (4MB); (New-Object Random).NextBytes($buf)
New-Item -ItemType Directory -Force C:\dualrun\load | Out-Null
while ($true) {
  $i++
  [IO.File]::WriteAllBytes("C:\dualrun\load\big$($i % 40).bin", $buf)
  1..50 | ForEach-Object { Set-Content "C:\dualrun\load\small-$i-$($_).txt" "x$i" }
  if ($i % 3 -eq 0) { Get-ChildItem C:\dualrun\load -Filter 'small-*' | Select-Object -First 80 | Remove-Item -Force }
  New-ItemProperty -Path 'HKCU:\Software\TurDualrun' -Name "v$($i % 20)" -Value $i -Force | Out-Null
}
'@
New-Item -Path 'HKCU:\Software\TurDualrun' -Force | Out-Null
Set-Content C:\dualrun\load.ps1 $code
Start-Process powershell.exe -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','C:\dualrun\load.ps1' -WindowStyle Hidden
'started'
'''

CLEAN_LOAD = r'''
Get-Process powershell | Where-Object { $_.Id -ne $PID -and $_.MainWindowTitle -eq '' -and $_.StartTime -gt (Get-Date).AddHours(-12) } | Out-Null
Remove-Item C:\dualrun\load -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item C:\dualrun\junk*.bin -Force -ErrorAction SilentlyContinue
"left: $(@(Get-ChildItem C:\dualrun -ErrorAction SilentlyContinue).Count)"
'''

# TUR's window helper, compiled inside Windows from this source by Windows' own compiler.
WINDOW_HELPER = r'''
if (-not ('TurWin' -as [type])) {
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @"
using System; using System.Text; using System.Runtime.InteropServices; using System.Drawing; using System.Drawing.Imaging;
public static class TurWin {
  delegate bool EnumProc(IntPtr h, IntPtr l);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr h, uint cmd);
  [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")] static extern IntPtr GetWindowLongPtr(IntPtr h, int i);
  [DllImport("user32.dll")] static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
  [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int attr, out RECT r, int size);
  [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int attr, out int v, int size);
  static RECT Bounds(IntPtr h) { RECT r; if (DwmGetWindowAttribute(h, 9, out r, 16) != 0) GetWindowRect(h, out r); return r; }
  public static string List() {
    var sb = new StringBuilder(); int z = 0; IntPtr fg = GetForegroundWindow();
    EnumWindows(delegate(IntPtr h, IntPtr l) {
      if (!IsWindowVisible(h)) return true;
      int cloaked = 0; DwmGetWindowAttribute(h, 14, out cloaked, 4);
      RECT r = Bounds(h);
      var t = new StringBuilder(512); GetWindowText(h, t, 512);
      var c = new StringBuilder(256); GetClassName(h, c, 256);
      uint pid; GetWindowThreadProcessId(h, out pid);
      long ex = GetWindowLongPtr(h, -20).ToInt64();
      sb.Append(z++).Append('\t').Append(h.ToInt64()).Append('\t').Append(pid).Append('\t')
        .Append(r.L).Append('\t').Append(r.T).Append('\t').Append(r.R).Append('\t').Append(r.B).Append('\t')
        .Append(cloaked).Append('\t').Append(IsIconic(h) ? 1 : 0).Append('\t').Append(ex).Append('\t')
        .Append(GetWindow(h, 4).ToInt64()).Append('\t').Append(h == fg ? 1 : 0).Append('\t')
        .Append(c.ToString()).Append('\t').Append(t.ToString().Replace('\t', ' ').Replace('\n', ' ')).Append('\n');
      return true; }, IntPtr.Zero);
    return sb.ToString();
  }
  public static string Capture(long hwnd, string path) {
    IntPtr h = new IntPtr(hwnd); RECT w; GetWindowRect(h, out w);
    int width = w.R - w.L, height = w.B - w.T; if (width <= 0 || height <= 0 || width > 8000 || height > 8000) return "0\t0\t0\t0\t0\t0";
    var watch = System.Diagnostics.Stopwatch.StartNew();
    using (var bmp = new Bitmap(width, height, PixelFormat.Format32bppArgb)) {
      bool ok; using (var g = Graphics.FromImage(bmp)) { IntPtr dc = g.GetHdc(); ok = PrintWindow(h, dc, 2); g.ReleaseHdc(dc); }
      long ms = watch.ElapsedMilliseconds;
      int first = bmp.GetPixel(width / 2, height / 2).ToArgb(), differ = 0, n = 0;
      for (int y = 2; y < height; y += Math.Max(1, height / 40)) for (int x = 2; x < width; x += Math.Max(1, width / 40)) { n++; if (bmp.GetPixel(x, y).ToArgb() != first) differ++; }
      if (path != null) bmp.Save(path, ImageFormat.Png);
      return (ok ? 1 : 0) + "\t" + width + "\t" + height + "\t" + ms + "\t" + differ + "\t" + n;
    }
  }
  public static void Place(long hwnd, int x, int y, int cx, int cy) { SetWindowPos(new IntPtr(hwnd), IntPtr.Zero, x, y, cx, cy, 0x0044); }
  public static void Close(long hwnd) { PostMessage(new IntPtr(hwnd), 0x0010, IntPtr.Zero, IntPtr.Zero); }
  public static void Minimise(long hwnd) { ShowWindow(new IntPtr(hwnd), 6); }
  public static void Restore(long hwnd) { ShowWindow(new IntPtr(hwnd), 9); }
  public static void Front(long hwnd) { SetForegroundWindow(new IntPtr(hwnd)); }
}
"@
[void][TurWin]::SetProcessDPIAware()
}
'''

LIST_WINDOWS = WINDOW_HELPER + r'''
[TurWin]::List()
'''


def capture_window(hwnd: int) -> str:
    """Capture one window's own content and return 'stats<newline>base64 PNG'."""
    return WINDOW_HELPER + rf'''
$path = "C:\dualrun\capture-{hwnd}.png"
$stats = [TurWin]::Capture({hwnd}, $path)
$stats
[Convert]::ToBase64String([IO.File]::ReadAllBytes($path))
Remove-Item $path -Force
'''


def time_captures(hwnd: int, count: int = 10) -> str:
    return WINDOW_HELPER + rf'''
$ms = @(1..{count} | ForEach-Object {{ ([TurWin]::Capture({hwnd}, $null) -split "`t")[3] }})
$ms -join ','
'''
