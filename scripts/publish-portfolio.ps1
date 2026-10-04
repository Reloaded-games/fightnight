<#
.SYNOPSIS
Package a committed production build into the Reloaded portfolio checkout.
.EXAMPLE
./scripts/publish-portfolio.ps1 -PortfolioPath ../reloaded-company-portfolio -ReleaseCommit <full-commit-sha>
.NOTES
Run scripts/build.ps1 first, commit the release, then call this script. It copies
only the production runtime and preserves every previous versioned release.
Publishing to the domain happens through the portfolio's normal Git deployment.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $PortfolioPath,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[a-fA-F0-9]{40}$')]
    [string] $ReleaseCommit
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$sourceRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$portfolioRoot = (Resolve-Path -LiteralPath $PortfolioPath).ProviderPath
$releaseCommit = $ReleaseCommit.ToLowerInvariant()
$assetDirectory = 'modes-' + $releaseCommit.Substring(0, 8)
$distRoot = Join-Path $sourceRoot 'dist'
$publicRoot = Join-Path $portfolioRoot 'public/fightnight-assets'
$releaseRoot = Join-Path $publicRoot $assetDirectory
$utf8 = [Text.UTF8Encoding]::new($false)

function Get-Sha256([string] $Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-TextSha256([string] $Text) {
    $algorithm = [Security.Cryptography.SHA256]::Create()
    try {
        $bytes = $algorithm.ComputeHash($utf8.GetBytes($Text))
        return ([BitConverter]::ToString($bytes)).Replace('-', '').ToLowerInvariant()
    } finally {
        $algorithm.Dispose()
    }
}

function Read-IntegerConstant([string] $Path, [string] $Name) {
    $text = [IO.File]::ReadAllText((Join-Path $sourceRoot $Path))
    $pattern = 'pub\s+const\s+' + [regex]::Escape($Name) + '\s*:\s*\w+\s*=\s*(\d+)(?:\.0)?\s*;'
    $match = [regex]::Match($text, $pattern)
    if (-not $match.Success) {
        throw "Cannot read release constant $Name from $Path."
    }
    return [int] $match.Groups[1].Value
}

if (-not (Test-Path -LiteralPath (Join-Path $portfolioRoot 'routes/web.php') -PathType Leaf) -or
    -not (Test-Path -LiteralPath (Join-Path $portfolioRoot 'public') -PathType Container)) {
    throw 'PortfolioPath must identify the Laravel portfolio checkout, including public/ and routes/web.php.'
}

$currentCommit = (& git -C $sourceRoot rev-parse HEAD | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or $currentCommit -ne $releaseCommit) {
    throw "ReleaseCommit must match the Fightnight checkout HEAD ($currentCommit)."
}
$pendingChanges = @(& git -C $sourceRoot status --porcelain --untracked-files=normal)
if ($LASTEXITCODE -ne 0 -or $pendingChanges.Count -ne 0) {
    throw 'Commit the Fightnight release before packaging it into the portfolio.'
}

# Explicit runtime allowlist: never copy dev.html, tools, screenshots, source
# archives or any files left over by a previous local build.
$runtimeFiles = @(
    'index.html', 'game.js', 'hud.js', 'icons.js', 'audio.js', 'style.css',
    'net.js', 'multiplayer.js', 'pkg/fightnight.js', 'pkg/fightnight_bg.wasm'
)
foreach ($relativePath in $runtimeFiles) {
    $sourcePath = Join-Path $distRoot $relativePath
    if (-not (Test-Path -LiteralPath $sourcePath -PathType Leaf)) {
        throw "Production build is missing dist/$relativePath. Run scripts/build.ps1."
    }
    if (-not $relativePath.StartsWith('pkg/')) {
        $webPath = Join-Path $sourceRoot ('web/' + $relativePath)
        if ((Get-Sha256 $sourcePath) -ne (Get-Sha256 $webPath)) {
            throw "dist/$relativePath differs from the committed web source. Rebuild before packaging."
        }
    }
}

$manifestPath = Join-Path $sourceRoot 'assets/cartoon/manifest.json'
$assetManifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
if (@($assetManifest.meshes.PSObject.Properties).Count -ne 37 -or
    @($assetManifest.animations).Count -ne 9 -or
    $assetManifest.animations -notcontains 'Aim' -or
    $assetManifest.animations -notcontains 'Fire' -or
    $assetManifest.weapons -notcontains 'AK-47' -or
    $assetManifest.vehicles -notcontains 'Island Buggy' -or
    $assetManifest.vehicles -notcontains 'Island Roadster') {
    throw 'The Blender asset manifest must contain the current 37 meshes, 9 animation clips, AK-47 and both vehicles.'
}
foreach ($mesh in $assetManifest.meshes.PSObject.Properties) {
    if ($mesh.Value.vertices -le 0 -or $mesh.Value.vertices -ge 5000) {
        throw "Mesh $($mesh.Name) exceeds the game's established vertex budget."
    }
}

$wasmPath = Join-Path $distRoot 'pkg/fightnight_bg.wasm'
$wasmStream = [IO.File]::OpenRead($wasmPath)
try {
    $header = [byte[]]::new(8)
    if ($wasmStream.Read($header, 0, 8) -ne 8 -or
        [BitConverter]::ToString($header) -ne '00-61-73-6D-01-00-00-00') {
        throw 'The production bundle does not contain a valid WebAssembly module.'
    }
} finally {
    $wasmStream.Dispose()
}
if ((Get-Item -LiteralPath $wasmPath).LastWriteTimeUtc -lt
    (Get-Item -LiteralPath $manifestPath).LastWriteTimeUtc) {
    throw 'The WASM build predates the Blender asset manifest. Rebuild before packaging.'
}

$indexHtml = [IO.File]::ReadAllText((Join-Path $distRoot 'index.html'))
if ($indexHtml -notmatch '(?i)<head(?:\s[^>]*)?>') {
    throw 'Production index.html has no head element.'
}
$indexHtml = [regex]::Replace($indexHtml, '(?is)<base\b[^>]*>\s*', '')
$baseElement = '<base href="/fightnight-assets/' + $assetDirectory + '/">'
$headPattern = [regex]::new('(?i)(<head(?:\s[^>]*)?>)')
$indexHtml = $headPattern.Replace($indexHtml, ('$1' + "`n" + $baseElement), 1)

$fileHashes = [ordered]@{}
foreach ($relativePath in $runtimeFiles) {
    $fileHashes[$relativePath] = if ($relativePath -eq 'index.html') {
        Get-TextSha256 $indexHtml
    } else {
        Get-Sha256 (Join-Path $distRoot $relativePath)
    }
}

$features = [ordered]@{
    worldSize = Read-IntegerConstant 'crates/fn-core/src/world/mod.rs' 'WORLD_SIZE'
    modes = @('battle-royale', 'zero-build', 'lego')
    multiplayerHost = 'room-creator'
    vehicles = @($assetManifest.vehicles)
    weapons = @($assetManifest.weapons)
    maxHumans = Read-IntegerConstant 'crates/fn-core/src/net/proto.rs' 'MAX_HUMANS'
    maxVehicles = Read-IntegerConstant 'crates/fn-core/src/game/vehicles.rs' 'MAX_VEHICLES'
    brickMeshes = 44
    networkVersion = Read-IntegerConstant 'crates/fn-core/src/net/proto.rs' 'VERSION'
}

# Existing versioned directories are immutable. A repeated packaging command
# may reuse an identical release, but cannot overwrite a different old bundle.
if (Test-Path -LiteralPath $releaseRoot) {
    foreach ($relativePath in $runtimeFiles) {
        $publishedPath = Join-Path $releaseRoot $relativePath
        if (-not (Test-Path -LiteralPath $publishedPath -PathType Leaf) -or
            (Get-Sha256 $publishedPath) -ne $fileHashes[$relativePath]) {
            throw "$assetDirectory already exists with different files; previous releases are preserved."
        }
    }
    if (@(Get-ChildItem -LiteralPath $releaseRoot -Recurse -File).Count -ne $runtimeFiles.Count) {
        throw "$assetDirectory contains unexpected files and cannot be reused."
    }
} else {
    New-Item -ItemType Directory -Path (Join-Path $releaseRoot 'pkg') -Force | Out-Null
    foreach ($relativePath in $runtimeFiles) {
        $destinationPath = Join-Path $releaseRoot $relativePath
        if ($relativePath -eq 'index.html') {
            [IO.File]::WriteAllText($destinationPath, $indexHtml, $utf8)
        } else {
            Copy-Item -LiteralPath (Join-Path $distRoot $relativePath) -Destination $destinationPath
        }
        if ((Get-Sha256 $destinationPath) -ne $fileHashes[$relativePath]) {
            throw "Published $relativePath does not match the validated production build."
        }
    }
}

$builtAt = [DateTime]::UtcNow.ToString('o')
$releaseJsonPath = Join-Path $publicRoot 'release.json'
if (Test-Path -LiteralPath $releaseJsonPath -PathType Leaf) {
    $previousRelease = Get-Content -LiteralPath $releaseJsonPath -Raw | ConvertFrom-Json
    if ($previousRelease.commit -eq $releaseCommit -and
        $previousRelease.wasmSha256 -eq $fileHashes['pkg/fightnight_bg.wasm']) {
        $builtAt = $previousRelease.builtAt
    }
}

$release = [ordered]@{
    commit = $releaseCommit
    wasmSha256 = $fileHashes['pkg/fightnight_bg.wasm']
    repository = 'Reloaded-games/fightnight'
    assetDirectory = $assetDirectory
    builtAt = $builtAt
    features = $features
    assets = $assetManifest
    files = $fileHashes
}
[IO.File]::WriteAllText((Join-Path $publicRoot 'index.html'), $indexHtml, $utf8)
[IO.File]::WriteAllText($releaseJsonPath, (($release | ConvertTo-Json -Depth 25) + "`n"), $utf8)

Write-Host "Packaged Fightnight $releaseCommit into $releaseRoot"
Write-Host "WASM SHA256: $($release.wasmSha256)"
Write-Host 'Run the portfolio Fightnight tests, then commit and push the portfolio to deploy.'
