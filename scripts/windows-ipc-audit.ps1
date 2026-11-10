param([Parameter(Mandatory=$true)][string]$BinaryDir, [string]$CorePath)
# Isolated current-user pipe tests. Does not change WinINet or login tasks.
$BinaryDir=(Resolve-Path $BinaryDir).Path
$ErrorActionPreference = 'Stop'
$work = Join-Path ([IO.Path]::GetTempPath()) ('xray-pilot-pipe-audit-' + [guid]::NewGuid().ToString('N'))
$data = Join-Path $work 'data'
$null = New-Item -ItemType Directory -Path $data -Force
$server = Start-Process -FilePath (Join-Path $BinaryDir 'xray-supervisor.exe') -ArgumentList @('--data-dir', ('"' + $data + '"')) -PassThru -WindowStyle Hidden -RedirectStandardError (Join-Path $work 'server.log')
function Open-Pipe {
    $stream = New-Object IO.Pipes.NamedPipeClientStream('.', $script:name, [IO.Pipes.PipeDirection]::InOut)
    $stream.Connect(10000)
    return $stream
}
function Invoke-Request($operation, $payload=$null, $generation=$null) {
    $stream = Open-Pipe
    try {
        $command=@{operation=$operation};if($null -ne $payload){$command.payload=$payload}
        $json = @{ protocol_version=1;request_id=[guid]::NewGuid().ToString();expected_generation=$generation;command=$command } | ConvertTo-Json -Depth 16 -Compress
        $bytes = [Text.Encoding]::UTF8.GetBytes($json)
        $writer = New-Object IO.BinaryWriter($stream)
        $writer.Write([uint32]$bytes.Length); $writer.Write($bytes); $writer.Flush()
        $reader = New-Object IO.BinaryReader($stream)
        $length = $reader.ReadUInt32(); $value = [Text.Encoding]::UTF8.GetString($reader.ReadBytes($length)) | ConvertFrom-Json
        $writer.Write([byte]1); $writer.Flush()
        if ($value.result.PSObject.Properties.Name -contains 'Err') { throw ($value.result.Err | ConvertTo-Json -Compress) }
        return $value.result.Ok
    } finally { $stream.Dispose() }
}
function Ping { $null=Invoke-Request 'GetStatus' }
try {
    for ($i=0; $i -lt 100; $i++) {
        if (Test-Path (Join-Path $data 'runtime/installation.json')) { break }
        if ($server.HasExited) { throw 'Server exited at startup' }
        Start-Sleep -Milliseconds 50
    }
    $root = (Resolve-Path $data).Path
    $principal = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    # Store uses a canonical extended-length Windows path.
    $canonical = '\\?\' + $root
    $sha = [Security.Cryptography.SHA256]::Create()
    $digest = -join ($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($principal + ':' + $canonical)) | ForEach-Object { $_.ToString('x2') })
    $script:name = 'xray-pilot-' + $digest.Substring(0,32)
    Ping
    foreach ($kind in @('oversize','invalid-json','truncated','no-header')) {
        $stream = Open-Pipe
        try {
            if ($kind -eq 'oversize') { $stream.Write([BitConverter]::GetBytes([uint32](8*1024*1024+1)),0,4);$stream.Flush() }
            if ($kind -eq 'invalid-json') { $bytes=[Text.Encoding]::UTF8.GetBytes('{broken');$stream.Write([BitConverter]::GetBytes([uint32]$bytes.Length),0,4);$stream.Write($bytes,0,$bytes.Length);$stream.Flush() }
            if ($kind -eq 'truncated') { $stream.Write([BitConverter]::GetBytes([uint32]100),0,4);$stream.WriteByte(123);$stream.Flush() }
            if ($kind -eq 'no-header') { Start-Sleep -Milliseconds 5600 }
        } finally { $stream.Dispose() }
        Ping
        if ($server.HasExited) { throw ('Server exited after ' + $kind) }
        Write-Output ('PASS: ' + $kind + ' rejected; next valid connection works')
    }
    # The second response must arrive before the first client acknowledges its response.
    $streams=@()
    try {
        for ($i=0; $i -lt 2; $i++) {
            $stream=Open-Pipe
            $bytes=[Text.Encoding]::UTF8.GetBytes((@{protocol_version=1;request_id=('parallel-'+$i);expected_generation=$null;command=@{operation='GetStatus'}} | ConvertTo-Json -Depth 8 -Compress))
            $writer=New-Object IO.BinaryWriter($stream)
            $writer.Write([uint32]$bytes.Length);$writer.Write($bytes);$writer.Flush()
            $streams += $stream
            if ($i -eq 0) {
                $reader=New-Object IO.BinaryReader($stream)
                $length=$reader.ReadUInt32();$first=[Text.Encoding]::UTF8.GetString($reader.ReadBytes($length)) | ConvertFrom-Json
                if ($first.request_id -ne 'parallel-0') {throw 'First response identity mismatch'}
            }
        }
        $watch=[Diagnostics.Stopwatch]::StartNew()
        $reader=New-Object IO.BinaryReader($streams[1])
        $length=$reader.ReadUInt32();$second=[Text.Encoding]::UTF8.GetString($reader.ReadBytes($length)) | ConvertFrom-Json
        if ($second.request_id -ne 'parallel-1') {throw 'Parallel response identity mismatch'}
        if ($watch.Elapsed.TotalSeconds -ge 2) {throw 'Second response waited for the first client acknowledgement'}
        $streams[0].WriteByte(1);$streams[0].Flush()
        $streams[1].WriteByte(1);$streams[1].Flush()
    } finally { foreach($stream in $streams){$stream.Dispose()} }
    Ping
    Write-Output 'PASS: two simultaneous client requests preserve response identity'
    if ($CorePath) {
        $profile=Invoke-Request 'CreateProfile' @{profile_id='supervision';json='{}'}
        $config='{"outbounds":[{"protocol":"freedom","tag":"direct"}]}'
        $candidate=Invoke-Request 'ValidateDraft' @{profile_id='supervision';expected_revision=$profile.revision;json=$config;core_path=(Resolve-Path $CorePath).Path;asset_root=$data}
        $generation=Invoke-Request 'ApplyProfile' @{candidate_id=$candidate.id;candidate_digest=$candidate.digest;proxy_mode='Direct';http_inbound_tag='http-in'}
        $children=@(Get-CimInstance Win32_Process | Where-Object {$_.ParentProcessId -eq $server.Id -and $_.Name -eq 'xray.exe'})
        if($children.Count -ne 1){throw 'Expected one Core owned by this isolated Supervisor'}
        $idle=Open-Pipe;$truncated=Open-Pipe;$unacknowledged=Open-Pipe
        try {
            $truncated.Write([BitConverter]::GetBytes([uint32]100),0,4);$truncated.WriteByte(123);$truncated.Flush()
            $bytes=[Text.Encoding]::UTF8.GetBytes((@{protocol_version=1;request_id='unacknowledged';expected_generation=$null;command=@{operation='GetStatus'}} | ConvertTo-Json -Depth 8 -Compress))
            $writer=New-Object IO.BinaryWriter($unacknowledged);$writer.Write([uint32]$bytes.Length);$writer.Write($bytes);$writer.Flush()
            $reader=New-Object IO.BinaryReader($unacknowledged);$length=$reader.ReadUInt32();$null=$reader.ReadBytes($length)
            Stop-Process -Id $children[0].ProcessId -Force
            $watch=[Diagnostics.Stopwatch]::StartNew()
            do {
                $status=Invoke-Request 'GetStatus'
                if(-not $status.runtime_instance_id -or $status.generation.id -ne $generation.id){break}
                Start-Sleep -Milliseconds 30
            }while($watch.Elapsed.TotalSeconds -lt 2)
            if($watch.Elapsed.TotalSeconds -ge 2){$recent=Invoke-Request 'GetLogs';throw ('Idle/truncated/unacknowledged clients delayed Core exit detection: elapsed='+$watch.Elapsed.TotalSeconds+'; state='+$status.core_state+'; current='+$status.generation.id+'; original='+$generation.id+'; logs='+($recent | ConvertTo-Json -Compress))}
            Write-Output ('PASS: real Core exit detected with three stalled clients in '+[Math]::Round($watch.Elapsed.TotalSeconds,3)+' s')
        }finally{$idle.Dispose();$truncated.Dispose();$unacknowledged.Dispose()}
        $status=Invoke-Request 'GetStatus'
        $null=Invoke-Request 'StopCore' $null $status.generation.id
    }
    $server.Kill(); $server.WaitForExit()
    $occupied = New-Object IO.Pipes.NamedPipeServerStream($script:name,[IO.Pipes.PipeDirection]::InOut,1)
    try {
        $duplicate = Start-Process -FilePath (Join-Path $BinaryDir 'xray-supervisor.exe') -ArgumentList @('--data-dir', ('"' + $data + '"')) -PassThru -WindowStyle Hidden -RedirectStandardError (Join-Path $work 'occupied.log')
        try {
            if (-not $duplicate.WaitForExit(10000)) {throw 'Supervisor ignored occupied endpoint'}
            if ($duplicate.ExitCode -eq 0) {throw 'Supervisor incorrectly accepted occupied endpoint'}
        } finally { if (-not $duplicate.HasExited) {$duplicate.Kill();$duplicate.WaitForExit()} }
    } finally {$occupied.Dispose()}
    Write-Output 'PASS: occupied pipe endpoint rejected without replacing the existing pipe'
    Write-Output ('Evidence: ' + $work)
} finally {
    if (-not $server.HasExited) { $server.Kill();$server.WaitForExit() }
}
