param(
    [Parameter(Mandatory = $true)][string]$BinaryDir,
    [Parameter(Mandatory = $true)][string]$CorePath,
    [switch]$Gui
)
# Isolated Direct-mode smoke test. Does not register tasks or write WinINet settings.
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class XraySmokeWindow {
    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool IsWindowVisible(IntPtr hWnd);
}
'@
$BinaryDir = (Resolve-Path $BinaryDir).Path
$CorePath = (Resolve-Path $CorePath).Path
$work = Join-Path ([IO.Path]::GetTempPath()) ('xray-pilot-smoke-' + [guid]::NewGuid().ToString('N'))
$data = Join-Path $work 'data with spaces'
$null = New-Item -ItemType Directory -Path $data -Force
$server = $null
$guiProcess = $null
$coreProcess = $null
function Call-Ctl([string]$action, [string]$inputJson = '') {
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = Join-Path $BinaryDir 'xrayctl.exe'
    $info.Arguments = '--data-dir "' + $data + '" ' + $action
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.StandardOutputEncoding = [Text.Encoding]::UTF8
    $info.StandardErrorEncoding = [Text.Encoding]::UTF8
    $p = New-Object System.Diagnostics.Process
    $p.StartInfo = $info
    $null = $p.Start()
    if ($inputJson) {
        $bytes = [Text.Encoding]::UTF8.GetBytes($inputJson + "`n")
        $p.StandardInput.BaseStream.Write($bytes, 0, $bytes.Length)
    }
    $p.StandardInput.BaseStream.Close()
    $output = $p.StandardOutput.ReadToEndAsync()
    $errorOutput = $p.StandardError.ReadToEndAsync()
    if (-not $p.WaitForExit(90000)) { $p.Kill(); throw 'CLI timeout' }
    if ($p.ExitCode -ne 0) { throw ('CLI failed: ' + $errorOutput.Result) }
    $response = $output.Result | ConvertFrom-Json
    if ($response.result.PSObject.Properties.Name -contains 'Err') { throw ($response.result.Err | ConvertTo-Json -Compress) }
    return $response.result.Ok
}
function New-Request([string]$operation, $payload, $generation) {
    $command = @{ operation = $operation }
    if ($null -ne $payload) { $command.payload = $payload }
    return (@{ protocol_version = 1; request_id = [guid]::NewGuid().ToString(); expected_generation = $generation; command = $command } | ConvertTo-Json -Depth 64 -Compress)
}
function Start-Server {
    $script:server = Start-Process -FilePath (Join-Path $BinaryDir 'xray-supervisor.exe') -ArgumentList @('--data-dir', ('"' + $data + '"')) -PassThru -WindowStyle Hidden -RedirectStandardError (Join-Path $work 'supervisor-error.log')
    for ($i=0; $i -lt 50; $i++) {
        try { $null = Call-Ctl 'status'; return } catch {
            $lastFailure = $_.Exception.Message
            if ($script:server.HasExited) { throw ('Supervisor exited: ' + (Get-Content (Join-Path $work 'supervisor-error.log') -Raw)) }
            Start-Sleep -Milliseconds 100
        }
    }
    throw ('Supervisor unavailable: ' + $lastFailure)
}
function Start-Gui {
    $script:guiProcess = Start-Process -FilePath (Join-Path $BinaryDir 'xray-manager.exe') -ArgumentList @('--data-dir', ('"' + $data + '"')) -PassThru
    for ($i=0; $i -lt 100; $i++) {
        $script:guiProcess.Refresh()
        if ($script:guiProcess.HasExited) { throw 'GUI exited during startup' }
        # A helper window can exist before eframe creates and initializes the actual UI.
        if ($script:guiProcess.MainWindowHandle -ne [IntPtr]::Zero -and
            $script:guiProcess.MainWindowTitle -eq 'Xray Pilot' -and
            [XraySmokeWindow]::IsWindowVisible($script:guiProcess.MainWindowHandle)) {
            $readyHandle = $script:guiProcess.MainWindowHandle
            Start-Sleep -Milliseconds 300
            $script:guiProcess.Refresh()
            if (-not $script:guiProcess.HasExited -and $script:guiProcess.MainWindowHandle -eq $readyHandle) { return }
        }
        Start-Sleep -Milliseconds 100
    }
    throw 'GUI window did not become ready'
}
function Proxy-Fingerprint {
    $v = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
    return (@{ enabled=$v.ProxyEnable; server=$v.ProxyServer; bypass=$v.ProxyOverride; pac=$v.AutoConfigURL } | ConvertTo-Json -Compress)
}
$proxyBefore = Proxy-Fingerprint
try {
    Start-Server
    $listener = New-Object System.Net.Sockets.TcpListener([Net.IPAddress]::Loopback, 0)
    $listener.Start(); $port = $listener.LocalEndpoint.Port; $listener.Stop()
    $json = @{ log=@{loglevel='warning'}; inbounds=@(@{tag='http-in';listen='127.0.0.1';port=$port;protocol='http';settings=@{}}); outbounds=@(@{tag='direct';protocol='freedom'}) } | ConvertTo-Json -Depth 32 -Compress
    $profile = Call-Ctl 'request' (New-Request 'CreateProfile' @{profile_id='smoke';json=$json} $null)
    $candidate = Call-Ctl 'request' (New-Request 'ValidateDraft' @{profile_id='smoke';expected_revision=$profile.revision;json=$json;core_path=$CorePath;asset_root=$data} $null)
    $apply = New-Request 'ApplyProfile' @{candidate_id=$candidate.id;candidate_digest=$candidate.digest;proxy_mode='Direct';http_inbound_tag='http-in'} $null
    $generation = Call-Ctl 'request' $apply
    $status = Call-Ctl 'status'
    if ($status.core_state -ne 'Running') { throw 'Core is not running' }
    $child = @(Get-CimInstance Win32_Process -Filter ("ParentProcessId = " + $server.Id) | Where-Object { $_.Name -eq 'xray.exe' })
    if ($child.Count -ne 1) { throw 'Expected exactly one owned Core child' }
    $coreProcess = Get-Process -Id $child[0].ProcessId
    'PASS: Native IPC, official Core validation, Direct apply and one owned Core'
    if ($Gui) {
        $epoch = $status.supervisor_epoch
        $instance = $status.runtime_instance_id
        Start-Gui
        $windowHandle = $guiProcess.MainWindowHandle
        $windowTitle = $guiProcess.MainWindowTitle
        if (-not $guiProcess.CloseMainWindow()) { throw 'Could not send GUI close request' }
        Start-Sleep -Milliseconds 1500
        $guiProcess.Refresh()
        if ($guiProcess.HasExited) { throw 'Closing the window exited instead of hiding to tray' }
        if ([XraySmokeWindow]::IsWindowVisible($windowHandle)) { throw ("GUI window remained visible after close: $windowTitle") }
        $guiProcess.Kill(); $guiProcess.WaitForExit()
        $status = Call-Ctl 'status'
        if ($status.supervisor_epoch -ne $epoch -or $status.runtime_instance_id -ne $instance) { throw 'GUI exit changed Supervisor/Core ownership' }
        Start-Gui
        $status = Call-Ctl 'status'
        if ($status.supervisor_epoch -ne $epoch -or $status.runtime_instance_id -ne $instance) { throw 'GUI reopen restarted Supervisor/Core' }
        $guiProcess.Kill(); $guiProcess.WaitForExit()
        'PASS: GUI window, close-to-tray, independent Core lifetime and reopen'
    }
    $server.Kill(); $server.WaitForExit()
    if (-not $coreProcess.WaitForExit(5000)) { throw 'Job did not terminate Core after Supervisor was killed' }
    'PASS: Kill-on-close Job terminates owned Core after forced Supervisor exit'
    Start-Server
    $status = Call-Ctl 'status'
    if ($status.core_state -ne 'Stopped' -or $status.generation.id -ne $generation.id) { throw 'Startup cleanup/committed generation mismatch' }
    $replayed = Call-Ctl 'request' $apply
    if ($replayed.id -ne $generation.id) { throw 'Committed request was not deduplicated after restart' }
    if ((Call-Ctl 'status').core_state -ne 'Stopped') { throw 'Replay unexpectedly started Core' }
    $null = Call-Ctl 'shutdown'
    if (-not $server.WaitForExit(5000)) { throw 'Supervisor did not shut down' }
    if ((Proxy-Fingerprint) -ne $proxyBefore) { throw 'Proxy settings changed during Direct smoke test' }
    'PASS: Startup without reconnect, historical success replay and unchanged system proxy'
    'Evidence directory: ' + $work
} finally {
    if ($guiProcess -and -not $guiProcess.HasExited) { $guiProcess.Kill(); $guiProcess.WaitForExit() }
    if ($server -and -not $server.HasExited) { $server.Kill(); $server.WaitForExit() }
    if ($coreProcess -and -not $coreProcess.HasExited) { $null = $coreProcess.WaitForExit(5000) }
}
