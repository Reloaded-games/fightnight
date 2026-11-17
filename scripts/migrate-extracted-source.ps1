# User requested moving the extracted material into Fightnight, then removing its old folder.
$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$sourceRoot = [IO.Path]::GetFullPath((Join-Path $projectRoot 'fortnite'))
$destinationRoot = [IO.Path]::GetFullPath((Join-Path $projectRoot 'docs/imported'))
$reportPath = Join-Path $destinationRoot 'import-verification.json'
$verified = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json
if (-not $verified.sourceDirectoryVerified -or $verified.catalogAssets -le 0 -or $verified.catalogAssets -ne $verified.exportedFiles -or $verified.errors.Count) {
    throw 'Complete source catalog and directory identity verification are required before migration.'
}
if ((Resolve-Path -LiteralPath $sourceRoot).Path -ne $sourceRoot) { throw 'Unexpected source root.' }
foreach ($root in @($sourceRoot, $destinationRoot)) {
    $resolved = (Resolve-Path -LiteralPath $root).Path
    if (-not $resolved.StartsWith($projectRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Path outside Fightnight: $resolved"
    }
    if ((Get-Item -LiteralPath $root).Attributes -band [IO.FileAttributes]::ReparsePoint) {
        throw "Refusing to move through a junction: $root"
    }
}
function Get-DirectoryIdentity([string]$literalPath) {
    $result = & fsutil file queryFileID $literalPath
    if ($LASTEXITCODE -ne 0 -or ($result -join ' ') -notmatch '0x[0-9a-fA-F]+') { throw 'Could not verify NTFS file identity.' }
    return $Matches[0]
}
$mapping = @{
    'fortnite-analysis' = 'fortnite-analysis'
    'fortnite-webgpu' = 'fortnite-webgpu'
    'README.md' = 'source-README.md'
}
$planPath = Join-Path $destinationRoot 'migration-plan.json'
$previousPlan = if (Test-Path -LiteralPath $planPath) { @(Get-Content -LiteralPath $planPath -Raw | ConvertFrom-Json) } else { @() }
$plan = foreach ($name in $mapping.Keys) {
    $from = Join-Path $sourceRoot $name
    $target = [IO.Path]::GetFullPath((Join-Path $destinationRoot $mapping[$name]))
    if (-not $target.StartsWith($destinationRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Target escaped archive folder.' }
    if (Test-Path -LiteralPath $from) {
        if ((Get-Item -LiteralPath $from).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Source entry is a junction.' }
        if (Test-Path -LiteralPath $target) { throw "Destination already exists: $target" }
        [pscustomobject]@{ from = $from; to = $target; beforeFileId = Get-DirectoryIdentity $from }
    } else {
        $previous = @($previousPlan | Where-Object { $_.from -eq $from -and $_.to -eq $target })
        if ($previous.Count -ne 1 -or (Get-DirectoryIdentity $target) -ne $previous[0].beforeFileId) { throw 'Cannot resume an unverified move.' }
        $previous[0]
    }
}
$plan | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $destinationRoot 'migration-plan.json') -Encoding utf8
$results = foreach ($item in $plan) {
    if (Test-Path -LiteralPath $item.from) {
        # Directory.Move performs the checked same-volume rename directly. The
        # PowerShell provider can traverse/recreate children, losing directory IDs.
        if ((Get-Item -LiteralPath $item.from).PSIsContainer) {
            [IO.Directory]::Move($item.from, $item.to)
        } else {
            [IO.File]::Move($item.from, $item.to)
        }
    }
    $afterId = Get-DirectoryIdentity $item.to
    if ($afterId -ne $item.beforeFileId) { throw 'Same-volume move did not preserve file identity.' }
    [pscustomobject]@{ from = $item.from; to = $item.to; beforeFileId = $item.beforeFileId; afterFileId = $afterId; preserved = $true }
}
if (@(Get-ChildItem -LiteralPath $sourceRoot -Force).Count -ne 0) { throw 'Source is not empty; no deletion performed.' }
# Deliberately non-recursive: only the now-empty, explicitly requested folder is removed.
Remove-Item -LiteralPath $sourceRoot -Force
$result = [pscustomobject]@{
    date = '2026-11-16'
    sourceRemoved = -not (Test-Path -LiteralPath $sourceRoot)
    catalogAssets = $verified.catalogAssets
    moves = $results
    preservation = 'Same-volume NTFS rename; directory/file identities retained. No source contents discarded.'
}
$result | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $destinationRoot 'migration-verification.json') -Encoding utf8
$result | ConvertTo-Json -Depth 6
