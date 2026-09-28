# MQTTX — 基于 GPUI 的跨平台 MQTT 桌面客户端

使用 **Rust + [gpui-kit](https://github.com/longbridge/gpui-kit)**（基于 [GPUI](https://github.com/zed-industries/zed)）重写的 MQTT 调试客户端，对标官方 [MQTTX](https://mqttx.app)。
纯 Rust 实现：MQTT 引擎、数据持久化与原生 UI 同属一个轻量二进制，无 Electron/WebView 运行时。

> 旧的 Tauri + Vue 版本仍保留在 `src/` 与 `src-tauri/`，新实现位于 `gpui-app/`。

## 功能特性

- **双协议支持** — MQTT 5.0 与 MQTT 3.1.1（[`rumqttc-next`](https://crates.io/crates/rumqttc-v5-next)）
- **多传输方式** — 明文 TCP、TLS、WebSocket(ws)、WebSocket over TLS(wss)，默认 rustls（aws-lc）
- **多连接管理** — 同时管理多个连接，侧边栏实时显示连接状态（未连接/连接中/已连接/错误），多标签页切换
- **完整 MQTT 5 属性** — 用户属性（可视化键值对编辑器）、Content-Type、消息过期间隔、Response Topic、Correlation Data
- **发布 / 订阅** — QoS 0/1/2、Retain、遗嘱消息（Last Will）、订阅自动恢复（auto resubscribe）、断线自动重连
- **消息流** — 收发方向区分、时间戳、QoS/Retain 标记、点击展开查看格式化 JSON 与 v5 属性、主题/内容过滤
- **全局变量** — 发布主题与负载中使用 `{{变量名}}`，内置 `{{$ts}}`、`{{$ts_ms}}`、`{{$uuid}}`
- **负载格式** — Plaintext / JSON（发送前校验）/ Base64 / Hex
- **阿里云 IoT** — Token（GroupId@@@DeviceId + HMAC-SHA1 签名）与一机一密两种鉴权方式一键生成连接
- **连接测试** — 表单内 5 秒握手测试，保存前验证连通性
- **日志面板** — 连接级事件日志（CONNACK/SUBACK/PUBACK/错误/重连等）
- **主题** — 浅色 / 深色切换
- **本地持久化** — 连接、订阅、预设、变量、设置以 JSON 存于系统数据目录

## 技术栈

| 层 | 技术 |
| --- | --- |
| UI | GPUI 0.3 + gpui-kit（gpui-component）0.6 |
| MQTT | rumqttc-v5-next 0.34 / rumqttc-v4-next 0.34（TCP/TLS/WS） |
| 异步 | 引擎跑在独立 tokio runtime，事件经 smol channel 泵回 UI 线程 |
| 持久化 | serde + serde_json，原子写（临时文件 + rename） |
| 其他 | chrono、dirs、uuid、hmac/sha1/base64（阿里云签名） |

## 目录结构

```
gpui-app/
├── Cargo.toml
├── src/
│   ├── main.rs          # 入口：创建窗口、挂载 gpui-kit Root
│   ├── lib.rs           # 库导出（模型/引擎/UI 可被集成测试复用）
│   ├── model.rs         # 数据模型（连接/订阅/消息/预设/变量/设置/模板渲染）
│   ├── mqtt.rs          # MQTT 引擎：多连接生命周期、TLS/WS、重连、事件通道
│   ├── store.rs         # JSON 持久化（%APPDATA%/MQTTX-GPUI）
│   ├── aliyun.rs        # 阿里云 IoT 签名与连接生成
│   └── ui/
│       ├── app.rs              # 应用外壳：标题栏/侧边栏/标签页/事件泵
│       ├── connection_form.rs  # 新建/编辑连接对话框
│       ├── connection_view.rs  # 连接工作区：订阅、消息流、发布面板、日志
│       ├── aliyun_dialog.rs    # 阿里云设备对话框
│       ├── variables_dialog.rs # 全局变量对话框
│       ├── settings_dialog.rs  # 设置对话框
│       └── widgets.rs          # 复用组件（枚举下拉、键值对编辑器等）
└── tests/
    └── engine_live.rs   # 真实 broker 端到端测试（默认 ignored）
```

## 构建与运行

前置：Rust nightly（GPUI 0.3 依赖，使用 `rustup default nightly`）。

```bash
# 开发运行
cargo run -p mqttx-desktop

# 优化构建
cargo build --release -p mqttx-desktop
# 产物：target/release/mqttx(.exe)
```

> 中国大陆网络下若 `index.crates.io` 不可达，仓库已提供仅作用于本工程的
> `.cargo/config.toml`（rsproxy 镜像）。

### 端到端连通性测试

测试默认 `#[ignore]`，需要外网：

```bash
MQTTX_LIVE=1 cargo test -p mqttx-desktop --test engine_live -- --ignored
```

会连接公共服务器 `broker.emqx.io:1883`，验证：

- MQTT 5.0 连接 → 订阅 → 发布 → 回环接收
- MQTT 3.1.1 连接握手

## 发布到 Gitea Package Registry

构建产物可通过 Gitea 的 Generic Package API 发布，两种方式：

### 方式一：手动发布（本地脚本）

前置：构建 release 产物 + 一个 Gitea 访问令牌（需 `package` 写权限）。

```powershell
# Windows（PowerShell）
$env:GITEA_URL = "https://gitea.example.com"
$env:GITEA_TOKEN = "gta_xxx"
.\scripts\publish-gitea.ps1                    # 默认产物 + 自动推断版本/owner
.\scripts\publish-gitea.ps1 -Version v1.0.1    # 指定版本
```

```bash
# macOS / Linux（bash）
export GITEA_URL=https://gitea.example.com
export GITEA_TOKEN=gta_xxx
./scripts/publish-gitea.sh                     # 默认产物
./scripts/publish-gitea.sh target/release/mqttx v1.0.1
```

- **owner** 自动从 `git remote origin` 推断（可用 `GITEA_OWNER` / `-Owner` 覆盖）
- **版本** 依次取参数 → `git describe --tags` → `Cargo.toml`
- 每个文件附带 `.sha256` 校验侧车；同名版本重复上传按 409 跳过（Gitea 不允许覆盖）
- 软件包页面：`{GITEA_URL}/{owner}?tab=packages`

### 方式二：CI 自动发布（Gitea Actions）

推送 `v*` tag 即触发 [.gitea/workflows/release.yml](.gitea/workflows/release.yml)：

1. 单元测试
2. Windows / Linux 双平台 release 构建（产物重命名为 `mqttx-<ver>-<target>-<ext>`）
3. 调用 `scripts/publish-gitea.sh` 上传到包注册表

需要在仓库 **Settings → Actions → Secrets** 配置 `GITEA_TOKEN`；
若 runner 使用自定义 label，请相应修改工作流的 `runs-on`。

## 数据目录

| 平台 | 路径 |
| --- | --- |
| Windows | `%APPDATA%\MQTTX-GPUI` |
| macOS | `~/Library/Application Support/MQTTX-GPUI` |
| Linux | `~/.local/share/MQTTX-GPUI` |

刻意与官方 Electron MQTTX 的 `MQTTX/` 目录区分，互不影响。读取时容忍 UTF-8 BOM。

## 与 Tauri 版本的差异

- 单一 Rust 二进制（release ~17MB），无需打包 Node/WebView 资源
- MQTT 引擎从 Tauri 命令模型改为独立 tokio runtime + 事件通道，UI 直接订阅
- 数据存储从浏览器 `localStorage` 迁移到系统数据目录下的 JSON 文件
- 新增 TLS/WSS 传输、负载格式（Base64/Hex）、遗嘱消息、连接测试等能力
