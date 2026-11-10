param([Parameter(Mandatory=$true)][string]$BinaryDir)
# Isolated default-directory checks. Never touches an existing installation or user data.
$ErrorActionPreference = 'Stop'
$work = Join-Path ([IO.Path]::GetTempPath()) ('xray-pilot-portable-' + [guid]::NewGuid().ToString('N'))
$install = Join-Path $work 'installation with spaces'
$outside = Join-Path $work 'unrelated working directory'
$null = New-Item -ItemType Directory -Path $install, $outside, (Join-Path $install 'scripts') -Force
foreach ($name in @('xray-manager.exe', 'xray-supervisor.exe', 'xrayctl.exe')) {
    Copy-Item -LiteralPath (Join-Path $BinaryDir $name) -Destination (Join-Path $install $name)
}
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'windows-dependencies.ps1') -Destination (Join-Path $install 'scripts\windows-dependencies.ps1')
$data = Join-Path $install 'data'
$portableOriginalLocalAppData = $env:LOCALAPPDATA
# A regression to the old default must still stay inside this test's isolated tree.
$env:LOCALAPPDATA = Join-Path $work 'isolated legacy appdata'
$gui = $null
$server = $null
function Call-Ctl([string]$action) {
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = Join-Path $install 'xrayctl.exe'
    $info.Arguments = $action
    $info.WorkingDirectory = $outside
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = New-Object Diagnostics.Process
    $process.StartInfo = $info
    $null = $process.Start()
    $output = $process.StandardOutput.ReadToEndAsync()
    $errors = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit(15000)) { $process.Kill(); throw 'CLI timeout' }
    if ($process.ExitCode -ne 0) { throw $errors.Result }
    return $output.Result | ConvertFrom-Json
}
function Wait-Server {
    for ($i=0; $i -lt 100; $i++) {
        try { $null = Call-Ctl 'status'; return } catch { Start-Sleep -Milliseconds 100 }
    }
    throw 'Default data directory could not be reached by CLI'
}
try {
    Push-Location $outside
    try {
        $report = & (Join-Path $install 'scripts\windows-dependencies.ps1') -Action Status | ConvertFrom-Json
        if ($report.selected) { throw 'Unexpected preexisting bundle' }
    } finally { Pop-Location }
    if (-not (Test-Path (Join-Path $data 'dependencies\install.lock'))) { throw 'Installer did not use installation/data' }
    $gui = Start-Process -FilePath (Join-Path $install 'xray-manager.exe') -ArgumentList '--background' -WorkingDirectory $outside -PassThru
    Wait-Server
    if ($gui.HasExited -or -not (Test-Path (Join-Path $data 'runtime\installation.json'))) { throw 'GUI did not create its adjacent data directory' }
    $gui.Kill(); $gui.WaitForExit()
    $null = Call-Ctl 'shutdown'
    # Wait for the GUI-owned Supervisor to release the installation lock.
    for ($i=0; $i -lt 100; $i++) {
        try { $null = Call-Ctl 'status' } catch { break }
        Start-Sleep -Milliseconds 100
    }
    $server = Start-Process -FilePath (Join-Path $install 'xray-supervisor.exe') -WorkingDirectory $outside -WindowStyle Hidden -PassThru
    Wait-Server
    if ($server.HasExited) { throw 'Standalone Supervisor did not use the same adjacent data directory' }
    $null = Call-Ctl 'shutdown'
    if (-not $server.WaitForExit(5000)) { throw 'Standalone Supervisor did not exit' }
    if (Test-Path (Join-Path $outside 'data')) { throw 'Data was created relative to the working directory' }
    'PASS: GUI, standalone Supervisor, CLI and installer default to installation/data from unrelated working directories'
    'Evidence: ' + $work
} finally {
    if ($gui -and -not $gui.HasExited) { $gui.Kill(); $gui.WaitForExit() }
    if ($server -and -not $server.HasExited) { $server.Kill(); $server.WaitForExit() }
    try { $null = Call-Ctl 'shutdown' } catch { }
    $env:LOCALAPPDATA = $portableOriginalLocalAppData
}
