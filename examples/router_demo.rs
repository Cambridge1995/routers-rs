//! routers 全功能可视化演示(独立 gpui 应用,零 framework/gpui-component 依赖)。
//!
//! 运行:`cargo run -p routers-rs --example router_demo`
//!
//! 一个窗口看全 routers 的公开能力:
//! - **navigate**:左侧导航栏点击切换,active 态自动高亮(matchit 语义)。
//! - **动态路由**:`/user/{id}` 注册一次,`/user/42` `/user/7` 都命中,内容区实时显示 params。
//! - **history back/forward**:顶栏 ◀ ▶ 按钮,置灰跟随 can_go_back/can_go_forward。
//! - **嵌套 Outlet**:RootLayout(/) 常驻包裹;`/settings/*` 再套一层 SettingsLayout。
//! - **路由守卫**:顶栏「守卫锁定」开关,开启后 `/admin` 被 allow_navigate 拦截,
//!   navigate 返回 false,内容区不变;「强制进入」走 navigate_force 绕过。
//! - **持久化/恢复**:每次导航写入系统临时目录 `routers-demo-route.json`,
//!   下次启动自动恢复到上次页面(Router::restore)。
//! - **事件钩子**:每次 RouteChanged 在 stdout 打印(publish_route_changed)。

use std::cell::Cell;
use std::rc::Rc;

use routers::gpui::prelude::*;
use routers::gpui::{
    App, AppContext, Application, Bounds, ClickEvent, Context, FontWeight, SharedString, Window,
    WindowBounds, WindowOptions, div, px, rgb,
};
use routers::{Outlet, Router, RouterEventBus};

// ════════════════════════════════════════════════════════════════════════
// 演示用 EventBus:守卫 + 持久化 + 恢复 + 事件打印
// ════════════════════════════════════════════════════════════════════════

struct DemoBus {
    /// 守卫开关(与 UI 共享,单线程 Rc)。
    guard_on: Rc<Cell<bool>>,
    /// 持久化文件路径(系统临时目录,演示用)。
    store_path: std::path::PathBuf,
}

impl RouterEventBus for DemoBus {
    fn publish_route_changed(&self, _cx: &mut App, path: SharedString) {
        // 事件钩子演示:真实项目会转发到自己的 EventBus / gpui emit。
        println!("[RouteChanged] {path}");
    }

    fn allow_navigate(
        &self,
        _cx: &mut App,
        _from: SharedString,
        to: SharedString,
    ) -> Result<bool, SharedString> {
        if self.guard_on.get() && to.starts_with("/admin") {
            return Err("守卫锁定中:未登录,拒绝进入 /admin".into());
        }
        Ok(true)
    }

    fn persist_pathname(&self, _cx: &mut App, path: &SharedString) {
        // 持久化钩子:写系统临时目录(演示,不污染任何 config/)。
        let _ = std::fs::write(&self.store_path, path.as_str());
    }

    fn restore_pathname(&self, _cx: &App) -> Option<SharedString> {
        std::fs::read_to_string(&self.store_path)
            .ok()
            .map(SharedString::from)
    }
}

// ════════════════════════════════════════════════════════════════════════
// 主视图
// ════════════════════════════════════════════════════════════════════════

/// 导航项:(目标路径, 显示名, active 判定用的模式)。
const NAV_ITEMS: &[(&str, &str, &str)] = &[
    ("/files", "文件", "/files"),
    ("/search", "搜索", "/search"),
    ("/user/42", "用户 42", "/user/{id}"),
    ("/user/7", "用户 7", "/user/{id}"),
    ("/settings/profile", "设置·档案", "/settings/profile"),
    ("/settings/account", "设置·账户", "/settings/account"),
    ("/admin/panel", "后台(受守卫)", "/admin/panel"),
];

struct DemoView {
    guard_on: Rc<Cell<bool>>,
    /// 最近一次导航的结果提示(演示 navigate 返回值)。
    last_message: SharedString,
}

impl DemoView {
    fn new(guard_on: Rc<Cell<bool>>) -> Self {
        Self {
            guard_on,
            last_message: "点击左侧导航开始".into(),
        }
    }

    /// 导航 + 结果反馈(navigate 返回 false = 被守卫拦截)。
    fn go(&mut self, target: &'static str, force: bool, cx: &mut Context<Self>) {
        let ok = if force {
            Router::navigate_force(cx, target)
        } else {
            Router::navigate(cx, target)
        };
        self.last_message = if ok {
            format!("navigate({target}) → true").into()
        } else {
            format!("navigate({target}) → false · 被守卫拦截,内容区未变").into()
        };
        cx.notify();
    }

    /// active 判定:
    /// - `pattern` 与 `target` 相同(静态路由)→ 用 matchit 模式语义 [`Router::is_active`]。
    /// - `pattern` 是动态模式(如 /user/{id})而 `target` 是具体路径 → 模式语义会让
    ///   同模式的多个入口同时命中,故改用精确匹配(类 NavLink exact)。
    fn nav_button(&self, target: &'static str, label: &str, pattern: &str, cx: &Context<Self>) -> impl IntoElement {
        let active = if pattern == target {
            Router::is_active(pattern)
        } else {
            routers::is_active_simple(&Router::current(), target, /*exact=*/ true)
        };
        let (bg, fg) = if active {
            (rgb(0x2f5fd0), rgb(0xffffff))
        } else {
            (rgb(0x23232e), rgb(0xb8b8c8))
        };
        div()
            .id(SharedString::from(format!("nav-{target}")))
            .px_3()
            .py_2()
            .mb_1()
            .rounded_md()
            .bg(bg)
            .text_color(fg)
            .text_sm()
            .cursor_pointer()
            .hover(|s| s.bg(if active { rgb(0x2f5fd0) } else { rgb(0x2c2c3a) }))
            .child(format!("{label}   {target}"))
            .on_click(cx.listener(move |this, _e: &ClickEvent, _w, cx| {
                this.go(target, false, cx);
            }))
    }
}

impl Render for DemoView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current = Router::current();
        let can_back = Router::can_go_back();
        let can_fwd = Router::can_go_forward();
        let history = format!("history: cursor={} len={}", Router::cursor(), Router::history_len());
        let params = Router::params();
        let params_text = if params.is_empty() {
            "params: (无)".to_string()
        } else {
            let mut pairs: Vec<String> = params
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            pairs.sort();
            format!("params: {}", pairs.join(", "))
        };
        let guard_on = self.guard_on.get();

        // ── 顶栏:back/forward + 当前路径 + 守卫开关 ──
        let nav_btn = |id: &'static str, label: &str, enabled: bool| {
            div()
                .id(id)
                .px_3()
                .py_1()
                .mr_2()
                .rounded_md()
                .text_sm()
                .bg(if enabled { rgb(0x2c2c3a) } else { rgb(0x1c1c24) })
                .text_color(if enabled { rgb(0xffffff) } else { rgb(0x555560) })
                .when(enabled, |el| {
                    el.cursor_pointer().hover(|s| s.bg(rgb(0x38384a)))
                })
                .child(label.to_string())
        };
        let top_bar = div()
            .flex()
            .items_center()
            .px_3()
            .py_2()
            .bg(rgb(0x16161d))
            .child(
                nav_btn("btn-back", "◀ back", can_back).on_click(
                    cx.listener(|this, _e: &ClickEvent, _w, cx| {
                        let ok = Router::back(cx);
                        this.last_message = format!("back() → {ok}").into();
                        cx.notify();
                    }),
                ),
            )
            .child(
                nav_btn("btn-fwd", "forward ▶", can_fwd).on_click(
                    cx.listener(|this, _e: &ClickEvent, _w, cx| {
                        let ok = Router::forward(cx);
                        this.last_message = format!("forward() → {ok}").into();
                        cx.notify();
                    }),
                ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0x7fd4a0))
                    .child(format!("当前: {current}")),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("btn-guard")
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .text_sm()
                    .cursor_pointer()
                    .bg(if guard_on { rgb(0x8c3a3a) } else { rgb(0x2c2c3a) })
                    .text_color(rgb(0xffffff))
                    .hover(|s| s.bg(if guard_on { rgb(0xa04848) } else { rgb(0x38384a) }))
                    .child(if guard_on {
                        "守卫锁定: 开 (/admin 将被拦截)"
                    } else {
                        "守卫锁定: 关"
                    })
                    .on_click(cx.listener(|this, _e: &ClickEvent, _w, cx| {
                        let new = !this.guard_on.get();
                        this.guard_on.set(new);
                        this.last_message = format!("守卫锁定 → {new}").into();
                        cx.notify();
                    })),
            );

        // ── 状态行:history / params / 最近导航结果 ──
        let status_bar = div()
            .flex()
            .items_center()
            .gap_4()
            .px_3()
            .py_1()
            .bg(rgb(0x1a1a22))
            .text_xs()
            .text_color(rgb(0x8a8a9a))
            .child(history)
            .child(params_text)
            .child(self.last_message.clone());

        // ── 左侧导航栏 ──
        let mut sidebar = div()
            .flex()
            .flex_col()
            .w(px(240.))
            .p_3()
            .bg(rgb(0x16161d))
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(0xe8e8f0))
                    .mb_2()
                    .child("routers 全功能演示"),
            );
        for (target, label, pattern) in NAV_ITEMS {
            sidebar = sidebar.child(self.nav_button(target, label, pattern, cx));
        }
        // 强制进入演示:navigate_force 绕过守卫。
        sidebar = sidebar.child(
            div()
                .id("btn-force-admin")
                .px_3()
                .py_2()
                .mt_2()
                .rounded_md()
                .text_sm()
                .cursor_pointer()
                .bg(rgb(0x4a3a20))
                .text_color(rgb(0xe8c97f))
                .hover(|s| s.bg(rgb(0x5c4a28)))
                .child("强制进入 /admin (navigate_force)")
                .on_click(cx.listener(|this, _e: &ClickEvent, _w, cx| {
                    this.go("/admin/panel", true, cx);
                })),
        );

        // ── 内容区:Outlet(路由自动管理的内容占位符)──
        let content = div()
            .flex_1()
            .p_3()
            .bg(rgb(0x101016))
            .child(Outlet::new());

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x101016))
            .text_color(rgb(0xe8e8f0))
            .child(top_bar)
            .child(status_bar)
            .child(div().flex().flex_1().child(sidebar).child(content))
    }
}

// ════════════════════════════════════════════════════════════════════════
// 内容面板(element 工厂):每个路由一个内容
// ════════════════════════════════════════════════════════════════════════

/// 统一的内容面板样式:标题 + 描述 + 实时 params。
fn panel(title: &str, desc: &str, accent: u32) -> impl IntoElement {
    let params = Router::params();
    let params_text = if params.is_empty() {
        String::new()
    } else {
        let mut pairs: Vec<String> = params
            .iter()
            .map(|(k, v)| format!("{k} = {v}"))
            .collect();
        pairs.sort();
        format!("动态参数(Router::params()):  {}", pairs.join(",  "))
    };
    div()
        .flex()
        .flex_col()
        .size_full()
        .p_4()
        .rounded_md()
        .bg(rgb(0x181822))
        .child(
            div()
                .text_lg()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(accent))
                .mb_2()
                .child(title.to_string()),
        )
        .child(
            div()
                .text_sm()
                .text_color(rgb(0xa0a0b0))
                .child(desc.to_string()),
        )
        .when(!params_text.is_empty(), |el| {
            el.child(
                div()
                    .mt_3()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(0x20301f))
                    .text_sm()
                    .text_color(rgb(0x7fd4a0))
                    .child(params_text),
            )
        })
}

fn register_routes() {
    // ① 路由模式:静态 + 动态段(matchit 语义,供 params / is_active 使用)。
    Router::register("/files");
    Router::register("/search");
    Router::register("/settings/profile");
    Router::register("/settings/account");
    Router::register("/admin/panel");
    // 注:/user/{id} 无需显式 register——下方 register_element 隐含登记。

    // ② 内容元素:每个路由一个工厂。
    Router::register_element("/files", |_w, _cx| {
        panel("📁 文件面板", "路径 /files · 静态路由", 0x7fb8ff).into_any_element()
    });
    Router::register_element("/search", |_w, _cx| {
        panel("🔍 搜索面板", "路径 /search · 静态路由", 0xc9a0ff).into_any_element()
    });
    // ★ 动态模式叶子(0.2 新能力):一条注册覆盖所有 /user/*,params 自动填充。
    Router::register_element("/user/{id}", |_w, _cx| {
        panel(
            "👤 用户面板",
            &format!("动态模式 /user/{{id}} · 当前 id = {}", Router::params().get("id").map(|v| v.as_ref()).unwrap_or("?")),
            0x7fd4a0,
        )
        .into_any_element()
    });
    Router::register_element("/settings/profile", |_w, _cx| {
        panel("⚙ 档案设置", "路径 /settings/profile · 被 /settings 布局嵌套", 0xe8c97f)
            .into_any_element()
    });
    Router::register_element("/settings/account", |_w, _cx| {
        panel("⚙ 账户设置", "路径 /settings/account · 被 /settings 布局嵌套", 0xe8c97f)
            .into_any_element()
    });
    Router::register_element("/admin/panel", |_w, _cx| {
        panel("🛡 后台面板", "路径 /admin/panel · 守卫锁定开启时普通导航会被拦截,仅 navigate_force 可达", 0xff8f8f)
            .into_any_element()
    });

    // ③ 嵌套布局:/settings 布局(内层)套在 RootLayout(外层)之内。
    Router::register_layout("/", |outlet| {
        div()
            .flex()
            .flex_col()
            .size_full()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(rgb(0x2c2c3a))
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(0x666677))
                    .mb_2()
                    .child("RootLayout(/) · 常驻根布局 · register_layout(\"/\")"),
            )
            .child(div().flex_1().child(outlet))
            .into_any_element()
    });
    Router::register_layout("/settings", |outlet| {
        div()
            .flex()
            .flex_col()
            .size_full()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(rgb(0x5c4a28))
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(0xe8c97f))
                    .mb_2()
                    .child("SettingsLayout(/settings) · 嵌套布局 · 仅 /settings/* 路径出现"),
            )
            .child(div().flex_1().child(outlet))
            .into_any_element()
    });
}

// ════════════════════════════════════════════════════════════════════════
// 入口:init → register → restore → 开窗
// ════════════════════════════════════════════════════════════════════════

fn main() {
    let guard_on = Rc::new(Cell::new(false));
    let store_path = std::env::temp_dir().join("routers-demo-route.json");
    println!("持久化文件: {}", store_path.display());

    let guard_for_bus = Rc::clone(&guard_on);
    Application::new().run(move |cx: &mut App| {
        // ① 初始化 Router(注入演示 bus:守卫 + 持久化 + 恢复 + 打印)。
        Router::init_with(DemoBus {
            guard_on: guard_for_bus,
            store_path,
        });
        // ② 注册路由模式 + 内容元素 + 嵌套布局。
        register_routes();
        // ③ 恢复上次页面(读临时目录的持久化文件;无记录则保持 "/")。
        Router::restore(cx);
        println!("[启动] 恢复到: {}", Router::current());

        let bounds = Bounds::centered(None, gpui_size(px(980.), px(640.)), cx);
        let guard_for_view = Rc::clone(&guard_on);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            move |_window, cx| cx.new(|_cx| DemoView::new(Rc::clone(&guard_for_view))),
        )
        .unwrap();
        cx.activate(true);
    });
}

/// gpui::size 的薄包装(避免与顶栏变量名混淆)。
fn gpui_size(w: gpui::Pixels, h: gpui::Pixels) -> gpui::Size<gpui::Pixels> {
    gpui::size(w, h)
}
