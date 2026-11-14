# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# TurWinCompat: a prototype module that makes PowerShell 7 for Linux answer
# the way a Windows PC's PowerShell does, with real data from this computer.
#
#   - the Windows environment variables ($env:USERPROFILE, $env:TEMP, ...)
#   - drive letters: C: is the Windows environment's drive_c, other letters
#     as the environment maps them
#   - the aliases Windows PowerShell has and Linux PowerShell drops (ls, cat,
#     cp, mv, rm, ps, kill, sleep, sort, ...)
#   - Get-CimInstance for common Win32_ classes, from /proc, /sys and the
#     real network and disks
#   - Get-Service from systemd and from the Windows environment's services
#   - HKLM:/HKCU: paths in the item cmdlets, served by the Windows
#     environment's registry (through Wine's reg.exe)
#   - Get-NetIPAddress, Get-NetAdapter, Test-NetConnection, Resolve-DnsName,
#     Get-ComputerInfo, Get-LocalUser, Set-/Get-ExecutionPolicy
#   - the classic tools (ipconfig, tasklist, reg, cmd, ...) through
#     tur-winshell
#   - refusals, with the reason, for disk and boot cmdlets, and for a
#     recursive delete that would leave the Windows environment through a
#     mapped folder
#
# It is a spike: the item cmdlets are wrapped by plain functions, which is
# enough to measure with and not what a product would ship (a real registry
# provider would be compiled code). Nothing here runs on a development
# machine; CI runs it.

Set-StrictMode -Off

$script:Prefix = if ($env:TUR_WINPREFIX) { $env:TUR_WINPREFIX } else { Join-Path $HOME '.wine' }
$script:DriveC = Join-Path $script:Prefix 'drive_c'
$script:Wine = if ($env:TUR_WINE) { $env:TUR_WINE } else { 'wine' }
$script:Winshell = $env:TUR_WINSHELL
$script:User = [Environment]::UserName
$script:Letters = @()
$script:ExecutionPolicy = $null
$script:RegRx = '^(?i)(Microsoft\.PowerShell\.Core\\)?(Registry::)?(HKLM|HKCU|HKCR|HKU|HKCC|HKEY_LOCAL_MACHINE|HKEY_CURRENT_USER|HKEY_CLASSES_ROOT|HKEY_USERS|HKEY_CURRENT_CONFIG)(:|\\|/|$)'

function Write-TurNote([string]$Kind, [string]$Text) {
    # The marker tur-winshell and the test harness look for.
    [Console]::Error.WriteLine("tur-winshell: ${Kind}: $Text")
}

# ------------------------------------------------------------ files ----

function Get-TurChildName([string]$Dir, [string]$Name) {
    # The name as it is spelled on disk (the Windows environment's folders
    # are lower case on a case-sensitive file system).
    if (Microsoft.PowerShell.Management\Test-Path -LiteralPath (Join-Path $Dir $Name)) { return $Name }
    if (Microsoft.PowerShell.Management\Test-Path -LiteralPath $Dir) {
        foreach ($entry in [System.IO.Directory]::GetFileSystemEntries($Dir)) {
            $leaf = [System.IO.Path]::GetFileName($entry)
            if ($leaf -ieq $Name) { return $leaf }
        }
    }
    return $Name
}

function Initialize-TurEnvironment {
    $users = Get-TurChildName $script:DriveC 'Users'
    $windows = Get-TurChildName $script:DriveC 'Windows'
    $userDir = "C:\$users\$($script:User)"
    $values = [ordered]@{
        USERNAME               = $script:User
        USERPROFILE            = $userDir
        HOMEDRIVE              = 'C:'
        HOMEPATH               = "\$users\$($script:User)"
        APPDATA                = "$userDir\AppData\Roaming"
        LOCALAPPDATA           = "$userDir\AppData\Local"
        TEMP                   = "$userDir\AppData\Local\Temp"
        TMP                    = "$userDir\AppData\Local\Temp"
        COMPUTERNAME           = [Environment]::MachineName
        USERDOMAIN             = [Environment]::MachineName
        OS                     = 'Windows_NT'
        SystemDrive            = 'C:'
        SystemRoot             = "C:\$windows"
        windir                 = "C:\$windows"
        ComSpec                = "C:\$windows\system32\cmd.exe"
        ProgramFiles           = 'C:\Program Files'
        'ProgramFiles(x86)'    = 'C:\Program Files (x86)'
        ProgramData            = 'C:\ProgramData'
        PUBLIC                 = "C:\$users\Public"
        PATHEXT                = '.COM;.EXE;.BAT;.CMD;.VBS;.JS;.WSF;.MSC;.CPL'
        PROCESSOR_ARCHITECTURE = 'AMD64'
        NUMBER_OF_PROCESSORS   = [string][Environment]::ProcessorCount
        Path                   = "C:\$windows\system32;C:\$windows;C:\$windows\System32\Wbem"
    }
    foreach ($pair in $values.GetEnumerator()) {
        if (-not [Environment]::GetEnvironmentVariable($pair.Key)) {
            [Environment]::SetEnvironmentVariable($pair.Key, $pair.Value)
        }
    }
}

function Initialize-TurDrives {
    if (-not (Microsoft.PowerShell.Management\Test-Path -LiteralPath $script:DriveC)) { return }
    $map = [ordered]@{ C = $script:DriveC }
    $dos = Join-Path $script:Prefix 'dosdevices'
    if (Microsoft.PowerShell.Management\Test-Path -LiteralPath $dos) {
        foreach ($entry in [System.IO.Directory]::GetFileSystemEntries($dos)) {
            $leaf = [System.IO.Path]::GetFileName($entry)
            if ($leaf -match '^([a-z]):$' -and $leaf -ne 'c:') {
                $target = [System.IO.Directory]::ResolveLinkTarget($entry, $true)
                if ($target) { $map[$Matches[1].ToUpper()] = $target.FullName }
            }
        }
    }
    foreach ($pair in $map.GetEnumerator()) {
        if (-not (Microsoft.PowerShell.Management\Get-PSDrive -Name $pair.Key -ErrorAction SilentlyContinue)) {
            $null = Microsoft.PowerShell.Management\New-PSDrive -Name $pair.Key -PSProvider FileSystem `
                -Root $pair.Value -Scope Global -ErrorAction SilentlyContinue
        }
        $script:Letters += $pair.Key
    }
    # Start where the caller is, seen through the drive letter.
    $here = (Microsoft.PowerShell.Management\Get-Location).ProviderPath
    $root = $script:DriveC.TrimEnd('/')
    if ($here -eq $root -or $here.StartsWith("$root/")) {
        $rest = $here.Substring($root.Length).Replace('/', '\')
        Microsoft.PowerShell.Management\Set-Location -LiteralPath "C:$(if ($rest) { $rest } else { '\' })" `
            -ErrorAction SilentlyContinue
    }
}

function Initialize-TurAliases {
    $aliases = [ordered]@{
        ls = 'Get-ChildItem'; cat = 'Get-Content'; cp = 'Copy-Item'; mv = 'Move-Item'; rm = 'Remove-Item'
        rmdir = 'Remove-Item'; ps = 'Get-Process'; kill = 'Stop-Process'; sleep = 'Start-Sleep'
        sort = 'Sort-Object'; tee = 'Tee-Object'; diff = 'Compare-Object'; man = 'help'; mount = 'New-PSDrive'
        write = 'Write-Output'; clear = 'Clear-Host'; lp = 'Out-Printer'
    }
    foreach ($pair in $aliases.GetEnumerator()) {
        Microsoft.PowerShell.Utility\Set-Alias -Name $pair.Key -Value $pair.Value -Scope Global -Option AllScope -Force
    }
}

# --------------------------------------------------------- registry ----

function Find-TurRegArg($List) {
    foreach ($item in $List) {
        if ($item -is [string] -and $item -match $script:RegRx) { return $item }
    }
    return $null
}

function ConvertTo-TurRegKey([string]$Path) {
    $p = $Path -replace '^(?i)(Microsoft\.PowerShell\.Core\\)?(Registry::)?', ''
    $p = $p.Replace('/', '\')
    $names = @{
        HKLM = 'HKEY_LOCAL_MACHINE'; HKCU = 'HKEY_CURRENT_USER'; HKCR = 'HKEY_CLASSES_ROOT'
        HKU = 'HKEY_USERS'; HKCC = 'HKEY_CURRENT_CONFIG'
    }
    $head, $rest = $p -split '[:\\]', 2
    $hive = if ($names.ContainsKey($head.ToUpper())) { $names[$head.ToUpper()] } else { $head.ToUpper() }
    $short = ($names.GetEnumerator() | Microsoft.PowerShell.Core\Where-Object { $_.Value -eq $hive }).Key
    $sub = "$rest".Trim('\').TrimStart(':').Trim('\')
    [pscustomobject]@{
        Hive  = $hive
        Sub   = $sub
        Full  = if ($sub) { "$hive\$sub" } else { $hive }
        Drive = "${short}:\$sub"
        Leaf  = if ($sub) { ($sub -split '\\')[-1] } else { $hive }
    }
}

function Invoke-TurWineReg([string[]]$RegArgs) {
    $oldPrefix = $env:WINEPREFIX
    $oldDebug = $env:WINEDEBUG
    $env:WINEPREFIX = $script:Prefix
    $env:WINEDEBUG = '-all'
    try {
        $out = & $script:Wine reg.exe @RegArgs 2>$null
        $code = $LASTEXITCODE
    }
    catch {
        $out = @()
        $code = 1
    }
    finally {
        $env:WINEPREFIX = $oldPrefix
        $env:WINEDEBUG = $oldDebug
    }
    [pscustomobject]@{ Code = $code; Lines = @($out | Microsoft.PowerShell.Core\ForEach-Object { "$_".TrimEnd("`r") }) }
}

function ConvertFrom-TurRegData([string]$Type, [string]$Data) {
    switch ($Type) {
        'REG_DWORD' { $n = [Convert]::ToUInt32(($Data -replace '^0x', ''), 16); if ($n -le [int]::MaxValue) { return [int]$n } else { return $n } }
        'REG_QWORD' { return [Convert]::ToInt64(($Data -replace '^0x', ''), 16) }
        'REG_MULTI_SZ' { return , @($Data -split '\\0' | Microsoft.PowerShell.Core\Where-Object { $_ -ne '' }) }
        'REG_EXPAND_SZ' {
            return [regex]::Replace($Data, '%([^%]+)%', [System.Text.RegularExpressions.MatchEvaluator] {
                    param($m)
                    $v = [Environment]::GetEnvironmentVariable($m.Groups[1].Value)
                    if ($v) { $v } else { $m.Value }
                })
        }
        'REG_BINARY' {
            $bytes = [System.Collections.Generic.List[byte]]::new()
            for ($i = 0; $i + 1 -lt $Data.Length; $i += 2) { $bytes.Add([Convert]::ToByte($Data.Substring($i, 2), 16)) }
            return , $bytes.ToArray()
        }
        default { return $Data }
    }
}

function Read-TurRegKey($Key) {
    # One level: the key's values and the names of its subkeys.
    $r = Invoke-TurWineReg @('query', $Key.Full)
    if ($r.Code -ne 0) { return $null }
    $values = [ordered]@{}
    $subkeys = @()
    $seen = $false
    foreach ($line in $r.Lines) {
        if ($line -match '^\s{4}(.+?)\s{4}(REG_[A-Z_]+)(?:\s{4}(.*))?$') {
            $name = if ($Matches[1] -eq '(Default)') { '(default)' } else { $Matches[1] }
            $values[$name] = ConvertFrom-TurRegData $Matches[2] "$($Matches[3])"
        }
        elseif ($line -match '^HKEY_') {
            if (-not $seen) { $seen = $true } else { $subkeys += ($line -split '\\')[-1] }
        }
    }
    [pscustomobject]@{ Values = $values; SubKeys = $subkeys }
}

function ConvertFrom-TurArgs($List, [string[]]$Positional) {
    # Named and positional arguments of an item cmdlet, without binding.
    $known = 'Path', 'LiteralPath', 'Name', 'Value', 'PropertyType', 'Type', 'ItemType', 'ErrorAction',
    'Force', 'Recurse', 'PassThru', 'Confirm', 'WhatIf', 'Filter', 'Include', 'Exclude', 'Depth'
    $switches = 'Force', 'Recurse', 'PassThru', 'Confirm', 'WhatIf'
    $named = @{}
    $free = @()
    $i = 0
    while ($i -lt $List.Count) {
        $item = $List[$i]
        if ($item -is [string] -and $item -match '^-([A-Za-z]+)(:?)$') {
            $given = $Matches[1]
            $colon = $Matches[2]
            $full = $known | Microsoft.PowerShell.Core\Where-Object { $_ -like "$given*" } | Microsoft.PowerShell.Utility\Select-Object -First 1
            if ($given -ieq 'ea') { $full = 'ErrorAction' }
            if (-not $full) { $full = $given }
            if ($switches -contains $full -and -not $colon) { $named[$full] = $true }
            else { $i++; $named[$full] = $List[$i] }
        }
        else { $free += , $item }
        $i++
    }
    if ($named.ContainsKey('LiteralPath') -and -not $named.ContainsKey('Path')) { $named['Path'] = $named['LiteralPath'] }
    foreach ($name in $Positional) {
        if (-not $named.ContainsKey($name) -and $free.Count -gt 0) {
            $named[$name] = $free[0]
            $free = @($free | Microsoft.PowerShell.Utility\Select-Object -Skip 1)
        }
    }
    $named
}

function Write-TurRegError($Bound, [string]$Text) {
    $action = "$($Bound['ErrorAction'])"
    if ($action -match '^(?i)(SilentlyContinue|Ignore|0|4)$') { return }
    Microsoft.PowerShell.Utility\Write-Error -Message $Text -Category ObjectNotFound
}

function New-TurRegItem($Key, $Data) {
    $item = [pscustomobject]@{
        Name          = $Key.Full
        PSChildName   = $Key.Leaf
        PSPath        = "Microsoft.PowerShell.Core\Registry::$($Key.Full)"
        PSProvider    = 'Registry'
        PSIsContainer = $true
        SubKeyCount   = @($Data.SubKeys).Count
        ValueCount    = $Data.Values.Count
        Property      = @($Data.Values.Keys)
    }
    $values = $Data.Values
    $subkeys = $Data.SubKeys
    $getValue = { param($n) if ("$n" -eq '') { $n = '(default)' }; $values[$n] }.GetNewClosure()
    $getValueNames = { @($values.Keys) }.GetNewClosure()
    $getSubKeyNames = { @($subkeys) }.GetNewClosure()
    $item | Microsoft.PowerShell.Utility\Add-Member -MemberType ScriptMethod -Name GetValue -Value $getValue
    $item | Microsoft.PowerShell.Utility\Add-Member -MemberType ScriptMethod -Name GetValueNames -Value $getValueNames
    $item | Microsoft.PowerShell.Utility\Add-Member -MemberType ScriptMethod -Name GetSubKeyNames -Value $getSubKeyNames
    $item
}

function ConvertTo-TurRegType($Bound) {
    $given = "$($Bound['PropertyType'])$($Bound['Type'])"
    switch -Regex ($given) {
        '^(?i)string$' { return 'REG_SZ' }
        '^(?i)expandstring$' { return 'REG_EXPAND_SZ' }
        '^(?i)dword$' { return 'REG_DWORD' }
        '^(?i)qword$' { return 'REG_QWORD' }
        '^(?i)multistring$' { return 'REG_MULTI_SZ' }
        '^(?i)binary$' { return 'REG_BINARY' }
    }
    $value = $Bound['Value']
    if ($value -is [int] -or $value -is [uint32]) { return 'REG_DWORD' }
    if ($value -is [long]) { return 'REG_QWORD' }
    if ($value -is [array]) { return 'REG_MULTI_SZ' }
    'REG_SZ'
}

function Invoke-TurRegistry([string]$Cmdlet, $List) {
    # The item cmdlets on a registry path, against the Windows environment's
    # registry. Writes can only ever reach that registry.
    switch ($Cmdlet) {
        'Test-Path' {
            $b = ConvertFrom-TurArgs $List @('Path')
            return ((Invoke-TurWineReg @('query', (ConvertTo-TurRegKey $b['Path']).Full)).Code -eq 0)
        }
        'Get-ItemProperty' {
            $b = ConvertFrom-TurArgs $List @('Path', 'Name')
            $key = ConvertTo-TurRegKey $b['Path']
            $data = Read-TurRegKey $key
            if (-not $data) { Write-TurRegError $b "Cannot find path '$($b['Path'])' because it does not exist."; return }
            $out = [ordered]@{}
            foreach ($pair in $data.Values.GetEnumerator()) {
                if (-not $b['Name'] -or @($b['Name']) -contains $pair.Key) { $out[$pair.Key] = $pair.Value }
            }
            if ($b['Name'] -and $out.Count -eq 0) {
                Write-TurRegError $b "Property $($b['Name']) does not exist at path $($key.Full)."
                return
            }
            $out['PSPath'] = "Microsoft.PowerShell.Core\Registry::$($key.Full)"
            $out['PSChildName'] = $key.Leaf
            $out['PSProvider'] = 'Registry'
            return [pscustomobject]$out
        }
        'Get-ItemPropertyValue' {
            $b = ConvertFrom-TurArgs $List @('Path', 'Name')
            $key = ConvertTo-TurRegKey $b['Path']
            $data = Read-TurRegKey $key
            if (-not $data) { Write-TurRegError $b "Cannot find path '$($b['Path'])' because it does not exist."; return }
            foreach ($name in @($b['Name'])) {
                if ($data.Values.Contains($name)) { $data.Values[$name] }
                else { Write-TurRegError $b "Property $name does not exist at path $($key.Full)." }
            }
            return
        }
        'Get-Item' {
            $b = ConvertFrom-TurArgs $List @('Path')
            $key = ConvertTo-TurRegKey $b['Path']
            $data = Read-TurRegKey $key
            if (-not $data) { Write-TurRegError $b "Cannot find path '$($b['Path'])' because it does not exist."; return }
            return (New-TurRegItem $key $data)
        }
        'Get-ChildItem' {
            $b = ConvertFrom-TurArgs $List @('Path')
            $key = ConvertTo-TurRegKey $b['Path']
            $data = Read-TurRegKey $key
            if (-not $data) { Write-TurRegError $b "Cannot find path '$($b['Path'])' because it does not exist."; return }
            foreach ($name in $data.SubKeys) {
                $child = ConvertTo-TurRegKey "$($key.Full)\$name"
                $childData = Read-TurRegKey $child
                if ($childData) { New-TurRegItem $child $childData }
            }
            return
        }
        'New-Item' {
            $b = ConvertFrom-TurArgs $List @('Path')
            $key = ConvertTo-TurRegKey $b['Path']
            if ($b['Name']) { $key = ConvertTo-TurRegKey "$($key.Full)\$($b['Name'])" }
            $r = Invoke-TurWineReg @('add', $key.Full, '/f')
            if ($r.Code -ne 0) { Write-TurRegError $b "Could not create $($key.Full)."; return }
            return (New-TurRegItem $key (Read-TurRegKey $key))
        }
        { $_ -in 'New-ItemProperty', 'Set-ItemProperty' } {
            $b = ConvertFrom-TurArgs $List @('Path', 'Name', 'Value')
            $key = ConvertTo-TurRegKey $b['Path']
            $type = ConvertTo-TurRegType $b
            $value = $b['Value']
            $text = if ($type -eq 'REG_MULTI_SZ') { @($value) -join '\0' }
            elseif ($type -eq 'REG_BINARY') { -join (@($value) | Microsoft.PowerShell.Core\ForEach-Object { '{0:x2}' -f [int]$_ }) }
            else { "$value" }
            $r = Invoke-TurWineReg @('add', $key.Full, '/v', "$($b['Name'])", '/t', $type, '/d', $text, '/f')
            if ($r.Code -ne 0) { Write-TurRegError $b "Could not set $($b['Name']) at $($key.Full)."; return }
            if ($Cmdlet -eq 'New-ItemProperty') {
                return (Invoke-TurRegistry 'Get-ItemProperty' @($key.Full, $b['Name']))
            }
            return
        }
        'Remove-Item' {
            $b = ConvertFrom-TurArgs $List @('Path')
            $key = ConvertTo-TurRegKey $b['Path']
            if (-not $key.Sub) { Write-TurRegError @{} 'A whole registry hive is not removed.'; return }
            $r = Invoke-TurWineReg @('delete', $key.Full, '/f')
            if ($r.Code -ne 0) { Write-TurRegError $b "Cannot find path '$($b['Path'])' because it does not exist." }
            return
        }
        'Remove-ItemProperty' {
            $b = ConvertFrom-TurArgs $List @('Path', 'Name')
            $key = ConvertTo-TurRegKey $b['Path']
            $r = Invoke-TurWineReg @('delete', $key.Full, '/v', "$($b['Name'])", '/f')
            if ($r.Code -ne 0) { Write-TurRegError $b "Property $($b['Name']) does not exist at path $($key.Full)." }
            return
        }
    }
}

function Test-TurLeavesEnvironment($List) {
    # A recursive delete given through a drive letter whose target is not
    # inside the Windows environment's own drive: a mapped folder.
    $b = ConvertFrom-TurArgs $List @('Path')
    if (-not $b['Recurse']) { return $null }
    if ($env:TUR_WINSHELL_CONFIRM -eq 'yes') { return $null }
    $inside = (& realpath -m -- $script:DriveC 2>$null)
    foreach ($path in @($b['Path'])) {
        if (-not $path) { continue }
        $provider = $null
        $drive = $null
        try { $real = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath("$path", [ref]$provider, [ref]$drive) }
        catch { continue }
        if (-not $drive -or $script:Letters -notcontains $drive.Name) { continue }
        $target = (& realpath -m -- $real 2>$null)
        if ($target -and $inside -and $target -ne $inside -and -not "$target".StartsWith("$inside/")) {
            return "$path is a folder of this computer ($target) seen from the Windows environment. " +
            'Deleting it with everything in it is not done from a Windows command without your confirmation.'
        }
        if ($target -eq $inside) {
            return "$path is the whole Windows environment's drive."
        }
    }
    return $null
}

# Plain functions over the item cmdlets: a registry path goes to the Windows
# environment's registry, anything else to the real cmdlet unchanged.
function Get-ItemProperty {
    if ($null -ne (Find-TurRegArg $args)) { return (Invoke-TurRegistry 'Get-ItemProperty' $args) }
    if ($MyInvocation.ExpectingInput) { $input | Microsoft.PowerShell.Management\Get-ItemProperty @args }
    else { Microsoft.PowerShell.Management\Get-ItemProperty @args }
}
function Get-ItemPropertyValue {
    if ($null -ne (Find-TurRegArg $args)) { return (Invoke-TurRegistry 'Get-ItemPropertyValue' $args) }
    if ($MyInvocation.ExpectingInput) { $input | Microsoft.PowerShell.Management\Get-ItemPropertyValue @args }
    else { Microsoft.PowerShell.Management\Get-ItemPropertyValue @args }
}
function Get-Item {
    if ($null -ne (Find-TurRegArg $args)) { return (Invoke-TurRegistry 'Get-Item' $args) }
    if ($MyInvocation.ExpectingInput) { $input | Microsoft.PowerShell.Management\Get-Item @args }
    else { Microsoft.PowerShell.Management\Get-Item @args }
}
function Get-ChildItem {
    if ($null -ne (Find-TurRegArg $args)) { return (Invoke-TurRegistry 'Get-ChildItem' $args) }
    if ($MyInvocation.ExpectingInput) { $input | Microsoft.PowerShell.Management\Get-ChildItem @args }
    else { Microsoft.PowerShell.Management\Get-ChildItem @args }
}
function Test-Path {
    if ($null -ne (Find-TurRegArg $args)) { return (Invoke-TurRegistry 'Test-Path' $args) }
    if ($MyInvocation.ExpectingInput) { $input | Microsoft.PowerShell.Management\Test-Path @args }
    else { Microsoft.PowerShell.Management\Test-Path @args }
}
function New-Item {
    if ($null -ne (Find-TurRegArg $args)) { return (Invoke-TurRegistry 'New-Item' $args) }
    if ($MyInvocation.ExpectingInput) { $input | Microsoft.PowerShell.Management\New-Item @args }
    else { Microsoft.PowerShell.Management\New-Item @args }
}
function New-ItemProperty {
    if ($null -ne (Find-TurRegArg $args)) { return (Invoke-TurRegistry 'New-ItemProperty' $args) }
    Microsoft.PowerShell.Management\New-ItemProperty @args
}
function Set-ItemProperty {
    if ($null -ne (Find-TurRegArg $args)) { return (Invoke-TurRegistry 'Set-ItemProperty' $args) }
    if ($MyInvocation.ExpectingInput) { $input | Microsoft.PowerShell.Management\Set-ItemProperty @args }
    else { Microsoft.PowerShell.Management\Set-ItemProperty @args }
}
function Remove-ItemProperty {
    if ($null -ne (Find-TurRegArg $args)) { return (Invoke-TurRegistry 'Remove-ItemProperty' $args) }
    Microsoft.PowerShell.Management\Remove-ItemProperty @args
}
function Remove-Item {
    if ($null -ne (Find-TurRegArg $args)) { return (Invoke-TurRegistry 'Remove-Item' $args) }
    $why = Test-TurLeavesEnvironment $args
    if ($why) {
        Write-TurNote 'refused' $why
        Microsoft.PowerShell.Utility\Write-Error -Message "Remove-Item: refused by TUR. $why" -Category PermissionDenied
        return
    }
    if ($MyInvocation.ExpectingInput) { $input | Microsoft.PowerShell.Management\Remove-Item @args }
    else { Microsoft.PowerShell.Management\Remove-Item @args }
}

# -------------------------------------------------------------- CIM ----

function Read-TurFile([string]$Path) {
    try { return ([System.IO.File]::ReadAllText($Path)).Trim() } catch { return '' }
}

function Get-TurMemInfo {
    $info = @{}
    foreach ($line in [System.IO.File]::ReadAllLines('/proc/meminfo')) {
        if ($line -match '^(\w+):\s+(\d+)') { $info[$Matches[1]] = [long]$Matches[2] }
    }
    $info
}

function Get-TurIpJson([string[]]$IpArgs) {
    try {
        $text = & ip -j @IpArgs 2>$null
        if ($LASTEXITCODE -eq 0 -and $text) { return @($text | Microsoft.PowerShell.Utility\ConvertFrom-Json) }
    }
    catch { }
    return @()
}

function Get-TurCimOperatingSystem {
    $release = @{}
    foreach ($line in [System.IO.File]::ReadAllLines('/etc/os-release')) {
        if ($line -match '^(\w+)=(.*)$') { $release[$Matches[1]] = $Matches[2].Trim('"') }
    }
    $mem = Get-TurMemInfo
    $uptime = [double]((Read-TurFile '/proc/uptime') -split '\s+')[0]
    $kernel = Read-TurFile '/proc/sys/kernel/osrelease'
    $version = if ($kernel -match '^(\d+\.\d+(\.\d+)?)') { $Matches[1] } else { $kernel }
    $windows = Get-TurChildName $script:DriveC 'Windows'
    [pscustomobject]@{
        Caption                = $release['PRETTY_NAME']
        Version                = $version
        BuildNumber            = $kernel
        OSArchitecture         = '64-bit'
        CSName                 = [Environment]::MachineName
        Manufacturer           = $release['NAME']
        LastBootUpTime         = (Microsoft.PowerShell.Utility\Get-Date).AddSeconds(-$uptime)
        LocalDateTime          = (Microsoft.PowerShell.Utility\Get-Date)
        TotalVisibleMemorySize = $mem['MemTotal']
        FreePhysicalMemory     = $mem['MemAvailable']
        TotalVirtualMemorySize = $mem['MemTotal'] + $mem['SwapTotal']
        FreeVirtualMemory      = $mem['MemAvailable'] + $mem['SwapFree']
        NumberOfProcesses      = @([System.IO.Directory]::GetDirectories('/proc') | Microsoft.PowerShell.Core\Where-Object { $_ -match '/\d+$' }).Count
        RegisteredUser         = $script:User
        SystemDirectory        = "C:\$windows\system32"
        WindowsDirectory       = "C:\$windows"
        SystemDrive            = 'C:'
        OSType                 = 36
        SerialNumber           = ''
    }
}

function Get-TurCpuInfo {
    $cpus = @()
    $current = @{}
    foreach ($line in [System.IO.File]::ReadAllLines('/proc/cpuinfo')) {
        if ($line -match '^([^:]+?)\s*:\s*(.*)$') { $current[$Matches[1]] = $Matches[2] }
        elseif ($line.Trim() -eq '' -and $current.Count) { $cpus += , $current; $current = @{} }
    }
    if ($current.Count) { $cpus += , $current }
    $cpus
}

function Get-TurCimProcessor {
    $n = 0
    foreach ($group in (Get-TurCpuInfo | Microsoft.PowerShell.Utility\Group-Object { $_['physical id'] })) {
        $first = $group.Group[0]
        [pscustomobject]@{
            DeviceID                  = "CPU$n"
            Name                      = $first['model name']
            Manufacturer              = $first['vendor_id']
            NumberOfCores             = if ($first['cpu cores']) { [int]$first['cpu cores'] } else { $group.Count }
            NumberOfLogicalProcessors = $group.Count
            MaxClockSpeed             = [int][double]$first['cpu MHz']
            AddressWidth              = 64
            Architecture              = 9
            Caption                   = $first['model name']
        }
        $n++
    }
}

function Get-TurCimComputerSystem {
    $mem = Get-TurMemInfo
    $cpus = Get-TurCpuInfo
    [pscustomobject]@{
        Name                      = [Environment]::MachineName
        Manufacturer              = Read-TurFile '/sys/class/dmi/id/sys_vendor'
        Model                     = Read-TurFile '/sys/class/dmi/id/product_name'
        TotalPhysicalMemory       = $mem['MemTotal'] * 1024
        NumberOfProcessors        = @($cpus | Microsoft.PowerShell.Utility\Group-Object { $_['physical id'] }).Count
        NumberOfLogicalProcessors = @($cpus).Count
        Domain                    = 'WORKGROUP'
        UserName                  = "$([Environment]::MachineName)\$($script:User)"
        SystemType                = 'x64-based PC'
        PrimaryOwnerName          = $script:User
    }
}

function Get-TurCimBios {
    [pscustomobject]@{
        Manufacturer      = Read-TurFile '/sys/class/dmi/id/bios_vendor'
        SMBIOSBIOSVersion = Read-TurFile '/sys/class/dmi/id/bios_version'
        Version           = Read-TurFile '/sys/class/dmi/id/bios_version'
        ReleaseDate       = Read-TurFile '/sys/class/dmi/id/bios_date'
        SerialNumber      = Read-TurFile '/sys/class/dmi/id/product_serial'
        Name              = Read-TurFile '/sys/class/dmi/id/bios_version'
    }
}

function Get-TurCimLogicalDisk {
    foreach ($letter in $script:Letters) {
        $drive = Microsoft.PowerShell.Management\Get-PSDrive -Name $letter -ErrorAction SilentlyContinue
        if (-not $drive) { continue }
        $line = @(& df -PkT -- $drive.Root 2>$null)[-1]
        $f = "$line" -split '\s+'
        if ($f.Count -lt 6) { continue }
        [pscustomobject]@{
            DeviceID    = "${letter}:"
            DriveType   = 3
            FileSystem  = $f[1]
            Size        = [long]$f[2] * 1024
            FreeSpace   = [long]$f[4] * 1024
            VolumeName  = ''
            Name        = "${letter}:"
            Description = 'Local Fixed Disk'
        }
    }
}

function Get-TurCimNetworkAdapterConfiguration {
    $routes = Get-TurIpJson @('route', 'show', 'default')
    $dns = @()
    if (Microsoft.PowerShell.Management\Test-Path -LiteralPath '/etc/resolv.conf') {
        foreach ($line in [System.IO.File]::ReadAllLines('/etc/resolv.conf')) {
            if ($line -match '^\s*nameserver\s+(\S+)') { $dns += $Matches[1] }
        }
    }
    foreach ($link in (Get-TurIpJson @('addr', 'show'))) {
        if ($link.ifname -eq 'lo') { continue }
        $v4 = @($link.addr_info | Microsoft.PowerShell.Core\Where-Object { $_.family -eq 'inet' })
        $all = @($link.addr_info | Microsoft.PowerShell.Core\ForEach-Object { $_.local })
        $gateway = @($routes | Microsoft.PowerShell.Core\Where-Object { $_.dev -eq $link.ifname } | Microsoft.PowerShell.Core\ForEach-Object { $_.gateway })
        [pscustomobject]@{
            Description          = $link.ifname
            Index                = $link.ifindex
            InterfaceIndex       = $link.ifindex
            IPEnabled            = ($v4.Count -gt 0)
            IPAddress            = $all
            IPSubnet             = @($link.addr_info | Microsoft.PowerShell.Core\ForEach-Object { $_.prefixlen })
            MACAddress           = "$($link.address)".ToUpper()
            DefaultIPGateway     = $gateway
            DNSServerSearchOrder = $dns
            DHCPEnabled          = [bool](@($v4 | Microsoft.PowerShell.Core\Where-Object { $_.dynamic }).Count)
            DNSHostName          = [Environment]::MachineName
        }
    }
}

function Get-TurCimProcess {
    foreach ($proc in [System.Diagnostics.Process]::GetProcesses()) {
        $name = $proc.ProcessName
        $line = ''
        try { $line = ([System.IO.File]::ReadAllText("/proc/$($proc.Id)/cmdline")).Replace("`0", ' ').Trim() } catch { }
        # A Windows program keeps its image name as its first argument.
        if ($line -match '^(?:[A-Za-z]:\\[^ ]*\\)?([^\\/ ]+\.exe)\b') { $name = $Matches[1] }
        $path = $null
        try { $path = $proc.MainModule.FileName } catch { }
        $threads = 0
        try { $threads = $proc.Threads.Count } catch { }
        [pscustomobject]@{
            Name            = $name
            ProcessId       = $proc.Id
            ParentProcessId = $null
            CommandLine     = $line
            ExecutablePath  = $path
            WorkingSetSize  = $proc.WorkingSet64
            ThreadCount     = $threads
            Handle          = [string]$proc.Id
        }
    }
}

function Get-TurCimVideoController {
    if (-not (Microsoft.PowerShell.Management\Test-Path -LiteralPath '/sys/class/drm')) { return }
    foreach ($card in [System.IO.Directory]::GetDirectories('/sys/class/drm')) {
        if ([System.IO.Path]::GetFileName($card) -notmatch '^card\d+$') { continue }
        $driver = ''
        try { $driver = [System.IO.Path]::GetFileName([System.IO.Directory]::ResolveLinkTarget("$card/device/driver", $true).FullName) } catch { }
        [pscustomobject]@{
            Name          = if ($driver) { $driver } else { [System.IO.Path]::GetFileName($card) }
            DeviceID      = [System.IO.Path]::GetFileName($card)
            PNPDeviceID   = "$(Read-TurFile "$card/device/vendor"):$(Read-TurFile "$card/device/device")"
            DriverVersion = ''
        }
    }
}

function ConvertTo-TurWhere([string]$Wql) {
    # A WQL condition as a PowerShell filter: Prop = 'x' AND Other > 3.
    $pattern = '(?i)\b([A-Za-z_][A-Za-z0-9_]*)\s*(<>|!=|>=|<=|=|>|<|\bLIKE\b)\s*(''[^'']*''|"[^"]*"|[A-Za-z0-9_.+-]+)'
    $expr = [regex]::Replace($Wql, $pattern, [System.Text.RegularExpressions.MatchEvaluator] {
            param($m)
            $prop = $m.Groups[1].Value
            $op = switch ($m.Groups[2].Value.ToUpper()) {
                '=' { '-eq' } '<>' { '-ne' } '!=' { '-ne' } '>' { '-gt' } '<' { '-lt' } '>=' { '-ge' } '<=' { '-le' } 'LIKE' { '-like' }
            }
            $raw = $m.Groups[3].Value
            if ($raw -match '^[''"](.*)[''"]$') {
                $text = $Matches[1].Replace("'", "''")
                if ($op -eq '-like') { $text = $text.Replace('%', '*').Replace('_', '?') }
                $value = "'$text'"
            }
            elseif ($raw -match '^(?i)true$') { $value = '$true' }
            elseif ($raw -match '^(?i)false$') { $value = '$false' }
            else { $value = $raw }
            "(`$_.'$prop' $op $value)"
        })
    $expr = $expr -replace '(?i)\)\s*AND\s*\(', ') -and (' -replace '(?i)\)\s*OR\s*\(', ') -or ('
    [scriptblock]::Create($expr)
}

function Get-CimInstance {
    [CmdletBinding()]
    param(
        [Parameter(Position = 0)][Alias('Class')][string]$ClassName,
        [string]$Filter,
        [string]$Query,
        [string]$Namespace = 'root/cimv2',
        [string[]]$Property,
        [Alias('CN')][string[]]$ComputerName
    )
    if ($Query -and $Query -match '(?is)^\s*SELECT\s+(.+?)\s+FROM\s+(\w+)(?:\s+WHERE\s+(.+))?\s*$') {
        $ClassName = $Matches[2]
        if ($Matches[3]) { $Filter = $Matches[3] }
        if ($Matches[1].Trim() -ne '*') { $Property = $Matches[1] -split '\s*,\s*' }
    }
    $items = switch ($ClassName.ToLower()) {
        'win32_operatingsystem' { Get-TurCimOperatingSystem }
        'win32_computersystem' { Get-TurCimComputerSystem }
        'win32_processor' { Get-TurCimProcessor }
        'win32_bios' { Get-TurCimBios }
        'win32_logicaldisk' { Get-TurCimLogicalDisk }
        'win32_networkadapterconfiguration' { Get-TurCimNetworkAdapterConfiguration }
        'win32_process' { Get-TurCimProcess }
        'win32_videocontroller' { Get-TurCimVideoController }
        'win32_service' {
            Get-TurServices | Microsoft.PowerShell.Core\ForEach-Object {
                [pscustomobject]@{
                    Name = $_.Name; DisplayName = $_.DisplayName; State = $_.Status
                    StartMode = ($_.StartType -replace '^Automatic$', 'Auto'); Started = ($_.Status -eq 'Running')
                    PathName = $_.BinaryPathName; ProcessId = $_.ProcessId
                }
            }
        }
        default {
            $text = "TUR's CIM answers do not cover the class '$ClassName' yet."
            Write-TurNote 'explain' $text
            $PSCmdlet.WriteError([System.Management.Automation.ErrorRecord]::new(
                    [System.NotSupportedException]::new($text), 'TurCimClassNotCovered',
                    [System.Management.Automation.ErrorCategory]::NotImplemented, $ClassName))
            return
        }
    }
    if ($Filter) { $items = $items | Microsoft.PowerShell.Core\Where-Object (ConvertTo-TurWhere $Filter) }
    $items
}

# --------------------------------------------------------- services ----

function Get-TurServices {
    $found = @()
    $states = @{}
    try {
        foreach ($line in (& systemctl list-unit-files --type=service --no-legend --no-pager 2>$null)) {
            $f = "$line".Trim() -split '\s+'
            if ($f.Count -ge 2) { $states[$f[0]] = $f[1] }
        }
        foreach ($line in (& systemctl list-units --type=service --all --no-legend --plain --no-pager 2>$null)) {
            $f = "$line".Trim() -split '\s+', 5
            if ($f.Count -lt 4 -or $f[0] -notmatch '\.service$') { continue }
            $start = switch -Regex ("$($states[$f[0]])") {
                '^(enabled|enabled-runtime|static|indirect|generated|alias)$' { 'Automatic' }
                '^(disabled|masked)$' { 'Disabled' }
                default { 'Manual' }
            }
            $found += [pscustomobject]@{
                PSTypeName = 'Tur.Service'; Status = if ($f[3] -eq 'running') { 'Running' } else { 'Stopped' }
                Name = ($f[0] -replace '\.service$', ''); DisplayName = "$($f[4])"; StartType = $start
                ServiceType = 'Win32OwnProcess'; Source = 'linux'; BinaryPathName = $null; ProcessId = $null
            }
        }
    }
    catch { }
    # The Windows environment's own services, from its registry. Running is
    # judged by a Windows process of that image being alive.
    $images = @{}
    foreach ($dir in [System.IO.Directory]::GetDirectories('/proc')) {
        if ($dir -notmatch '/\d+$') { continue }
        try { $first = ([System.IO.File]::ReadAllText("$dir/cmdline") -split "`0")[0] } catch { continue }
        if ($first -match '([^\\/]+\.exe)$') { $images[$Matches[1].ToLower()] = $true }
    }
    $r = Invoke-TurWineReg @('query', 'HKLM\System\CurrentControlSet\Services', '/s')
    $name = $null
    $props = @{}
    $emit = {
        if ($name -and $props.ContainsKey('Type') -and (([Convert]::ToInt32($props['Type'], 16)) -band 0x30)) {
            $image = "$($props['ImagePath'])"
            $exe = if ($image -match '([^\\/" ]+\.exe)') { $Matches[1].ToLower() } else { '' }
            $start = switch ([Convert]::ToInt32("0$($props['Start'] -replace '^0x', '')", 16)) { 2 { 'Automatic' } 3 { 'Manual' } 4 { 'Disabled' } default { 'Automatic' } }
            [pscustomobject]@{
                PSTypeName = 'Tur.Service'; Status = if ($exe -and $images[$exe]) { 'Running' } else { 'Stopped' }
                Name = $name; DisplayName = if ($props['DisplayName']) { $props['DisplayName'] } else { $name }
                StartType = $start; ServiceType = 'Win32OwnProcess'; Source = 'windows'; BinaryPathName = $image
                ProcessId = $null
            }
        }
    }
    foreach ($line in $r.Lines) {
        if ($line -match '^HKEY_LOCAL_MACHINE\\System\\CurrentControlSet\\Services\\([^\\]+)(\\.*)?$') {
            if ($Matches[2]) { continue }
            $one = & $emit
            if ($one) { $found += $one }
            $name = $Matches[1]
            $props = @{}
        }
        elseif ($line -match '^\s{4}(.+?)\s{4}(REG_[A-Z_]+)(?:\s{4}(.*))?$' -and $name) {
            if (-not $props.ContainsKey($Matches[1])) { $props[$Matches[1]] = ("$($Matches[3])" -replace '^0x', '') }
        }
    }
    $one = & $emit
    if ($one) { $found += $one }
    $found | Microsoft.PowerShell.Utility\Sort-Object Name
}

function Get-Service {
    [CmdletBinding()]
    param(
        [Parameter(Position = 0, ValueFromPipeline = $true)][Alias('ServiceName')][string[]]$Name,
        [string[]]$DisplayName
    )
    begin { $all = @(Get-TurServices) }
    process {
        if (-not $Name -and -not $DisplayName) { return $all }
        foreach ($wanted in @($Name)) {
            if (-not $wanted) { continue }
            $hits = @($all | Microsoft.PowerShell.Core\Where-Object { $_.Name -like $wanted })
            if (-not $hits.Count -and $wanted -notmatch '[*?\[]') {
                $text = "Cannot find any service with service name '$wanted'."
                $PSCmdlet.WriteError([System.Management.Automation.ErrorRecord]::new(
                        [System.InvalidOperationException]::new($text), 'NoServiceFoundForGivenName',
                        [System.Management.Automation.ErrorCategory]::ObjectNotFound, $wanted))
            }
            $hits
        }
        foreach ($wanted in @($DisplayName)) {
            if ($wanted) { $all | Microsoft.PowerShell.Core\Where-Object { $_.DisplayName -like $wanted } }
        }
    }
}

# ---------------------------------------------------------- network ----

function Get-NetIPAddress {
    [CmdletBinding()]
    param([string]$AddressFamily, [string]$InterfaceAlias)
    foreach ($link in (Get-TurIpJson @('addr', 'show'))) {
        foreach ($addr in $link.addr_info) {
            $family = if ($addr.family -eq 'inet') { 'IPv4' } else { 'IPv6' }
            if ($AddressFamily -and $AddressFamily -ne $family) { continue }
            if ($InterfaceAlias -and $link.ifname -notlike $InterfaceAlias) { continue }
            [pscustomobject]@{
                IPAddress = $addr.local; InterfaceAlias = $link.ifname; InterfaceIndex = $link.ifindex
                AddressFamily = $family; PrefixLength = $addr.prefixlen
                PrefixOrigin = if ($addr.dynamic) { 'Dhcp' } else { 'Manual' }
            }
        }
    }
}

function Get-NetAdapter {
    [CmdletBinding()]
    param([Parameter(Position = 0)][string]$Name = '*')
    foreach ($link in (Get-TurIpJson @('link', 'show'))) {
        if ($link.ifname -eq 'lo' -or $link.ifname -notlike $Name) { continue }
        $speed = Read-TurFile "/sys/class/net/$($link.ifname)/speed"
        [pscustomobject]@{
            PSTypeName = 'Tur.NetAdapter'; Name = $link.ifname; InterfaceDescription = $link.ifname
            ifIndex = $link.ifindex; Status = if ($link.operstate -eq 'DOWN') { 'Disconnected' } else { 'Up' }
            MacAddress = "$($link.address)".ToUpper().Replace(':', '-')
            LinkSpeed = if ($speed -match '^\d+$') { "$speed Mbps" } else { '' }
        }
    }
}

function Test-NetConnection {
    [CmdletBinding()]
    param([Parameter(Position = 0)][Alias('RemoteAddress', 'cn')][string]$ComputerName = 'internetbeacon.msedge.net',
        [Alias('RemotePort')][int]$Port)
    $address = $null
    try { $address = [System.Net.Dns]::GetHostAddresses($ComputerName) | Microsoft.PowerShell.Utility\Select-Object -First 1 } catch { }
    $result = [ordered]@{ ComputerName = $ComputerName; RemoteAddress = $address }
    if ($Port) {
        $ok = $false
        $client = [System.Net.Sockets.TcpClient]::new()
        try { $ok = $client.ConnectAsync($ComputerName, $Port).Wait(5000) -and $client.Connected } catch { }
        finally { $client.Dispose() }
        $result['RemotePort'] = $Port
        $result['TcpTestSucceeded'] = $ok
    }
    else {
        $result['PingSucceeded'] = [bool](Microsoft.PowerShell.Management\Test-Connection -TargetName $ComputerName -Count 1 -Quiet -ErrorAction SilentlyContinue)
    }
    [pscustomobject]$result
}

function Resolve-DnsName {
    [CmdletBinding()]
    param([Parameter(Position = 0, Mandatory = $true)][string]$Name, [Parameter(Position = 1)][string]$Type)
    try { $addresses = [System.Net.Dns]::GetHostAddresses($Name) }
    catch {
        $PSCmdlet.WriteError([System.Management.Automation.ErrorRecord]::new(
                [System.ComponentModel.Win32Exception]::new("$Name : DNS name does not exist"), 'DNS_ERROR_RCODE_NAME_ERROR',
                [System.Management.Automation.ErrorCategory]::ResourceUnavailable, $Name))
        return
    }
    foreach ($address in $addresses) {
        $kind = if ($address.AddressFamily -eq 'InterNetwork') { 'A' } else { 'AAAA' }
        if ($Type -and $Type -ne 'A_AAAA' -and $Type -ne 'ANY' -and $Type -ne $kind) { continue }
        [pscustomobject]@{ Name = $Name; Type = $kind; TTL = $null; Section = 'Answer'; IPAddress = $address.ToString() }
    }
}

function Get-ComputerInfo {
    [CmdletBinding()]
    param([Parameter(Position = 0)][string[]]$Property)
    $os = Get-TurCimOperatingSystem
    $cs = Get-TurCimComputerSystem
    $bios = Get-TurCimBios
    $all = [ordered]@{
        OsName = $os.Caption; OsVersion = $os.Version; OsBuildNumber = $os.BuildNumber
        OsArchitecture = $os.OSArchitecture; OsLastBootUpTime = $os.LastBootUpTime
        OsTotalVisibleMemorySize = $os.TotalVisibleMemorySize; OsFreePhysicalMemory = $os.FreePhysicalMemory
        CsName = $cs.Name; CsManufacturer = $cs.Manufacturer; CsModel = $cs.Model
        CsNumberOfLogicalProcessors = $cs.NumberOfLogicalProcessors; CsTotalPhysicalMemory = $cs.TotalPhysicalMemory
        CsDomain = $cs.Domain; CsUserName = $cs.UserName; BiosManufacturer = $bios.Manufacturer
        BiosSMBIOSBIOSVersion = $bios.SMBIOSBIOSVersion; TimeZone = [System.TimeZoneInfo]::Local.DisplayName
    }
    if (-not $Property) { return [pscustomobject]$all }
    $out = [ordered]@{}
    foreach ($pattern in $Property) {
        foreach ($key in $all.Keys) { if ($key -like $pattern) { $out[$key] = $all[$key] } }
    }
    [pscustomobject]$out
}

function Get-LocalUser {
    [CmdletBinding()]
    param([Parameter(Position = 0)][string]$Name = '*')
    foreach ($line in [System.IO.File]::ReadAllLines('/etc/passwd')) {
        $f = $line -split ':'
        if ($f.Count -lt 7) { continue }
        $uid = [int]$f[2]
        if (($uid -eq 0 -or $uid -ge 1000) -and $f[0] -ne 'nobody' -and $f[0] -like $Name) {
            [pscustomobject]@{ PSTypeName = 'Tur.LocalUser'; Name = $f[0]; Enabled = ($f[6] -notmatch 'nologin|false'); Description = $f[4] }
        }
    }
}

function Set-ExecutionPolicy {
    [CmdletBinding()]
    param([Parameter(Position = 0, Mandatory = $true)][string]$ExecutionPolicy, [Parameter(Position = 1)][string]$Scope = 'LocalMachine', [switch]$Force)
    # PowerShell on Linux runs every script; the setting is remembered so
    # that a tutorial's next line reads back what it set.
    $script:ExecutionPolicy = $ExecutionPolicy
}

function Get-ExecutionPolicy {
    [CmdletBinding()]
    param([Parameter(Position = 0)][string]$Scope, [switch]$List)
    if ($script:ExecutionPolicy) { return $script:ExecutionPolicy }
    if ($Scope) { return 'Undefined' }
    'Unrestricted'
}

# ----------------------------------------------- refusals and tools ----

function New-TurRefusal([string]$Name, [string]$Why) {
    $body = "Write-TurNote 'refused' '$Name $Why'; Microsoft.PowerShell.Utility\Write-Error -Message '${Name}: refused by TUR. $Why' -Category PermissionDenied"
    Microsoft.PowerShell.Management\Set-Item -Path "function:global:$Name" -Value ([scriptblock]::Create($body))
}

function Invoke-TurWinshellNote([string]$Kind, [string]$Text) { Write-TurNote $Kind $Text }

function Initialize-TurCommands {
    $disk = 'changes disks or partitions on a Windows PC. On TUR, disks are managed by TUR Storage, which asks for confirmation on a path no script can operate.'
    foreach ($name in 'Format-Volume', 'Clear-Disk', 'Initialize-Disk', 'Remove-Partition', 'New-Partition',
        'Set-Partition', 'Resize-Partition', 'Set-Disk', 'Repair-Volume', 'Optimize-Volume') {
        New-TurRefusal $name $disk
    }
    $power = 'turns a Windows PC off or restarts it. On TUR, use the power menu; a pasted command does not restart this computer.'
    foreach ($name in 'Stop-Computer', 'Restart-Computer') { New-TurRefusal $name $power }

    if (-not $script:Winshell) { return }
    $tools = 'ipconfig', 'ping', 'tracert', 'nslookup', 'tasklist', 'taskkill', 'systeminfo', 'findstr', 'xcopy',
    'robocopy', 'attrib', 'net', 'reg', 'certutil', 'schtasks', 'shutdown', 'winget', 'cmd', 'whoami', 'sfc',
    'chkdsk', 'dism', 'getmac', 'netstat', 'route', 'arp', 'wmic', 'setx', 'icacls', 'tree', 'format',
    'diskpart', 'bcdedit', 'fsutil', 'netsh', 'chcp', 'subst', 'fc', 'where'
    $python = 'python3'
    foreach ($tool in $tools) {
        $body = "if (`$MyInvocation.ExpectingInput) { `$input | & '$python' '$($script:Winshell)' --argv $tool @args } " +
        "else { & '$python' '$($script:Winshell)' --argv $tool @args }"
        $block = [scriptblock]::Create($body)
        if ($tool -ne 'where') { Microsoft.PowerShell.Management\Set-Item -Path "function:global:$tool" -Value $block }
        Microsoft.PowerShell.Management\Set-Item -Path "function:global:$tool.exe" -Value $block
    }
}

Initialize-TurEnvironment
Initialize-TurDrives
Initialize-TurAliases
Initialize-TurCommands
Microsoft.PowerShell.Utility\Update-TypeData -TypeName 'Tur.Service' -DefaultDisplayPropertySet Status, Name, DisplayName -Force
Microsoft.PowerShell.Utility\Update-TypeData -TypeName 'Tur.NetAdapter' -DefaultDisplayPropertySet Name, InterfaceDescription, ifIndex, Status, MacAddress, LinkSpeed -Force
Microsoft.PowerShell.Utility\Update-TypeData -TypeName 'Tur.LocalUser' -DefaultDisplayPropertySet Name, Enabled, Description -Force

Export-ModuleMember -Function * -Alias *
