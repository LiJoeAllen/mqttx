//! MQTTX 品牌 Logo：纯 GPUI 原语绘制的 pub/sub 拓扑徽标。
//!
//! - 圆角方块作为品牌底（主色）；
//! - 中心圆点代表 broker，连线连向发布端与订阅端，
//!   表达 MQTT “发布 / 代理 / 订阅” 的星型拓扑；
//! - 末端节点用环形描边，与中心实心 broker 区分。

use gpui_kit::{
    canvas, div, px, App, Bounds, Corners, Edges, Hsla, IntoElement, ParentElement,
    PathBuilder, Pixels, Point, Size, Styled as _, Window,
};

/// 绘制一个指定边长的 Logo（容器被设置为该尺寸）。
pub fn logo(side: Pixels, bg: Hsla, fg: Hsla) -> impl IntoElement {
    let inner = canvas(
        move |_bounds, _window, _cx| (bg, fg, side),
        move |bounds, (bg, fg, side), window, _cx| {
            // 品牌圆角方块
            let radius = side * 0.26;
            window.paint_quad(gpui_kit::quad(
                bounds,
                Corners::all(radius),
                bg,
                Edges::all(px(0.)),
                bg,
                gpui_kit::BorderStyle::default(),
            ));

            let origin = bounds.origin;
            // 归一化比例坐标 → 像素
            let at = move |x: f32, y: f32| Point::new(origin.x + side * x, origin.y + side * y);

            // 中心 broker 与三个客户端节点（右上任一发布、右下与左侧订阅）
            let center = (0.5_f32, 0.5_f32);
            let nodes = [(0.78_f32, 0.27_f32), (0.78, 0.73), (0.23, 0.5)];

            // 连线，两端补圆点形成圆头效果（不依赖 lyon LineCap）
            for &(nx, ny) in &nodes {
                let a = at(center.0, center.1);
                let b = at(nx, ny);
                if let Some(p) = line_path(a, b, side * 0.04) {
                    window.paint_path(p, fg.alpha(0.9));
                }
                paint_dot(window, a, side * 0.05, fg.alpha(0.9));
                paint_dot(window, b, side * 0.05, fg.alpha(0.9));
            }

            // 实心中心 broker（覆盖端点）
            paint_dot(window, at(center.0, center.1), side * 0.17, fg);
            // 环形末端节点
            for &(nx, ny) in &nodes {
                paint_ring(window, at(nx, ny), side * 0.15, side * 0.03, bg, fg);
            }
        },
    )
    .absolute()
    .size_full();

    div()
        .relative()
        .w(side)
        .h(side)
        .child(inner)
}

fn paint_dot(window: &mut Window, center: Point<Pixels>, d: Pixels, color: Hsla) {
    let r = d * 0.5;
    let bounds = Bounds {
        origin: Point::new(center.x - r, center.y - r),
        size: Size {
            width: d,
            height: d,
        },
    };
    window.paint_quad(gpui_kit::quad(
        bounds,
        Corners::all(r),
        color,
        Edges::all(px(0.)),
        color,
        gpui_kit::BorderStyle::default(),
    ));
}

fn paint_ring(
    window: &mut Window,
    center: Point<Pixels>,
    d: Pixels,
    thickness: Pixels,
    fill: Hsla,
    border: Hsla,
) {
    let r = d * 0.5;
    let bounds = Bounds {
        origin: Point::new(center.x - r, center.y - r),
        size: Size {
            width: d,
            height: d,
        },
    };
    window.paint_quad(gpui_kit::quad(
        bounds,
        Corners::all(r),
        fill,
        Edges::all(thickness),
        border,
        gpui_kit::BorderStyle::Solid,
    ));
}

fn line_path(
    a: Point<Pixels>,
    b: Point<Pixels>,
    width: Pixels,
) -> Option<gpui_kit::Path<Pixels>> {
    let mut pb = PathBuilder::stroke(width);
    pb.move_to(a);
    pb.line_to(b);
    pb.build().ok()
}

#[allow(unused_imports)]
use App as _App;
