# MQTTX（GPUI 重写版）项目深度分析报告

> - **分析对象**：`mqttx` 工作区（唯一成员 `gpui-app`），HEAD = `bc9f55c`（"refactor(ui): 拆分 app/connection_view/widgets 为子组件"），工作区干净
> - **分析日期**：2026-09-30
> - **分析性质**：**分析阶段全程只读**（未修改/创建/删除任何源文件）。分析完成后按第 0.4 节实施了批次 0 修复，改动清单与验证证据见该节
> - **验证环境**：Windows / rustc 1.101.0-nightly (d080e7dff 2026-09-27) / cargo 1.101.0-nightly
> - **方法**：全量通读 `gpui-app/src` 48 个源文件 + CI/发布脚本/依赖清单；三路并行模块复审（OTA 与阿里云 / UI 表单与对话框 / 支撑模块与工程化）；对复审结论逐条回归源码复核
> - **可信度标注约定**：
>   - ✅ **实测量**：本机实际运行命令得到的结论
>   - ✅ **代码读取**：直接读到源码或依赖源码得到的结论
>   - ⚠️ **依赖语义**：结论依赖第三方库的文档/源码语义，未做实验
>   - 🟡 **需实测**：无法在离线环境判定，必须用真实环境或官方测试向量核对
>   - ❌ **未采信**：证据不足或存在相反证据，本报告不作为缺陷传递

---

## 目录

- [0. 摘要](#0-摘要)
- [1. 项目画像](#1-项目画像)
- [2. 架构分析](#2-架构分析)
- [3. 模块点评](#3-模块点评)
- [4. 值得肯定的设计](#4-值得肯定的设计)
- [5. 问题清单](#5-问题清单)
- [6. 专项分析](#6-专项分析)
- [7. 测试与质量保障](#7-测试与质量保障)
- [8. README 与实现不一致清单](#8-readme-与实现不一致清单)
- [9. 修复路线图](#9-修复路线图)
- [10. 附录](#10-附录)

---

## 0. 摘要

### 0.1 一句话结论

**代码底子明显高于平均线，但工程化外圈是全项目最脆弱的部分。** 分层清晰、竞态处理有针对性设计、持久化语义正确；然而 CI 从未在 Linux 上跑通过、发布默认路径会造出非 semver 版本、Windows 产物靠本地脚本手工补、OTA 的信任边界只有 TLS 一道。**当前最大的问题不是"写得对不对"，而是"发不出去、也升不上来"。**

### 0.2 最紧急的 5 件事（批次 0，约半天）

| # | 事项 | 证据 | 后果 |
|---|---|---|---|
| 1 | Linux 单测确定性失败 + test job 缺图形库依赖 | sysmon.rs:194 + release.yml:39-46 | CI 永远红，无任何产物 —— ✅ **已修复（0.4）** |
| 2 | 发布脚本用 `git describe --tags --always` 当版本 | publish-gitea.sh:71-78 | 造出 `v1.0.0-4-gbc9f55c` 这类 Release，客户端判定"不新"，且遮蔽真更新 —— ✅ **已修复（0.4）** |
| 3 | 版本双真源（tag vs `CARGO_PKG_VERSION`）无校验 | Cargo.toml:3 + release.yml:33 | 忘 bump → 装完仍自报旧版 → 无限提示更新 —— ✅ **已修复（0.4）** |
| 4 | Release 创建/上传失败被静默吞掉（`exit 0`） | publish-gitea.sh:174-181 | job 变绿而 OTA 通道为空 —— ✅ **已修复（0.4）** |
| 5 | 非 Windows 自更新缺可执行位，必失败 | install.rs:92、220 | Linux 用户永远装不上 —— ⏳ 待批次 1 |

### 0.3 问题分布

| 级别 | 数量 | 代表 |
|---|---|---|
| **P0** 阻断发布 / 安全边界 | 6 | 7z 路径穿越、事件总线无背压、发布链路三不通 |
| **P1** 正确性 | 11 | MQTT5 收到 0 值直接断连、主题键名不生效、TOCTOU |
| **P2** 一致性 / 工程化 | 22 | 对话框栈语义、README 漂移、死代码 |
| **未决** 需实测 | 2 | 阿里云签名 content、securemode 与传输层矛盾 |

### 0.4 本次已修复项（2026-09-30，批次 0）

分析完成后实施了批次 0 的修复，改动集中在 CI/脚本/测试与文档，**未触碰任何业务逻辑**：

| 对应问题 | 改动 | 文件 | 验证证据 |
|---|---|---|---|
| P0-1 | 采样测试改为分平台断言（`if cfg!` 而非无条件 `> 0`，两个分支都参与编译） | `gpui-app/src/sysmon.rs` | Windows `cargo test --lib` **33 passed**；`clippy --all-targets` 零告警 |
| P0-1 | test job 补齐 GPUI 的 Linux 系统依赖；命令改为 `--lib --tests`（集成测试只编译不执行） | `.gitea/workflows/release.yml` | YAML 解析无错误；test job 4 个 step 结构正确 |
| P0-2 | 版本解析改用 `git describe --tags --exact-match` + `Cargo.toml` 回退；新增 semver 强制校验 | `scripts/publish-gitea.sh`、`.ps1` | 实测：无参数时回退 `1.0.0`（**不再产出 `v1.0.0-4-gbc9f55c`**）；显式传该串被拒绝且不发生网络请求；`v1.0.1` 通过校验 |
| P0-3 | meta job 新增「tag == v + Cargo.toml 版本」断言，不一致即整条流水线失败 | `.gitea/workflows/release.yml` | 实测断言逻辑：`v1.0.0` 通过，`v1.0.1` / `V1.0.0` 拒绝 |
| 摘要-4 | Release 创建/查询/解析失败一律 `exit 1`（原为 `exit 0`） | `scripts/publish-gitea.sh`、`.ps1` | bash `-n` 语法检查通过；PowerShell 解析器无错误 |
| README 漂移 | 订正版本推断说明，并重写「方式二」CI 流程（双平台 → 实际仅 Linux；补 Release/7z OTA 通道与命名约定、repo 写权限要求） | `README.md` | 人工核对 |

> **批次 0 只解除了"CI 必红"与"版本不可用"两个确定性故障。** 让 Linux 客户端真正装上
> 更新的 **P0-6（产物缺可执行位）属于批次 1，尚未实施**；OTA 信任模型（P0-4）、
> 事件总线背压（P0-5）同样未动。

### 0.5 第二批修复（2026-09-30，批次 1 + 批次 2）

在 0.4 之后继续实施，共 30 个文件、+747/−170。验证：`cargo test --lib` **34 passed**
（新增 1 个资产挑选回归测试）、`clippy --all-targets` **零告警**、主题 JSON 解析确认
`font.size=14 / radius.lg=10` 已生效。

**安全与数据完整性**

| 问题 | 修复 |
|---|---|
| P0-4 7z 路径穿越 | 新增 `verify_archive_entries`：解压前只读归档头校验条目清单，必须**恰好一个**且名字与约定二进制名完全一致（fail-closed）；sevenz-rust 0.6.1 的 `decompress_file` 不做任何路径校验 |
| P0-5 事件总线无背压 | 通道改有界（容量 1024）；`emit` 分级：高频事件（消息/日志）满即丢并计数、每 1000 条提示一次，低频关键事件（状态/订阅结果/发布回执）异步补投 |
| P0-6 非 Windows 缺可执行位 | 新增 `make_executable`（Unix `chmod +x`，Windows 空实现），暂存产物落盘即补权限 |
| P1-7 校验可静默关闭 | ① 无 `.sha256` 侧车 → 拒绝安装（fail-closed）② `install_staged` 安装前重算哈希 ③ 同意标记绑定「版本 + 暂存文件哈希」 |
| P1-8 跨卷 rename 失败 | 新增 `place_file`：rename 失败回退「复制 + fsync + 删源」 |
| P1-9 备份即删 | `cleanup_old` 保留最近一份 `.old` 至 `OLD_BACKUP_KEEP`（7 天），更早的清理 |
| P2-19 无安装互斥 | 新增 `InstallLock`（`create_new` 原子抢占 + 10 分钟陈旧锁接管） |
| P2-21 死分支 | 删除永不命中的 `.update.tmp` 清理条件 |

**正确性**

| 问题 | 修复 |
|---|---|
| P1-1 协议非法值 | 端口改 `parse_port`（1~65535）；接收上限/最大报文/主题别名拒 0（`parse_opt_positive_u32`，0 是 MQTT 5 协议错误）；遗嘱主题 trim + 拒绝通配符 |
| P1-2 校验反馈滞后 | 16 个被校验字段订阅 `InputEvent::Change`，编辑即清空字段错误 |
| P1-3 重连提示漏字段 | `session_params_changed` 补齐 4 个 v5 CONNECT 属性；测试改为逐字段防漏式断言 |
| P1-4 三元组写死 | `platform_triple()` 改用 `std::env::consts::ARCH`；连带修复两个写死三元组导致 Linux CI 必红的资产测试，并新增「非约定附件不得选中」测试 |
| P1-5 固定 2s 退避 | 新增 `reconnect_delay`：指数退避（1s→30s 封顶）+ 0~500ms 抖动 |
| P1-6 测试连接占住 worker | `test_handshake` 的建连（读证书）放 `spawn_blocking`，与 `connect` 一致 |
| P1-10 主题键名不生效 | `font_size`→`font.size`、`radius_lg`→`radius.lg`（实测解析出 14px/10px） |
| 复审 D8 资产挑选退化 | `pick_assets` 按命名约定精确后缀匹配（`mqttx[.exe]` / `.7z`），任何含三元组的其它附件不再被当成可执行文件 |

**性能（P1-11 的低成本部分）**

| 问题 | 修复 |
|---|---|
| 每帧深拷贝消息记录 | `MqttRecord.user_properties` 改 `Arc<[(String, String)]>`（每帧 300 条记录的属性 Vec 深拷贝 → 引用计数） |
| 每帧深拷贝日志 | `app.logs` 改 `VecDeque<Arc<LogEntry>`>（每帧最多 3000 条 × 4 个 String 的堆分配 → 引用计数） |
| 每行重复读实体 | `show_millis` 提升到 `render_messages` 帧级，一次读取传入每行 |

**其他（低成本 P2）**

P2-5 五个对话框 `open()` 统一加 `has_active_dialog` 守卫；P2-6 `OptionDelegate::position`
按框架契约如实实现；P2-9 阿里云对话框预填即选中（不再复制预设）；P2-12
`ConnectionConfig` / `AliyunPreset` 手写脱敏 Debug（`password`/密钥输出 `<redacted>`，
杜绝一次 `{:?}` 把凭据送进 Sentry）；P2-14 CPU 采样间隔过近时沿用上次结果且不前移
差分基准、修正注释口径；P2-17 非 Windows 不渲染必然报错的置顶按钮；
aliyun.rs 补 DeviceName 空值校验；Cargo.toml 补 `license = "Apache-2.0"`。

**仍未修复（明确留待后续）**

P1-11 完整版（消息/日志虚拟列表）；P2-8 KvEditor 行 uid；P2-11 保存失败不可见；
P2-13 硬编码更新源与 Sentry DSN；P2-16 采样在 UI 线程；P2-20 curl 令牌暴露面；
复审 D11（tag 大写 `V` 的归一化不一致）；D12（预发布版本永远收不到正式版）；
LICENSE 文件本体（元数据已声明 Apache-2.0，正文需从官方源获取）；
aliyun.rs 一机一密 securemode 与传输层的矛盾（`0.5` 之外，见 6.4 未决项）。

---

## 1. 项目画像

### 1.1 定位

用 Rust + [gpui-kit](https://github.com/longbridge/gpui-kit)（GPUI 0.3 / gpui-component 0.6）重写的 MQTT 调试客户端，对标官方 Electron 版 MQTTX。**纯 Rust 单二进制，无 Electron/WebView 运行时**。早期 Tauri + Vue 实现已归档到 `tauri-legacy` 分支。

### 1.2 代码量化

| 层 | 行数 | 文件数 | 说明 |
|---|---:|---:|---|
| `ui/` | 7,607 | 27 | 占 66%，是所有复杂度的集中地 |
| `mqtt/` | 1,388 | 4 | 引擎：连接生命周期 + v5/v4 事件循环 + TLS |
| `model/` | 1,055 | 8 | 纯数据层，零 GPUI 依赖 |
| 根模块（store/aliyun/sysmon/platform/main/lib） | 822 | 6 | 持久化、签名、采样、入口 |
| `update/` | 621 | 3 | OTA |
| **合计** | **11,493** | **48** | 另有 tests 97 行、examples 61 行 |

### 1.3 依赖画像

- 直接依赖 **30 项**；`Cargo.lock` **905 个包**，其中 **89 个包存在多版本**（集中在 `windows*` / `hashbrown` / `getrandom` / `rand` 等传递依赖）。
- `rustls` **单版本 0.23.45** —— 这是自定义 TLS（`TlsConfiguration::Rustls`）能与 rumqttc 内部类型互通的前提，属关键约束。
- `ring` 0.17.14 与 `aws-lc-rs` 1.18.1 **同时存在**，因此必须在运行时显式安装 CryptoProvider（tls.rs:104-114）——作者已正确处理。
- 重量级自有选型：`sentry`（错误上报）、`ureq`（OTA 下载）、`sevenz-rust`（纯 Rust 解压）、`rfd`（原生文件对话框）、`embed-resource`（图标）。

### 1.4 产物与构建

| 项 | 实测值 |
|---|---|
| `target/release/mqttx.exe` | **20.4 MB**（README 写 ~17MB，已过时） |
| `target/debug/mqttx.exe` | 68.0 MB |
| release profile | `lto=true, codegen-units=1, opt-level=3, panic="abort", strip=true` |
| 工具链 | 本机同时装有 stable / nightly；README 声称"必须 nightly"，但仓库无 `rust-toolchain.toml`，CI 用默认 stable 🟡 |

### 1.5 提交历史

- 66 次提交，2026-08-18 → 2026-09-29（41 天），单人双身份（JoeAllen / Joe Allen）。
- 提交信息质量高，且**修复类提交带根因描述**（如"连接生命周期竞态与事件处理""数据文件损坏防护、写盘 fsync 与临时文件清理"）。
- 最近两次提交是纯结构重构（拆 `model/mqtt/update`、拆 `ui/app`），说明项目正处在**"功能收口、开始整理结构"**的阶段——此刻做架构级修复成本最低。

---

## 2. 架构分析

### 2.1 分层与依赖方向

```
main.rs ──► ui::app::MqttXApp（唯一状态所有者）
              ├─ model/*      纯数据 + 纯函数（不依赖 GPUI）
              ├─ store.rs     JSON 原子写（tmp + fsync + rename）
              ├─ mqtt/*       引擎（独立 tokio runtime）
              └─ update/*     OTA（ureq + 7z + 自替换）
lib.rs 导出 model/mqtt/store/update，使 tests/ 能脱离 GUI 测引擎
```

**依赖方向严格单向、无环**：`ui → {model, store, mqtt, update}`，`mqtt → model`，`model` 不依赖任何上层。这是 11k 行规模下最难得的一点：数据层零 GPUI 依赖，33 个单测里有 25 个因此可以直接测业务逻辑。

### 2.2 并发模型

```
GPUI 主线程                          engine 线程（tokio, 2 worker）
─────────────                        ──────────────────────────────
MqttXApp 状态                        MqttEngine { conns: Mutex<HashMap>, runtime }
   ▲                                      │
   │ cx.notify()                          ├─ connect(): 占位句柄(client=None) + generation
   │                                      ├─ spawn_blocking: 建 client（读证书/CA 文件）
   │                                      ├─ 事件循环: tokio::select!{ poll() , cancel_rx }
   └── smol::channel::unbounded ◄─────────┘  emit(): tx.try_send(event)   ← 无界、不阻塞
```

三个关键设计：

1. **连接代数（generation）**：同一 `id` 的每次 `connect` 递增代数；事件循环退出时只删自己那一代（`remove_handle_if`）。这修掉了"重连后句柄被旧循环误删、从此无法从 UI 断开"的经典竞态。
2. **建连期占位句柄 `client: None`**：让 `close()` 在建连中途也能取消，且 `is_connected()` 在此之前恒为 false，天然做了防抖。
3. **阻塞 IO 分离**：`connect` 把证书读取放进 `spawn_blocking`（mqtt/mod.rs:198-200），OTA 下载走 `run_blocking`。**但 `test_connection` 是例外**（见 P1-6）。

**缺口**：`emit` 用 `try_send`（mqtt/mod.rs:127-128），通道容量由 UI 侧决定（ui/app/mod.rs:129 用的是 `unbounded`）。当前等价于"永不丢弃、永不阻塞"——见 P0-5。

### 2.3 一次消息的完整旅程

```
broker ──► eventloop.poll() ──► v5_record() ──► engine.intern(topic)   ← Arc<str> 驻留
                              │                  engine.emit(Message)
                              ▼
                     [无界 smol 通道]  ← 背压缺口在这里
                              ▼
   UI pump ──► on_engine_event():
        ├─ 丢弃已删除连接的事件（防泄漏）
        ├─ push_message()  → MessageRing（条数 + 32MB 字节双预算驱逐）
        ├─ push_log()      → 内存环形(3000) + 同步写 mqttx-YYYY-MM-DD.log  ← 磁盘 IO 在 UI 线程
        └─ view.notify()   → 下一次渲染重建 ≤300 行消息
```

这条链路有三处刻意优化（主题驻留、双预算驱逐、日志句柄按日缓存），也有三处代价（无背压、UI 线程落盘、每帧全量重建），构成性能与可靠性问题的主干。

### 2.4 连接生命周期

`ConnectionStatus` 四态由引擎单向推送，UI 不猜测状态，只在 `Connected` 事件上触发 **auto resubscribe**。订阅采用**乐观更新**：先入库落盘，SUBACK 失败再回滚，并特意避免"SUBACK 前被删除的订阅被事件用默认字段复活"。

### 2.5 持久化

- 数据目录：`%APPDATA%\MQTTX-GPUI`（刻意与官方 Electron 版 `MQTTX/` 区分）。
- 6 个 JSON 文件：connections / subscriptions / presets / variables / aliyun / settings。
- 写入语义：`tmp(pid 命名) → write_all → fsync → rename`，失败清理 tmp。
- **损坏文件防护**：加载失败的文件名进 session 集合，下次保存前先重命名为 `.corrupt-<时间戳>` 备份，把"连接全没了且被空数据覆盖"从不可恢复降为可抢救。
- 读取容忍 UTF-8 BOM。

### 2.6 OTA 链路

```
启动 ──► cleanup_old() ──► load_staged() ──► consent 匹配? ──► install_staged()（改名+spawn 新进程）──► 退出
                                  │ 否
                                  └─► 进入 UI，auto_check_update 时联网检查
check_latest() ─► pick_assets() ─► download_and_stage() ─► 大小对账 + sha256 ─► 弹窗征得同意
```

设计上明确"**下载即暂存、用户同意才安装**"，拒绝静默强制升级——产品方向正确；代价是暂存区成为长期驻留的攻击面（见 P0-4、P1-7）。

### 2.7 UI 组织与渲染

`ui/app/`（外壳与状态）+ `ui/connection_view/`（单连接工作区）+ `ui/dialogs/`（模态）+ `ui/widgets/`（复用件）。`ConnectionView` 是最大的实体（约 30 个字段，含 12 个 `Entity<InputState>`）。

渲染上限：`MAX_RENDERED_MESSAGES = 300`、`MAX_RENDERED_LOGS = 3000`；消息最新在最上方。**每帧全量重建**，无虚拟列表——见 P1-11。

---

## 3. 模块点评

| 模块 | 职责 | 亮点 | 主要问题 |
|---|---|---|---|
| `model/` | 纯数据 + 纯函数 | 字节预算环形缓冲；非文本负载保留原始字节；模板渲染单时间基准 | `session_params_changed` 漏字段（P1-3） |
| `mqtt/` | 引擎与事件循环 | 连接代数、占位句柄、可取消退避、显式 CryptoProvider | 无背压（P0-5）、`test_connection` 阻塞 IO（P1-6）、固定 2s 退避（P1-5） |
| `store.rs` | JSON 原子持久化 | fsync+rename、损坏备份、BOM 容忍 | 明文密码落盘（P2-12） |
| `update/` | OTA | 大小对账、双阶段回滚、同意绑定版本 | 路径穿越（P0-4）、缺可执行位（P0-6）、TOCTOU（P1-7）、跨卷 rename（P1-8）、备份即删（P1-9） |
| `aliyun.rs` | 阿里云签名 | Token 模式与官方一致 | securemode/传输矛盾（未决-2）、无测试 |
| `sysmon.rs` | 进程采样 | Win32 结构口径正确、失败不 panic | Linux 测试必失败（P0-1）、CPU 基准前移（P2-14） |
| `platform.rs` | 窗口置顶 | 失败路径不 panic | 非 Windows 必报错（P2-17）、双分派死代码 |
| `ui/` | 全部界面 | 色值零硬编码、异步回调统一弱引用 | 渲染热路径（P1-11）、对话框栈语义（P2-5）、键名不生效（P1-10） |

---

## 4. 值得肯定的设计

1. **连接代数 + 占位句柄**（mqtt/mod.rs:153-250）——正确解决"重连/关闭与建连并发"这一最难调的竞态。
2. **消息环形缓冲的条数 + 字节双预算**（message.rs:136-146，32MB/连接）——大报文流下条数上限形同虚设（2000×128KB≈256MB），作者显式解决了。
3. **非文本负载保留原始字节**（message.rs:180-197）——lossy 文本会失真，Hex/Base64 详情仍能对上线上报文。
4. **损坏文件"先备份再覆盖"**（store.rs:88-103）——把不可恢复的丢失降为可抢救。
5. **落盘 tmp+fsync+rename + pid 命名**（store.rs:105-122）——掉电语义正确，并发不互踩。
6. **显式安装 rustls CryptoProvider**（tls.rs:104-114）——ring 与 aws-lc-rs 并存时 `ClientConfig::builder()` 会 panic，属隐藏地雷，被提前拆掉。
7. **下载大小对账**（install.rs:110-116）——弥补 `read()==0` 无法区分"正常结束/断流"，判断精准。
8. **自替换双阶段回滚**（install.rs:212-242）——rename 失败与 spawn 失败都尝试回滚，并区分"已恢复/回滚失败+备份路径"。
9. **同意标记绑定版本**（install.rs:265-271）——避免"下完就换"。
10. **主题色值零硬编码**——范围内 11 个文件仅 3 处出现字面色，且都是主题色的 alpha 派生，浅/深色都成立。
11. **异步回调统一弱引用 + `.ok()`**——无 Entity 泄漏，不会在实体/窗口销毁后 panic；资源监控用弱引用退出循环并有注释说明。
12. **消息内存按字节预算**与 `flood` 压测工具并存，说明作者主动验证过内存行为。
13. **sysmon 的 Win32 用法正确**——`PROCESS_MEMORY_COUNTERS_EX` 扁平结构 + 按 EX 大小传 `cb`，`PrivateUsage` 取"提交内存"口径；CPU 用 `GetProcessTimes` 差分而非近似采样。
14. **build.rs 用 `CARGO_CFG_TARGET_OS` 而非宿主 OS**——交叉编译时行为正确；"资源 ID 1"的注释与 gpui-pre-windows 的 `LoadImageW` 实测一致。

---

## 5. 问题清单

> 每条格式：**现象 → 证据（文件:行）→ 影响 → 修复方向**。标题后的标记表示验证状态。文件路径相对仓库根，格式为 `路径:行`。

### P0-1 ✅实测 【阻断】Linux 单测确定性失败，CI 永远红 —— 🛠 已修复（见 0.4）

- **证据**：sysmon.rs:9（`Sample` 派生 `Default`）、41（非 Windows 直接返回 `Sample::default()`，`working_set = 0`）、194（无条件 `assert!(second.working_set > 0)`）；release.yml:39-46 在 `ubuntu-latest` 跑 `cargo test -p mqttx-desktop --lib`；release.yml:52-53 build/publish `needs: [meta, test]`。
- **加重因素**：test job 未安装任何图形库 `-dev` 包，而同一 workflow 的 build job 装了 10 个（"Install Linux system deps (GPUI)"）。同一份依赖图，test job 在干净 runner 上大概率连编译都过不去。
- **影响**：打 `v*` tag 后 Test 阶段即断，产物与 Release 一个都发不出去。该红灯在 `sysmon` 引入之后才存在，而最后一次 tag（v1.0.0）早于它，**因此从未暴露**。
- **修复**：该测试加 `#[cfg(target_os = "windows")]` 或分平台断言；test job 复用 build job 的依赖安装步骤。

### P0-2 ✅实测 【OTA】发布脚本产出非 semver 版本，静默废掉更新通道 —— 🛠 已修复（见 0.4）

- **证据**：publish-gitea.sh:71-78、publish-gitea.ps1:115-123 用 `git describe --tags --always` 兜底版本。实测本仓库 HEAD：`git describe --tags --always` → `v1.0.0-4-gbc9f55c`，`--exact-match` 失败（HEAD 无 tag）。
- **影响链**：`VER_NUM = 1.0.0-4-gbc9f55c` → 创建/复用该 tag 的公开 Release → 客户端 `release_part`（update/mod.rs:78-84）截成 `1.0.0` → `is_newer` 判否 → **老用户永远收不到更新**；且该 Release 会被 `/releases/latest` 命中，**把真正的更新挡在后面**。
- **修复**：脚本入口强制 semver 正则校验，不匹配即失败；把 `--always` 换成 `--exact-match`。

### P0-3 ✅代码读取 【发布一致性】版本双真源，无一致性校验 —— 🛠 已修复（见 0.4）

- **证据**：Cargo.toml:3（`CARGO_PKG_VERSION`，当前 1.0.0）vs release.yml:33（`GITHUB_REF_NAME`），workflow 内无比对步骤。
- **影响**：打 `v1.0.4` 但忘 bump → 新包装完仍自报 1.0.0 → 每次启动重复提示同一更新（无限循环）；反向则永远无更新提示。
- **修复**：CI 增加 `v$(Cargo.toml version) == tag` 断言；或让 build.rs 从 tag 生成版本，收敛为单一真源。

### P0-4 ✅代码读取 【安全】7z 解压路径穿越 + 同源 sha256

- **证据**：install.rs:59 调用 `sevenz_rust::decompress_file`；依赖 `sevenz-rust-0.6.1/src/de_funcs.rs:102` 直接 `dest.join(entry.name())`，**不校验绝对路径与 `..`**，且 `default_entry_extract_fn` 会自动 `create_dir_all` 父目录。校验值来自同一 Release 的侧车（check.rs:52-59 → install.rs:119）。
- **影响**：能创建 Release 的人（或能改写 Gitea 响应的一方）可对所有客户端完成任意路径写入（含启动目录），随后 `install_staged` 还会执行解压出的同名二进制。sha256 只防传输损坏，不防伪造。
- **修复**：改用 `decompress_with_extract_fn` 做白名单 + 拒绝 `..`/盘符/UNC + 限制条目数与解压总量；同时引入 ed25519/minisign 签名（内嵌公钥）作为真正的信任锚。

### P0-5 ✅代码读取 【可靠性】事件总线无背压，极端流量下内存无上界

- **证据**：ui/app/mod.rs:129 建无界通道；mqtt/mod.rs:127-128 `try_send` 永不失败；消费端每条事件做线性查找 + 入环形缓冲 + **同步写日志文件**（events.rs:184 → 195-259）+ `cx.notify()`。
- **影响**：环形缓冲只约束"已入库"的记录，**不约束"在途"事件**。到达速率持续超过 UI 消费速率时队列无界增长，每个在途 `Message` 事件最长可携带 128KB payload。仓库自带的 `examples/flood.rs` 正是按速率压测的发布器，说明作者关心该场景，但管道本身无兜底。
- **修复**：改有界通道 + 显式丢弃策略（消息可丢、状态/日志不可丢）；或把 `Message` 事件批量化（一次携带 `Vec<MqttRecord>`），把"每条一次 notify"变成"每帧一次"；日志落盘移出热路径。

### P0-6 ✅代码读取 【OTA】非 Windows 自更新因缺可执行位必定失败

- **证据**：install.rs:92 用 `File::create` 落盘（Unix 权限 0644），7z 路径的解压同样走 `File::create`；install.rs:212 `rename` 就位（保留权限）；install.rs:220 `Command::new(&cur).spawn()` → **EACCES**，走回滚 222-227。
- **影响**：Linux/macOS 用户永远装不上，且提示只有一句"启动新版本失败"。
- **修复**：Unix 下在 rename 前 `set_permissions(0o755)`。

### P1-1 ✅代码读取 【协议】MQTT 5 的 0 值参数直接进 CONNECT 并落盘

- **证据**：connection_form.rs:431-444 的 `parse_opt_u32` 对 `"0"` 返回 `Some(0)`（只做 u16 上限转换）→ v5.rs:79-85 原样 `set_receive_maximum(Some(0))` / `set_max_packet_size(Some(0))` → 保存即写 `connections.json`。
- **影响**：MQTT 5 规范把 **Receive Maximum = 0** 与 **Maximum Packet Size = 0** 定义为 Protocol Error，broker 直接断连；值被持久化，重启后持续失败，用户只看到"连接测试失败"。
- **修复**：解析后 `filter(|v| *v > 0)`，文案写明"必须 ≥1（0 为协议错误）"。

### P1-2 ✅代码读取 【校验】表单校验"一次性 + 首错即停 + 无变更订阅"

- **证据**：connection_form.rs:387-389（每次 build 先 `clear`）、346-350（`fail` 后立即 `?` 返回）、731-734（仅从 `field_errors` 渲染，无输入变更订阅）。
- **影响**：改好端口后红字仍在；3 个字段非法要点 3 次保存才能看全。
- **修复**：订阅 `InputState` 的 `Change` 清除对应 key；或一次性收集全部错误。

### P1-3 ✅代码读取 【正确性】`session_params_changed` 漏掉 4 个 v5 CONNECT 属性

- **证据**：connection.rs:255-268 比较了 host/port/path/transport/protocol/client_id/账号/clean_start/keep_alive/遗嘱/SSL，**唯独漏了** `session_expiry_interval`、`receive_maximum`、`maximum_packet_size`、`topic_alias_maximum`——而它们在 v5.rs:73-87 确实是随 CONNECT 一次性生效的。
- **影响**：连接已建立时改这四项，用户不会收到"重连后生效"提示，改动静默不生效。对应的单测自称覆盖"所有 CONNECT 时报文生效字段"，实际未覆盖，**给了虚假安全感**。
- **修复**：补齐比较；测试改为遍历式断言（列出全部 CONNECT 字段逐一改动验证）。

### P1-4 ✅代码读取 【平台】`platform_triple()` 硬编码 x86_64

- **证据**：update/mod.rs:56-64 只按 `cfg!(target_os)` 判断，架构写死 `x86_64-*`；而发布脚本 publish-gitea.sh:107-108 明确支持 `aarch64-apple-darwin` / `aarch64-unknown-linux-gnu`。
- **影响**：ARM 客户端永远发现不了更新；发布端若产出 aarch64 资产，则成为**永远无人下载的僵尸资产**。当前 CI 只出 Linux x86_64，属潜在坑。
- **修复**：`std::env::consts::ARCH` 参与拼接；补"客户端三元组 == 脚本三元组"的对照测试。

### P1-5 ✅代码读取 【可靠性】重连固定 2 秒，无指数退避与抖动

- **证据**：v5.rs:338-341、v4.rs:301-304；`max_reconnect_times` 默认 0 即无限重连。
- **影响**：broker 长时间不可用时每 2 秒一次尝试 + 每 2 秒一次 `Status::Error` 事件 → UI 抖动与日志刷屏；多连接会在同一时刻齐步重连（惊群）。
- **修复**：指数退避（1s→2s→…→60s）+ 抖动；连续失败期间不重复推送 `Error` 状态。

### P1-6 ✅代码读取 【一致性】`test_connection` 在 runtime 线程做阻塞 IO

- **证据**：mqtt/mod.rs:568-585 直接把 `test_handshake(&cfg)` 放进 `timeout`，而 `test_handshake` 内部同步调用 `build_v5/build_v4`（读 CA/证书文件）。对照 mqtt/mod.rs:198-200，作者在 `connect` 里明确写了"build 含阻塞 IO，放 spawn_blocking"。
- **影响**：慢盘上测试连接会占住 2 个 worker 之一，影响同一 runtime 上其他连接的收发。
- **修复**：同样走 `spawn_blocking`。

### P1-7 ✅代码读取 【安全】安装前不复核 + 无侧车静默跳过校验

- **证据**：install.rs:204-231 只 rename+spawn，不重算哈希；check.rs:115-119 同版本已暂存即跳过下载；install.rs:177-200 信任目录内任何命名合规文件；install.rs:265-271 consent 只比对版本字符串。侧车缺失时 check.rs:52-59 返回 `None` → install.rs:153-156 → 跳过校验，而 UI 仍写"已通过 sha256 校验"（ui/app/mod.rs:173）。
- **影响**：一次 Release 漏传侧车，所有客户端就在零完整性校验下安装；下载校验完成到"下次启动安装"之间，暂存文件被替换/损坏仍会被原样安装。
- **修复**：无侧车拒绝安装或显式降级提示；安装前重算哈希；consent 绑定哈希。

### P1-8 ✅代码读取 【可靠性】跨卷 `rename` 必失败，且报错误导

- **证据**：install.rs:11-18 暂存在 `%APPDATA%\MQTTX-GPUI\updates`；install.rs:205-212 把 exe 同目录的文件改名就位。Windows 上跨卷 `MoveFile` 直接失败（`ERROR_NOT_SAME_DEVICE`），而错误文案是"替换失败（可能无写入权限）"。
- **影响**：应用装在 D 盘、`%APPDATA%` 在 C 盘时更新必失败；域环境 Roaming 重定向到网络盘更常见。
- **修复**：rename 失败回退 `copy + fsync + replace`；或把暂存放 exe 同目录。

### P1-9 ✅代码读取 【可靠性】新版本首启即删除唯一回滚备份

- **证据**：main.rs:26（安装后新进程启动第一步就 `cleanup_old()`）→ install.rs:289-293 无条件删 `*.old`；ui/app/mod.rs:144 再删一次。
- **影响**：自替换已覆盖原 exe，备份又被新版本自己删掉；新版本一旦启动即崩溃，用户没有任何自助回退路径。
- **修复**：新版本健康运行一段时间后再删；或保留最近 1-2 份 + 启动失败自动回退。

### P1-10 ✅代码读取 【UI】主题 `font_size` / `radius_lg` 键名不符规范，被静默忽略

- **证据**：linear-theme.json:6-8 写 `"font_size": 14` / `"radius": 6` / `"radius_lg": 10`；依赖 schema `gpui-component-0.6.6/src/theme/schema.rs:36-71` 为 `#[serde(default)]` 且 `rename = "font.size"`（默认 16）、`rename = "radius.lg"`（默认 8）、`rename = "radius"`（默认 6），未知键不报错。
- **影响**：主题里写的 14px 字号与 10px 大圆角**从未生效**（实际是框架默认 16px / 8px），只有 `radius: 6` 因为同名而生效。README 的"14px 紧凑排版"因此不成立。
- **修复**：改成 `"font.size": 14`、`"radius.lg": 10`；建议补一条"主题键名与 schema 对齐"的测试。

### P1-11 ✅代码读取 【性能】渲染热路径全量深拷贝

- **证据**：消息每帧 `r.clone()` 最多 300 条（messages.rs:438-478），`MqttRecord::clone` 虽共享 `Arc<str>`，但 `user_properties: Vec<(String,String)>` 与 4 个 `Option<String>` 是深拷贝；日志每帧 `filter(..).take(3000).cloned()`（messages.rs:687-698），每条 `LogEntry` 含 4 个 `String`；发布面板**每帧**对 topic+payload 做 `extract_placeholder_keys`（publish.rs:180-184），展开时还每帧 `render_template`（vars.rs:84-88）；每行额外一次 `with_app` 读 `show_millis`（messages.rs:153-155）。
- **影响**：日志面板打开且有 3000 条日志时，60fps 下约 **18 万次 LogEntry 克隆/秒（≈72 万次 String 分配/秒）**；发布大负载时每帧一次 MB 级拷贝。
- **修复**：虚拟列表（只渲染可见窗口）；`show_millis`/`subs` 提到帧级只算一次；用 `Arc<MqttRecord>` 让列表渲染零拷贝；占位符提取改为 `Change` 事件增量计算并缓存。

### P2 清单（22 项，摘要）

| # | 问题 | 证据 | 影响 |
|---|---|---|---|
| P2-1 | 端口允许 0 | connection_form.rs:361-365、405 | 非法值落盘，报错与真实原因无关 |
| P2-2 | 遗嘱主题不 trim、不校验通配符 | connection_form.rs:530-546 | 保存成功、连接必失败 |
| P2-3 | 传输切换的"默认端口"判定跨所有传输 | connection_form.rs:257-267 | 手输 8084 后切 TLS 被静默改 8883 |
| P2-4 | 非 WS 传输下 path 仍入库但输入框隐藏 | connection_form.rs:500-503 | 隐形脏数据 |
| P2-5 | `close_dialog` 关栈顶而非自己；叠加守卫只挂 5 个快捷键 | settings_dialog.rs:568、ui/app/mod.rs:284-330 | 嵌套确认时可能关错层 |
| P2-6 | `OptionDelegate::position()` 恒 `None`，违背框架契约 | widgets/select.rs:49-59 | 一旦启用搜索或 `set_selected_value` 即"下拉显示空、保存写默认值" |
| P2-7 | KvEditor 空 key 静默丢弃、重复 key 静默覆盖 | widgets/kv_editor.rs:54-73 | 用户填的行保存后无声消失 |
| P2-8 | KvEditor 用下标做元素 id | widgets/kv_editor.rs:85-100 | 删中间行后 hover/焦点串台一帧 |
| P2-9 | 阿里云对话框预填 first 但 `selected=None` | dialogs/aliyun_dialog.rs:54-58、188-190 | 打开即保存 → 凭空复制一条预设 |
| P2-10 | 阿里云"生成并连接"每次新建连接并落盘 | dialogs/aliyun_dialog.rs:206-213、ui/app/actions.rs:16-23 | 连点两次多两条同名连接 |
| P2-11 | 保存失败不可见（写盘失败仅 `elog`） | ui/app/actions.rs:16-23、store.rs:140-144 | 磁盘满/无权限时用户以为已保存 |
| P2-12 | 明文落盘与导出：连接密码、AccessKeySecret/DeviceSecret | store.rs:221-225、aliyun.rs:66-77 | 导出文件即凭据；两结构体 `derive(Debug)`，一次 `{:?}` 就进 stderr+Sentry（当前未见此类调用） |
| P2-13 | 硬编码基础设施地址与 Sentry DSN | main.rs:8-9、update/mod.rs:22-24 | 换源/开源需改代码 |
| P2-14 | CPU 采样 dt≤50ms 时既报 0 又前移基准；注释与公式矛盾 | sysmon.rs:118-125、17 | 手点"立即刷新"读到 0.0%；按注释理解会误判 8 倍 |
| P2-15 | 非 Windows 返回"全 0"而非"能力缺失" | sysmon.rs:41-43 | UI 显示 `0 B / 0%`，无法区分"不支持" |
| P2-16 | 线程采样抓全系统快照并同步跑在 UI 线程 | sysmon.rs:131-148 | 线程数上万时毫秒级开销落在渲染路径 |
| P2-17 | 非 Windows 置顶按钮必弹错误；双分派死代码 | platform.rs:5-8、37-40 | 跨平台产品上的坏按钮 |
| P2-18 | `build.rs` 用 `manifest_required()` 把"缺 rc.exe"升级为硬失败 ⚠️ | build.rs:11-13 | 无 Windows SDK 时连 UI 都起不来（代价只是图标回退默认） |
| P2-19 | 无单实例/安装互斥 | 全仓库无单实例锁 | 两实例互删暂存、争抢同一个 `.old` |
| P2-20 | 发布脚本令牌走 curl 命令行；Release 失败 `exit 0` | publish-gitea.sh:119-139、174-181 | 同机 `ps` 可见令牌；job 变绿但 OTA 通道为空 |
| P2-21 | `.update.tmp` 清理分支永不命中 | install.rs:290 vs 39-47 | 历史遗留 tmp 永远清不掉 |
| P2-22 | 无 LICENSE 却宣称 Apache-2.0；Cargo 无 license 字段 | settings_dialog.rs:506；`glob LICENSE*` 为空 | 对外分发前的合规缺口 |

其他低危项（供参考）：`engine_live.rs` 从不读取 `MQTTX_LIVE` 环境变量；`pump()` 把通道关闭当空轮询；`flood.rs` 的 eventloop 静默吞错且统计不区分"已请求/已发出"；`widgets/select.rs:84` 的 `select_element` 零调用；`logo.rs:122` 死导入；`resource_dialog` 连接卡缺溢出保护。

---

## 6. 专项分析

### 6.1 发布链路：三种走法都不通（推演）

| 走法 | 结果 | 依据 |
|---|---|---|
| **CI 自动发布**（tag → build → publish） | **不通**：Test 阶段必断 | P0-1 |
| **本地脚本默认路径**（不带 `-Version` 直接跑） | **通路但产出垃圾版本**：Release tag 变成 `v1.0.0-4-gbc9f55c` | P0-2（实测） |
| **本地脚本显式版本**（`-Version v1.0.1`） | **相对可用**，但仍需人工保证 tag 与 Cargo 版本一致，且 Windows triple 硬编码 | P0-3、P2-20 |

**结论**：在批次 0 完成之前，"发布"这件事在三种走法里没有一条是可靠的。这也解释了为什么 OTA 相关缺陷至今未被触发——通道本身从未真正跑通过。

### 6.2 OTA 信任模型

当前信任锚只有两个：**HTTPS/TLS 证书** 与 **Gitea 的发布权限**。完整性校验（sha256 侧车）与产物同源，只能证明"传输没坏"。叠加以下事实，构成一条完整的自动执行链：

1. 暂存目录与同意标记都在用户可写的 `%APPDATA%`（P1-7 / P2-19）；
2. `load_staged` 信任目录内任何命名合规的文件（install.rs:177-200）；
3. 同意标记是纯文本版本号，任何同用户进程可伪造（install.rs:246-271）；
4. 安装前不复核哈希（P1-7）；
5. 7z 解压可越界写（P0-4）。

当应用安装在 `Program Files`（替换 exe 需管理员）时，这等于**把用户可写目录接进了管理员执行路径**。

**加固优先级**：签名验证 > 解压白名单 > consent 绑定哈希 > 安装前复核 > 安装互斥锁。

### 6.3 性能与内存

- **已做对的**：单条 payload 128KB 截断、每连接 32MB 字节预算、主题字符串驻留、消息列表渲染上限 300。
- **缺口**：无背压（P0-5）、每帧全量深拷贝（P1-11）、日志同步落盘。
- **失效模式链**：高频消息 → UI 线程写日志/重建元素变慢 → 无界队列积压 → 内存增长。三处缺一不可，**先修背压收益最大**。
- 建议观测指标：单连接 payload+raw 驻留字节（已有 `retained_bytes()` 可直接复用）、通道在途事件数、单帧渲染耗时。

### 6.4 阿里云签名争议 ❌ 未采信

**争议点**：一机一密的 signing content 中 `clientId` 段应取什么值。

**复审意见**（被列为中高缺陷）：认为官方 content 应使用"客户端实际发送的完整 clientId"，且 clientId 前缀应为 `deviceName` 而非 `productKey.deviceName`。

**本报告不采信，理由**：

1. 实现、模块文档、提交信息三者自洽——aliyun.rs:11-13 明写 content 为 `clientId{ProductKey}.{DeviceName}deviceName{...}productKey{...}timestamp{...}`，代码 176-179 完全照此实现；提交 `2f35bca`（"按物联网平台官方直连规范重写"）的 diff 显示这是**对照规范后的重写结果**，不是随手写的。
2. 该拼装形式（content 的 clientId 段取 `pk.dn`、不带 `|securemode…|` 后缀）与常见 Aliyun IoT SDK 示例一致。
3. 复审自身声明"无法联网核对官方原文"，其论据"两处拼装必须自洽"并不成立——规范允许 content 中的占位值与报文中的字面值不同。
4. 本报告作者尝试核验亦失败：本环境 `help.aliyun.com` 解析到非公网 IP（`web_fetch` 直接拒绝），`web_search` 超时。

**处理建议（发布前必做，约 10 分钟）**：用官方文档自带的示例四元组（ProductKey / DeviceName / DeviceSecret / timestamp → 示例 password）建立**离线测试向量**，两种拼装各算一次，与示例比对。这同时补上了 `aliyun.rs` 目前零测试的缺口。

**但"securemode 与传输层矛盾"保留为未决项**（见附录 C）：代码在 clientId 里声明 `securemode=2`（官方语义为 TLS 直连），而 aliyun.rs:142-143 固定 `TransportKind::Tcp`(1883)。这是**代码内部可判定的不一致**，与 content 规则如何无关——声明与实际传输必须一致（要么 `securemode=3` + TCP，要么 TLS + 8883）。需设备实测确认 broker 是否拒绝。

### 6.5 主题与视觉

- **做得好的**：色值零硬编码，全部经 `cx.theme()` 或主题色 alpha 派生；`hue_color()` 按色相段微调明度以兼顾浅/深色；订阅色板有语义。
- **缺陷**：P1-10 的两个键名被静默忽略，导致"14px 紧凑排版"名不副实。
- **建议**：把主题 JSON 的键名与 `schema.rs` 做一次对齐检查并纳入 CI（该 JSON 是 `include_str!` 内嵌的，编译期无法发现键名错误）。

---

## 7. 测试与质量保障

### 7.1 现状

| 项 | 值 |
|---|---|
| 单测数量 | **33 个**，全部通过（本机 Windows，0.19s） |
| 分布 | `model/mod.rs` 18、`update/mod.rs` 6、`store.rs` 5、`sysmon.rs` 2、`connection_view/util.rs` 2 |
| 集成测试 | `tests/engine_live.rs` 2 个，**默认 `#[ignore]`**，依赖外网 |
| 无测试的源文件 | **43 / 48** |
| 静态检查 | `cargo clippy --all-targets` 零告警 |
| CI 覆盖 | `cargo test --lib`（Linux，当前必失败）+ 默认 `cargo build`；**不编译** tests/examples |

### 7.2 覆盖矩阵

| 模块 | 单测 | 说明 |
|---|---|---|
| model（纯函数） | ✅ 充分 | 覆盖了 Hex 奇数长度 panic、UTF-8 截断边界、字节预算、向后兼容反序列化等真实缺陷 |
| store | ✅ 基本够 | 导入导出往返、BOM、损坏备份 |
| update（纯函数） | ⚠️ 偏弱 | 版本比较/资产挑选有覆盖；**签名、解压、安装路径三条最危险的分支零覆盖**；"命名约定"测试是同义反复（把同一个 `format!` 再写一遍），无法发现与 CI/脚本的偏离 |
| **mqtt（引擎）** | ❌ **零** | 并发、生命周期、代数、取消语义全无自动化验证 |
| **aliyun** | ❌ **零** | 签名是纯函数，最适合表驱动测试 |
| **ui** | ❌ 仅 util | 表单校验、对话框生命周期无覆盖 |

**根本问题不是测试写得差，而是模块选错了**：测试全押在纯数据层，把风险最高的引擎留给了手工验证。

### 7.3 建议补的最小测试集（均无需外网）

1. 连接代数与 `remove_handle_if` 语义、`close` 期间建连完成的丢弃路径；
2. `max_reconnect_times` 计数与成功后清零；
3. `topic_matches` 的 MQTT 规范边界（`a/#` vs `a`、`+` vs 空层、`sport/+` vs `sport/`）；
4. `session_params_changed` 的字段完备性（遍历式防漏）；
5. 阿里云两种签名对官方测试向量；
6. `connection_form::build` 的边界（端口 0 / 65536、receive_maximum 0、遗嘱主题带 `#`）；
7. 主题 JSON 键名与 `schema.rs` 对齐。

---

## 8. README 与实现不一致清单

| # | README | 实际 |
|---|---|---|
| 1 | L13 "用户属性…连接/遗嘱/发布三处均支持" | **只有发布支持**。`ConnectionConfig` 无该字段；`LastWill` 只有 content_type/response_topic |
| 2 | L160 "Windows / Linux 双平台 release 构建" | release.yml:62-65 Windows 矩阵整段被注释，CI 只出 Linux |
| 3 | L160 命名 `mqttx-<ver>-<target>-<ext>` | 实际是 `mqttx-<ver>-<target>-mqttx[.exe]`（`<ext>` 易被误读） |
| 4 | L161 只写"上传到包注册表" | **完全漏掉 Release 附件（7z + 裸二进制 + .sha256）这一 OTA 唯一通道**与命名约定 |
| 5 | L99 "前置：Rust nightly" | 无 `rust-toolchain.toml`；CI 用默认 stable；代码无 `#![feature]` 🟡 |
| 6 | L118 `MQTTX_LIVE=1` | 代码从不读该变量，只有 `#[ignore]` 生效 |
| 7 | L132 只需 package 写权限 | 脚本还要创建 Release/上传附件，需 repo 写权限 |
| 8 | L6 归档链接 `../../archive/tauri-legacy.tar.gz` | 不存在；真实归档是分支 `tauri-legacy` |
| 9 | L24 快捷键列表 | 缺 `Ctrl+Shift+R`（资源监控）；`Ctrl+Enter` 只在主题/负载有焦点时生效 |
| 10 | L26 设置项列表 | 漏了手动检查更新/下载进度 |
| 11 | L29-37 技术栈表 | 漏 `sentry`（**全文未提会把 panic 上报到自建 Sentry**，且该功能无开关）、`ureq`/`sha2`/`sevenz-rust`/`rfd`/`rustls`/`smol` 等 |
| 12 | L43-95 目录树 | 漏 `build.rs`、`resources/`、`examples/flood.rs`、`linear-theme.json`、`scripts/`、`.cargo/config.toml`、`.gitea/` |
| 13 | L178 "release ~17MB" | 实测 20.4 MB |
| 14 | 功能特性 | 完全未提 OTA 自更新与资源监控两个子系统；关于页宣称 Apache-2.0 但仓库无 LICENSE |
| 15 | — | **已核对一致的部分**：数据目录表、BOM 容忍、产物路径、Ctrl+N / Ctrl+, / Ctrl+Shift+E / Ctrl+Shift+I、重连与超时字段、订阅 v5 选项、分组 chips、`.cargo/config.toml` 镜像说明 |

---

## 9. 修复路线图

### 批次 0 —— 解阻发布（约半天，最高 ROI）

1. `sysmon` 测试加 `#[cfg(target_os = "windows")]` 或分平台断言；test job 复用 build job 的系统依赖安装，并核实 `libasound2-dev` 是否多余（`Cargo.lock` 中无 alsa/cpal 相关包）。
2. 发布脚本入口强制 semver 正则校验 + `--exact-match`，不匹配即失败。
3. CI 增加 `tag == Cargo.toml version` 断言。
4. Release 创建/上传失败一律 `exit 1`。
5. **验收**：本地 `cargo test --lib` 在 Linux 容器内通过；打一个测试 tag，CI 全绿并产出 Release 资产；客户端能发现该版本。

### 批次 1 —— 安全与数据完整性（1~2 天）

6. 7z 解压改 `decompress_with_extract_fn`：白名单文件名、拒绝 `..`/盘符/UNC、限制条目数与总量；引入 ed25519 签名验证。
7. Unix 下 `set_permissions(0o755)`。
8. 无侧车不静默跳过；安装前复核哈希；consent 绑定哈希；安装互斥锁。
9. 事件总线改有界 + 显式丢弃策略；日志落盘移出事件热路径（批量/异步）。
10. 跨卷 rename 回退 `copy+fsync+replace`；`.old` 保留至新版本健康启动。
11. **验收**：在 Linux 上完整走通一次自更新；构造一个带 `..` 条目的 7z 验证被拒绝；构造洪水流量观察内存曲线有上界。

### 批次 2 —— 正确性与边界（1~2 天）

12. `Receive Maximum`/`Maximum Packet Size` 拒 0；端口 `1..=65535`；遗嘱主题 trim + 通配符校验。
13. `session_params_changed` 补 4 字段并改成防漏式测试。
14. `platform_triple` 用 `std::env::consts::ARCH`。
15. `test_connection` 补 `spawn_blocking`；重连指数退避 + 抖动。
16. 主题 JSON 键改 `"font.size"` / `"radius.lg"`。
17. **验收**：新增单测全绿；CI 加 `cargo clippy -D warnings` 与 `cargo test --lib --tests`（`#[ignore]` 只跳执行、仍会编译）。

### 批次 3 —— 体验与工程化（2~3 天）

18. 消息/日志列表虚拟化 + 帧级缓存。
19. 对话框栈守卫下沉到各 `dialogs::*::open`；`close_dialog` 语义明确化。
20. 实现 `OptionDelegate::position`；KvEditor 空/重复 key 显式提示。
21. 阿里云对话框语义对齐（预填即选中、按预设复用连接）。
22. 补 LICENSE 与 Cargo 元数据；README 按第 8 节逐条订正。

---

## 10. 附录

### 附录 A：方法与验证记录

| 动作 | 结果 |
|---|---|
| `cargo test -p mqttx-desktop --lib` | **33 passed / 0 failed**（0.19s，Windows） |
| `cargo clippy -p mqttx-desktop --all-targets` | **零告警** |
| `git status` | 干净（仅 `.zcodeignore` 未跟踪）；本报告为唯一新增文件 |
| `git describe --tags --always` | `v1.0.0-4-gbc9f55c`（仓库唯一 tag 为 v1.0.0） |
| 依赖核对 | 直接读取 `Cargo.lock`（905 包 / 89 个多版本）与 cargo registry 源码（`sevenz-rust-0.6.1`、`gpui-component-0.6.6`、`embed-resource`、`ureq-2.12.1`） |
| 未完成的核验 | 阿里云官方文档（本环境 `help.aliyun.com` 解析到非公网 IP，`web_fetch` 被拒；`web_search` 超时） |

### 附录 B：可信度台账

| 结论 | 状态 |
|---|---|
| Linux 单测必失败、`git describe` 产出垃圾版本、非 Windows 缺 +x、主题键名不生效、阿里云对话框空保存复制预设、7z 路径穿越 | ✅ 实测 / 代码读取 |
| ureq 默认 `https_only=false` 且跟随重定向；`manifest_required()` 的失败语义 | ⚠️ 依赖源码读取，未做实验 |
| 阿里云 content 拼装"错误" | ❌ **未采信**（见 6.4） |
| rc.exe 缺失导致构建失败、Windows 跨卷 rename 失败 | 🟡 语义明确但未在目标环境复现 |
| MQTT 5 参数语义（Receive Maximum / Maximum Packet Size 为 0 即 Protocol Error） | ✅ 规范明确 + 代码路径已核对 |

### 附录 C：未决 / 需实测问题

1. **阿里云一机一密 signing content**：需官方测试向量离线比对（见 6.4）。
2. **`securemode=2` + 明文 TCP:1883 是否被 broker 拒绝**：需真实设备实测；无论结果如何，"声明与传输必须一致"这一条成立。
3. **stable 工具链能否构建**：本机只用 nightly 验证过；CI 用 stable，需一次干净环境实测。
4. **`libasound2-dev` 是否必要**：`Cargo.lock` 中无 alsa/cpal/libpulse/jack，倾向于多余，但需在干净 runner 上验证。
5. **发布脚本 `git credential fill` 回退路径的实际作用域**：取决于本机 GCM 配置，需按最小权限原则收窄。

### 附录 D：关键文件索引

| 关注点 | 文件 |
|---|---|
| 引擎与生命周期 | `gpui-app/src/mqtt/mod.rs`、`v5.rs`、`v4.rs`、`tls.rs` |
| 数据模型 | `gpui-app/src/model/*.rs` |
| 持久化 | `gpui-app/src/store.rs` |
| OTA | `gpui-app/src/update/{mod,check,install}.rs` |
| 阿里云 | `gpui-app/src/aliyun.rs` |
| UI 外壳与状态 | `gpui-app/src/ui/app/*.rs` |
| 连接工作区 | `gpui-app/src/ui/connection_view/*.rs` |
| 表单与对话框 | `gpui-app/src/ui/connection_form.rs`、`gpui-app/src/ui/dialogs/*.rs` |
| 发布与 CI | `.gitea/workflows/release.yml`、`scripts/publish-gitea.{sh,ps1}` |
| 测试与压测 | `gpui-app/tests/engine_live.rs`、`gpui-app/examples/flood.rs` |

---

*本报告基于 HEAD `bc9f55c` 的只读分析，未对仓库源文件做任何修改。*
