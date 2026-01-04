$ErrorActionPreference = "Stop"

$root = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $root
Write-Host "Project Root: $root" -ForegroundColor Magenta

if (-not (Get-Command "cargo" -ErrorAction SilentlyContinue)) {
    Write-Error "Cargo not found. Install Rust: https://rustup.rs/"
}
if (Get-Command "rustc" -ErrorAction SilentlyContinue) {
    $rustcVersionText = (& rustc --version) 2>$null
    if ($rustcVersionText) {
        $rustcVersion = ($rustcVersionText -split " ")[1]
        if ([Version]$rustcVersion -lt [Version]"1.88.0") {
            Write-Error "Rust toolchain too old ($rustcVersion). Require rustc >= 1.88.0"
        }
    }
}
if (-not (Get-Command "npm" -ErrorAction SilentlyContinue)) {
    Write-Error "npm not found. Install Node.js (>= 20)."
}

if (-not (Get-Command "docker" -ErrorAction SilentlyContinue)) {
    Write-Error "Docker not found. Install Docker Desktop and ensure `docker` is in PATH."
}

Write-Host ">>> Tool: start wkhtmltoimage renderer" -ForegroundColor Cyan
Set-Location $root
$env:NBOT_RENDER_PORT = "32180"
docker compose up -d --build wkhtmltoimage | Out-Host
$env:WKHTMLTOIMAGE_URL = "http://localhost:$($env:NBOT_RENDER_PORT)"

Write-Host ">>> WebUI (React): install deps" -ForegroundColor Cyan
Set-Location (Join-Path $root "webui")
cmd /c npm install

Write-Host ">>> WebUI (React): build dist/" -ForegroundColor Cyan
cmd /c npm run build

Write-Host ">>> Start backend" -ForegroundColor Green
Set-Location $root
$env:RUST_LOG = "backend=info,tower_http=warn"
$env:NBOT_PORT = "32100"
Write-Host "WebUI: http://localhost:$($env:NBOT_PORT)" -ForegroundColor White
cargo run -p backend --bin backend
