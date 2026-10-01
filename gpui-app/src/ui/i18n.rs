//! 极简 i18n：以中文原文为 key 的双语文案表。
//!
//! 设计取舍：
//! - 中文是基准语言，`t("订阅")` 直接返回原文，代码里不引入间接 key，
//!   漏翻时回落中文而不是出现裸 key；
//! - 英文表集中在本文件一个 `match`，新增文案先写中文、补翻英文即可；
//! - 带参数的文案用 [`tf`]，占位符 `{name}` 在中英模式中同名替换；
//! - 语言是进程级全局（UI 渲染热路径每帧取用，用原子量免锁）。

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

/// 0 = 中文，1 = English。默认中文（基准语言）。
static LANG: AtomicU8 = AtomicU8::new(0);

pub fn set_lang(lang: Lang) {
    LANG.store(lang as u8, Ordering::Relaxed);
}

pub fn lang() -> Lang {
    if LANG.load(Ordering::Relaxed) == 1 {
        Lang::En
    } else {
        Lang::Zh
    }
}

/// 按系统语言解析初始语言（AppSettings::System 时使用）。
pub fn from_system() -> Lang {
    match sys_locale::get_locale().unwrap_or_default().to_lowercase() {
        l if l.starts_with("zh") => Lang::Zh,
        _ => Lang::En,
    }
}

/// 取当前语言的界面文案。中文原文即 key，英文缺失时回退中文。
pub fn t(zh: &'static str) -> &'static str {
    if lang() == Lang::Zh {
        return zh;
    }
    en(zh)
}

/// 带参数的界面文案：中英模式均为 `{name}` 占位符，按同名替换。
pub fn tf(zh: &'static str, args: &[(&str, &str)]) -> String {
    let pattern = if lang() == Lang::Zh {
        zh.to_string()
    } else {
        en(zh).to_string()
    };
    let mut out = pattern;
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// 英文文案表。key = 中文原文（与调用处字面量一一对应）。
fn en(zh: &'static str) -> &'static str {
    match zh {
        // ── 通用 ──
        "保存" => "Save",
        "取消" => "Cancel",
        "删除" => "Delete",
        "确定" => "OK",
        "全部" => "All",
        "清空" => "Clear",
        "发送" => "Send",
        "连接" => "Connect",
        "断开" => "Disconnect",
        "订阅" => "Subscriptions",
        "编辑" => "Edit",
        "高级" => "Advanced",
        "错误" => "Error",
        "未连接" => "Disconnected",
        "连接中" => "Connecting",
        "连接中…" => "Connecting…",
        "已连接" => "Connected",
        "暂停" => "Pause",
        "恢复" => "Resume",
        "恢复实时滚动" => "Resume live scrolling",
        "暂停消息流滚动（消息仍在后台接收）" => {
            "Pause message stream (messages keep arriving in background)"
        }
        "MQTT 调试客户端" => "MQTT debug client",
        "新建连接" => "New Connection",

        // ── 标题栏 ──
        "置顶窗口" => "Pin window on top",
        "取消窗口置顶" => "Unpin window",
        "置顶操作失败" => "Failed to set always-on-top",
        "切换为浅色" => "Switch to light theme",
        "切换为深色" => "Switch to dark theme",
        "阿里云设备" => "Alibaba Cloud Devices",
        "全局变量" => "Global Variables",
        "资源监控" => "Resource Monitor",
        "导出连接" => "Export Connections",
        "导入连接" => "Import Connections",
        "设置" => "Settings",

        // ── 侧边栏 ──
        "搜索连接…" => "Search connections…",
        "还没有连接，点击上方 + 新建" => "No connections yet. Click + above to create one",
        "没有匹配的连接" => "No matching connections",
        "收起侧栏（Ctrl+B）" => "Collapse sidebar (Ctrl+B)",
        "展开连接侧栏（Ctrl+B）" => "Expand sidebar (Ctrl+B)",
        "复制连接" => "Duplicate",
        "删除连接" => "Delete Connection",
        "未分组" => "Ungrouped",

        // ── 主区 / 标签页 ──
        "开始使用 MQTTX" => "Get started with MQTTX",
        "从左侧选择一个连接，或新建连接开始调试" => {
            "Pick a connection on the left, or create a new one to start debugging"
        }
        "消息流" => "Messages",
        "日志" => "Logs",
        "关闭标签页（Ctrl+W）" => "Close tab (Ctrl+W)",

        // ── 连接工作区 ──
        "收起侧栏" => "Collapse sidebar",
        "收起订阅面板" => "Collapse subscriptions panel",
        "展开订阅面板" => "Expand subscriptions panel",
        "订阅主题后，消息会显示在右侧" => "Subscribe to a topic and messages will show up here",
        "订阅颜色" => "Subscription color",
        "随机颜色" => "Random color",
        "清除颜色" => "Clear color",
        "停用订阅" => "Disable subscription",
        "启用订阅" => "Enable subscription",
        "编辑订阅" => "Edit subscription",
        "取消订阅" => "Unsubscribe",
        "订阅主题，支持 # +，多个用逗号/空格分隔" => {
            "Topic filter, supports # +; separate multiple with commas/spaces"
        }
        "别名（可选）" => "Alias (optional)",
        "订阅标识符" => "Subscription Identifier",
        "No Local" => "No Local",
        "Retain As Published" => "Retain As Published",
        "Retain Handling" => "Retain Handling",
        "0 每次发送" => "0 Send at every subscribe",
        "1 仅新订阅" => "1 Send only for new subscription",
        "2 不发送" => "2 Do not send",
        "正在编辑：{topic}" => "Editing: {topic}",
        "订阅主题不能为空" => "Topic filter cannot be empty",
        "订阅标识符须为正整数" => "Subscription identifier must be a positive integer",
        "未连接：订阅已保存，连接后自动恢复" => {
            "Offline: subscription saved, restored on connect"
        }
        "未连接：订阅已保存" => "Offline: subscription saved",
        "QoS 0" => "QoS 0",
        "QoS 1" => "QoS 1",
        "QoS 2" => "QoS 2",
        "更新" => "Update",

        // ── 消息流 ──
        "过滤主题或内容…" => "Filter by topic or content…",
        "{n} 条" => "{n} msgs",
        "仅显示最新 {n} 条" => "Showing latest {n} only",
        "清空当前连接的消息" => "Clear messages of this connection",
        "清空消息" => "Clear Messages",
        "确定清空当前连接的全部消息吗？此操作不可撤销。" => {
            "Clear all messages of this connection? This cannot be undone."
        }
        "接收" => "Received",
        "发布" => "Published",
        "没有匹配的消息" => "No matching messages",
        "暂无消息，订阅主题后消息会显示在这里" => {
            "No messages yet — they will appear here once you subscribe"
        }
        "retain" => "retain",
        "（非文本，详情可切 Hex）" => " (binary — switch to Hex in details)",
        "用户属性" => "User Properties",
        "Content-Type" => "Content-Type",
        "Response Topic" => "Response Topic",
        "Correlation Data" => "Correlation Data",
        "消息过期" => "Message Expiry",
        "{n} 秒" => "{n}s",
        "复制主题" => "Copy topic",
        "复制负载原文" => "Copy raw payload",
        "复制完整详情（含 v5 属性）" => "Copy full details (incl. MQTT 5 props)",
        "已复制主题" => "Topic copied",
        "已复制负载" => "Payload copied",
        "已复制详情" => "Details copied",
        "主题" => "Topic",
        "界面主题" => "Theme",
        "负载" => "Payload",
        "详情" => "Details",
        "自动" => "Auto",
        "文本" => "Text",
        "连接日志" => "Connection Logs",
        "暂无日志" => "No logs yet",

        // ── 发布面板 ──
        "发布主题" => "Publish topic",
        "输入消息负载，支持 {{变量名}} 与 {{$ts}} {{$uuid}}" => {
            "Message payload; supports {{variables}} and {{$ts}} {{$uuid}}"
        }
        "消息预设" => "Message Presets",
        "保存当前为预设…" => "Save current as preset…",
        "管理预设…" => "Manage presets…",
        "删除预设" => "Delete Preset",
        "确定删除预设「{name}」吗？此操作不可撤销。" => {
            "Delete preset \"{name}\"? This cannot be undone."
        }
        "预设已保存" => "Preset saved",
        "已覆盖预设「{name}」" => "Preset \"{name}\" overwritten",
        "请填写预设名称" => "Preset name is required",
        "预设名称，如 温度上报" => "Preset name, e.g. temperature-report",
        "保存消息预设" => "Save Message Preset",
        "预设名称" => "Preset name",
        "变量注入" => "Variable injection",
        "变量注入（{n} 个占位符）" => "Variable injection ({n} placeholders)",
        "MQTT 5 属性" => "MQTT 5 Properties",
        "MQTT 5 订阅选项" => "MQTT 5 Subscription Options",
        "未连接：消息未发送" => "Offline: message not sent",
        "发布主题不能为空" => "Publish topic cannot be empty",
        "消息过期须为非负整数（秒）" => "Message expiry must be a non-negative integer (seconds)",
        "JSON 格式无效: {e}" => "Invalid JSON: {e}",
        "订阅失败: {e}" => "Subscribe failed: {e}",
        "Keep Alive" => "Keep Alive",
        "接收上限" => "Receive Maximum",
        "主题别名" => "Topic Alias",
        "需要是 0~65535 的整数" => "must be an integer 0-65535",
        "必须大于 0（0 是 MQTT 5 协议错误）" => "must be > 0 (0 is an MQTT 5 protocol error)",
        "需要是非负整数" => "must be a non-negative integer",
        "已启用" => "Enabled",
        "添加属性" => "Add property",
        "删除此属性" => "Remove this property",
        "收起" => "Collapse",
        "变量值" => "Value",
        "主题/负载中没有 {{变量}} 引用" => "No {{variable}} reference in topic/payload",
        "重新提取占位符" => "Re-extract placeholders",
        "内置：{{$ts}}（秒） {{$ts_ms}}（毫秒） {{$uuid}} · 发布时注入" => "Built-in: {{$ts}} (s), {{$ts_ms}} (ms), {{$uuid}} · injected on publish",
        "预览：{tp} | {pl}" => "Preview: {tp} | {pl}",
        "变量（{n}）" => "Variables ({n})",
        "清除订阅过滤" => "Clear subscription filter",
        "取消编辑" => "Cancel editing",
        "鉴权方式" => "Auth mode",
        "地域" => "Region",
        "实例 ID" => "Instance ID",
        "已保存预设" => "Saved presets",
        "保存预设" => "Save preset",
        "生成并连接" => "Generate & connect",
        "无法生成连接: {e}" => "Failed to build connection: {e}",
        "分组（可选）" => "Group (optional)",
        "如 cn-shanghai（可留空）" => "e.g. cn-shanghai (optional)",
        "设备 ID" => "Device ID",
        "一机一密：ProductKey" => "Triple: ProductKey",
        "一机一密：DeviceSecret" => "Triple: DeviceSecret",
        "阿里云 IoT 设备" => "Alibaba Cloud IoT",
        "按可用核数归一" => "Normalized by core count",
        "峰值 {v}" => "Peak {v}",
        "进程独占的物理页承诺" => "Physical pages committed by the process",
        "tokio 引擎 + GPUI 渲染" => "tokio engine + GPUI renderer",
        "暂无连接" => "No connections",
        "消息与连接" => "Messages & Connections",
        "{n} 条消息" => "{n} msgs",
        "运行 {t}" => "Up {t}",
        "立即刷新" => "Refresh now",
        "发布主题，支持 {{变量名}}" => "Publish topic; supports {{variables}}",
        "消息负载，支持 {{变量名}}" => "Message payload; supports {{variables}}",
        "Payload（原文保存，发送时渲染变量）" => "Payload (stored raw; variables rendered on send)",
        "Payload 格式" => "Payload format",
        "暂无发布预设，点击上方「新建」创建" => "No presets yet — click \"New\" above to create one",
        "请先选择或新建预设" => "Select or create a preset first",
        "请先选择要删除的预设" => "Select a preset to delete first",
        "预设「{name}」已保存" => "Preset \"{name}\" saved",
        "预设名称「{name}」已存在" => "Preset name \"{name}\" already exists",
        "新预设 {n}" => "New preset {n}",
        "新预设" => "New preset",
        "管理发布预设" => "Manage Publish Presets",
        "在发布主题、消息负载中使用 {{变量名}} 引用。" => "Reference {{variables}} in publish topics and payloads.",
        "内置变量：{{$ts}}（秒）、{{$ts_ms}}（毫秒）、{{$uuid}}" => "Built-in: {{$ts}} (s), {{$ts_ms}} (ms), {{$uuid}}",
        "测试任务丢失" => "Test task lost",
        "Hex 长度必须为偶数（每两个字符一个字节）" => "Hex length must be even (two chars per byte)",
        "Hex 解码失败: 含非十六进制字符" => "Hex decode failed: non-hex character present",
        "Hex 解码失败: {e}" => "Hex decode failed: {e}",
        "Base64 解码失败: {e}" => "Base64 decode failed: {e}",
        "已发送（被当前过滤条件隐藏，消息流中不显示）" => {
            "Sent (hidden by current filter — not shown in the stream)"
        }
        "收起发布面板" => "Collapse publish panel",
        "展开发布面板" => "Expand publish panel",
        "发布面板已收起" => "Publish panel collapsed",
        "Content-Type（可选）" => "Content-Type (optional)",
        "秒，如 60" => "seconds, e.g. 60",
        "Response Topic（可选）" => "Response Topic (optional)",
        "Correlation Data（可选）" => "Correlation Data (optional)",
        "消息过期(秒)" => "Expiry (s)",

        // ── 变量面板 ──
        "检测到模板变量，发布时注入全局变量值" => {
            "Template variables detected — global values injected on publish"
        }

        // ── 连接表单 ──
        "连接配置" => "Connection Settings",
        "基本信息" => "Basics",
        "主机" => "Host",
        "端口" => "Port",
        "名称" => "Name",
        "协议" => "Protocol",
        "传输" => "Transport",
        "分组" => "Group",
        "新建分组" => "New group",
        "填写后优先于左侧所选分组" => "Overrides the selected group when filled",
        "Client ID" => "Client ID",
        "重新生成 Client ID" => "Regenerate Client ID",
        "WS 路径" => "WS Path",
        "broker.example.com" => "broker.example.com",
        "连接名称" => "Connection name",
        "无分组" => "No group",
        "认证与心跳" => "Auth & Keep-alive",
        "用户名" => "Username",
        "密码" => "Password",
        "用户名（可选）" => "Username (optional)",
        "密码（可选）" => "Password (optional)",
        "Keep Alive (秒)" => "Keep Alive (s)",
        "Keep Alive (秒) 需要是 0~65535 的整数" => "Keep Alive must be an integer 0-65535",
        "连接超时 (秒)" => "Connect timeout (s)",
        "最大重连次数 (0=无限)" => "Max reconnects (0 = unlimited)",
        "Clean Start / Clean Session" => "Clean Start / Clean Session",
        "断线自动重连" => "Auto reconnect on disconnect",
        "连接后自动恢复订阅" => "Auto-resubscribe on connect",
        "启动时自动连接" => "Connect on app start",
        "SSL/TLS" => "SSL/TLS",
        "CA 证书 (PEM)" => "CA certificate (PEM)",
        "客户端证书 (PEM)" => "Client certificate (PEM)",
        "客户端密钥 (PEM)" => "Client key (PEM)",
        "浏览…" => "Browse…",
        "选择证书 / 密钥文件" => "Select certificate / key file",
        "忽略 CA 校验（不验证服务器证书）" => "Skip CA verification (trust any server cert)",
        "已配置 {n} 项" => "{n} configured",
        "MQTT 5 属性已配置" => "MQTT 5 properties configured",
        "遗嘱消息 (Last Will)" => "Last Will",
        "启用遗嘱消息" => "Enable last will",
        "遗嘱主题" => "Will topic",
        "遗嘱内容" => "Will payload",
        "遗嘱主题不能为空" => "Will topic cannot be empty",
        "遗嘱主题不能包含通配符 + 或 #" => "Will topic cannot contain wildcards + or #",
        "遗嘱消息已启用" => "Last will enabled",
        "Retain" => "Retain",
        "内容类型 (Content Type)" => "Content Type",
        "响应主题 (Response Topic)" => "Response Topic",
        "连接名称不能为空" => "Connection name cannot be empty",
        "主机地址不能为空" => "Host cannot be empty",
        "Client ID 不能为空" => "Client ID cannot be empty",
        "会话过期间隔(秒)" => "Session expiry (s)",
        "接收最大值" => "Receive Maximum",
        "最大报文长度" => "Maximum Packet Size",
        "主题别名上限" => "Topic Alias Maximum",
        "测试连接" => "Test Connection",
        "测试中…" => "Testing…",
        "连接测试成功" => "Connection test succeeded",
        "配置已保存，重连后生效" => "Saved. Takes effect after reconnect",

        // ── 设置 ──
        "跟随系统" => "Follow system",
        "浅色" => "Light",
        "深色" => "Dark",
        "界面语言" => "Language",
        "每条连接内存中保留的消息条数（100~100000）" => {
            "Messages kept in memory per connection (100-100000)"
        }
        "另有每连接 {n}MB 的字节预算兜底：大报文流会优先按字节从最旧开始驱逐" => {
            "Plus a per-connection byte budget of {n} MB: under heavy traffic, oldest messages are evicted by size first"
        }
        "时间戳显示毫秒" => "Show milliseconds in timestamps",
        "自动检查更新（启动时）" => "Check for updates on start",
        "数据目录" => "Data directory",
        "日志目录" => "Log directory",
        "打开目录" => "Open folder",
        "打开日志目录" => "Open log folder",
        "打开目录失败: {e}" => "Failed to open folder: {e}",
        "创建日志目录失败: {e}" => "Failed to create log folder: {e}",
        "关于" => "About",
        "版本 {v}（更新源 {src}）" => "Version {v} (update source: {src})",
        "Apache-2.0 许可证" => "Apache-2.0 license",
        "对标 MQTTX 的 GPUI 跨平台 MQTT 调试客户端" => {
            "A cross-platform MQTT debug client built with GPUI, aiming at MQTTX"
        }
        "检查更新" => "Check for updates",
        "检查 GitHub Releases 上的新版本" => "Check GitHub Releases for a new version",
        "检查中…" => "Checking…",
        "正在检查更新…" => "Checking for updates…",
        "重新检查" => "Check again",
        "已是最新版本" => "You are up to date",
        "立即更新到 v{v}" => "Update to v{v} now",
        "发现新版本 v{v}，下载后自动替换重启" => {
            "New version v{v} found — downloads, replaces itself and restarts"
        }
        "新版本 v{v} 已下载并通过校验，等待安装" => {
            "Version v{v} downloaded and verified, ready to install"
        }
        "立即安装" => "Install now",
        "下次启动安装" => "Install on next start",
        "已暂存，下次启动时将自动安装" => "Staged — it will install on next start",
        "放弃此次更新" => "Discard this update",
        "已删除暂存的更新包" => "Staged update discarded",
        "下载中 {pct}%（{done} / {total}）" => "Downloading {pct}% ({done} / {total})",
        "下载中 {done}" => "Downloading {done}",
        "重试" => "Retry",
        "更新失败：{e}" => "Update failed: {e}",
        "更新就绪" => "Update Ready",
        "新版本 v{v} 已下载完成（已通过 sha256 校验）。" => {
            "Version v{v} has been downloaded (sha256 verified)."
        }
        "暂不更新" => "Not now",
        "安装失败：{e}" => "Install failed: {e}",

        // ── 导入 / 导出 / 通知 ──
        "导出 {n} 条连接到 {path}" => "Exported {n} connections to {path}",
        "导入 {n} 条，跳过 {n2} 条" => "Imported {n}, skipped {n2}",
        "已保存" => "Saved",

        // ── 预设管理 ──
        "发布预设" => "Publish Presets",
        "新建预设" => "New Preset",
        "预设列表" => "Presets",

        // ── 资源监控 / 其他对话框 ──
        "自身进程资源采样" => "Self-process resource sampling",

        _ => zh,
    }
}
