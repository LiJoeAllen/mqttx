# -*- coding: utf-8 -*-
"""从 gpui-component 默认主题生成 Linear 风格主题 linear-theme.json。
映射规则见各表；未列出的键（chart/base 等）保留默认值。"""
import json, sys, collections

SRC = sys.argv[1]
DST = sys.argv[2]

with open(SRC, "r", encoding="utf-8") as f:
    base = json.load(f)

# ── Linear Dark ────────────────────────────────────────────────────────────
DARK = {
    "background": "#08090a",
    "foreground": "#f7f8f8",
    "accordion.background": "#101012",
    "border": "#2b2c31",
    "group_box.background": "#08090a",
    "group_box.foreground": "#f7f8f8",
    "caret": "#f7f8f8",
    "accent.background": "#1c1d1f",
    "accent.foreground": "#f7f8f8",
    "danger.background": "#e5484d",
    "danger.foreground": "#ffe9e9",
    "description_list_label.background": "#101012",
    "description_list_label.foreground": "#8a8f98",
    "drag_border": "#5e6ad2",
    "drop_target.background": "#5e6ad219",
    "input.border": "#2b2c31",
    "link.foreground": "#f7f8f8",
    "link.active.foreground": "#d5d7de",
    "link.hover.foreground": "#ffffff",
    "list.background": "#08090a",
    "list.active.background": "#5e6ad233",
    "list.active.border": "#5e6ad2",
    "list.even.background": "#10101266",
    "list.head.background": "#10101266",
    "muted.background": "#1d1e22",
    "muted.foreground": "#8a8f98",
    "popover.background": "#101012",
    "popover.foreground": "#f7f8f8",
    "primary.background": "#5e6ad2",
    "primary.hover.background": "#6e79d6",
    "primary.active.background": "#5661c4",
    "primary.foreground": "#ffffff",
    "progress_bar.background": "#5e6ad2",
    "ring": "#5e6ad2",
    "scrollbar.background": "#08090a00",
    "scrollbar.thumb.background": "#3a3b42e6",
    "scrollbar.thumb.hover.background": "#4a4b54",
    "secondary.background": "#101012",
    "secondary.hover.background": "#1c1d1f",
    "secondary.active.background": "#232429",
    "secondary.foreground": "#f7f8f8",
    "selection.background": "#5e6ad2",
    "sidebar.background": "#08090a",
    "sidebar.accent.background": "#1c1d1f",
    "sidebar.accent.foreground": "#f7f8f8",
    "sidebar.border": "#1f2024",
    "sidebar.foreground": "#f7f8f8",
    "sidebar.primary.background": "#f7f8f8",
    "sidebar.primary.foreground": "#08090a",
    "skeleton.background": "#1c1d1f",
    "slider.bar.background": "#f7f8f8",
    "slider.thumb.background": "#08090a",
    "success.background": "#4cb782",
    "success.foreground": "#0b3a26",
    "switch.background": "#2b2c31",
    "tab.background": "#00000000",
    "tab.active.background": "#101012",
    "tab.active.foreground": "#f7f8f8",
    "tab.foreground": "#8a8f98",
    "tab_bar.background": "#08090a",
    "tab_bar.segmented.background": "#101012",
    "table.background": "#08090a",
    "table.head.foreground": "#8a8f98",
    "table.row.border": "#1f2024b3",
    "title_bar.background": "#08090a",
    "title_bar.border": "#1f2024",
    "status_bar.background": "#08090a",
    "status_bar.border": "#1f2024",
    "warning.background": "#f2a93b",
    "warning.foreground": "#3a2504",
    "info.background": "#5e6ad2",
    "info.foreground": "#e0e3ff",
    "overlay": "#00000066",
    "window.border": "#1f2024",
}

# ── Linear Light ───────────────────────────────────────────────────────────
LIGHT = {
    "background": "#ffffff",
    "foreground": "#0d0e10",
    "accordion.background": "#f7f7f8",
    "border": "#e5e5e7",
    "group_box.background": "#ffffff",
    "group_box.foreground": "#0d0e10",
    "caret": "#0d0e10",
    "accent.background": "#eeeef0",
    "accent.foreground": "#0d0e10",
    "danger.background": "#e5484d",
    "danger.foreground": "#ffffff",
    "description_list_label.background": "#f7f7f8",
    "description_list_label.foreground": "#6f7076",
    "drag_border": "#5e6ad2",
    "drop_target.background": "#5e6ad219",
    "input.border": "#e5e5e7",
    "link.foreground": "#0d0e10",
    "link.active.foreground": "#3d3e44",
    "link.hover.foreground": "#000000",
    "list.background": "#ffffff",
    "list.active.background": "#5e6ad233",
    "list.active.border": "#5e6ad2",
    "list.even.background": "#f1f1f266",
    "list.head.background": "#f1f1f266",
    "muted.background": "#f1f1f2",
    "muted.foreground": "#6f7076",
    "popover.background": "#ffffff",
    "popover.foreground": "#0d0e10",
    "primary.background": "#5e6ad2",
    "primary.hover.background": "#6b76d4",
    "primary.active.background": "#5661c4",
    "primary.foreground": "#ffffff",
    "progress_bar.background": "#5e6ad2",
    "ring": "#5e6ad2",
    "scrollbar.background": "#ffffff00",
    "scrollbar.thumb.background": "#c9c9cee6",
    "scrollbar.thumb.hover.background": "#b5b5bb",
    "secondary.background": "#f7f7f8",
    "secondary.hover.background": "#eeeef0",
    "secondary.active.background": "#e7e7e9",
    "secondary.foreground": "#0d0e10",
    "selection.background": "#5e6ad2",
    "sidebar.background": "#fbfbfb",
    "sidebar.accent.background": "#eeeef0",
    "sidebar.accent.foreground": "#0d0e10",
    "sidebar.border": "#e5e5e7",
    "sidebar.foreground": "#0d0e10",
    "sidebar.primary.background": "#0d0e10",
    "sidebar.primary.foreground": "#ffffff",
    "skeleton.background": "#eeeef0",
    "slider.bar.background": "#0d0e10",
    "slider.thumb.background": "#ffffff",
    "success.background": "#4cb782",
    "success.foreground": "#ffffff",
    "switch.background": "#d7d7db",
    "tab.background": "#00000000",
    "tab.active.background": "#ffffff",
    "tab.active.foreground": "#0d0e10",
    "tab.foreground": "#6f7076",
    "tab_bar.background": "#ffffff",
    "tab_bar.segmented.background": "#f1f1f2",
    "table.background": "#ffffff",
    "table.head.foreground": "#6f7076",
    "table.row.border": "#ebebed",
    "title_bar.background": "#ffffff",
    "title_bar.border": "#e5e5e7",
    "status_bar.background": "#ffffff",
    "status_bar.border": "#e5e5e7",
    "warning.background": "#f2a93b",
    "warning.foreground": "#3a2504",
    "info.background": "#5e6ad2",
    "info.foreground": "#ffffff",
    "overlay": "#00000033",
    "window.border": "#e5e5e7",
}


def build(theme, overrides, name):
    colors = dict(theme.get("colors") or {})
    missing = [k for k in overrides if k not in colors]
    if missing:
        print(f"WARN {name}: 目标主题缺少键 {missing}", file=sys.stderr)
    colors.update(overrides)
    out = collections.OrderedDict()
    # 键序跟随默认主题，新键追加
    for k in (theme.get("colors") or {}):
        out[k] = colors[k]
    for k in overrides:
        if k not in out:
            out[k] = overrides[k]
    cfg = collections.OrderedDict()
    cfg["name"] = name
    cfg["mode"] = theme["mode"]
    cfg["font_size"] = 14
    cfg["radius"] = 6
    cfg["radius_lg"] = 10
    cfg["colors"] = out
    if theme.get("highlight"):
        cfg["highlight"] = theme["highlight"]
    return cfg


by_mode = {t["mode"]: t for t in base["themes"]}
themes = [
    build(by_mode["light"], LIGHT, "Linear Light"),
    build(by_mode["dark"], DARK, "Linear Dark"),
]
doc = {"themes": themes}
with open(DST, "w", encoding="utf-8") as f:
    json.dump(doc, f, ensure_ascii=False, indent=2)
print(f"OK -> {DST} ({len(themes)} themes)")
