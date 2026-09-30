# MQTTX — 基于 GPUI 的跨平台 MQTT 桌面客户端

使用 **Rust + [gpui-kit](https://github.com/longbridge/gpui-kit)**（基于 [GPUI](https://github.com/zed-industries/zed)）重写的 MQTT 调试客户端，对标官方 [MQTTX](https://mqttx.app)。
纯 Rust 实现：MQTT 引擎、数据持久化与原生 UI 同属一个轻量二进制，无 Electron/WebView 运行时。

> 早期 Tauri + Vue 实现已归档到 [`tauri-legacy`](../../archive/tauri-legacy.tar.gz) 分支，本项目仅维护 `gpui-app/` 原生实现。

## 功能特性

- **双协议支持** — MQTT 5.0 与 MQTT 3.1.1（[`rumqttc-next`](https://crates.io/crates/rumqttc-v5-next)）
- **多传输方式** — 明文 TCP、TLS、WebSocket(ws)、WebSocket over TLS(wss)，默认 rustls（aws-lc）
- **多连接管理** — 同时管理多个连接，侧边栏实时显示连接状态（未连接/连接中/已连接/错误），多标签页切换；连接分组（分组 chips 过滤 + 表单分组归属）
- **完整 MQTT 5 属性** — 用户属性（可视化键值对编辑器）、Content-Type、消息过期间隔、Response Topic、Correlation Data（连接/遗嘱/发布三处均支持）
- **发布 / 订阅** — QoS 0/1/2、Retain、遗嘱消息（Last Will）、订阅自动恢复（auto resubscribe）、断线自动重连（可配置最大重连次数与连接超时）
- **订阅增强** — 逗号/换行多主题批量订阅、订阅别名、颜色标签（消息按订阅着色）、启用/禁用、点击订阅过滤消息流、MQTT 5 订阅选项（订阅标识符 / No Local / Retain As Published / Retain Handling）
- **消息流** — 收发方向区分、时间戳、QoS/Retain 标记、点击展开查看格式化 JSON 与 v5 属性、全部/接收/发布过滤、主题/内容搜索、逐条 payload 格式切换（自动/文本/Hex/Base64）、复制主题/负载/详情、清空历史
- **连接导入 / 导出** — 原生文件对话框（rfd）导出/导入连接 JSON，按 id 去重
- **SSL/TLS 证书** — 自定义 CA、客户端证书/私钥（双向认证）、忽略 CA 校验
- **全局变量** — 发布主题与负载中使用 `{{变量名}}`，内置 `{{$ts}}`、`{{$ts_ms}}`、`{{$uuid}}`，用户属性同样参与渲染
- **负载格式** — Plaintext / JSON（发送前校验）/ Base64 / Hex
- **阿里云 IoT** — Token（GroupId@@@DeviceId + HMAC-SHA1 签名）与一机一密两种鉴权方式一键生成连接
- **连接测试** — 表单内握手测试（超时可配置），保存前验证连通性
- **日志面板** — 连接级事件日志（CONNACK/SUBACK/PUBACK/错误/重连等），按日落盘 `mqttx-YYYY-MM-DD.log`，设置内一键打开日志目录
- **快捷键** — `Ctrl+N` 新建连接、`Ctrl+,` 设置、`Ctrl+Shift+E` 导出、`Ctrl+Shift+I` 导入、`Ctrl+Enter` 发送消息
- **主题** — Linear 风格配色（近黑/纯白扁平底 + 靛蓝 #5e6ad2 主色，14px 紧凑排版、6px 圆角），浅色 / 深色 / 跟随系统
- **设置** — 自动检查更新开关、消息缓存条数、时间戳毫秒、数据目录展示与打开、关于页（版本 / 许可）
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

每个文件与目录单一职责；模块门面（`mod.rs`）只做聚合导出与再分派。

```
gpui-app/
├── Cargo.toml
├── src/
│   ├── main.rs              # 入口：Sentry 初始化、创建窗口、挂载 gpui-kit Root
│   ├── lib.rs               # 库导出（模型/引擎/UI 可被集成测试复用）
│   ├── model/               # 数据模型（纯数据层，不依赖 GPUI）
│   │   ├── mod.rs           #   门面：聚合导出 + 模型测试
│   │   ├── connection.rs    #   连接配置：协议/传输/遗嘱/TLS/会话参数
│   │   ├── subscription.rs  #   订阅条目与订阅选项
│   │   ├── message.rs       #   消息记录 + 条数/字节双预算环形缓冲
│   │   ├── publish.rs       #   发布参数与发布预设
│   │   ├── variable.rs      #   全局变量与 {{占位符}} 模板渲染
│   │   ├── log.rs           #   运行日志级别与条目
│   │   └── settings.rs      #   应用设置与主题偏好
│   ├── mqtt/                # MQTT 引擎（独立 tokio runtime）
│   │   ├── mod.rs           #   门面：EngineEvent / MqttEngine 连接生命周期
│   │   ├── tls.rs           #   rustls 配置与跳过校验
│   │   ├── v5.rs            #   MQTT 5.0 客户端构建与事件循环
│   │   └── v4.rs            #   MQTT 3.1.1 客户端构建与事件循环
│   ├── update/              # OTA 自更新（Gitea Release 源）
│   │   ├── mod.rs           #   门面：版本比较、资产命名约定
│   │   ├── check.rs         #   检查最新版本与资产挑选
│   │   └── install.rs       #   7z 下载暂存、sha256 校验、自替换安装
│   ├── store.rs             # JSON 持久化（损坏备份与 fsync）
│   ├── sysmon.rs            # 自身进程资源采样（CPU/内存/线程/句柄）
│   ├── platform.rs          # 平台窗口操作（Windows 置顶）
│   ├── aliyun.rs            # 阿里云 IoT 签名与连接生成
│   └── ui/
│       ├── mod.rs           # 界面层导出
│       ├── app/             # 应用外壳
│       │   ├── mod.rs       #   MqttXApp 状态、初始化与渲染分派
│       │   ├── events.rs    #   引擎事件 → 应用状态（消息/日志缓冲）
│       │   ├── actions.rs   #   连接/订阅/预设/设置等用户动作
│       │   ├── io.rs        #   连接配置导入导出
│       │   ├── theme.rs     #   Linear 风格主题应用
│       │   ├── titlebar.rs  #   标题栏（置顶钉子/主题切换/动作菜单）
│       │   ├── sidebar.rs   #   侧边栏（搜索/分组/连接列表）
│       │   └── main_area.rs #   标签页栏与内容装配
│       ├── connection_view/ # 连接工作区
│       │   ├── mod.rs       #   ConnectionView 状态与渲染分派
│       │   ├── subscribe.rs #   订阅表单/列表
│       │   ├── messages.rs  #   消息流、单条消息行与日志
│       │   ├── publish.rs   #   发布表单与发布预设
│       │   ├── vars.rs      #   模板变量面板
│       │   └── util.rs      #   topic 匹配/格式化纯函数
│       ├── connection_form.rs  # 新建/编辑连接表单
│       ├── dialogs/         # 模态对话框集合（设置/资源监控/发布预设/阿里云/变量）
│       ├── widgets/         # 复用小组件（枚举下拉/键值对编辑器/字段行/状态指示）
│       └── logo.rs          # 品牌图标绘制
└── tests/
    └── engine_live.rs       # 真实 broker 端到端测试（默认 ignored）
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
- **版本** 依次取参数 → 当前提交的精确 tag（`git describe --tags --exact-match`）→ `Cargo.toml`；
  必须是合法 semver（形如 `v1.0.1` 或 `1.0.1-rc.1`），否则脚本直接拒绝发布
  —— 非 semver 版本（如 `v1.0.0-4-gabc1234`）会被客户端截断比对，更新永远发不出去
- 每个文件附带 `.sha256` 校验侧车；同名版本重复上传按 409 跳过（Gitea 不允许覆盖）
- 软件包页面：`{GITEA_URL}/{owner}?tab=packages`

### 方式二：CI 自动发布（Gitea Actions）

推送 `v*` tag 即触发 [.gitea/workflows/release.yml](.gitea/workflows/release.yml)：

1. 版本一致性校验：tag 必须等于 `v` + `gpui-app/Cargo.toml` 的版本，否则整条流水线失败
2. 单元测试（`--lib --tests`，集成测试只编译不执行）
3. Linux release 构建（产物重命名为 `mqttx-v<ver>-<target>-mqttx`）
4. 调用 `scripts/publish-gitea.sh`：上传包注册表，并创建同名 Release、
   上传 7z / 裸二进制 / `.sha256` 附件

> **应用内 OTA 只认 Release 附件**，附件名必须匹配 `mqttx-v<ver>-<triple>-mqttx[.exe]`。
> Windows 产物目前由本地 `scripts/publish-gitea.ps1` 上传补充（CI 的 Windows 矩阵项
> 在 runner 就绪前保持注释，见工作流内说明）。

需要在仓库 **Settings → Actions → Secrets** 配置 `GITEA_TOKEN`（需 **repo 写权限**：
脚本除包注册表外还要创建 Release 并上传附件）；
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
