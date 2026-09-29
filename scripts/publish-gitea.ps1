<#
.SYNOPSIS
  发布构建产物到 Gitea：Generic Package + Release 附件（OTA 更新源）。

.DESCRIPTION
  1) Generic Package API 上传：
       PUT {GITEA_URL}/api/packages/{owner}/generic/{pkg}/{version}/{filename}
  2) Release 附件上传（应用内 OTA 只认 Release 附件）：
       POST {GITEA_URL}/api/v1/repos/{owner}/{repo}/releases/{id}/assets?name=...
     命名约定 mqttx-v<ver>-<triple>-mqttx[.exe]，含 .sha256 侧车。
  认证：Authorization: token <GITEA_TOKEN>
  同名包版本重复上传会得到 409，脚本按"已存在"处理。

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
.PARAMETER Repo
  覆盖 GITEA_REPO（缺省时从 git remote origin 推断，失败为 mqttx）。
.PARAMETER PackageName
  覆盖 GITEA_PKG（默认 mqttx）。
#>
[CmdletBinding()]
param(
    [string[]]$Files = @(),
    [string]$Version = "",
    [string]$Owner = "",
    [string]$Repo = "",
    [string]$PackageName = ""
)

$ErrorActionPreference = "Stop"
# Windows PowerShell 5.1 默认可能协商 TLS1.0，现代 Gitea 实例要求 TLS1.2+
try {
    [Net.ServicePointManager]::SecurityProtocol = `
        [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
} catch { }

function Die($msg) { Write-Error "错误: $msg"; exit 1 }

$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$Pkg = if ($PackageName) { $PackageName } elseif ($env:GITEA_PKG) { $env:GITEA_PKG } else { "mqttx" }

# ── 必填配置 ────────────────────────────────────────────────────────────────
# 地址：优先环境变量，否则从 git remote origin 推断
$GiteaUrl = $env:GITEA_URL
if (-not $GiteaUrl) {
    $remote = git -C $Root remote get-url origin 2>$null
    if ($remote) {
        if ($remote -match "://") { $hostPart = (($remote -split "://", 2)[-1] -split "/", 2)[0] }
        else { $hostPart = ($remote -split ":", 2)[0] }
        $hostPart = ($hostPart -split "@")[-1]
        if ($hostPart) { $GiteaUrl = "https://$hostPart" }
    }
}
if (-not $GiteaUrl) { Die "缺少环境变量 GITEA_URL（如 https://gitea.example.com），且无法从 git remote 推断" }
$GiteaUrl = $GiteaUrl.TrimEnd("/")

# 令牌：优先环境变量，否则取 Git Credential Manager 中同一主机的凭证
# （即推送用的那条 OAuth 凭证，无需另行创建 API token）
$Token = $env:GITEA_TOKEN
if (-not $Token) {
    # 从 Git Credential Manager 取同一主机的凭证（与 git 推送同源）。
    # 注意：git credential fill 对 CRLF 管道输入敏感（PS 管道送出 \r\n
    # 会被拒收），必须写 LF 临时文件、由 cmd 做重定向，stderr 也留在 cmd
    # 内部（PS 5.1 在 EAP=Stop 下对原生命令的 2>$null 会抛异常）。
    $credHost = $GiteaUrl -replace "^https?://", ""
    $req = Join-Path $env:TEMP ("mqttx-cred-" + [guid]::NewGuid().ToString("N") + ".txt")
    [IO.File]::WriteAllText($req, "protocol=https`nhost=$credHost`n`n")
    try {
        $raw = cmd /c "git credential fill < ""$req"" 2>nul"
        $Token = ($raw | Where-Object { $_ -like "password=*" }) -replace "^password=", ""
    }
    catch { $Token = $null }
    finally { Remove-Item $req -ErrorAction SilentlyContinue }
}
if (-not $Token) { Die "缺少环境变量 GITEA_TOKEN，且无法从 git credential store 自动获取" }

# ── owner/repo：参数 > 环境变量 > git remote origin 推断 ─────────────────────
if (-not $Owner) { $Owner = $env:GITEA_OWNER }
if (-not $Repo)  { $Repo  = $env:GITEA_REPO }
if (-not $Owner -or -not $Repo) {
    try { $remote = git -C $Root remote get-url origin 2>$null } catch { $remote = $null }
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
        if ($seg.Count -ge 2) {
            if (-not $Owner) { $Owner = $seg[0] }
            if (-not $Repo)  { $Repo  = $seg[1] }
        }
    }
}
if (-not $Owner) { Die "无法推断 owner，请用 -Owner 或 GITEA_OWNER 指定" }
if (-not $Repo)  { $Repo = $Pkg }

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
Write-Host "→ 仓库:    $Owner/$Repo"
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

# ── Release 附件：OTA 更新源（应用内 update.rs 只查 /releases）──────────────
# 规范化版本号：tag 推断出的 "v1.0.2" 与 Cargo 的 "1.0.2" 统一为无 v 数字段
$VerNum = $Version -replace "^v", ""
$Triple = "x86_64-pc-windows-msvc"
$Api = "$GiteaUrl/api/v1/repos/$Owner/$Repo"

# 取（或创建）与版本同名 tag 的 Release
$rel = $null
try {
    $rel = Invoke-RestMethod -Headers @{ Authorization = "token $Token" } -Uri "$Api/releases/tags/v$VerNum"
} catch { }
if (-not $rel) {
    try {
        $rel = Invoke-RestMethod -Method Post `
            -Headers @{ Authorization = "token $Token" } `
            -ContentType "application/json" `
            -Body (@{ tag_name = "v$VerNum"; name = "v$VerNum"; draft = $false; prerelease = $false } | ConvertTo-Json) `
            -Uri "$Api/releases"
        Write-Host "→ 已创建 Release v$VerNum"
    }
    catch {
        Write-Host "  ! Release 创建/查询失败：$($_.Exception.Message)；跳过附件上传" -ForegroundColor Yellow
        return
    }
}
$releaseId = $rel.id
Write-Host "→ Release v$VerNum (id=$releaseId)，上传附件…"

function Upload-ReleaseAsset($uri, $filePath) {
    # Windows PowerShell 5.1 没有 -Form，用 .NET HttpClient 组 multipart（字段名必须为 attachment）
    Add-Type -AssemblyName System.Net.Http
    $client = New-Object System.Net.Http.HttpClient
    $client.DefaultRequestHeaders.Authorization =
        New-Object System.Net.Http.Headers.AuthenticationHeaderValue("token", $Token)
    $content = New-Object System.Net.Http.MultipartFormDataContent
    $fs = [System.IO.File]::OpenRead($filePath)
    try {
        $fileContent = New-Object System.Net.Http.StreamContent($fs)
        $fileContent.Headers.ContentType =
            [System.Net.Http.Headers.MediaTypeHeaderValue]::Parse("application/octet-stream")
        $content.Add($fileContent, "attachment", (Split-Path -Leaf $filePath))
        $resp = $client.PostAsync($uri, $content).GetAwaiter().GetResult()
        return [int]$resp.StatusCode
    }
    finally {
        $fs.Dispose()
        $client.Dispose()
    }
}

foreach ($f in $Files) {
    $name = Split-Path -Leaf $f
    # 已符合 OTA 命名约定的原样使用；否则按约定规范重命名后上传
    if ($name -match "^mqttx-v.*-mqttx(\.exe)?$") { $asset = $name }
    elseif ($name.EndsWith(".exe")) { $asset = "mqttx-v$VerNum-$Triple-mqttx.exe" }
    else { $asset = "mqttx-v$VerNum-$Triple-mqttx" }

    # ── 7z 压缩附件（OTA 主通道：体积小、下载快）──
    # 内部文件名 = $asset（应用解压后按此名暂存安装）
    $SevenZip = @("$env:ProgramFiles\7-Zip\7z.exe", "${env:ProgramFiles(x86)}\7-Zip\7z.exe") |
        Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $SevenZip) { $SevenZip = (Get-Command 7z -ErrorAction SilentlyContinue).Source }
    if ($SevenZip) {
        $packDir = Join-Path $env:TEMP ("mqttx-pack-" + [guid]::NewGuid().ToString("N"))
        New-Item -ItemType Directory -Path $packDir | Out-Null
        Copy-Item $f (Join-Path $packDir $asset)
        $archive = Join-Path $env:TEMP "$asset.7z"
        & $SevenZip a -y -mx=9 $archive (Join-Path $packDir $asset) | Out-Null
        if ($LASTEXITCODE -eq 0 -and (Test-Path $archive)) {
            $asum = (Get-FileHash -Algorithm SHA256 $archive).Hash.ToLower()
            "$asum  $asset.7z" | Set-Content -NoNewline -Encoding ascii "$archive.sha256"
            foreach ($a in @("$asset.7z", "$asset.7z.sha256")) {
                $code = Upload-ReleaseAsset "$Api/releases/$releaseId/assets?name=$a" (Join-Path $env:TEMP $a)
                if ($code -eq 201) {
                    Write-Host "  ✓ Release 附件 $a" -ForegroundColor Green
                }
                else {
                    Write-Host "  ! Release 附件 $a 上传失败 (HTTP $code)" -ForegroundColor Yellow
                }
            }
        }
        else {
            Write-Host "  ! 7z 打包失败（exit=$LASTEXITCODE），跳过压缩附件" -ForegroundColor Yellow
        }
        Remove-Item $packDir, $archive, "$archive.sha256" -Recurse -Force -ErrorAction SilentlyContinue
    }
    else {
        Write-Host "  ! 未找到 7-Zip，跳过 7z 附件（仅上传裸二进制）" -ForegroundColor Yellow
    }

    # ── 裸二进制附件：v1.0.3 及更早客户端不认识 7z，需要它升级 ──
    $tmp = Join-Path $env:TEMP $asset
    Copy-Item $f $tmp -Force
    $sum = (Get-FileHash -Algorithm SHA256 $f).Hash.ToLower()
    "$sum  $asset" | Set-Content -NoNewline -Encoding ascii "$tmp.sha256"
    foreach ($a in @($asset, "$asset.sha256")) {
        $code = Upload-ReleaseAsset "$Api/releases/$releaseId/assets?name=$a" (Join-Path $env:TEMP $a)
        if ($code -eq 201) {
            Write-Host "  ✓ Release 附件 $a" -ForegroundColor Green
        }
        else {
            Write-Host "  ! Release 附件 $a 上传失败 (HTTP $code)" -ForegroundColor Yellow
        }
    }
    Remove-Item $tmp, "$tmp.sha256" -ErrorAction SilentlyContinue
}
Write-Host "完成。Release 页面: $GiteaUrl/$Owner/$Repo/releases/tag/v$VerNum" -ForegroundColor Cyan
