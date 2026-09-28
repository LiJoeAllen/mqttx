#!/usr/bin/env bash
# 发布构建产物到 Gitea Package Registry（Generic 包）。
#
# 用法：
#   GITEA_URL=https://gitea.example.com GITEA_TOKEN=xxx \
#     ./scripts/publish-gitea.sh [文件...] [版本]
#
# 环境变量：
#   GITEA_URL    实例地址，如 https://gitea.example.com（必填）
#   GITEA_TOKEN  访问令牌，需 package 写权限（必填）
#   GITEA_OWNER  软件包所属用户/组织（默认：由 git remote 推断，失败则报错）
#   GITEA_PKG    软件包名（默认 mqttx）
#
# 版本推断顺序：参数 > git describe --tags > gpui-app/Cargo.toml。
set -euo pipefail

die() { echo "错误: $*" >&2; exit 1; }

[ -n "${GITEA_URL:-}" ] || die "缺少 GITEA_URL"
[ -n "${GITEA_TOKEN:-}" ] || die "缺少 GITEA_TOKEN"
GITEA_URL="${GITEA_URL%/}"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKG="${GITEA_PKG:-mqttx}"

# ── owner：优先环境变量，其次从 git remote 推断 ──────────────────────────────
owner_of_remote() {
  local url path
  url="$(git -C "$ROOT" remote get-url origin 2>/dev/null)" || return 1
  case "$url" in
    *://*) path="${url#*://}"; path="${path#*/}" ;;  # https://host/owner/repo.git
    *:*)   path="${url#*:}" ;;                       # git@host:owner/repo.git
    *)     return 1 ;;
  esac
  path="${path%.git}"
  printf '%s\n' "${path%%/*}"
}
OWNER="${GITEA_OWNER:-$(owner_of_remote || true)}"
[ -n "${OWNER:-}" ] || die "无法推断 GITEA_OWNER，请显式设置"

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
echo "→ 产物: ${FILES[*]}"

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

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
