//! 可运行示例：`cargo run --example router_demo`。

use std::cell::Cell;
use std::rc::Rc;

use routers::gpui_kit::base::StyledExt;
use routers::gpui_kit::component::{
    ActiveTheme, Disableable, Selectable, Sizable, button::Button, switch::Switch,
};
use routers::gpui_kit::{
    App, AppContext, Bounds, Context, Entity, IntoElement, ParentElement, Render, SharedString,
    Styled, Subscription, Window, WindowBounds, WindowOptions, application, div, init, open_window,
    px, size,
};
use routers::{Outlet, Route, RouteChanged, Router, RouterDelegate};

struct DemoDelegate {
    guard_on: Rc<Cell<bool>>,
    store_path: std::path::PathBuf,
}

impl RouterDelegate for DemoDelegate {
    fn allow_navigate(
        &self,
        _cx: &mut App,
        _from: &SharedString,
        to: &SharedString,
    ) -> Result<bool, SharedString> {
        if self.guard_on.get() && to.starts_with("/admin") {
            return Err("守卫已开启，进入后台需要确认".into());
        }
        Ok(true)
    }

    fn persist_pathname(&self, _cx: &mut App, path: &SharedString) {
        if let Err(error) = std::fs::write(&self.store_path, path.as_str()) {
            eprintln!("无法保存演示路径：{error}");
        }
    }

    fn restore_pathname(&self, _cx: &App) -> Option<SharedString> {
        match std::fs::read_to_string(&self.store_path) {
            Ok(path) => Some(path.into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                eprintln!("无法读取演示路径：{error}");
                None
            }
        }
    }
}

const NAV_ITEMS: &[(&str, &str, &str)] = &[
    ("/files", "文件", "/files"),
    ("/search", "搜索", "/search"),
    ("/user/42", "用户 42", "/user/{id}"),
    ("/user/7", "用户 7", "/user/{id}"),
    ("/settings/profile", "档案", "/settings/profile"),
    ("/settings/account", "账户", "/settings/account"),
    ("/admin/panel", "后台", "/admin/panel"),
];

struct DemoView {
    router: Entity<Router>,
    guard_on: Rc<Cell<bool>>,
    last_message: SharedString,
    _subscriptions: Vec<Subscription>,
}

impl DemoView {
    fn new(router: Entity<Router>, guard_on: Rc<Cell<bool>>, cx: &mut Context<Self>) -> Self {
        let observation = cx.observe(&router, |_, _, cx| cx.notify());
        let event_subscription = cx.subscribe(&router, |_, _, event: &RouteChanged, _| {
            println!("[RouteChanged] {}", event.pathname())
        });
        Self {
            router,
            guard_on,
            last_message: "选择左侧页面开始导航".into(),
            _subscriptions: vec![observation, event_subscription],
        }
    }

    fn go(&mut self, target: &'static str, force: bool, cx: &mut Context<Self>) {
        let succeeded = self.router.update(cx, |router, cx| {
            if force {
                router.navigate_force(cx, target)
            } else {
                router.navigate(cx, target)
            }
        });
        self.last_message = if succeeded {
            format!("已进入 {target}").into()
        } else {
            format!("前往 {target} 被守卫拒绝，当前页面未变").into()
        };
        cx.notify();
    }

    fn nav_button(
        &self,
        target: &'static str,
        label: &str,
        pattern: &str,
        cx: &mut Context<Self>,
    ) -> Button {
        let active = self.router.read(cx).is_active(pattern)
            && (pattern == target
                || routers::is_active_simple(self.router.read(cx).current(), target, true));
        Button::new(format!("nav-{target}"))
            .label(format!("{label} · {target}"))
            .selected(active)
            .on_click(cx.listener(move |this, _, _, cx| this.go(target, false, cx)))
    }
}

impl Render for DemoView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (current, can_back, can_forward, history, params) = {
            let router = self.router.read(cx);
            let params = if router.params().is_empty() {
                "无".to_string()
            } else {
                let mut values: Vec<String> = router
                    .params()
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect();
                values.sort();
                values.join(", ")
            };
            (
                router.current().clone(),
                router.can_go_back(),
                router.can_go_forward(),
                format!("历史 {} / {}", router.cursor() + 1, router.history_len()),
                params,
            )
        };

        let mut sidebar = div()
            .flex()
            .flex_col()
            .w_64()
            .p_3()
            .gap_1()
            .bg(cx.theme().sidebar)
            .child(
                div()
                    .font_medium()
                    .text_sm()
                    .pb_2()
                    .child("routers 导航示例"),
            );
        for &(target, label, pattern) in NAV_ITEMS {
            sidebar = sidebar.child(self.nav_button(target, label, pattern, cx));
        }

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .flex()
                    .gap_2()
                    .p_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("back")
                            .label("后退")
                            .small()
                            .disabled(!can_back)
                            .on_click(cx.listener(|this, _, _, cx| {
                                let succeeded =
                                    this.router.update(cx, |router, cx| router.back(cx));
                                this.last_message = if succeeded {
                                    "已后退".into()
                                } else {
                                    "无法后退".into()
                                };
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("forward")
                            .label("前进")
                            .small()
                            .disabled(!can_forward)
                            .on_click(cx.listener(|this, _, _, cx| {
                                let succeeded =
                                    this.router.update(cx, |router, cx| router.forward(cx));
                                this.last_message = if succeeded {
                                    "已前进".into()
                                } else {
                                    "无法前进".into()
                                };
                                cx.notify();
                            })),
                    )
                    .child(div().text_sm().child(format!("当前路径：{current}")))
                    .child(div().flex_1())
                    .child(
                        Switch::new("guard")
                            .label("守卫拦截后台")
                            .checked(self.guard_on.get())
                            .on_change(cx.listener(|this, enabled, _, cx| {
                                this.guard_on.set(*enabled);
                                this.last_message = if *enabled {
                                    "普通后台导航将被拒绝".into()
                                } else {
                                    "后台守卫已关闭".into()
                                };
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_4()
                    .px_3()
                    .py_2()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(history)
                    .child(format!("参数：{params}"))
                    .child(self.last_message.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_stretch()
                    .flex_1()
                    .min_h_0()
                    .child(sidebar)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .p_4()
                            .gap_3()
                            .child(
                                Button::new("force-admin")
                                    .label("确认并强制进入后台")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.go("/admin/panel", true, cx);
                                    })),
                            )
                            .child(div().flex_1().min_h_0().child(Outlet::new(&self.router))),
                    ),
            )
    }
}

fn panel(route: &Route, title: &str, description: &str, cx: &mut App) -> impl IntoElement {
    let params = if route.params().is_empty() {
        "当前路由没有动态参数".to_string()
    } else {
        let mut values: Vec<String> = route
            .params()
            .iter()
            .map(|(key, value)| format!("{key} = {value}"))
            .collect();
        values.sort();
        format!("动态参数：{}", values.join(", "))
    };
    div()
        .flex()
        .flex_col()
        .size_full()
        .gap_2()
        .p_4()
        .rounded(cx.theme().radius)
        .bg(cx.theme().group_box)
        .child(div().text_lg().font_medium().child(title.to_string()))
        .child(div().text_sm().child(description.to_string()))
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(format!("路径：{}", route.pathname())),
        )
        .child(div().text_sm().child(params))
}

fn register_routes(router: &mut Router) {
    router.register_element("/files", |route, _, cx| {
        panel(route, "文件", "静态路由 /files", cx).into_any_element()
    });
    router.register_element("/search", |route, _, cx| {
        panel(route, "搜索", "静态路由 /search", cx).into_any_element()
    });
    router.register_element("/user/{id}", |route, _, cx| {
        panel(route, "用户", "动态路由 /user/{id}", cx).into_any_element()
    });
    router.register_element("/settings/profile", |route, _, cx| {
        panel(route, "档案", "嵌套在设置布局中", cx).into_any_element()
    });
    router.register_element("/settings/account", |route, _, cx| {
        panel(route, "账户", "嵌套在设置布局中", cx).into_any_element()
    });
    router.register_element("/admin/panel", |route, _, cx| {
        panel(route, "后台", "守卫开启时可使用强制导航进入", cx).into_any_element()
    });
    router.register_layout("/", |outlet| {
        div()
            .flex()
            .flex_col()
            .size_full()
            .gap_2()
            .child(div().text_xs().child("根布局 /"))
            .child(div().flex_1().min_h_0().child(outlet))
            .into_any_element()
    });
    router.register_layout("/settings", |outlet| {
        div()
            .flex()
            .flex_col()
            .size_full()
            .gap_2()
            .p_3()
            .child(div().text_xs().child("设置布局 /settings"))
            .child(div().flex_1().min_h_0().child(outlet))
            .into_any_element()
    });
}

fn main() {
    let guard_on = Rc::new(Cell::new(false));
    let store_path = std::env::temp_dir().join("routers-demo-route.txt");
    println!("持久化文件：{}", store_path.display());

    application().run(move |cx| {
        init(cx);
        let router = cx.new(|_| {
            Router::with_delegate(DemoDelegate {
                guard_on: Rc::clone(&guard_on),
                store_path,
            })
        });
        router.update(cx, |router, _| register_routes(router));
        router.update(cx, |router, cx| {
            router.restore(cx);
        });
        println!("[启动] 当前路径：{}", router.read(cx).current());

        let bounds = Bounds::centered(None, size(px(980.), px(640.)), cx);
        open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            cx,
            move |window, cx| {
                window.set_window_title("routers 路由示例");
                cx.new(|cx| DemoView::new(router.clone(), Rc::clone(&guard_on), cx))
            },
        )
        .expect("无法创建 routers 示例窗口");
        cx.activate(true);
    });
}
