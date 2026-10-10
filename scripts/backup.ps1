param(
    [string]$Source = 'C:\Users\Owner\iCloudPhotos',
    [string]$Library = 'C:\Users\Owner\Pictures\photoxfer',
    [string]$Archive = 'C:\Users\Owner\OneDrive\PhotoXfer Archive',
    [string]$Executable = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\release\photoxfer.exe'),
    [switch]$NoPause
)

$ErrorActionPreference = 'Stop'
$steps = @(
    [pscustomobject]@{ Name = 'Import'; Arguments = @('import', '--from', $Source, '--to', $Library) },
    [pscustomobject]@{ Name = 'Check'; Arguments = @('check', '--from', $Source, '--to', $Library) },
    [pscustomobject]@{ Name = 'Archive'; Arguments = @('archive', '--from', $Library, '--to', $Archive) }
)

$exitCode = 0
$completed = [System.Collections.Generic.List[string]]::new()
Write-Host 'PhotoXfer backup'
Write-Host "Source:  $Source"
Write-Host "Library: $Library"
Write-Host "Archive: $Archive"
Write-Host "Program: $Executable"

try {
    if (-not (Test-Path -LiteralPath $Executable -PathType Leaf)) {
        throw "PhotoXfer executable was not found: $Executable"
    }
    foreach ($step in $steps) {
        Write-Host "`n--- $($step.Name) ---"
        $arguments = $step.Arguments
        & $Executable @arguments
        $stepExitCode = $LASTEXITCODE
        if ($stepExitCode -ne 0) {
            Write-Host "$($step.Name) failed with exit code $stepExitCode. Later steps were skipped." -ForegroundColor Red
            $exitCode = $stepExitCode
            break
        }
        $completed.Add($step.Name)
    }

} catch {
    Write-Host $_.Exception.Message -ForegroundColor Red
    $exitCode = 1
}

Write-Host "`nSummary: $($completed -join ', ') completed."
Write-Host 'Cloud upload is not yet verified; keep the source files.' -ForegroundColor Yellow
Write-Host 'Cleanup remains blocked until phone completeness (including Live Photos) and a full cloud-restored manifest batch are verified.' -ForegroundColor Yellow
if (-not $NoPause) {
    [void](Read-Host 'Press Enter to close')
}
exit $exitCode
