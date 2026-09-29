"""按 logo.rs 的几何与合成顺序光栅化品牌 logo，打包为多尺寸 .ico。"""
import math, struct, zlib

PRIMARY = (0x5e, 0x6a, 0xd2)   # Linear 主紫
FG      = (0xff, 0xff, 0xff)   # primary_foreground

NODES = [(0.78, 0.27), (0.78, 0.73), (0.23, 0.50)]
CENTER = (0.50, 0.50)

def blend(dst, src, a):
    return tuple(round(sd + (sc - sd) * a) for sd, sc in zip(dst, src))

def render(size):
    S = float(size)
    ss = 4  # 4x4 超采样抗锯齿
    px = [[PRIMARY for _ in range(size)] for _ in range(size)]

    def sample(nx, ny):
        """单采样点（归一化坐标）处的颜色，按 logo.rs 合成顺序。"""
        col = PRIMARY
        # 底：圆角方块 radius 0.26
        r = 0.26
        if not (nx >= 0 and nx <= 1 and ny >= 0 and ny <= 1):
            return None
        qx = max(abs(nx - 0.5) - (0.5 - r), 0)
        qy = max(abs(ny - 0.5) - (0.5 - r), 0)
        if math.hypot(qx, qy) > r:
            return None
        def dot_dist(px_, py_, cx, cy):
            return math.hypot(nx - cx, ny - cy)  # placeholder
        # 连线 + 端点（白 α0.9）
        line_w = 0.04
        for (tx, ty) in NODES:
            # 点到线段距离
            ax, ay = CENTER; bx, by = tx, ty
            vx, vy = bx - ax, by - ay
            t = ((nx - ax) * vx + (ny - ay) * vy) / (vx * vx + vy * vy)
            t = max(0.0, min(1.0, t))
            d = math.hypot(nx - (ax + t * vx), ny - (ay + t * vy))
            if d <= line_w / 2:
                col = blend(col, FG, 0.9)
            if math.hypot(nx - tx, ny - ty) <= 0.05:
                col = blend(col, FG, 0.9)
        for (cx, cy) in [CENTER]:
            if math.hypot(nx - cx, ny - cy) <= 0.17:
                col = blend(col, FG, 1.0)
        # 环：外径 0.15，厚 0.06（填充紫、边白）
        for (tx, ty) in NODES:
            d = math.hypot(nx - tx, ny - ty)
            if d <= 0.15 - 0.03:
                col = PRIMARY
            elif d <= 0.15 + 0.03:
                col = blend(col, FG, 1.0)
        return col

    for y in range(size):
        for x in range(size):
            acc = [0, 0, 0, 0]  # rgb + coverage
            for sy in range(ss):
                for sx in range(ss):
                    nx = (x + (sx + 0.5) / ss) / S
                    ny = (y + (sy + 0.5) / ss) / S
                    c = sample(nx, ny)
                    if c is not None:
                        acc[0] += c[0]; acc[1] += c[1]; acc[2] += c[2]; acc[3] += 1
            n = acc[3]
            if n == 0:
                px[y][x] = (0, 0, 0, 0)
            else:
                px[y][x] = (acc[0] // n, acc[1] // n, acc[2] // n, 255)
    return px

def png_bytes(px):
    h = len(px); w = len(px[0])
    raw = b"".join(b"\x00" + b"".join(bytes(p) for p in row) for row in px)
    def chunk(tag, data):
        c = struct.pack(">I", len(data)) + tag + data
        return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
    return (b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b""))

sizes = [256, 64, 48, 32, 24, 16]
frames = {s: png_bytes(render(s)) for s in sizes}

out = struct.pack("<HHH", 0, 1, len(sizes))
offset = 6 + 16 * len(sizes)
body = b""
for s in sizes:
    data = frames[s]
    out += struct.pack("<BBBBHHII",
        s if s < 256 else 0, s if s < 256 else 0, 0, 0, 1, 32, len(data), offset)
    body += data
    offset += len(data)
open("gpui-app/resources/icon.ico", "wb").write(out + body)
print("icon.ico written:", [f"{s}:{len(frames[s])}B" for s in sizes])
