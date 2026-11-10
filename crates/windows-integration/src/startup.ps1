$ErrorActionPreference = 'Stop'
[Console]::InputEncoding = New-Object System.Text.UTF8Encoding($false)
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$p = [Console]::In.ReadToEnd() | ConvertFrom-Json
$service = New-Object -ComObject 'Schedule.Service'
$service.Connect()
$folder = $service.GetFolder('\')
function Normalize([string] $text) {
    # Scheduler omits defaults, reorders nodes and resolves trigger users to account names.
    # Round-trip through an unregistered definition to expand those defaults consistently.
    $definition = $service.NewTask(0)
    $definition.XmlText = $text
    if (-not $definition.Principal.Id) { $definition.Principal.Id = 'Author' }
    if (-not $definition.Actions.Context) { $definition.Actions.Context = $definition.Principal.Id }
    foreach ($trigger in $definition.Triggers) {
        if ($trigger.Type -eq 9 -and $trigger.UserId) {
            $account = New-Object System.Security.Principal.NTAccount($trigger.UserId)
            if (-not $trigger.UserId.StartsWith('S-1-')) {
                $trigger.UserId = $account.Translate([System.Security.Principal.SecurityIdentifier]).Value
            }
        }
    }
    $xml = New-Object System.Xml.XmlDocument
    $xml.PreserveWhitespace = $false
    $xml.LoadXml($definition.XmlText)
    $uri = $xml.SelectSingleNode('//*[local-name()="RegistrationInfo"]/*[local-name()="URI"]')
    if ($uri) { [void]$uri.ParentNode.RemoveChild($uri) }
    return $xml.OuterXml
}
function ReadTask([string] $name) {
    try { $task = $folder.GetTask($name) }
    catch {
        if (($_.Exception.HResult -band 0xffff) -eq 2) { return $null }
        throw
    }
    return @{ xml = (Normalize $task.Xml) }
}
if ($p.op -eq 'prepare') {
    $task = $service.NewTask(0)
    $task.RegistrationInfo.Description = $p.owner
    $task.Principal.UserId = $p.sid
    $task.Principal.LogonType = 3 # Interactive token; no stored credentials.
    $task.Principal.RunLevel = 0
    $task.Settings.DisallowStartIfOnBatteries = $false
    $task.Settings.StopIfGoingOnBatteries = $false
    $task.Settings.ExecutionTimeLimit = 'PT0S'
    $trigger = $task.Triggers.Create(9)
    $trigger.UserId = $p.sid
    $action = $task.Actions.Create(0)
    $action.Path = $p.exe
    $action.Arguments = '--data-dir "' + $p.root + '" --background'
    $result = @{ xml = (Normalize $task.XmlText) }
} elseif ($p.op -eq 'read') {
    $result = ReadTask $p.name
} elseif ($p.op -eq 'replace') {
    $current = ReadTask $p.name
    if (($null -eq $current) -ne ($null -eq $p.before) -or
        ($null -ne $current -and $current.xml -cne $p.before.xml)) { throw 'STARTUP_CONFLICT' }
    if ($null -eq $p.target) {
        if ($null -ne $current) { $folder.DeleteTask($p.name, 0) }
    } else {
        $flags = if ($null -eq $current) { 2 } else { 4 }
        [void]$folder.RegisterTask($p.name, $p.target.xml, $flags, $null, $null, 3, $null)
    }
    $result = $null
} else { throw 'Unknown operation' }
if ($null -eq $result) { [Console]::Out.Write('null') }
else { ConvertTo-Json -InputObject $result -Compress -Depth 8 }
