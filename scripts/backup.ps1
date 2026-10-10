param(
    [string]$Source = 'C:\Users\Owner\iCloudPhotos',
    [string]$Library = 'C:\Users\Owner\Pictures\iPhone Backup',
    [string]$Archive = 'C:\Users\Owner\OneDrive\Photo Backups',
    [string]$Executable = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\release\photoxfer.exe'),
    [switch]$NoPause
)

$ErrorActionPreference = 'Stop'
$steps = @(
    [pscustomobject]@{ Name = 'Save photos and videos'; Arguments = @('import', '--from', $Source, '--to', $Library) },
    [pscustomobject]@{ Name = 'Check your saved library'; Arguments = @('check', '--from', $Source, '--to', $Library) },
    [pscustomobject]@{ Name = 'Update and check your second copy'; Arguments = @('archive', '--from', $Library, '--to', $Archive) }
)

$exitCode = 0
Write-Host 'Photo backup'
Write-Host "Read photos from: $Source"
Write-Host "Saved library:    $Library"
Write-Host "Second copy:      $Archive"

try {
    if (-not (Test-Path -LiteralPath $Executable -PathType Leaf)) {
        throw "PhotoXfer executable was not found: $Executable"
    }
    for ($stepIndex = 0; $stepIndex -lt $steps.Count; $stepIndex++) {
        $step = $steps[$stepIndex]
        Write-Host "`n$($stepIndex + 1) of 3: $($step.Name)"
        $arguments = $step.Arguments
        & $Executable @arguments
        $stepExitCode = $LASTEXITCODE
        if ($stepExitCode -ne 0) {
            Write-Host 'This step failed. The remaining steps did not run.' -ForegroundColor Red
            $exitCode = $stepExitCode
            break
        }
    }

} catch {
    Write-Host $_.Exception.Message -ForegroundColor Red
    $exitCode = 1
}

if ($exitCode -eq 0) {
    Write-Host "`nBackup finished successfully. Both local copies passed their checks." -ForegroundColor Green
    Write-Host 'Files removed from iCloud are still kept in your saved library and second copy.'
    Write-Host "`nBefore deleting photos from your phone or iCloud:" -ForegroundColor Yellow
    Write-Host '1. Confirm every photo you want to keep is saved, including both parts of any Live Photos.'
    Write-Host '2. Let OneDrive finish uploading, then download and verify a separate copy from OneDrive.'
    Write-Host 'This run has not completed those two checks. Keep your phone/iCloud copies until they are done.'
} else {
    Write-Host "`nBackup did not finish. Keep your phone/iCloud copies." -ForegroundColor Red
    Write-Host 'Read the error above and resolve it before running the backup again.'
}
if (-not $NoPause) {
    [void](Read-Host 'Press Enter to close')
}
exit $exitCode
