$ErrorActionPreference = 'Stop'
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    cargo build -p fn-web --target wasm32-unknown-unknown --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'WASM build failed' }
    wasm-bindgen --target web --no-typescript --out-dir dist/pkg --out-name fightnight target/wasm32-unknown-unknown/release/fn_web.wasm
    if ($LASTEXITCODE -ne 0) { throw 'wasm-bindgen failed' }
    Copy-Item web/* dist -Recurse -Force
    Write-Host 'Fightnight is ready in dist/. Serve on localhost or HTTPS.'
} finally {
    Pop-Location
}
