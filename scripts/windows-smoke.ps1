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
using System.Text;
using System.Threading;
public static class XraySmokeWindow {
    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool IsWindowVisible(IntPtr hWnd);
    private delegate bool EnumProc(IntPtr hWnd, IntPtr data);
    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumProc callback, IntPtr data);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern int GetClassName(IntPtr window, StringBuilder text, int size);
    [DllImport("user32.dll")] private static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] private static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] private static extern int GetMenuItemCount(IntPtr menu);
    [DllImport("user32.dll")] private static extern uint GetMenuItemID(IntPtr menu, int position);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern int GetMenuString(IntPtr menu, uint position, StringBuilder text, int size, uint flags);
    private static IntPtr Find(uint processId, string className) {
        IntPtr result = IntPtr.Zero;
        EnumWindows((window, data) => {
            uint owner; GetWindowThreadProcessId(window, out owner);
            if (owner != processId) return true;
            var text = new StringBuilder(256); GetClassName(window, text, text.Capacity);
            if (text.ToString() != className) return true;
            result = window; return false;
        }, IntPtr.Zero);
        return result;
    }
    public static void TrayCommand(uint processId, string title) {
        var tray = IntPtr.Zero;
        for (int attempt=0; attempt<100 && tray==IntPtr.Zero; attempt++) {
            tray = Find(processId, "tray_icon_app");
            if (tray==IntPtr.Zero) Thread.Sleep(100);
        }
        if (tray == IntPtr.Zero) throw new Exception("Tray HWND unavailable");
        // Open this instance's real popup, and resolve the command by its visible text.
        if (!PostMessage(tray, 6002, IntPtr.Zero, (IntPtr)0x204)) throw new Exception("Cannot open tray popup");
        uint command = UInt32.MaxValue;
        try {
            for (int attempt=0; attempt<100; attempt++) {
                var popup = Find(processId, "#32768");
                if (popup != IntPtr.Zero) {
                    var menu = SendMessage(popup, 0x1E1, IntPtr.Zero, IntPtr.Zero);
                    for (int i=0; i<GetMenuItemCount(menu); i++) {
                        var text = new StringBuilder(256); GetMenuString(menu, (uint)i, text, text.Capacity, 0x400);
                        if (text.ToString().Replace("&", "") == title) { command = GetMenuItemID(menu, i); break; }
                    }
                }
                if (command != UInt32.MaxValue) break;
                Thread.Sleep(50);
            }
        } finally { PostMessage(tray, 0x1F, IntPtr.Zero, IntPtr.Zero); }
        if (command == UInt32.MaxValue) throw new Exception("Tray item unavailable: " + title);
        // Dispatch the same WM_COMMAND produced by selecting the resolved native menu item.
        if (!PostMessage(tray, 0x111, (IntPtr)command, IntPtr.Zero)) throw new Exception("Cannot select tray item");
    }
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
        [XraySmokeWindow]::TrayCommand($guiProcess.Id, '打开 Xray Pilot')
        for ($i=0; $i -lt 100 -and -not [XraySmokeWindow]::IsWindowVisible($windowHandle); $i++) { Start-Sleep -Milliseconds 100 }
        if (-not [XraySmokeWindow]::IsWindowVisible($windowHandle)) { throw 'Tray Open did not wake the hidden GUI' }
        if (-not $guiProcess.CloseMainWindow()) { throw 'Cannot hide GUI again' }
        Start-Sleep -Milliseconds 500
        [XraySmokeWindow]::TrayCommand($guiProcess.Id, '仅退出界面，后台继续运行')
        if (-not $guiProcess.WaitForExit(10000)) { throw 'Tray Exit GUI did not close the hidden GUI' }
        $status = Call-Ctl 'status'
        if ($status.supervisor_epoch -ne $epoch -or $status.runtime_instance_id -ne $instance) { throw 'GUI exit changed Supervisor/Core ownership' }
        Start-Gui
        $status = Call-Ctl 'status'
        if ($status.supervisor_epoch -ne $epoch -or $status.runtime_instance_id -ne $instance) { throw 'GUI reopen restarted Supervisor/Core' }
        [XraySmokeWindow]::TrayCommand($guiProcess.Id, '仅退出界面，后台继续运行')
        if (-not $guiProcess.WaitForExit(10000)) { throw 'Tray Exit GUI did not close the visible GUI' }
        'PASS: tray Open, hidden/visible Exit GUI, close-to-tray, independent Core lifetime and reopen'
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
    if ($Gui) {
        $null = Call-Ctl 'start'
        $children = @(Get-CimInstance Win32_Process -Filter ("ParentProcessId = " + $server.Id) | Where-Object { $_.Name -eq 'xray.exe' })
        if ($children.Count -ne 1) { throw 'Expected one restarted Core' }
        $coreProcess = Get-Process -Id $children[0].ProcessId
        $guiProcess = Start-Process -FilePath (Join-Path $BinaryDir 'xray-manager.exe') -ArgumentList @('--background', '--data-dir', ('"' + $data + '"')) -PassThru
        # A background-started instance has no visible viewport to drive polling.
        Start-Sleep -Seconds 3
        [XraySmokeWindow]::TrayCommand($guiProcess.Id, '退出程序（停止 Core 并恢复代理）')
        if (-not $guiProcess.WaitForExit(10000)) { throw 'Tray Quit did not close the background GUI' }
        if (-not $server.WaitForExit(5000)) { throw 'Tray Quit did not stop Supervisor' }
        if (-not $coreProcess.WaitForExit(5000)) { throw 'Tray Quit did not stop Core' }
        'PASS: background tray Quit stops GUI, Supervisor and Core'
    } else {
        $null = Call-Ctl 'shutdown'
        if (-not $server.WaitForExit(5000)) { throw 'Supervisor did not shut down' }
    }
    if ((Proxy-Fingerprint) -ne $proxyBefore) { throw 'Proxy settings changed during Direct smoke test' }
    'PASS: Startup without reconnect, historical success replay and unchanged system proxy'
    'Evidence directory: ' + $work
} finally {
    if ($guiProcess -and -not $guiProcess.HasExited) { $guiProcess.Kill(); $guiProcess.WaitForExit() }
    if ($server -and -not $server.HasExited) { $server.Kill(); $server.WaitForExit() }
    if ($coreProcess -and -not $coreProcess.HasExited) { $null = $coreProcess.WaitForExit(5000) }
}
