$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$executable = Join-Path $repoRoot 'target\release\photoxfer-desktop.exe'
if (-not (Test-Path -LiteralPath $executable)) {
    throw 'Build the desktop app first: cargo build -p photoxfer-gui --release'
}
$shortcutPath = Join-Path $repoRoot 'PhotoXfer Desktop.lnk'
$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($shortcutPath)
$shortcut.TargetPath = $executable
$shortcut.WorkingDirectory = $repoRoot
$shortcut.Description = 'Open the PhotoXfer backup window'
$shortcut.Save()
Write-Host "Created shortcut: $shortcutPath"
