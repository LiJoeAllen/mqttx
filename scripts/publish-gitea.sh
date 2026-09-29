#!/usr/bin/env bash
# 发布构建产物到 Gitea：
# 1) Generic Package Registry（API 包，GET /api/packages/...）
# 2) Release 附件 —— OTA 更新源只认 Release 附件
#    （命名约定 mqttx-v<ver>-<triple>-mqttx[.exe]，含 .sha256 侧车）
#
# 用法：
#   GITEA_URL=https://gitea.example.com GITEA_TOKEN=xxx \
#     ./scripts/publish-gitea.sh [文件...] [版本]
#
# 环境变量：
#   GITEA_URL    实例地址，如 https://gitea.example.com（必填）
#   GITEA_TOKEN  访问令牌，需 package + repo 写权限（必填）
#   GITEA_OWNER  软件包所属用户/组织（默认：由 git remote 推断，失败则报错）
#   GITEA_REPO   仓库名（默认：由 git remote 推断，失败为 mqttx）
#   GITEA_PKG    软件包名（默认 mqttx）
#
# 版本推断顺序：参数 > git describe --tags > gpui-app/Cargo.toml。
set -euo pipefail

die() { echo "错误: $*" >&2; exit 1; }

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# ── 地址：优先环境变量，否则从 git remote origin 推断 ────────────────────────
if [ -z "${GITEA_URL:-}" ]; then
  remote_url="$(git -C "$ROOT" remote get-url origin 2>/dev/null || true)"
  case "$remote_url" in
    *://*) host="${remote_url#*://}"; host="${host%%/*}" ;;
    *:*)   host="${remote_url%%:*}" ;;
    *)     host="" ;;
  esac
  host="${host##*@}"  # 去掉 git@user@ 前缀
  [ -n "$host" ] && GITEA_URL="https://$host"
fi
[ -n "${GITEA_URL:-}" ] || die "缺少 GITEA_URL（或配置 git remote origin）"
GITEA_URL="${GITEA_URL%/}"

# ── 令牌：优先环境变量，否则取 Git Credential Manager 中同一主机的凭证 ────────
if [ -z "${GITEA_TOKEN:-}" ]; then
  GITEA_TOKEN="$(printf 'protocol=https\nhost=%s\n\n' "${GITEA_URL#*://}" \
    | GCM_INTERACTIVE=Never GIT_TERMINAL_PROMPT=0 git credential fill 2>/dev/null \
    | sed -n 's/^password=//p')"
  [ -n "${GITEA_TOKEN:-}" ] || die "缺少 GITEA_TOKEN，且无法从 git credential store 自动获取"
fi
PKG="${GITEA_PKG:-mqttx}"

# ── owner/repo：优先环境变量，其次从 git remote 推断 ─────────────────────────
remote_owner_repo() {
  local url path
  url="$(git -C "$ROOT" remote get-url origin 2>/dev/null)" || return 1
  case "$url" in
    *://*) path="${url#*://}"; path="${path#*/}" ;;  # https://host/owner/repo.git
    *:*)   path="${url#*:}" ;;                       # git@host:owner/repo.git
    *)     return 1 ;;
  esac
  path="${path%.git}"
  printf '%s\n' "${path%%/*}" "${path#*/}"
}
OWNER="${GITEA_OWNER:-$(remote_owner_repo | head -1 || true)}"
REPO="${GITEA_REPO:-$(remote_owner_repo | tail -1 || true)}"
[ -n "${OWNER:-}" ] || die "无法推断 GITEA_OWNER，请显式设置"
[ -n "${REPO:-}" ] || REPO="$PKG"

# ── 版本：参数 > git describe > Cargo.toml ───────────────────────────────────
FILES=()
for arg in "$@"; do
  if [ -f "$arg" ]; then FILES+=("$arg"); else VERSION="$arg"; fi
done
if [ -z "${VERSION:-}" ]; then
  VERSION="$(git -C "$ROOT" describe --tags --always 2>/dev/null || true)"
fi
if [ -z "${VERSION:-}" ]; then
  VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/gpui-app/Cargo.toml" | head -1)"
fi
[ -n "${VERSION:-}" ] || die "无法确定版本号"
# Gitea 版本段不允许 "/"，tag 形如 v1.0.0-3-gabc123 时规范化
VERSION="${VERSION//\//-}"

# ── 默认产物 ────────────────────────────────────────────────────────────────
if [ "${#FILES[@]}" -eq 0 ]; then
  if [ -x "$ROOT/target/release/mqttx.exe" ]; then
    FILES=("$ROOT/target/release/mqttx.exe")
  elif [ -x "$ROOT/target/release/mqttx" ]; then
    FILES=("$ROOT/target/release/mqttx")
  else
    die "未找到产物，请先 cargo build --release，或显式传入文件"
  fi
fi

echo "→ Gitea: $GITEA_URL"
echo "→ 软件包: $OWNER/$PKG@$VERSION"
echo "→ 仓库: $OWNER/$REPO"
echo "→ 产物: ${FILES[*]}"

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

# OTA 产物命名约定的平台三元组（与 update.rs::platform_triple 一致）
triple_of() {
  case "$(uname -s)/$(uname -m)" in
    MINGW*/*x86_64*|MSYS*/*x86_64*|CYGWIN*/*x86_64*) echo "x86_64-pc-windows-msvc" ;;
    Linux/x86_64|Linux/amd64)  echo "x86_64-unknown-linux-gnu" ;;
    Darwin/x86_64)             echo "x86_64-apple-darwin" ;;
    Darwin/arm64)              echo "aarch64-apple-darwin" ;;
    Linux/aarch64|Linux/arm64) echo "aarch64-unknown-linux-gnu" ;;
    *) echo "" ;;
  esac
}
TRIPLE="${GITEA_TRIPLE:-$(triple_of)}"

for f in "${FILES[@]}"; do
  [ -f "$f" ] || die "文件不存在: $f"
  name="$(basename "$f")"
  echo "→ 上传 $name ($(du -h "$f" | cut -f1))"

  code="$(curl -sS -o /tmp/gitea-pkg-resp.txt -w '%{http_code}' \
    -X PUT \
    -H "Authorization: token ${GITEA_TOKEN}" \
    --upload-file "$f" \
    "${GITEA_URL}/api/packages/${OWNER}/generic/${PKG}/${VERSION}/${name}")"

  case "$code" in
    201|200|409) # 409 = 同名版本已存在，Gitea 拒绝覆盖
      if [ "$code" = "409" ]; then
        echo "  ! 已存在同名版本（409），跳过：$name"
      else
        echo "  ✓ 已发布 $name (HTTP $code)"
        # 附带 sha256 校验文件，便于下游验证
        sum="$(sha256 "$f")"
        echo "$sum  $name" > "${f}.sha256"
        code2="$(curl -sS -o /dev/null -w '%{http_code}' \
          -X PUT \
          -H "Authorization: token ${GITEA_TOKEN}" \
          -H "Content-Type: text/plain" \
          --data-binary "@${f}.sha256" \
          "${GITEA_URL}/api/packages/${OWNER}/generic/${PKG}/${VERSION}/${name}.sha256")"
        [ "$code2" = "201" ] || echo "  ! sha256 文件上传失败 (HTTP $code2)"
      fi
      ;;
    *)
      echo "  ✗ 上传失败 (HTTP $code)" >&2
      cat /tmp/gitea-pkg-resp.txt >&2 || true
      exit 1
      ;;
  esac
done

echo "完成。软件包页面: ${GITEA_URL}/${OWNER}?tab=packages"

# ── Release 附件：OTA 更新源（update.rs 只查 /releases）──────────────────────
API="${GITEA_URL}/api/v1/repos/${OWNER}/${REPO}"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

# 规范化版本号：tag 推断出的 "v1.0.2" 与 Cargo 的 "1.0.2" 统一为无 v 数字段
VER_NUM="${VERSION#v}"
[ -n "$TRIPLE" ] || TRIPLE="unknown"

# 取（或创建）与版本同名 tag 的 Release；找不到时按需创建
code="$(curl -sS -o "$STAGE/rel.json" -w '%{http_code}' \
  -H "Authorization: token ${GITEA_TOKEN}" \
  "${API}/releases/tags/v${VER_NUM}")"
if [ "$code" = "404" ]; then
  echo "→ Release v${VER_NUM} 不存在，创建中…"
  code="$(curl -sS -o "$STAGE/rel.json" -w '%{http_code}' \
    -X POST \
    -H "Authorization: token ${GITEA_TOKEN}" \
    -H "Content-Type: application/json" \
    -d "{\"tag_name\":\"v${VER_NUM}\",\"name\":\"v${VER_NUM}\",\"draft\":false,\"prerelease\":false}" \
    "${API}/releases")"
  if [ "$code" != "201" ]; then
    echo "  ! Release 创建失败 (HTTP $code)，跳过附件上传" >&2
    cat "$STAGE/rel.json" >&2 || true
    exit 0
  fi
elif [ "$code" != "200" ]; then
  echo "  ! 查询 Release 失败 (HTTP $code)，跳过附件上传" >&2
  exit 0
fi
# 顶层 "id" 是首个出现的 id 字段
RELEASE_ID="$(sed -n 's/.*"id":[[:space:]]*\([0-9][0-9]*\).*/\1/p' "$STAGE/rel.json" | head -1)"
[ -n "$RELEASE_ID" ] || { echo "  ! 无法解析 Release id，跳过附件上传" >&2; exit 0; }
echo "→ Release v${VER_NUM} (id=$RELEASE_ID)，上传附件…"

for f in "${FILES[@]}"; do
  name="$(basename "$f")"
  # 已符合 OTA 命名约定的原样使用；否则按约定规范重命名后上传
  case "$name" in
    mqttx-v*-mqttx|mqttx-v*-mqttx.exe) asset="$name" ;;
    *.exe) asset="mqttx-v${VER_NUM}-${TRIPLE}-mqttx.exe" ;;
    *)     asset="mqttx-v${VER_NUM}-${TRIPLE}-mqttx" ;;
  esac

  # ── 7z 压缩附件（OTA 主通道）：内部文件名 = $asset（应用解压后按此名安装）──
  SEVENZIP="$(command -v 7z 7za 2>/dev/null | head -1)"
  [ -z "$SEVENZIP" ] && [ -x "/c/Program Files/7-Zip/7z.exe" ] && SEVENZIP="/c/Program Files/7-Zip/7z.exe"
  if [ -n "$SEVENZIP" ]; then
    "$SEVENZIP" a -y -mx=9 "$STAGE/$asset.7z" "$f" >/dev/null
    # 归档内的文件名需为 $asset：7z 存的是源文件名，先复制改名再压
    rm -f "$STAGE/$asset.7z"
    cp -f "$f" "$STAGE/$asset"
    "$SEVENZIP" a -y -mx=9 "$STAGE/$asset.7z" "$STAGE/$asset" >/dev/null
    if [ -f "$STAGE/$asset.7z" ]; then
      printf '%s  %s\n' "$(sha256 "$STAGE/$asset.7z")" "$asset.7z" > "$STAGE/$asset.7z.sha256"
      for a in "$asset.7z" "$asset.7z.sha256"; do
        code="$(curl -sS -o "$STAGE/up.json" -w '%{http_code}' \
          -X POST \
          -H "Authorization: token ${GITEA_TOKEN}" \
          -F "attachment=@${STAGE}/${a}" \
          "${API}/releases/${RELEASE_ID}/assets?name=${a}")"
        case "$code" in
          201) echo "  ✓ Release 附件 $a" ;;
          *)   echo "  ! Release 附件 $a 上传失败 (HTTP $code)" >&2; cat "$STAGE/up.json" >&2 || true ;;
        esac
      done
    else
      echo "  ! 7z 打包失败，跳过压缩附件" >&2
    fi
  else
    echo "  ! 未找到 7z/7za，跳过 7z 附件（仅上传裸二进制）" >&2
  fi

  # ── 裸二进制附件：v1.0.3 及更早客户端不认识 7z，需要它升级 ──
  cp -f "$f" "$STAGE/$asset"
  printf '%s  %s\n' "$(sha256 "$f")" "$asset" > "$STAGE/$asset.sha256"
  for a in "$asset" "$asset.sha256"; do
    code="$(curl -sS -o "$STAGE/up.json" -w '%{http_code}' \
      -X POST \
      -H "Authorization: token ${GITEA_TOKEN}" \
      -F "attachment=@${STAGE}/${a}" \
      "${API}/releases/${RELEASE_ID}/assets?name=${a}")"
    case "$code" in
      201) echo "  ✓ Release 附件 $a" ;;
      *)   echo "  ! Release 附件 $a 上传失败 (HTTP $code)" >&2; cat "$STAGE/up.json" >&2 || true ;;
    esac
  done
done
echo "完成。Release 页面: ${GITEA_URL}/${OWNER}/${REPO}/releases/tag/v${VER_NUM}"
