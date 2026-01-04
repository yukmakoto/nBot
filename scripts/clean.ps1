$ErrorActionPreference = "Stop"

Set-Location (Join-Path $PSScriptRoot "..")

$paths = @(
    "target",
    "dist",
    "frontend\\node_modules",
    "frontend\\dist",
    "webui\\node_modules",
    "webui\\public\\nbot_logo.png",
    "backend\\dist",
    "frontend\\public\\tailwind.css"
)

foreach ($p in $paths) {
    if (Test-Path $p) {
        Write-Host "Removing $p" -ForegroundColor Cyan
        Remove-Item -Recurse -Force $p
    }
}

Write-Host "Done." -ForegroundColor Green
