param([string]$UnrealRoot = 'X:\UE\UE_5.8', [string]$VisualStudio = 'X:\vstudio')
$ErrorActionPreference = 'Stop'
$project = Split-Path $PSScriptRoot -Parent
$sdk = Join-Path $UnrealRoot 'Engine\Source\Runtime\RadAudioCodec\SDK'
$build = Join-Path $project 'target\native-audio'
$null = New-Item -ItemType Directory -Force -Path $build
$source = Join-Path $project 'tools\assetgen\decode_rada.cpp'
$vcvars = Join-Path $VisualStudio 'VC\Auxiliary\Build\vcvars64.bat'
$buildCommand = '"' + $vcvars + '" >nul && cl /nologo /O2 /EHsc /std:c++17 /I"' + $sdk + '\Include" "' + $source + '" "' + $sdk + '\Lib\radaudio_decoder_win64.lib" /Fe:decode_rada.exe'
Push-Location $build
try {
    & cmd.exe /d /s /c $buildCommand
    if ($LASTEXITCODE -ne 0) { throw 'Native audio build failed' }
    Get-FileHash -LiteralPath (Join-Path $build 'decode_rada.exe') -Algorithm SHA256
} finally { Pop-Location }
