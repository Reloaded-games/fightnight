$ErrorActionPreference = 'Stop'
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    cargo build -p fn-web --target wasm32-unknown-unknown --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'WASM build failed' }
    $bindgen = if ($env:WASM_BINDGEN) { $env:WASM_BINDGEN } else { 'wasm-bindgen' }
    & $bindgen --target web --no-typescript --out-dir dist/pkg --out-name fightnight target/wasm32-unknown-unknown/release/fn_web.wasm
    if ($LASTEXITCODE -ne 0) { throw 'wasm-bindgen failed' }
    Copy-Item web/* dist -Recurse -Force
    python tools/assetgen/stage_imported.py
    if ($LASTEXITCODE -ne 0) { throw 'Imported assets staging failed' }
    if (Test-Path -LiteralPath 'target/native-audio/decode_rada.exe') {
        python tools/assetgen/prepare_native_audio.py
        if ($LASTEXITCODE -ne 0) { throw 'Native sound preparation failed' }
    }
    Write-Host 'Fightnight is ready in dist/. Serve on localhost or HTTPS.'
} finally {
    Pop-Location
}
