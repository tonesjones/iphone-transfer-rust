$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$launcher = Join-Path $PSScriptRoot 'backup.ps1'
$shortcutPath = Join-Path $repoRoot 'PhotoXfer Backup.lnk'
$powershell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'

$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($shortcutPath)
$shortcut.TargetPath = $powershell
$shortcut.Arguments = '-NoProfile -ExecutionPolicy Bypass -File "' + $launcher + '"'
$shortcut.WorkingDirectory = $repoRoot
$shortcut.Description = 'Import, check, and archive the PhotoXfer library'
$shortcut.Save()

Write-Host "Created shortcut: $shortcutPath"
