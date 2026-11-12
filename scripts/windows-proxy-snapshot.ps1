param(
    [string]$Save,
    [string]$Compare
)
# Read-only WinINet snapshot helper for V01/V10 validation.
# It never writes the registry; comparison exits 1 when any of the four
# user proxy settings changed during a controlled test.
$ErrorActionPreference = 'Stop'
$path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
$names = @('ProxyEnable', 'ProxyServer', 'ProxyOverride', 'AutoConfigURL', 'AutoDetect')
function Read-ProxySettings {
    $values = [ordered]@{}
    foreach ($name in $names) {
        $item = Get-ItemProperty -Path $path -Name $name -ErrorAction SilentlyContinue
        $values[$name] = if ($null -eq $item) { $null } else { $item.$name }
    }
    return [pscustomobject]$values
}
if ($Save) {
    Read-ProxySettings | ConvertTo-Json -Depth 4 | Set-Content -Path $Save -Encoding UTF8
    Write-Output ('Saved WinINet snapshot to ' + $Save)
    exit 0
}
if ($Compare) {
    $before = Get-Content -Path $Compare -Raw -Encoding UTF8 | ConvertFrom-Json
    $after = Read-ProxySettings
    $changed = $false
    foreach ($name in $names) {
        $a = $after.$name
        $b = $before.$name
        if ($a -ne $b) {
            $changed = $true
            Write-Output ($name + ': before=' + $b + ' after=' + $a)
        }
    }
    if ($changed) {
        Write-Error 'WinINet settings changed during the controlled test.'
        exit 1
    }
    Write-Output 'WinINet settings match the saved snapshot.'
    exit 0
}
Read-ProxySettings | ConvertTo-Json -Depth 4
