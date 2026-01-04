$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

function Step([string]$msg) { Write-Host ">>> $msg" -ForegroundColor Cyan }
function Die([string]$msg) { Write-Host "ERROR: $msg" -ForegroundColor Red; exit 1 }

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $Root

$EnvFile = Join-Path $Root ".env"
$RunDir = Join-Path $Root "run"
$LogDir = Join-Path $Root "logs"

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

function Get-FreeTcpPort([int]$Min = 32000, [int]$Max = 60000, [int]$Attempts = 120) {
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

function Test-TcpPortInUse([int]$Port) {
    try {
        $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
        $listener.Start()
        $listener.Stop()
        return $false
    } catch {
        return $true
    }
}

function Ensure-Winget {
    if (-not (Get-Command "winget" -ErrorAction SilentlyContinue)) {
        Die "winget 未安装：请安装 Microsoft Store 的 App Installer（或手动安装 wkhtmltopdf + ffmpeg）。"
    }
}

function Invoke-WingetInstall([string]$Id, [string]$Name) {
    Ensure-Winget

    $safe = ($Id -replace "[^a-zA-Z0-9._-]", "_") -replace "[.]", "_"
    $outPath = Join-Path $LogDir ("winget-{0}.out.log" -f $safe)
    $errPath = Join-Path $LogDir ("winget-{0}.err.log" -f $safe)

    Step "Install $Name ... (logs: $outPath, $errPath)"

    $args = @(
        "install",
        "-e",
        "--id", $Id,
        "--source", "winget",
        "--accept-package-agreements",
        "--accept-source-agreements",
        "--silent",
        "--disable-interactivity"
    )

    $proc = Start-Process -FilePath "winget" -ArgumentList $args -NoNewWindow -Wait -PassThru -RedirectStandardOutput $outPath -RedirectStandardError $errPath
    if ($proc.ExitCode -ne 0) {
        Die "$Name 安装失败 (exit=$($proc.ExitCode))，日志：$outPath / $errPath"
    }
}

function Ensure-Wkhtmltoimage {
    if ($env:WKHTMLTOIMAGE_BIN -and (Test-Path $env:WKHTMLTOIMAGE_BIN)) { return }
    $bundled = Join-Path $Root "tools\\wkhtmltox\\bin\\wkhtmltoimage.exe"
    if (Test-Path $bundled) {
        $env:WKHTMLTOIMAGE_BIN = $bundled
        return
    }
    if (Get-Command "wkhtmltoimage" -ErrorAction SilentlyContinue) { return }

    $candidates = @(
        (Join-Path $Env:ProgramFiles "wkhtmltopdf\\bin\\wkhtmltoimage.exe"),
        (Join-Path ${Env:ProgramFiles(x86)} "wkhtmltopdf\\bin\\wkhtmltoimage.exe")
    ) | Where-Object { $_ -and (Test-Path $_) }

    if ($candidates.Count -gt 0) {
        $env:WKHTMLTOIMAGE_BIN = $candidates[0]
        return
    }

    Invoke-WingetInstall -Id "wkhtmltopdf.wkhtmltox" -Name "wkhtmltopdf (wkhtmltoimage)"

    $candidate = Join-Path $Env:ProgramFiles "wkhtmltopdf\\bin\\wkhtmltoimage.exe"
    if (-not (Test-Path $candidate)) {
        $candidate = Join-Path ${Env:ProgramFiles(x86)} "wkhtmltopdf\\bin\\wkhtmltoimage.exe"
    }
    if (-not (Test-Path $candidate)) {
        Die "wkhtmltoimage 安装失败：未找到 $candidate"
    }
    $env:WKHTMLTOIMAGE_BIN = $candidate
}

function Ensure-Ffmpeg {
    if ($env:NBOT_FFMPEG_BIN -and (Test-Path $env:NBOT_FFMPEG_BIN)) { return }
    $bundledDir = Join-Path $Root "tools\\ffmpeg\\bin"
    $bundled = Join-Path $bundledDir "ffmpeg.exe"
    if (Test-Path $bundled) {
        $env:NBOT_FFMPEG_BIN = $bundled
        $ffprobe = Join-Path $bundledDir "ffprobe.exe"
        if (Test-Path $ffprobe) { $env:NBOT_FFPROBE_BIN = $ffprobe }
        return
    }
    $cmd = Get-Command "ffmpeg" -ErrorAction SilentlyContinue
    if ($cmd) {
        $env:NBOT_FFMPEG_BIN = $cmd.Source
        $ffprobe = Join-Path (Split-Path -Parent $cmd.Source) "ffprobe.exe"
        if (Test-Path $ffprobe) { $env:NBOT_FFPROBE_BIN = $ffprobe }
        return
    }

    Invoke-WingetInstall -Id "Gyan.FFmpeg" -Name "ffmpeg"

    $cmd = Get-Command "ffmpeg" -ErrorAction SilentlyContinue
    if ($cmd) {
        $env:NBOT_FFMPEG_BIN = $cmd.Source
        $ffprobe = Join-Path (Split-Path -Parent $cmd.Source) "ffprobe.exe"
        if (Test-Path $ffprobe) { $env:NBOT_FFPROBE_BIN = $ffprobe }
        return
    }

    $candidateDirs = @(
        (Join-Path $Env:ProgramFiles "FFmpeg\\bin"),
        (Join-Path ${Env:ProgramFiles(x86)} "FFmpeg\\bin")
    ) | Where-Object { $_ -and (Test-Path (Join-Path $_ "ffmpeg.exe")) }

    if ($candidateDirs.Count -gt 0) {
        $env:PATH = "$($candidateDirs[0]);$env:PATH"
        $env:NBOT_FFMPEG_BIN = (Join-Path $candidateDirs[0] "ffmpeg.exe")
        $ffprobe = (Join-Path $candidateDirs[0] "ffprobe.exe")
        if (Test-Path $ffprobe) { $env:NBOT_FFPROBE_BIN = $ffprobe }
        return
    }

    Die "ffmpeg 安装后仍未找到（请确认 winget 安装成功，或手动安装 ffmpeg 并加入 PATH）。"
}

function Load-Or-Init-Env {
    $cfg = Read-EnvFile $EnvFile

    $nbotPort = $env:NBOT_PORT
    if (-not $nbotPort -and $cfg.ContainsKey("NBOT_PORT")) { $nbotPort = $cfg["NBOT_PORT"] }

    $renderPort = $env:NBOT_RENDER_PORT
    if (-not $renderPort -and $cfg.ContainsKey("NBOT_RENDER_PORT")) { $renderPort = $cfg["NBOT_RENDER_PORT"] }

    if (-not $nbotPort) { $nbotPort = "32100" }
    if (-not $renderPort) { $renderPort = "32180" }

    try {
        [void][int]$nbotPort
        [void][int]$renderPort
    } catch {
        $nbotPort = "32100"
        $renderPort = "32180"
    }

    if (Test-TcpPortInUse -Port ([int]$nbotPort)) {
        $nbotPort = (Get-FreeTcpPort).ToString()
    }

    if ($renderPort -eq $nbotPort) {
        $renderPort = (Get-FreeTcpPort).ToString()
    } elseif (Test-TcpPortInUse -Port ([int]$renderPort)) {
        $renderPort = (Get-FreeTcpPort).ToString()
    }

    $env:NBOT_PORT = $nbotPort
    $env:NBOT_BIND = "127.0.0.1:$nbotPort"
    $env:WKHTMLTOIMAGE_URL = "http://127.0.0.1:$renderPort"
    $env:NBOT_RENDER_PORT = $renderPort

    Step "Write $EnvFile"
    @(
        "NBOT_PORT=$nbotPort"
        "NBOT_BIND=127.0.0.1:$nbotPort"
        "NBOT_RENDER_PORT=$renderPort"
        "WKHTMLTOIMAGE_URL=http://127.0.0.1:$renderPort"
    ) | Set-Content -Path $EnvFile -Encoding UTF8
}

function Start-Or-Reuse-Process([string]$name, [string]$exePath, [string]$pidFile, [hashtable]$procEnv, [string]$stdoutPath, [string]$stderrPath) {
    if (Test-Path $pidFile) {
        $pidStr = (Get-Content -Path $pidFile -TotalCount 1).Trim()
        if ($pidStr) {
            $existingPid = 0
            if ([int]::TryParse($pidStr, [ref]$existingPid)) {
                $p = Get-Process -Id $existingPid -ErrorAction SilentlyContinue
                if ($p) {
                    Step "$name already running (pid=$existingPid)"
                    return
                }
            }
        }
    }

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $exePath
    $psi.WorkingDirectory = $Root
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    foreach ($k in $procEnv.Keys) { $psi.Environment[$k] = [string]$procEnv[$k] }

    $p2 = New-Object System.Diagnostics.Process
    $p2.StartInfo = $psi

    $outWriter = New-Object System.IO.StreamWriter($stdoutPath, $true, [System.Text.Encoding]::UTF8)
    $errWriter = New-Object System.IO.StreamWriter($stderrPath, $true, [System.Text.Encoding]::UTF8)

    $p2.add_OutputDataReceived({ param($sender, $e) if ($e.Data) { $outWriter.WriteLine($e.Data); $outWriter.Flush() } })
    $p2.add_ErrorDataReceived({ param($sender, $e) if ($e.Data) { $errWriter.WriteLine($e.Data); $errWriter.Flush() } })

    $null = $p2.Start()
    $p2.BeginOutputReadLine()
    $p2.BeginErrorReadLine()

    Set-Content -Path $pidFile -Value $p2.Id -Encoding ASCII
    Step "$name started (pid=$($p2.Id))"
}

Step "Prepare directories ..."
New-Item -ItemType Directory -Force -Path $RunDir | Out-Null
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Root "data\\state") | Out-Null

Load-Or-Init-Env

Step "Check runtime dependencies ..."
Ensure-Wkhtmltoimage
Ensure-Ffmpeg

$renderdExe = Join-Path $Root "renderd.exe"
$backendExe = Join-Path $Root "backend.exe"
if (-not (Test-Path $renderdExe)) { Die "Missing file: $renderdExe" }
if (-not (Test-Path $backendExe)) { Die "Missing file: $backendExe" }

$renderdPid = Join-Path $RunDir "renderd.pid"
$backendPid = Join-Path $RunDir "backend.pid"

Start-Or-Reuse-Process `
    -name "renderd" `
    -exePath $renderdExe `
    -pidFile $renderdPid `
    -procEnv @{ "PORT" = $env:NBOT_RENDER_PORT; "WKHTMLTOIMAGE_BIN" = $env:WKHTMLTOIMAGE_BIN } `
    -stdoutPath (Join-Path $LogDir "renderd.out.log") `
    -stderrPath (Join-Path $LogDir "renderd.err.log")

Start-Or-Reuse-Process `
    -name "backend" `
    -exePath $backendExe `
    -pidFile $backendPid `
    -procEnv @{ "NBOT_PORT" = $env:NBOT_PORT; "NBOT_BIND" = $env:NBOT_BIND; "WKHTMLTOIMAGE_URL" = $env:WKHTMLTOIMAGE_URL; "NBOT_FFMPEG_BIN" = $env:NBOT_FFMPEG_BIN; "NBOT_FFPROBE_BIN" = $env:NBOT_FFPROBE_BIN } `
    -stdoutPath (Join-Path $LogDir "backend.out.log") `
    -stderrPath (Join-Path $LogDir "backend.err.log")

Start-Sleep -Milliseconds 300

$TokenPath = Join-Path $Root "data\\state\\api_token.txt"
$ApiToken = $null
for ($i = 0; $i -lt 50; $i++) {
    if (Test-Path $TokenPath) {
        $ApiToken = (Get-Content -Path $TokenPath -TotalCount 1).Trim()
        if ($ApiToken) { break }
    }
    Start-Sleep -Milliseconds 100
}

Write-Host ""
Write-Host "OK" -ForegroundColor Green
Write-Host "WebUI: http://127.0.0.1:$($env:NBOT_PORT)" -ForegroundColor White
Write-Host "Logs: $LogDir" -ForegroundColor White
if ($ApiToken) {
    Write-Host "API Token: $ApiToken" -ForegroundColor White
}
Write-Host "API Token File: data/state/api_token.txt" -ForegroundColor White

try { Start-Process "http://127.0.0.1:$($env:NBOT_PORT)" | Out-Null } catch { }
