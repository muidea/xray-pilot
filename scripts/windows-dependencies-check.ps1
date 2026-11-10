param([string]$Installer = (Join-Path $PSScriptRoot 'windows-dependencies.ps1'), [string]$ArchivePath = '')
$ErrorActionPreference = 'Stop'
$work = Join-Path ([IO.Path]::GetTempPath()) ('xray-pilot-dependencies-' + [guid]::NewGuid().ToString('N'))
$data = Join-Path $work 'data 中文 spaces'
$null = New-Item -ItemType Directory -Path (Join-Path $data 'config') -Force
$settingsPath = Join-Path $data 'config\manager.json'
[IO.File]::WriteAllText($settingsPath, '{"sentinel":"do not change"}')
$settingsBefore = [IO.File]::ReadAllBytes($settingsPath)
function Install([string]$directory = $data, [string]$archive = $ArchivePath, [string]$version = 'v26.11.9') {
    return (& $Installer -Action Install -DataDir $directory -CoreVersion $version -ArchivePath $archive | ConvertFrom-Json)
}
function Assert-Fails([scriptblock]$call) {
    $failed=$false
    try { & $call | Out-Null } catch { $failed=$true }
    if (-not $failed) { throw 'Expected an operation to fail' }
}
$first = Install
if (-not $first.selected -or $first.installations.Count -ne 1 -or -not $first.installations[0].valid) { throw 'Initial official installation failed' }
$again = Install
if ($again.selected.install_id -ne $first.selected.install_id) { throw 'Healthy installation was not reused' }
'PASS: official archive/digest verification, complete bundle and idempotent reuse'
$status = & $Installer -Action Status -DataDir $data | ConvertFrom-Json
if ($status.selected.install_id -ne $first.selected.install_id) { throw 'Status did not recover the installed selection' }
$geo = Join-Path $first.selected.asset_root 'geoip.dat'
[IO.File]::AppendAllText($geo, 'corruption probe')
$status = & $Installer -Action Status -DataDir $data | ConvertFrom-Json
if ($status.selected -or $status.installations[0].valid) { throw 'Damaged dependency was trusted' }
$repaired = Install
if ($repaired.selected.install_id -eq $first.selected.install_id -or -not (Test-Path -LiteralPath $geo)) { throw 'Repair overwrote an existing installation' }
if (-not ($repaired.installations | Where-Object { $_.valid })) { throw 'Repair did not publish a valid bundle' }
'PASS: corruption detected; repair uses a new installation and preserves the old directory'
$currentPath = Join-Path $data 'dependencies\current.json'
$before = [IO.File]::ReadAllText($currentPath)
$held = [IO.File]::Open($currentPath, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
try { Assert-Fails { & $Installer -Action Select -DataDir $data -CoreVersion $repaired.selected.version -InstallId $repaired.selected.install_id } }
finally { $held.Dispose() }
if ([IO.File]::ReadAllText($currentPath) -cne $before) { throw 'Failed publication changed the current selection' }
$selected = & $Installer -Action Select -DataDir $data -CoreVersion $repaired.selected.version -InstallId $repaired.selected.install_id | ConvertFrom-Json
if ($selected.selected.install_id -ne $repaired.selected.install_id) { throw 'Verified selection failed' }
$held = [IO.File]::Open((Join-Path $data 'dependencies\install.lock'), [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
try { Assert-Fails { & $Installer -Action Status -DataDir $data } } finally { $held.Dispose() }
'PASS: atomic selection failure preserves prior state; concurrent installer lock is enforced'
if (-not $ArchivePath) { $ArchivePath = Join-Path $data 'dependencies\downloads\v26.11.9\Xray-windows-64.zip' }
$offline = Install (Join-Path $work 'offline') $ArchivePath
if (-not $offline.selected) { throw 'Offline official archive installation failed' }
Assert-Fails { Install (Join-Path $work 'wrong-version') $ArchivePath 'v0.0.0' }
if (Test-Path (Join-Path $work 'wrong-version\dependencies\current.json')) { throw 'Wrong-version archive was published' }
$bad = Join-Path $work 'bad.zip'
[IO.File]::WriteAllBytes($bad, [byte[]]@(1,2,3))
Copy-Item -LiteralPath ($ArchivePath + '.dgst') -Destination ($bad + '.dgst')
Assert-Fails { Install (Join-Path $work 'bad-archive') $bad }
if (Test-Path (Join-Path $work 'bad-archive\dependencies\current.json')) { throw 'Invalid checksum was published' }
if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($settingsPath)) -cne [Convert]::ToBase64String($settingsBefore)) { throw 'Installer changed manager settings' }
'PASS: offline install; wrong version and bad checksum rejected; manager settings unchanged'
'Evidence: ' + $work
