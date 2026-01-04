param(
    [string]$Repo = "yukmakoto/nBot",
    [switch]$RerunLatestFailed
)

$ErrorActionPreference = "Stop"

function Die([string]$msg) { Write-Host "ERROR: $msg" -ForegroundColor Red; exit 1 }
function Step([string]$msg) { Write-Host ">>> $msg" -ForegroundColor Cyan }

if (-not (Get-Command "gh" -ErrorAction SilentlyContinue)) {
    Die "GitHub CLI (gh) not found. Install: https://github.com/cli/cli#installation"
}

Step "Verify GitHub auth ..."
gh auth status *> $null
if ($LASTEXITCODE -ne 0) {
    Die "Not logged into GitHub CLI. Run: gh auth login"
}

Write-Host ""
Write-Host "Docker Hub 账号/Token 获取方式：" -ForegroundColor Yellow
Write-Host "1) 注册: https://hub.docker.com/signup" -ForegroundColor Yellow
Write-Host "2) 生成 Token: Docker Hub -> Account Settings -> Security -> New Access Token (Read & Write)" -ForegroundColor Yellow
Write-Host ""

$username = (Read-Host "Docker Hub Username").Trim()
if (-not $username) { Die "Username is empty." }

$secure = Read-Host "Docker Hub Access Token (不会回显)" -AsSecureString
$bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
try {
    $token = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr)
} finally {
    [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr)
}

if (-not $token) { Die "Token is empty." }

Step "Set GitHub Actions secrets for $Repo ..."
gh secret set DOCKERHUB_USERNAME --repo $Repo --body $username
gh secret set DOCKERHUB_TOKEN --repo $Repo --body $token

Step "Done."
Write-Host "Secrets set: DOCKERHUB_USERNAME, DOCKERHUB_TOKEN" -ForegroundColor Green

if ($RerunLatestFailed) {
    Step "Rerun latest failed 'Build & Push (Docker Hub)' workflow ..."
    $runId = gh run list --repo $Repo --workflow "Build & Push (Docker Hub)" --status failure --limit 1 --json databaseId --jq ".[0].databaseId"
    if ($runId) {
        gh run rerun $runId --repo $Repo | Out-Host
        Write-Host "Re-run started: run id $runId" -ForegroundColor Green
    } else {
        Write-Host "No failed DockerHub runs found." -ForegroundColor Yellow
    }
}

