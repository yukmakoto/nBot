param(
    [string]$InstallDir = $(if ($env:NBOT_INSTALL_DIR) { $env:NBOT_INSTALL_DIR } else { (Join-Path $env:ProgramData "nbot") }),
    [string]$Namespace = $(if ($env:NBOT_DOCKERHUB_NAMESPACE) { $env:NBOT_DOCKERHUB_NAMESPACE } else { "yukmakoto" }),
    [string]$Tag = $(if ($env:NBOT_TAG) { $env:NBOT_TAG } else { "latest" }),
    [string]$Registry = $(if ($env:NBOT_DOCKER_REGISTRY) { $env:NBOT_DOCKER_REGISTRY } else { "docker.nailed.dev" }),
    [string]$TwemojiBaseUrl = $(if ($env:NBOT_TWEMOJI_BASE_URL) { $env:NBOT_TWEMOJI_BASE_URL } else { "https://cdn.staticfile.org/twemoji/14.0.2/svg/" }),
    [string]$NapCatImage = $(if ($env:NBOT_NAPCAT_IMAGE) { $env:NBOT_NAPCAT_IMAGE } else { "docker.nailed.dev/yukmakoto/napcat-docker:latest" }),
    [Alias("WebUiHost")]
    [string]$WebUiBindHost = $(if ($env:NBOT_WEBUI_BIND_HOST) { $env:NBOT_WEBUI_BIND_HOST } elseif ($env:NBOT_WEBUI_HOST) { $env:NBOT_WEBUI_HOST } else { "0.0.0.0" }),
    [string]$WebUiPublicHost = $(if ($env:NBOT_WEBUI_PUBLIC_HOST) { $env:NBOT_WEBUI_PUBLIC_HOST } else { "" }),
    [string]$WebUiPort = $(if ($env:NBOT_WEBUI_PORT) { $env:NBOT_WEBUI_PORT } else { "" }),
    [Alias("RenderHost")]
    [string]$RenderBindHost = $(if ($env:NBOT_RENDER_BIND_HOST) { $env:NBOT_RENDER_BIND_HOST } elseif ($env:NBOT_RENDER_HOST) { $env:NBOT_RENDER_HOST } else { "127.0.0.1" }),
    [string]$RenderPort = $(if ($env:NBOT_RENDER_PORT) { $env:NBOT_RENDER_PORT } else { "" }),
    [string]$ProjectName = $(if ($env:COMPOSE_PROJECT_NAME) { $env:COMPOSE_PROJECT_NAME } else { "nbot" }),
    [switch]$AutoReboot,
    [switch]$NoReboot,
    [switch]$Resume
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

# ============================================================================
#                              nBot Installer
#                    QQ Bot Framework - One-Click Deploy
# ============================================================================

# Enable UTF-8 output
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8

# ANSI Color support check
$SupportsAnsi = $true
try {
    $null = [Console]::Write("`e[0m")
} catch {
    $SupportsAnsi = $false
}

# Color functions
function Write-Color {
    param(
        [string]$Text,
        [ConsoleColor]$ForegroundColor = [ConsoleColor]::White,
        [switch]$NoNewline
    )
    if ($NoNewline) {
        Write-Host $Text -ForegroundColor $ForegroundColor -NoNewline
    } else {
        Write-Host $Text -ForegroundColor $ForegroundColor
    }
}

function Show-Banner {
    Clear-Host
    Write-Host ""
    Write-Color "                ____        __" -ForegroundColor Cyan
    Write-Color "      ____     / __ )____  / /_" -ForegroundColor Cyan
    Write-Color "     / __ \   / __  / __ \/ __/" -ForegroundColor Cyan
    Write-Color "    / / / /  / /_/ / /_/ / /_" -ForegroundColor Cyan
    Write-Color "   /_/ /_/  /_____/\____/\__/" -ForegroundColor Cyan
    Write-Host ""
    Write-Color "   QQ Bot Framework - One-Click Deploy" -ForegroundColor Cyan
    Write-Host ""
    Write-Color "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor DarkGray
    Write-Color -NoNewline "  Version: " -ForegroundColor DarkGray
    Write-Color -NoNewline "$Tag" -ForegroundColor White
    Write-Color -NoNewline "  |  Registry: " -ForegroundColor DarkGray
    Write-Color "$Registry" -ForegroundColor White
    Write-Color "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor DarkGray
    Write-Host ""
}

function Step([string]$msg) {
    Write-Color -NoNewline ">>> " -ForegroundColor Cyan
    Write-Color $msg -ForegroundColor White
}

function Success([string]$msg) {
    Write-Color -NoNewline "[OK] " -ForegroundColor Green
    Write-Color $msg -ForegroundColor Green
}

function Warn([string]$msg) {
    Write-Color -NoNewline "[!] " -ForegroundColor Yellow
    Write-Color $msg -ForegroundColor Yellow
}

function Info([string]$msg) {
    Write-Color -NoNewline "[i] " -ForegroundColor Blue
    Write-Color $msg -ForegroundColor Blue
}

function Die([string]$msg) {
    Write-Color -NoNewline "[X] ERROR: " -ForegroundColor Red
    Write-Color $msg -ForegroundColor Red
    exit 1
}

function Show-Section([string]$title) {
    Write-Host ""
    Write-Color "┌─────────────────────────────────────────────────────────────────────────────┐" -ForegroundColor Magenta
    Write-Color -NoNewline "│ " -ForegroundColor Magenta
    Write-Color -NoNewline $title -ForegroundColor White
    $padding = 77 - $title.Length
    Write-Color (" " * $padding + "│") -ForegroundColor Magenta
    Write-Color "└─────────────────────────────────────────────────────────────────────────────┘" -ForegroundColor Magenta
}

function Show-Progress {
    param(
        [string]$Activity,
        [int]$PercentComplete
    )
    $width = 40
    $filled = [math]::Floor($PercentComplete * $width / 100)
    $empty = $width - $filled
    $bar = ("█" * $filled) + ("░" * $empty)
    Write-Host -NoNewline "`r[$bar] $PercentComplete%"
}

function Show-Spinner {
    param(
        [string]$Message,
        [scriptblock]$ScriptBlock
    )
    $frames = @('⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏')
    $job = Start-Job -ScriptBlock $ScriptBlock

    $i = 0
    while ($job.State -eq 'Running') {
        Write-Host -NoNewline "`r$($frames[$i % $frames.Length]) $Message"
        Start-Sleep -Milliseconds 100
        $i++
    }

    $result = Receive-Job -Job $job
    Remove-Job -Job $job

    Write-Host "`r[OK] $Message                    "
    return $result
}

function Show-SuccessBanner {
    Write-Host ""
    Write-Color "   _____ _    _  _____ _____ ______  _____ _____" -ForegroundColor Green
    Write-Color "  / ____| |  | |/ ____/ ____|  ____|/ ____/ ____|" -ForegroundColor Green
    Write-Color " | (___ | |  | | |   | |    | |__  | (___| (___" -ForegroundColor Green
    Write-Color "  \___ \| |  | | |   | |    |  __|  \___ \\___ \" -ForegroundColor Green
    Write-Color "  ____) | |__| | |___| |____| |____ ____) |___) |" -ForegroundColor Green
    Write-Color " |_____/ \____/ \_____\_____|______|_____/_____/" -ForegroundColor Green
    Write-Host ""
}

function Show-InfoBox {
    param(
        [string]$Title,
        [string[]]$Lines
    )
    Write-Color "╭─ $Title ─────────────────────────────────────────────────────────────────╮" -ForegroundColor Cyan
    foreach ($line in $Lines) {
        Write-Color -NoNewline "│ " -ForegroundColor Cyan
        Write-Host $line
    }
    Write-Color "╰──────────────────────────────────────────────────────────────────────────────╯" -ForegroundColor Cyan
}

# Show banner
Show-Banner

function New-ApiToken {
    # 64 hex chars, URL-safe, no external dependencies.
    return ([guid]::NewGuid().ToString("N") + [guid]::NewGuid().ToString("N"))
}

function Normalize-RegistryHost([string]$r) {
    if (-not $r) { return "" }
    $r = $r.Trim()
    $r = $r -replace '^https?://', ''
    $r = $r.TrimEnd('/')
    return $r
}

function Write-DockerAuthConfig([string]$configDir, [string]$registry, [string]$user, [string]$password) {
    $reg = Normalize-RegistryHost $registry
    if (-not $reg) { return }

    New-Item -ItemType Directory -Force -Path $configDir | Out-Null

    $pair = "${user}:${password}"
    $auth = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($pair))

    $obj = @{
        auths = @{
            $reg = @{
                auth = $auth
            }
        }
    }

    $cfg = Join-Path $configDir "config.json"
    ($obj | ConvertTo-Json -Depth 10) | Set-Content -Path $cfg -Encoding UTF8
}

function Get-RegistryCredentialFromEnv {
    $u = $env:NBOT_DOCKER_REGISTRY_USERNAME
    if (-not $u) { $u = $env:NBOT_REGISTRY_USERNAME }
    $p = $env:NBOT_DOCKER_REGISTRY_PASSWORD
    if (-not $p) { $p = $env:NBOT_REGISTRY_PASSWORD }
    if ($u -and $p) {
        return @{ User = $u; Password = $p }
    }
    return $null
}

function Get-PublicIPv4 {
    try {
        $ip = (Invoke-RestMethod -UseBasicParsing -TimeoutSec 3 -Uri "https://api.ipify.org").ToString().Trim()
        if ($ip -match '^\d+\.\d+\.\d+\.\d+$') { return $ip }
    } catch {
    }
    return ""
}

function Resolve-WebUiDisplayHost([string]$bindHost, [string]$publicHost) {
    if ($publicHost -and $publicHost.Trim()) { return $publicHost.Trim() }
    if ($bindHost -and $bindHost.Trim() -and $bindHost -ne "0.0.0.0" -and $bindHost -ne "127.0.0.1" -and $bindHost -ne "localhost") {
        return $bindHost.Trim()
    }
    if ($bindHost -eq "0.0.0.0") {
        $ip = Get-PublicIPv4
        if ($ip) { return $ip }
    }
    return "127.0.0.1"
}

function Is-Admin {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    $p = New-Object Security.Principal.WindowsPrincipal($id)
    return $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-SelfScriptText {
    if ($PSCommandPath -and (Test-Path $PSCommandPath)) {
        return Get-Content -Path $PSCommandPath -Raw
    }
    try {
        if ($MyInvocation.MyCommand.ScriptBlock) {
            return $MyInvocation.MyCommand.ScriptBlock.ToString()
        }
    } catch {
    }
    return $null
}

function Relaunch-AsAdmin {
    $selfText = Get-SelfScriptText
    if (-not $selfText) {
        Die "无法自举获取脚本文本。请改用：iwr <url> -OutFile install.ps1; powershell -ExecutionPolicy Bypass -File install.ps1"
    }

    $tmp = Join-Path $env:TEMP ("nbot-install-docker-{0}.ps1" -f ([Guid]::NewGuid().ToString("N")))
    Set-Content -Path $tmp -Value $selfText -Encoding UTF8

    $argList = @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-File", "`"$tmp`"",
        "-InstallDir", "`"$InstallDir`"",
        "-Namespace", "`"$Namespace`"",
        "-Tag", "`"$Tag`"",
        "-Registry", "`"$Registry`"",
        "-TwemojiBaseUrl", "`"$TwemojiBaseUrl`"",
        "-NapCatImage", "`"$NapCatImage`"",
        "-WebUiBindHost", "`"$WebUiBindHost`"",
        "-WebUiPublicHost", "`"$WebUiPublicHost`"",
        "-WebUiPort", "`"$WebUiPort`"",
        "-RenderBindHost", "`"$RenderBindHost`"",
        "-RenderPort", "`"$RenderPort`"",
        "-ProjectName", "`"$ProjectName`""
    )
    if ($AutoReboot) { $argList += "-AutoReboot" }
    if ($NoReboot) { $argList += "-NoReboot" }
    if ($Resume) { $argList += "-Resume" }

    Step "Request admin privileges ..."
    Start-Process -FilePath "powershell" -Verb RunAs -ArgumentList ($argList -join " ")
    exit 0
}

function Read-EnvFile([string]$path) {
    $map = @{}
    if (-not (Test-Path $path)) { return $map }
    Get-Content -Path $path | ForEach-Object {
        $line = $_.Trim()
        if (-not $line -or $line.StartsWith("#")) { return }
        $idx = $line.IndexOf("=")
        if ($idx -lt 1) { return }
        $k = $line.Substring(0, $idx).Trim()
        $v = $line.Substring($idx + 1).Trim()
        if ($k) { $map[$k] = $v }
    }
    return $map
}

function Get-FreeTcpPort {
    param(
        [int]$Min = 32000,
        [int]$Max = 60000,
        [int]$Attempts = 120
    )

    for ($i = 1; $i -le $Attempts; $i++) {
        $port = Get-Random -Minimum $Min -Maximum $Max
        try {
            $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $port)
            $listener.Start()
            $listener.Stop()
            return $port
        } catch {
        }
    }
    throw "Failed to pick a free TCP port after $Attempts attempts."
}

function Enable-Feature([string]$name) {
    $f = Get-WindowsOptionalFeature -Online -FeatureName $name -ErrorAction SilentlyContinue
    if ($f -and $f.State -eq "Enabled") { return $false }
    $r = Enable-WindowsOptionalFeature -Online -FeatureName $name -All -NoRestart
    return [bool]$r.RestartNeeded
}

function Ensure-Wsl2 {
    Step "Ensure WSL2 prerequisites ..."
    $reboot = $false
    $reboot = (Enable-Feature "Microsoft-Windows-Subsystem-Linux") -or $reboot
    $reboot = (Enable-Feature "VirtualMachinePlatform") -or $reboot

    if ($reboot) {
        return $true
    }

    if (-not (Get-Command "wsl" -ErrorAction SilentlyContinue)) {
        Warn "wsl.exe not found after enabling features. You may need Windows Update."
        return $false
    }

    try {
        wsl --set-default-version 2 *> $null
    } catch {
        Warn "Failed to set WSL default version to 2: $($_.Exception.Message)"
    }

    try {
        wsl --update *> $null
    } catch {
        # harmless; on some systems this requires Store WSL
    }

    return $false
}

function Write-ResumeTask([string]$scriptPath, [string]$args) {
    $taskName = "nBot-Install-Resume"
    $cmd = "powershell -NoProfile -ExecutionPolicy Bypass -File `"$scriptPath`" $args"
    schtasks /Create /TN $taskName /SC ONLOGON /RL HIGHEST /F /TR $cmd | Out-Null
}

function Remove-ResumeTask {
    $taskName = "nBot-Install-Resume"
    schtasks /Delete /TN $taskName /F *> $null
}

function Ensure-DockerCliInPath {
    if (Get-Command "docker" -ErrorAction SilentlyContinue) { return }
    $candidates = @(
        (Join-Path $Env:ProgramFiles "Docker\Docker\resources\bin\docker.exe"),
        (Join-Path ${Env:ProgramFiles(x86)} "Docker\Docker\resources\bin\docker.exe")
    ) | Where-Object { $_ -and (Test-Path $_) }

    if ($candidates.Count -gt 0) {
        $dir = Split-Path -Parent $candidates[0]
        $env:PATH = "$dir;$env:PATH"
        return
    }
}

function Install-DockerDesktop {
    if (Get-Command "docker" -ErrorAction SilentlyContinue) {
        return
    }

    Step "Install Docker Desktop ..."
    $winget = Get-Command "winget" -ErrorAction SilentlyContinue
    if ($winget) {
        winget install -e --id Docker.DockerDesktop --accept-package-agreements --accept-source-agreements --silent --disable-interactivity
        Ensure-DockerCliInPath
        if (Get-Command "docker" -ErrorAction SilentlyContinue) { return }
    }

    $url = if ($env:NBOT_DOCKER_DESKTOP_INSTALLER_URL -and $env:NBOT_DOCKER_DESKTOP_INSTALLER_URL.Trim()) {
        $env:NBOT_DOCKER_DESKTOP_INSTALLER_URL.Trim()
    } else {
        "https://desktop.docker.com/win/main/amd64/Docker%20Desktop%20Installer.exe"
    }

    $tmp = Join-Path $env:TEMP ("DockerDesktopInstaller-{0}.exe" -f ([Guid]::NewGuid().ToString("N")))
    Step "Download: $url"
    Invoke-WebRequest -Uri $url -OutFile $tmp
    Start-Process -FilePath $tmp -ArgumentList "install --quiet --accept-license" -Wait
    Ensure-DockerCliInPath
    if (-not (Get-Command "docker" -ErrorAction SilentlyContinue)) {
        Die "Docker 安装后仍未找到 docker.exe，请重启电脑后再试。"
    }
}

function Ensure-DockerDaemon {
    Step "Ensure Docker daemon ..."
    try {
        docker info *> $null
        if ($LASTEXITCODE -eq 0) { return }
    } catch {
    }

    $dockerDesktop = Join-Path $Env:ProgramFiles "Docker\Docker\Docker Desktop.exe"
    if (Test-Path $dockerDesktop) {
        try { Start-Process $dockerDesktop | Out-Null } catch { }
    } else {
        Warn "Docker Desktop executable not found at: $dockerDesktop"
    }

    $maxAttempts = 120
    for ($i = 1; $i -le $maxAttempts; $i++) {
        try {
            docker info *> $null
            if ($LASTEXITCODE -eq 0) { return }
        } catch {
        }
        Start-Sleep -Seconds 2
    }
    Die "Docker daemon 未就绪：请确认 Docker Desktop 已启动（首次启动可能需要你手动点一次“Accept”）。"
}

if (-not (Is-Admin)) {
    Relaunch-AsAdmin
}

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

$envFile = Join-Path $InstallDir ".env"
$composeFile = Join-Path $InstallDir "docker-compose.yml"
$stateFile = Join-Path $InstallDir "install.state.json"
$selfCopy = Join-Path $InstallDir "install-docker.ps1"
$dockerConfigDir = Join-Path $InstallDir "docker-config"
New-Item -ItemType Directory -Force -Path $dockerConfigDir | Out-Null
$env:DOCKER_CONFIG = $dockerConfigDir

$selfText = Get-SelfScriptText
if ($selfText) {
    Set-Content -Path $selfCopy -Value $selfText -Encoding UTF8
}

$existing = Read-EnvFile $envFile

function Pick-Or-Use([string]$val, [string]$key, [string]$default) {
    if ($val -and $val.Trim()) { return $val.Trim() }
    if ($existing.ContainsKey($key) -and $existing[$key].Trim()) { return $existing[$key].Trim() }
    return $default
}

$Namespace = Pick-Or-Use $Namespace "NBOT_DOCKERHUB_NAMESPACE" "yukmakoto"
$Tag = Pick-Or-Use $Tag "NBOT_TAG" "latest"
$Registry = Pick-Or-Use $Registry "NBOT_DOCKER_REGISTRY" "docker.nailed.dev"
$TwemojiBaseUrl = Pick-Or-Use $TwemojiBaseUrl "NBOT_TWEMOJI_BASE_URL" "https://cdn.staticfile.org/twemoji/14.0.2/svg/"
$NapCatImage = Pick-Or-Use $NapCatImage "NBOT_NAPCAT_IMAGE" "docker.nailed.dev/yukmakoto/napcat-docker:latest"
$WebUiBindHost = Pick-Or-Use $WebUiBindHost "NBOT_WEBUI_BIND_HOST" ""
if (-not $WebUiBindHost) { $WebUiBindHost = Pick-Or-Use "" "NBOT_WEBUI_HOST" "0.0.0.0" }
$WebUiPublicHost = Pick-Or-Use $WebUiPublicHost "NBOT_WEBUI_PUBLIC_HOST" ""
$RenderBindHost = Pick-Or-Use $RenderBindHost "NBOT_RENDER_BIND_HOST" ""
if (-not $RenderBindHost) { $RenderBindHost = Pick-Or-Use "" "NBOT_RENDER_HOST" "127.0.0.1" }
$ProjectName = Pick-Or-Use $ProjectName "COMPOSE_PROJECT_NAME" "nbot"
$ApiToken = Pick-Or-Use $env:NBOT_API_TOKEN "NBOT_API_TOKEN" ""
if (-not $ApiToken) { $ApiToken = New-ApiToken }

$resolvedWebPort = Pick-Or-Use $WebUiPort "NBOT_WEBUI_PORT" ""
if (-not $resolvedWebPort) {
    $resolvedWebPort = (Get-FreeTcpPort).ToString()
}

$resolvedRenderPort = Pick-Or-Use $RenderPort "NBOT_RENDER_PORT" ""
if (-not $resolvedRenderPort) {
    for ($i = 1; $i -le 40; $i++) {
        $p = (Get-FreeTcpPort).ToString()
        if ($p -ne $resolvedWebPort) { $resolvedRenderPort = $p; break }
    }
    if (-not $resolvedRenderPort) { Die "无法挑选渲染服务空闲端口。" }
}

Step "Write $envFile"
@(
    "NBOT_DOCKERHUB_NAMESPACE=$Namespace"
    "NBOT_TAG=$Tag"
    "NBOT_DOCKER_REGISTRY=$Registry"
    "NBOT_TWEMOJI_BASE_URL=$TwemojiBaseUrl"
    "NBOT_NAPCAT_IMAGE=$NapCatImage"
    "NBOT_WEBUI_BIND_HOST=$WebUiBindHost"
    "NBOT_WEBUI_PUBLIC_HOST=$WebUiPublicHost"
    "NBOT_WEBUI_PORT=$resolvedWebPort"
    "NBOT_RENDER_BIND_HOST=$RenderBindHost"
    "NBOT_RENDER_PORT=$resolvedRenderPort"
    "NBOT_API_TOKEN=$ApiToken"
    "COMPOSE_PROJECT_NAME=$ProjectName"
) | Set-Content -Path $envFile -Encoding UTF8

Step "Write $composeFile"
@'
services:
  bot:
    image: ${NBOT_DOCKER_REGISTRY:-docker.nailed.dev}/${NBOT_DOCKERHUB_NAMESPACE:-yukmakoto}/nbot-bot:${NBOT_TAG:-latest}
    restart: unless-stopped
    ports:
      - "${NBOT_WEBUI_BIND_HOST:-0.0.0.0}:${NBOT_WEBUI_PORT:-32100}:32100"
    depends_on:
      - wkhtmltoimage
    environment:
      - RUST_LOG=info
      - WKHTMLTOIMAGE_URL=http://wkhtmltoimage:8080
      - NBOT_TWEMOJI_BASE_URL=${NBOT_TWEMOJI_BASE_URL:-https://cdn.staticfile.org/twemoji/14.0.2/svg/}
      - NBOT_DOCKERHUB_NAMESPACE=${NBOT_DOCKERHUB_NAMESPACE:-yukmakoto}
      - NBOT_DOCKER_REGISTRY=${NBOT_DOCKER_REGISTRY:-docker.nailed.dev}
      - NBOT_TAG=${NBOT_TAG:-latest}
      - NBOT_NAPCAT_IMAGE=${NBOT_NAPCAT_IMAGE:-docker.nailed.dev/yukmakoto/napcat-docker:latest}
      - NBOT_API_TOKEN=${NBOT_API_TOKEN}
      - NBOT_DOCKER_MODE=true
      - DOCKER_CONFIG=/docker-config
      - PROJECT_ROOT=/compose
      - COMPOSE_PROJECT_NAME=${COMPOSE_PROJECT_NAME:-nbot}
    volumes:
      - nbot_data:/app/data
      - /var/run/docker.sock:/var/run/docker.sock
      - ./:/compose:ro
      - ./docker-config:/docker-config:ro
    networks:
      - nbot_default

  wkhtmltoimage:
    image: ${NBOT_DOCKER_REGISTRY:-docker.nailed.dev}/${NBOT_DOCKERHUB_NAMESPACE:-yukmakoto}/nbot-render:${NBOT_TAG:-latest}
    restart: unless-stopped
    ports:
      - "${NBOT_RENDER_BIND_HOST:-127.0.0.1}:${NBOT_RENDER_PORT:-32180}:8080"
    environment:
      - RUST_LOG=info
      - PORT=8080
    networks:
      - nbot_default
    labels:
      - "nbot.infra=true"
      - "nbot.infra.name=wkhtmltoimage"
      - "nbot.infra.description=HTML 转图片服务"

volumes:
  nbot_data:

networks:
  nbot_default:
    name: nbot_default
    driver: bridge
'@ | Set-Content -Path $composeFile -Encoding UTF8

$stage = "start"
if (Test-Path $stateFile) {
    try {
        $stage = (Get-Content -Path $stateFile -Raw | ConvertFrom-Json).Stage
    } catch {
        $stage = "start"
    }
}

if ($stage -eq "start") {
    $needReboot = Ensure-Wsl2
    if ($needReboot) {
        Step "WSL2 prerequisites enabled; reboot required."
        @{ Stage = "after_wsl_reboot" } | ConvertTo-Json | Set-Content -Path $stateFile -Encoding UTF8

        $args = "-InstallDir `"$InstallDir`" -Resume"
        Write-ResumeTask $selfCopy $args

        Warn "需要重启一次才能继续安装（脚本已设置为开机自动续跑，不需要你再次执行命令）。"
        if ($NoReboot) {
            Warn "你选择了 -NoReboot：请自行重启电脑后登录，安装会自动继续。"
            exit 0
        }

        if ($AutoReboot) {
            Step "Rebooting in 8 seconds ..."
            Start-Sleep -Seconds 8
            Restart-Computer -Force
            exit 0
        }

        $ans = Read-Host "是否现在重启？输入 Y 立即重启 / N 稍后手动重启"
        if ($ans -and $ans.Trim().ToUpperInvariant() -eq "Y") {
            Step "Rebooting in 8 seconds ..."
            Start-Sleep -Seconds 8
            Restart-Computer -Force
        } else {
            Warn "请稍后手动重启电脑并登录，安装会自动继续。"
        }
        exit 0
    }
}

if ($stage -eq "after_wsl_reboot") {
    try {
        wsl --set-default-version 2 *> $null
    } catch {
    }
}

Ensure-DockerCliInPath
Install-DockerDesktop
Ensure-DockerDaemon

Step "Pull images (no build) ..."
Push-Location $InstallDir
try {
    docker compose pull
    if ($LASTEXITCODE -ne 0) {
        $pullOut = docker compose pull 2>&1
        $msg = ($pullOut | Out-String)
        if ($msg -match '(?i)unauthorized|authentication required|denied') {
            $reg = Normalize-RegistryHost $Registry
            if ($reg -and $reg -notmatch '^(docker\.io|index\.docker\.io|registry-1\.docker\.io)$') {
                Write-Host ""
                Warn "镜像拉取需要登录镜像仓库：$reg"
                $cred = Get-RegistryCredentialFromEnv
                if ($cred) {
                    Write-DockerAuthConfig $dockerConfigDir $reg $cred.User $cred.Password
                    $env:NBOT_DOCKER_REGISTRY_PASSWORD = $null
                    $env:NBOT_REGISTRY_PASSWORD = $null
                    Step "Registry 登录成功：$reg"
                } else {
                    $user = Read-Host "用户名（留空取消）"
                    if (-not ($user -and $user.Trim())) {
                        Die ("镜像拉取失败（未登录）: `n" + $msg)
                    }
                    $sec = Read-Host "密码（不会回显）" -AsSecureString
                    $bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($sec)
                    try {
                        $plain = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr)
                    } finally {
                        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr)
                    }
                    Write-DockerAuthConfig $dockerConfigDir $reg $user $plain
                    $plain = $null
                    Step "Registry 登录成功：$reg"
                }

                $pullOut = @()
                docker compose pull
                if ($LASTEXITCODE -ne 0) {
                    $pullOut = docker compose pull 2>&1
                    Die ("镜像拉取失败（已登录仍失败）: `n" + ($pullOut | Out-String))
                }
            } else {
                Die ("镜像拉取失败（需要登录）: `n" + $msg)
            }
        } else {
            Die ("镜像拉取失败: `n" + $msg)
        }
    }

    Step "Pull NapCat image: $NapCatImage ..."
    docker pull $NapCatImage
    if ($LASTEXITCODE -ne 0) {
        Warn "NapCat 镜像拉取失败，创建机器人实例时会自动重试"
    }

    Step "Start services ..."
    docker compose up -d
} finally {
    Pop-Location
}

Step "Health check (WebUI) ..."
$ok = $false
for ($i = 0; $i -lt 30; $i++) {
    try {
        $resp = Invoke-WebRequest -UseBasicParsing -TimeoutSec 2 -Uri ("http://127.0.0.1:{0}/" -f $resolvedWebPort)
        if ($resp.StatusCode -ge 200 -and $resp.StatusCode -lt 500) { $ok = $true; break }
    } catch {
    }
    Start-Sleep -Seconds 1
}
if (-not $ok) {
    Write-Host ""
    Warn ("WebUI 本机探测失败：127.0.0.1:{0}" -f $resolvedWebPort)
    Write-Host ""
    Warn "容器状态："
    try { Push-Location $InstallDir; docker compose ps -a } catch { } finally { Pop-Location }
    Write-Host ""
    Warn "bot 日志（最后 200 行）："
    try { Push-Location $InstallDir; docker compose logs --tail 200 bot } catch { } finally { Pop-Location }
    Write-Host ""
    Warn "wkhtmltoimage 日志（最后 200 行）："
    try { Push-Location $InstallDir; docker compose logs --tail 200 wkhtmltoimage } catch { } finally { Pop-Location }
    Die "启动失败：请根据上方日志排查。"
}

Remove-ResumeTask
Remove-Item -Force $stateFile -ErrorAction SilentlyContinue

$displayHost = Resolve-WebUiDisplayHost $WebUiBindHost $WebUiPublicHost
$webUrl = "http://$displayHost`:$resolvedWebPort"

Show-SuccessBanner

Show-InfoBox -Title "安装完成" -Lines @(
    "[OK] 安装目录: $InstallDir",
    "[OK] 部署模式: Docker",
    "",
    "WebUI 地址: http://$displayHost`:$resolvedWebPort",
    "WebUI 绑定: $WebUiBindHost`:$resolvedWebPort",
    "渲染服务: http://$RenderBindHost`:$resolvedRenderPort/health"
)

Write-Host ""
Write-Color "╭─────────────────────────────────────────────────────────────────────────────╮" -ForegroundColor Yellow
Write-Color -NoNewline "│" -ForegroundColor Yellow
Write-Color -NoNewline "                          " -ForegroundColor Yellow
Write-Color -NoNewline "nBot 认证信息" -ForegroundColor White
Write-Color "                                   │" -ForegroundColor Yellow
Write-Color "├─────────────────────────────────────────────────────────────────────────────┤" -ForegroundColor Yellow
Write-Color -NoNewline "│  API Token: " -ForegroundColor Yellow
Write-Color "$ApiToken" -ForegroundColor White
Write-Color "╰─────────────────────────────────────────────────────────────────────────────╯" -ForegroundColor Yellow

if ($WebUiBindHost -eq "0.0.0.0") {
    Write-Host ""
    Warn "已允许公网/局域网访问。请确保防火墙/安全组放行端口：$resolvedWebPort"
} else {
    Info "当前仅本机可访问。如需公网/局域网访问，请将 NBOT_WEBUI_BIND_HOST 设为 0.0.0.0 后重新运行脚本。"
}

Write-Host ""
Write-Color "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor DarkGray
Write-Color "  常用命令：" -ForegroundColor DarkGray
Write-Color -NoNewline "    查看状态: " -ForegroundColor DarkGray
Write-Color "cd $InstallDir; docker compose ps" -ForegroundColor White
Write-Color -NoNewline "    查看日志: " -ForegroundColor DarkGray
Write-Color "cd $InstallDir; docker compose logs -f" -ForegroundColor White
Write-Color -NoNewline "    重启服务: " -ForegroundColor DarkGray
Write-Color "cd $InstallDir; docker compose restart" -ForegroundColor White
Write-Color -NoNewline "    停止服务: " -ForegroundColor DarkGray
Write-Color "cd $InstallDir; docker compose down" -ForegroundColor White
Write-Color "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor DarkGray
Write-Host ""

try { Start-Process $webUrl | Out-Null } catch { }
