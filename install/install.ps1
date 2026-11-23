# Install wake on Windows for the current user and keep it updated daily.
#
#   irm https://raw.githubusercontent.com/dantebarbieri/wake/main/install/install.ps1 | iex
#   & ([scriptblock]::Create((irm https://raw.githubusercontent.com/dantebarbieri/wake/main/install/install.ps1))) -Uninstall
#
# Installs %LOCALAPPDATA%\Programs\wake\wake.exe, adds it to the user PATH, and
# registers a "wake self-update" scheduled task that runs daily at 04:15 (or at
# the next logon if the PC was off), hidden through conhost --headless.
param([switch]$Uninstall)
$ErrorActionPreference = 'Stop'

$Dir = Join-Path $env:LOCALAPPDATA 'Programs\wake'
$Exe = Join-Path $Dir 'wake.exe'
$Task = 'wake self-update'
$Url = 'https://github.com/dantebarbieri/wake/releases/latest/download/wake-x86_64-pc-windows-msvc.zip'

function Set-UserPath([scriptblock]$Edit) {
    $parts = @([Environment]::GetEnvironmentVariable('Path', 'User') -split ';' | Where-Object { $_ })
    $new = (& $Edit $parts) -join ';'
    [Environment]::SetEnvironmentVariable('Path', $new, 'User')
}

if ($Uninstall) {
    Unregister-ScheduledTask -TaskName $Task -Confirm:$false -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force $Dir -ErrorAction SilentlyContinue
    Set-UserPath { param($p) $p | Where-Object { $_ -ne $Dir } }
    Write-Host 'wake removed'
    return
}

$tmp = Join-Path ([IO.Path]::GetTempPath()) "wake-$([guid]::NewGuid())"
New-Item -ItemType Directory -Force $tmp, $Dir | Out-Null
try {
    Invoke-WebRequest -UseBasicParsing $Url -OutFile "$tmp\wake.zip"
    Expand-Archive -Force "$tmp\wake.zip" $tmp
    Copy-Item -Force "$tmp\wake.exe" $Exe
} finally {
    Remove-Item -Recurse -Force $tmp
}

Set-UserPath { param($p) if ($p -contains $Dir) { $p } else { $p + $Dir } }
if (($env:Path -split ';') -notcontains $Dir) { $env:Path += ";$Dir" }

$action = New-ScheduledTaskAction -Execute "$env:WINDIR\System32\conhost.exe" -Argument "--headless `"$Exe`" self-update --quiet"
$triggers = @(
    (New-ScheduledTaskTrigger -Daily -At 4:15am),
    (New-ScheduledTaskTrigger -AtLogOn -User "$env:USERDOMAIN\$env:USERNAME")
)
$settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Minutes 10)
Register-ScheduledTask -TaskName $Task -Action $action -Trigger $triggers -Settings $settings -Description 'Keeps wake (github.com/dantebarbieri/wake) up to date' -Force | Out-Null

& $Exe --version
Write-Host "daily update: scheduled task '$Task'. Open a new terminal to get wake on PATH."
