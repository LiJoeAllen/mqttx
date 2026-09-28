<#
.SYNOPSIS
  发布构建产物到 Gitea Package Registry（Generic 包）。

.DESCRIPTION
  用 Gitea Generic Package API 上传文件：
    PUT {GITEA_URL}/api/packages/{owner}/generic/{pkg}/{version}/{filename}
  认证：Authorization: token <GITEA_TOKEN>
  同名版本重复上传会得到 409，脚本按"已存在"处理。
  上传成功后附加 .sha256 校验文件。

.EXAMPLE
  $env:GITEA_URL = "https://gitea.example.com"
  $env:GITEA_TOKEN = "xxxxxxxx"
  .\scripts\publish-gitea.ps1                          # 上传默认产物，版本自动推断
  .\scripts\publish-gitea.ps1 -Version v1.0.1          # 指定版本
  .\scripts\publish-gitea.ps1 -Files a.exe, b.zip      # 指定文件

.PARAMETER Files
  待上传文件列表。缺省时自动探测 target/release/mqttx(.exe)。
.PARAMETER Version
  软件包版本。缺省时依次尝试 git describe --tags、gpui-app/Cargo.toml。
.PARAMETER Owner
  覆盖 GITEA_OWNER（缺省时从 git remote origin 推断）。
.PARAMETER PackageName
  覆盖 GITEA_PKG（默认 mqttx）。
#>
[CmdletBinding()]
param(
    [string[]]$Files = @(),
    [string]$Version = "",
    [string]$Owner = "",
    [string]$PackageName = ""
)

$ErrorActionPreference = "Stop"
# Windows PowerShell 5.1 默认可能协商 TLS1.0，现代 Gitea 实例要求 TLS1.2+
try {
    [Net.ServicePointManager]::SecurityProtocol = `
        [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
} catch { }

function Die($msg) { Write-Error "错误: $msg"; exit 1 }

# ── 必填配置 ────────────────────────────────────────────────────────────────
$GiteaUrl = $env:GITEA_URL
$Token = $env:GITEA_TOKEN
if (-not $GiteaUrl) { Die "缺少环境变量 GITEA_URL（如 https://gitea.example.com）" }
if (-not $Token) { Die "缺少环境变量 GITEA_TOKEN（需 package 写权限）" }
$GiteaUrl = $GiteaUrl.TrimEnd("/")

$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$Pkg = if ($PackageName) { $PackageName } elseif ($env:GITEA_PKG) { $env:GITEA_PKG } else { "mqttx" }

# ── owner：参数 > GITEA_OWNER > git remote origin 推断 ──────────────────────
if (-not $Owner) { $Owner = $env:GITEA_OWNER }
if (-not $Owner) {
    $remote = git -C $Root remote get-url origin 2>$null
    if ($remote) {
        # 兼容 git@host:owner/repo.git 与 https://host/owner/repo.git
        if ($remote -match "://") {
            # URL 形式：取 host 之后的路径段
            $path = ($remote -split "/", 4)[-1]
        }
        else {
            # SCP 形式 git@host:owner/repo
            $path = ($remote -split ":", 2)[-1]
        }
        $path = $path -replace "\.git$", ""
        $seg = @($path -split "/" | Where-Object { $_ })
        if ($seg.Count -ge 2) { $Owner = $seg[0] }
    }
}
if (-not $Owner) { Die "无法推断 owner，请用 -Owner 或 GITEA_OWNER 指定" }

# ── 版本：参数 > git describe > Cargo.toml ──────────────────────────────────
if (-not $Version) {
    $Version = (git -C $Root describe --tags --always 2>$null)
}
if (-not $Version) {
    $m = Select-String -Path (Join-Path $Root "gpui-app\Cargo.toml") -Pattern '^version = "(.+)"' | Select-Object -First 1
    if ($m) { $Version = $m.Matches[0].Groups[1].Value }
}
if (-not $Version) { Die "无法确定版本号" }
$Version = $Version -replace "/", "-"   # Gitea 版本段不允许 "/"

# ── 默认产物 ────────────────────────────────────────────────────────────────
if (-not $Files -or $Files.Count -eq 0) {
    $exe = Join-Path $Root "target\release\mqttx.exe"
    $bin = Join-Path $Root "target\release\mqttx"
    if (Test-Path $exe) { $Files = @($exe) }
    elseif (Test-Path $bin) { $Files = @($bin) }
    else { Die "未找到产物，请先 cargo build --release，或用 -Files 指定" }
}

Write-Host "→ Gitea:   $GiteaUrl"
Write-Host "→ 软件包:  $Owner/$Pkg@$Version"
Write-Host "→ 产物:    $($Files -join ', ')"

foreach ($f in $Files) {
    if (-not (Test-Path $f)) { Die "文件不存在: $f" }
    $name = Split-Path -Leaf $f
    $size = "{0:N1} MB" -f ((Get-Item $f).Length / 1MB)
    Write-Host "→ 上传 $name ($size)"

    $url = "$GiteaUrl/api/packages/$Owner/generic/$Pkg/$Version/$name"
    # Windows PowerShell 5.1 无 -InFile 的 token 认证上传也走 Invoke-WebRequest
    try {
        Invoke-RestMethod -Method Put `
            -Headers @{ Authorization = "token $Token" } `
            -InFile $f `
            -ContentType "application/octet-stream" `
            -Uri $url | Out-Null
        Write-Host "  ✓ 已发布 $name" -ForegroundColor Green
    }
    catch {
        $status = $null
        if ($_.Exception.Response) { $status = [int]$_.Exception.Response.StatusCode }
        if ($status -eq 409) {
            Write-Host "  ! 已存在同名版本（409），跳过：$name" -ForegroundColor Yellow
            continue
        }
        Die "上传失败: $($_.Exception.Message)"
    }

    # 附带 sha256 校验文件
    $sum = (Get-FileHash -Algorithm SHA256 $f).Hash.ToLower()
    $sidecar = "$f.sha256"
    "$sum  $name" | Set-Content -NoNewline -Encoding ascii $sidecar
    try {
        Invoke-RestMethod -Method Put `
            -Headers @{ Authorization = "token $Token" } `
            -InFile $sidecar `
            -ContentType "text/plain" `
            -Uri "$url.sha256" | Out-Null
        Write-Host "  ✓ 已发布 $name.sha256" -ForegroundColor Green
    }
    catch {
        Write-Host "  ! sha256 文件上传失败" -ForegroundColor Yellow
    }
}

Write-Host "完成。软件包页面: $GiteaUrl/$Owner?tab=packages" -ForegroundColor Cyan
