param(
    [ValidateSet('Install', 'Status', 'Select')][string]$Action = 'Install',
    [string]$DataDir = '',
    [string]$CoreVersion = 'v26.11.9',
    [string]$InstallId = '',
    [string]$ArchivePath = '',
    [switch]$StdinJson
)
# Installs only official Xray dependency bundles; never changes manager settings or a running Core.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)
[Console]::InputEncoding = New-Object Text.UTF8Encoding($false)
if ($StdinJson) {
    $inputObject = [Console]::In.ReadToEnd() | ConvertFrom-Json
    $Action = [string]$inputObject.action
    $DataDir = [string]$inputObject.data_dir
    $CoreVersion = [string]$inputObject.version
    $InstallId = [string]$inputObject.install_id
    $ArchivePath = ''
}
if ($Action -notin @('Install', 'Status', 'Select')) { throw 'Unknown dependency action' }
if ($CoreVersion -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+$') { throw 'Use an official version tag such as v26.11.9' }
if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64') { throw 'Managed dependencies currently require Windows x64 PowerShell' }
if (-not $DataDir) {
    if (-not $PSScriptRoot) { throw 'A data directory is required for embedded installation' }
    $DataDir = Join-Path (Split-Path $PSScriptRoot -Parent) 'data'
}
if ($DataDir.StartsWith('\\?\UNC\')) { $DataDir = '\\' + $DataDir.Substring(8) }
elseif ($DataDir.StartsWith('\\?\')) { $DataDir = $DataDir.Substring(4) }
$DataDir = [IO.Path]::GetFullPath($DataDir)
$root = Join-Path $DataDir 'dependencies'
$null = New-Item -ItemType Directory -Path $root -Force
$lock = $null
$stage = $null
function Write-AtomicJson([string]$path, $value) {
    $temporary = $path + '.' + [guid]::NewGuid().ToString('N') + '.tmp'
    try {
        $bytes = [Text.Encoding]::UTF8.GetBytes(($value | ConvertTo-Json -Depth 12))
        $file = New-Object IO.FileStream($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try { $file.Write($bytes, 0, $bytes.Length); $file.Flush($true) } finally { $file.Dispose() }
        if ([IO.File]::Exists($path)) { [IO.File]::Replace($temporary, $path, [NullString]::Value) }
        else { [IO.File]::Move($temporary, $path) }
    } finally { if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force } }
}
function Hash-File([string]$path) { return (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }
function Verify-Archive([string]$path) {
    if (-not (Test-Path -LiteralPath ($path + '.dgst'))) { throw 'Official .zip.dgst file is required alongside the archive' }
    $match = [regex]::Match([IO.File]::ReadAllText($path + '.dgst'), '(?m)^SHA2-256=\s*([0-9a-fA-F]{64})\s*$')
    if (-not $match.Success -or (Hash-File $path) -ne $match.Groups[1].Value.ToLowerInvariant()) { throw 'Official archive SHA-256 verification failed' }
    return $match.Groups[1].Value.ToLowerInvariant()
}
function Read-Bundle([string]$directory) {
    $manifest = [IO.File]::ReadAllText((Join-Path $directory 'manifest.json')) | ConvertFrom-Json
    if ($manifest.schema_version -ne 1 -or $manifest.version -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+$' -or
        $manifest.install_id -notmatch '^[0-9a-f]{32}$' -or
        (Split-Path $directory -Leaf) -ne $manifest.install_id -or
        (Split-Path (Split-Path $directory -Parent) -Leaf) -ne $manifest.version -or
        $manifest.archive_sha256 -notmatch '^[0-9a-f]{64}$' -or
        $manifest.release_url -ne ('https://github.com/XTLS/Xray-core/releases/download/' + $manifest.version + '/Xray-windows-64.zip')) {
        throw 'Invalid managed dependency manifest'
    }
    $expected = @('xray.exe', 'geoip.dat', 'geosite.dat')
    if (@($manifest.files).Count -ne 3) { throw 'Incomplete dependency manifest' }
    foreach ($name in $expected) {
        $entry = @($manifest.files | Where-Object { $_.name -ceq $name })
        if ($entry.Count -ne 1 -or $entry[0].sha256 -notmatch '^[0-9a-f]{64}$' -or
            (Hash-File (Join-Path $directory $name)) -ne $entry[0].sha256) { throw ('Dependency integrity check failed: ' + $name) }
    }
    return @{ version=$manifest.version; install_id=$manifest.install_id; core_path=(Join-Path $directory 'xray.exe'); asset_root=$directory; archive_sha256=$manifest.archive_sha256 }
}
function Get-Directories {
    $versions = Join-Path $root 'xray'
    if (-not (Test-Path -LiteralPath $versions)) { return @() }
    return @(Get-ChildItem -LiteralPath $versions -Directory | Where-Object { $_.Name -match '^v[0-9]+\.[0-9]+\.[0-9]+$' } |
        ForEach-Object { Get-ChildItem -LiteralPath $_.FullName -Directory | Where-Object { $_.Name -match '^[0-9a-f]{32}$' } })
}
function Get-Report($selected) {
    $installations = @()
    foreach ($directory in (Get-Directories)) {
        $item = @{ version=$directory.Parent.Name; install_id=$directory.Name; valid=$false; error=$null }
        try { $null = Read-Bundle $directory.FullName; $item.valid=$true } catch { $item.error=$_.Exception.Message }
        $installations += $item
    }
    return @{ selected=$selected; installations=@($installations) }
}
function Download-Official([string]$url, [string]$destination) {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    for ($attempt = 0; $attempt -lt 3; $attempt++) {
        try { Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $destination -TimeoutSec 90; return }
        catch { if ($attempt -eq 2) { throw }; Start-Sleep -Milliseconds (500 * ($attempt + 1)) }
    }
}
try {
    $lock = New-Object IO.FileStream((Join-Path $root 'install.lock'), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    $currentPath = Join-Path $root 'current.json'
    $selected = $null
    if ($Action -eq 'Status') {
        if (Test-Path -LiteralPath $currentPath) {
            $current = [IO.File]::ReadAllText($currentPath) | ConvertFrom-Json
            if ($current.version -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+$' -or $current.install_id -notmatch '^[0-9a-f]{32}$') { throw 'Invalid current dependency selection' }
            $directory = Join-Path (Join-Path (Join-Path $root 'xray') $current.version) $current.install_id
            try { $selected = Read-Bundle $directory } catch { $selected = $null }
        }
    } elseif ($Action -eq 'Select') {
        if ($InstallId -notmatch '^[0-9a-f]{32}$') { throw 'Invalid managed installation identity' }
        $selected = Read-Bundle (Join-Path (Join-Path (Join-Path $root 'xray') $CoreVersion) $InstallId)
        Write-AtomicJson $currentPath @{version=$selected.version;install_id=$selected.install_id}
    } else {
        # A healthy installed copy is reused; repairs always publish a new installation.
        foreach ($directory in (Get-Directories)) {
            if ($directory.Parent.Name -eq $CoreVersion) {
                try { $selected = Read-Bundle $directory.FullName; break } catch { }
            }
        }
        if (-not $selected) {
            $stage = Join-Path $root ('.staging-' + [guid]::NewGuid().ToString('N'))
            $null = New-Item -ItemType Directory -Path $stage
            $url = 'https://github.com/XTLS/Xray-core/releases/download/' + $CoreVersion + '/Xray-windows-64.zip'
            $cache = Join-Path (Join-Path $root 'downloads') $CoreVersion
            $cachedArchive = Join-Path $cache 'Xray-windows-64.zip'
            $archive = $ArchivePath
            if ($archive) { $archive = (Resolve-Path -LiteralPath $archive).Path; $digest = Verify-Archive $archive }
            else {
                try { $digest = Verify-Archive $cachedArchive; $archive=$cachedArchive } catch { $archive='' }
                if (-not $archive) {
                    $archive = Join-Path $stage 'Xray-windows-64.zip'
                    Download-Official ($url + '.dgst') ($archive + '.dgst')
                    Download-Official $url $archive
                    $digest = Verify-Archive $archive
                    $null = New-Item -ItemType Directory -Path $cache -Force
                    # Cache is replaceable; every reuse rechecks the complete archive/digest pair.
                    Copy-Item -LiteralPath $archive -Destination $cachedArchive -Force
                    Copy-Item -LiteralPath ($archive + '.dgst') -Destination ($cachedArchive + '.dgst') -Force
                }
            }
            Add-Type -AssemblyName System.IO.Compression.FileSystem
            $prepared = Join-Path $stage 'prepared'
            $null = New-Item -ItemType Directory -Path $prepared
            $zip = [IO.Compression.ZipFile]::OpenRead($archive)
            $files = @()
            try {
                foreach ($name in @('xray.exe', 'geoip.dat', 'geosite.dat')) {
                    $entries = @($zip.Entries | Where-Object { $_.FullName -ceq $name })
                    if ($entries.Count -ne 1 -or $entries[0].Length -le 0 -or $entries[0].Length -gt 209715200) { throw ('Missing or invalid official bundle entry: ' + $name) }
                    $path = Join-Path $prepared $name
                    [IO.Compression.ZipFileExtensions]::ExtractToFile($entries[0], $path, $false)
                    $files += @{name=$name;sha256=(Hash-File $path)}
                }
            } finally { $zip.Dispose() }
            $versionOutput = @(& (Join-Path $prepared 'xray.exe') version 2>&1)
            if ($LASTEXITCODE -ne 0 -or $versionOutput.Count -eq 0 -or
                [string]$versionOutput[0] -notmatch ('^Xray ' + [regex]::Escape($CoreVersion.Substring(1)) + '\b')) {
                throw 'Downloaded Core does not identify as the selected official version'
            }
            $identity = [guid]::NewGuid().ToString('N')
            Write-AtomicJson (Join-Path $prepared 'manifest.json') @{schema_version=1;version=$CoreVersion;install_id=$identity;release_url=$url;archive_sha256=$digest;files=@($files)}
            $versionRoot = Join-Path (Join-Path $root 'xray') $CoreVersion
            $null = New-Item -ItemType Directory -Path $versionRoot -Force
            $destination = Join-Path $versionRoot $identity
            [IO.Directory]::Move($prepared, $destination)
            $selected = Read-Bundle $destination
        }
        Write-AtomicJson $currentPath @{version=$selected.version;install_id=$selected.install_id}
    }
    Get-Report $selected | ConvertTo-Json -Depth 12 -Compress
} finally {
    if ($stage -and (Test-Path -LiteralPath $stage)) { Remove-Item -LiteralPath $stage -Recurse -Force }
    if ($lock) { $lock.Dispose() }
}
