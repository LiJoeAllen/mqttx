# MQTTX — MQTT v5 桌面调试工具

基于 **Tauri v2** + **Vue 3** + **TypeScript** 构建的跨平台 MQTT v5 调试客户端，支持完整的 MQTT v5 特性（用户属性、内容类型、消息过期、响应主题、关联数据等），提供直观的 GUI 操作体验。

## 功能特性

- **MQTT v5 完整支持** — 连接、发布、订阅、取消订阅，全面支持 MQTT v5 属性
- **多连接管理** — 同时管理多个 MQTT 连接，连接状态实时可见
- **消息流监控** — 实时查看收发消息，支持消息过滤和搜索
- **主题树** — 以树形结构组织订阅的主题，方便浏览
- **预设模板** — 保存常用的连接/发布/订阅配置为预设，快速复用
- **全局变量** — 支持在消息负载中使用 `{{变量名}}` 模板语法
- **用户属性编辑器** — 可视化编辑 MQTT v5 用户属性键值对
- **日志系统** — 实时日志面板 + 文件日志持久化，方便排查问题
- **暗色模式** — 支持亮色/暗色主题切换，自动跟随系统
- **自动更新** — 内置 Tauri 更新器，支持应用自动升级

## 技术栈

| 层 | 技术 |
|---|---|
| 桌面框架 | [Tauri v2](https://v2.tauri.app/) |
| 前端框架 | [Vue 3](https://vuejs.org/) (Composition API + `<script setup>`) |
| 语言 | TypeScript / Rust |
| UI 组件库 | [Element Plus](https://element-plus.org/) |
| MQTT 客户端 | [rumqttc v5-next](https://crates.io/crates/rumqttc-v5-next) |
| 构建工具 | [Vite](https://vitejs.dev/) |
| 包管理 | pnpm |

## 快速开始

### 前置要求

- [Node.js](https://nodejs.org/) >= 18
- [pnpm](https://pnpm.io/)
- [Rust](https://www.rust-lang.org/) (安装参考 [Tauri 环境准备](https://v2.tauri.app/start/prerequisites/))
- Windows: [Microsoft Visual Studio C++ 生成工具](https://visualstudio.microsoft.com/visual-cpp-build-tools/)

### 安装依赖

```bash
pnpm install
```

### 开发模式运行

```bash
pnpm tauri dev
```

### 生产构建

```bash
pnpm tauri build
```

构建产物将输出到 `src-tauri/target/release/bundle/` 目录。

## 自动更新配置

MQTTX 内置了 Tauri 更新器插件。要启用自动更新，需要完成以下步骤：

### 1. 生成签名密钥

```bash
pnpm tauri signer generate -w ~/.tauri/mqttx.key
```

### 2. 配置公钥

将生成的公钥粘贴到 `src-tauri/tauri.conf.json` 中的 `plugins.updater.pubkey` 字段。

### 3. 部署更新服务器

更新端点默认配置为 `https://releases.mqttx.app/update/{{target}}-{{arch}}/{{current_version}}`，需要替换为你自己的更新服务器地址。更新服务器需返回如下格式的 JSON：

```json
{
  "version": "1.0.0",
  "notes": "更新说明",
  "pub_date": "2024-01-01T00:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "...",
      "url": "https://releases.example.com/mqttx_1.0.0_x64.msi.zip"
    },
    "darwin-x86_64": {
      "signature": "...",
      "url": "https://releases.example.com/mqttx_1.0.0_x64.dmg"
    },
    "darwin-aarch64": {
      "signature": "...",
      "url": "https://releases.example.com/mqttx_1.0.0_aarch64.dmg"
    },
    "linux-x86_64": {
      "signature": "...",
      "url": "https://releases.example.com/mqttx_1.0.0_amd64.AppImage"
    }
  }
}
```

参考文档：[Tauri 更新器插件](https://v2.tauri.org.cn/plugin/updater/)

## 项目结构

```
mqttx/
├── src/                          # 前端源码
│   ├── components/               # Vue 组件
│   │   ├── common/               # 通用组件 (ContextMenu, HoverPreview)
│   │   ├── connections/          # 连接管理相关组件
│   │   ├── dashboard/            # 看板相关组件 (消息流、发送面板、日志等)
│   │   ├── presets/              # 预设管理相关组件
│   │   ├── AppLayout.vue         # 主布局
│   │   └── App.vue               # 根组件
│   ├── composables/              # 组合式函数
│   │   ├── useContextMenu.ts     # 右键菜单
│   │   └── useUpdater.ts         # 自动更新器
│   ├── stores/                   # Pinia 状态管理
│   │   ├── useConnections.ts     # 连接状态
│   │   ├── useMqttBridge.ts      # MQTT 桥接 (Tauri 命令调用)
│   │   ├── useMessages.ts        # 消息存储
│   │   ├── useLogs.ts            # 日志管理
│   │   └── ...
│   ├── types/                    # TypeScript 类型定义
│   ├── utils/                    # 工具函数
│   ├── styles/                   # 全局样式
│   └── main.ts                   # 入口文件
├── src-tauri/                    # Rust 后端源码
│   ├── src/
│   │   ├── lib.rs                # Tauri 应用入口、命令定义
│   │   └── main.rs               # 桌面入口点
│   ├── Cargo.toml                # Rust 依赖
│   └── tauri.conf.json           # Tauri 配置
├── package.json
├── vite.config.ts
└── README.md
```

## 命令参考

| 命令 | 说明 |
|---|---|
| `pnpm dev` | 启动 Vite 前端开发服务器 |
| `pnpm build` | 构建前端 |
| `pnpm tauri dev` | 启动 Tauri 开发模式（前端 + 桌面窗口） |
| `pnpm tauri build` | 生产构建 |
| `pnpm tauri signer generate` | 生成更新签名密钥对 |

## 协议

[MIT](./LICENSE)