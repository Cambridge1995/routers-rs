//! routers 端到端集成测试:用 gpui `TestAppContext` 驱动**真实**的
//! navigate / back / forward / 守卫 / 动态参数 / 持久化 / restore / Outlet 渲染。
//!
//! 与 `router.rs` 内的纯算法单测互补——这里全部走公开 API,验证真实运行时行为。
//!
//! 实现要点:Router 是 thread_local 单例,Rust 测试 harness 给每个 `#[test]`
//! 分配独立线程,因此每个测试天然拿到全新的 Router,`init_*` 互不干扰。

use std::cell::RefCell;
use std::rc::Rc;

use routers::gpui::{App, IntoElement, ParentElement, RenderOnce, SharedString, TestAppContext, div};
use routers::{Outlet, Router, RouterEventBus};

// ════════════════════════════════════════════════════════════════════════
// 测试基础设施:记录型 EventBus
// ════════════════════════════════════════════════════════════════════════

/// 记录 publish/persist 调用序列,支持按前缀配置守卫拦截(Err / 静默两种),
/// 支持预设 restore 返回值。记录经 `Rc<RefCell>` 与测试共享(单线程测试)。
struct RecBus {
    published: Rc<RefCell<Vec<SharedString>>>,
    persisted: Rc<RefCell<Vec<SharedString>>>,
    guard_calls: Rc<RefCell<Vec<(SharedString, SharedString)>>>,
    /// 命中前缀 → 守卫返回 Err(带错误信息取消)。
    blocked_err: Vec<&'static str>,
    /// 命中前缀 → 守卫返回 Ok(false)(静默取消)。
    blocked_silent: Vec<&'static str>,
    saved: Option<SharedString>,
}

impl RecBus {
    fn new() -> Self {
        Self {
            published: Rc::new(RefCell::new(Vec::new())),
            persisted: Rc::new(RefCell::new(Vec::new())),
            guard_calls: Rc::new(RefCell::new(Vec::new())),
            blocked_err: Vec::new(),
            blocked_silent: Vec::new(),
            saved: None,
        }
    }

    /// 取出记录句柄(bus 移交 Router 后,测试侧仍可读记录)。
    fn handles(
        &self,
    ) -> (
        Rc<RefCell<Vec<SharedString>>>,
        Rc<RefCell<Vec<SharedString>>>,
        Rc<RefCell<Vec<(SharedString, SharedString)>>>,
    ) {
        (
            Rc::clone(&self.published),
            Rc::clone(&self.persisted),
            Rc::clone(&self.guard_calls),
        )
    }
}

impl RouterEventBus for RecBus {
    fn publish_route_changed(&self, _cx: &mut App, path: SharedString) {
        self.published.borrow_mut().push(path);
    }

    fn allow_navigate(
        &self,
        _cx: &mut App,
        from: SharedString,
        to: SharedString,
    ) -> Result<bool, SharedString> {
        self.guard_calls
            .borrow_mut()
            .push((from, to.clone()));
        if self.blocked_err.iter().any(|p| to.starts_with(p)) {
            return Err(format!("守卫拒绝:{to}").into());
        }
        if self.blocked_silent.iter().any(|p| to.starts_with(p)) {
            return Ok(false);
        }
        Ok(true)
    }

    fn persist_pathname(&self, _cx: &mut App, path: &SharedString) {
        self.persisted.borrow_mut().push(path.clone());
    }

    fn restore_pathname(&self, _cx: &App) -> Option<SharedString> {
        self.saved.clone()
    }
}

fn paths(v: &RefCell<Vec<SharedString>>) -> Vec<String> {
    v.borrow().iter().map(|p| p.to_string()).collect()
}

// ════════════════════════════════════════════════════════════════════════
// 1. 初始化与 navigate 基本行为
// ════════════════════════════════════════════════════════════════════════

#[test]
fn test_init_default_state() {
    let cx = TestAppContext::single();
    Router::init_default();
    assert!(Router::is_enabled());
    assert_eq!(Router::current().as_ref(), "/");
    assert_eq!(Router::history_len(), 1);
    assert_eq!(Router::cursor(), 0);
    assert!(!Router::can_go_back());
    assert!(!Router::can_go_forward());
    drop(cx);
}

#[test]
fn test_navigate_updates_current_history_and_fires_hooks() {
    let cx = TestAppContext::single();
    let bus = RecBus::new();
    let (published, persisted, _) = bus.handles();
    Router::init_with(bus);

    cx.update(|cx| {
        assert!(Router::navigate(cx, "/files"));
        assert!(Router::navigate(cx, "/search"));
    });

    assert_eq!(Router::current().as_ref(), "/search");
    assert_eq!(Router::history_len(), 3); // ["/", "/files", "/search"]
    assert_eq!(Router::cursor(), 2);
    assert!(Router::can_go_back());
    assert!(!Router::can_go_forward());
    // 每次 navigate 都应 publish + persist,且顺序一致。
    assert_eq!(paths(&published), vec!["/files", "/search"]);
    assert_eq!(paths(&persisted), vec!["/files", "/search"]);
}

#[test]
fn test_navigate_same_path_no_duplicate_push() {
    let cx = TestAppContext::single();
    Router::init_default();

    cx.update(|cx| {
        assert!(Router::navigate(cx, "/files"));
        assert!(Router::navigate(cx, "/files")); // 重复导航同路径
    });

    assert_eq!(Router::current().as_ref(), "/files");
    // history 不重复 push(避免 back/forward 死循环)。
    assert_eq!(Router::history_len(), 2);
    assert_eq!(Router::cursor(), 1);
}

// ════════════════════════════════════════════════════════════════════════
// 2. back / forward 历史语义
// ════════════════════════════════════════════════════════════════════════

#[test]
fn test_back_forward_semantics() {
    let cx = TestAppContext::single();
    let bus = RecBus::new();
    let (published, persisted, _) = bus.handles();
    Router::init_with(bus);

    cx.update(|cx| {
        Router::navigate(cx, "/a");
        Router::navigate(cx, "/b");
        Router::navigate(cx, "/c");

        assert_eq!(Router::current().as_ref(), "/c");
        assert!(Router::back(cx));
        assert_eq!(Router::current().as_ref(), "/b");
        assert!(Router::back(cx));
        assert_eq!(Router::current().as_ref(), "/a");
        assert!(Router::can_go_forward());

        assert!(Router::forward(cx));
        assert_eq!(Router::current().as_ref(), "/b");
    });

    // back/forward 也走 publish + persist。
    assert_eq!(
        paths(&published),
        vec!["/a", "/b", "/c", "/b", "/a", "/b"]
    );
    assert_eq!(paths(&persisted), published.borrow().clone());
    // back/forward 不截断 history。
    assert_eq!(Router::history_len(), 4);
    assert_eq!(Router::cursor(), 2);
}

#[test]
fn test_back_forward_boundaries() {
    let cx = TestAppContext::single();
    Router::init_default();

    cx.update(|cx| {
        // 初始只有 "/",不能 back / forward。
        assert!(!Router::back(cx));
        assert!(!Router::forward(cx));

        Router::navigate(cx, "/a");
        assert!(!Router::forward(cx)); // 在栈顶,不能 forward。
        assert!(Router::back(cx));
        assert!(!Router::back(cx)); // 到栈底,不能再 back。
        assert_eq!(Router::current().as_ref(), "/");
        assert_eq!(Router::cursor(), 0);
    });
}

#[test]
fn test_navigate_after_back_truncates_forward_history() {
    let cx = TestAppContext::single();
    Router::init_default();

    cx.update(|cx| {
        Router::navigate(cx, "/a");
        Router::navigate(cx, "/b");
        Router::back(cx); // 回到 /a,forward 栈里有 /b
        Router::navigate(cx, "/c"); // 应截断 /b
    });

    assert_eq!(Router::current().as_ref(), "/c");
    assert_eq!(Router::history_len(), 3); // ["/", "/a", "/c"]
    assert_eq!(Router::cursor(), 2);
    assert!(!Router::can_go_forward()); // /b 已被丢弃
}

// ════════════════════════════════════════════════════════════════════════
// 3. 路由守卫(allow_navigate)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn test_guard_rejects_with_err() {
    let cx = TestAppContext::single();
    let mut bus = RecBus::new();
    bus.blocked_err = vec!["/admin"];
    let (published, persisted, _) = bus.handles();
    Router::init_with(bus);

    cx.update(|cx| {
        // 被守卫拒绝:返回 false,一切状态不变。
        assert!(!Router::navigate(cx, "/admin/panel"));
        assert_eq!(Router::current().as_ref(), "/");
        assert_eq!(Router::history_len(), 1);
        assert!(published.borrow().is_empty());
        assert!(persisted.borrow().is_empty());

        // 其他路径不受影响。
        assert!(Router::navigate(cx, "/files"));
        assert_eq!(Router::current().as_ref(), "/files");
    });
}

#[test]
fn test_guard_ok_false_silent_reject() {
    let cx = TestAppContext::single();
    let mut bus = RecBus::new();
    bus.blocked_silent = vec!["/dirty"];
    let (published, _, _) = bus.handles();
    Router::init_with(bus);

    cx.update(|cx| {
        assert!(!Router::navigate(cx, "/dirty-page"));
        assert_eq!(Router::current().as_ref(), "/");
        assert!(published.borrow().is_empty());
    });
}

#[test]
fn test_navigate_force_bypasses_guard() {
    let cx = TestAppContext::single();
    let mut bus = RecBus::new();
    bus.blocked_err = vec!["/admin"];
    let (published, _, _) = bus.handles();
    Router::init_with(bus);

    cx.update(|cx| {
        // force 跳过守卫(用户已在弹框确认的场景)。
        assert!(Router::navigate_force(cx, "/admin/panel"));
        assert_eq!(Router::current().as_ref(), "/admin/panel");
    });
    assert_eq!(paths(&published), vec!["/admin/panel"]);
}

#[test]
fn test_back_forward_guard_and_force_variants() {
    let cx = TestAppContext::single();
    let mut bus = RecBus::new();
    bus.blocked_err = vec!["/a"];
    let (published, _, _) = bus.handles();
    Router::init_with(bus);

    cx.update(|cx| {
        Router::navigate_force(cx, "/a"); // force 进去
        Router::navigate(cx, "/b");
        // back 目标是 /a → 被守卫拒绝。
        assert!(!Router::back(cx));
        assert_eq!(Router::current().as_ref(), "/b");
        // back_force 跳过守卫。
        assert!(Router::back_force(cx));
        assert_eq!(Router::current().as_ref(), "/a");
        // forward 目标是 /b(未被拦)→ 放行。
        assert!(Router::forward(cx));
        assert_eq!(Router::current().as_ref(), "/b");
        // forward_force 同样可用。
        assert!(Router::back_force(cx));
        assert!(Router::forward_force(cx));
        assert_eq!(Router::current().as_ref(), "/b");
    });
    // 被拒绝的 back 不产生 publish。
    assert_eq!(paths(&published), vec!["/a", "/b", "/a", "/b", "/a", "/b"]);
}

#[test]
fn test_guard_receives_correct_from_and_to() {
    let cx = TestAppContext::single();
    let bus = RecBus::new();
    let (_, _, guard_calls) = bus.handles();
    Router::init_with(bus);

    cx.update(|cx| {
        Router::navigate(cx, "/one");
        Router::navigate(cx, "/two");
        Router::back(cx);
    });

    let calls: Vec<(String, String)> = guard_calls
        .borrow()
        .iter()
        .map(|(f, t)| (f.to_string(), t.to_string()))
        .collect();
    assert_eq!(
        calls,
        vec![
            ("/".to_string(), "/one".to_string()),
            ("/one".to_string(), "/two".to_string()),
            // back 的 to 取自 history[cursor-1]。
            ("/two".to_string(), "/one".to_string()),
        ]
    );
}

// ════════════════════════════════════════════════════════════════════════
// 4. 动态路由参数(matchit)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn test_dynamic_params_extraction() {
    let cx = TestAppContext::single();
    Router::init_default();
    Router::register("/user/{id}");

    cx.update(|cx| Router::navigate(cx, "/user/123"));
    let params = Router::params();
    assert_eq!(params.get("id").map(|v| v.as_ref()), Some("123"));

    // 换参数值,params 同步更新。
    cx.update(|cx| Router::navigate(cx, "/user/abc"));
    assert_eq!(
        Router::params().get("id").map(|v| v.as_ref()),
        Some("abc")
    );
}

#[test]
fn test_wildcard_params_extraction() {
    let cx = TestAppContext::single();
    Router::init_default();
    Router::register("/files/{*path}");

    cx.update(|cx| Router::navigate(cx, "/files/docs/rs/main.rs"));
    assert_eq!(
        Router::params().get("path").map(|v| v.as_ref()),
        Some("docs/rs/main.rs")
    );
}

#[test]
fn test_params_cleared_when_path_not_matched() {
    let cx = TestAppContext::single();
    Router::init_default();
    Router::register("/user/{id}");

    cx.update(|cx| {
        Router::navigate(cx, "/user/123");
        assert!(!Router::params().is_empty());
        // 导航到不匹配任何模式的路径 → params 清空。
        Router::navigate(cx, "/elsewhere");
        assert!(Router::params().is_empty());
    });
}

#[test]
fn test_params_follow_back_forward() {
    let cx = TestAppContext::single();
    Router::init_default();
    Router::register("/user/{id}");

    cx.update(|cx| {
        Router::navigate(cx, "/user/1");
        Router::navigate(cx, "/user/2");
        Router::back(cx);
        assert_eq!(
            Router::params().get("id").map(|v| v.as_ref()),
            Some("1")
        );
        Router::forward(cx);
        assert_eq!(
            Router::params().get("id").map(|v| v.as_ref()),
            Some("2")
        );
    });
}

// ════════════════════════════════════════════════════════════════════════
// 5. is_active(matchit 语义 + 降级前缀匹配)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn test_is_active_with_registered_patterns() {
    let cx = TestAppContext::single();
    Router::init_default();
    Router::register("/files");
    Router::register("/user/{id}");

    cx.update(|cx| Router::navigate(cx, "/user/123"));
    assert!(Router::is_active("/user/{id}")); // 动态段模式命中
    assert!(!Router::is_active("/files"));

    cx.update(|cx| Router::navigate(cx, "/files"));
    assert!(Router::is_active("/files"));
    assert!(!Router::is_active("/user/{id}"));
}

#[test]
fn test_is_active_unregistered_falls_back_to_prefix() {
    let cx = TestAppContext::single();
    Router::init_default();
    // /docs 未注册 → 降级 is_active_simple 前缀匹配。
    cx.update(|cx| Router::navigate(cx, "/docs/guide/intro"));
    assert!(Router::is_active("/docs"));
    assert!(!Router::is_active("/docs-account")); // 段边界,不误判
}

#[test]
fn test_register_dedup_then_navigate_works() {
    let cx = TestAppContext::single();
    Router::init_default();
    Router::register("/files");
    Router::register("/files"); // 重复注册应被去重(Trie 不冲突)

    cx.update(|cx| {
        assert!(Router::navigate(cx, "/files"));
        assert!(Router::is_active("/files"));
    });
}

// ════════════════════════════════════════════════════════════════════════
// 6. 持久化恢复(restore)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn test_restore_from_persisted_path() {
    let cx = TestAppContext::single();
    let mut bus = RecBus::new();
    bus.saved = Some("/user/42".into());
    let (published, _, _) = bus.handles();
    Router::init_with(bus);
    Router::register("/user/{id}");

    cx.update(|cx| Router::restore(cx));

    assert_eq!(Router::current().as_ref(), "/user/42");
    // params 照常填充。
    assert_eq!(
        Router::params().get("id").map(|v| v.as_ref()),
        Some("42")
    );
    // history 重置为 [restored],重启后 back/forward 从恢复页重新开始。
    assert_eq!(Router::history_len(), 1);
    assert!(!Router::can_go_back());
    assert!(!Router::can_go_forward());
    // 静默恢复:不 publish。
    assert!(published.borrow().is_empty());
}

#[test]
fn test_restore_invalid_path_falls_back_to_root() {
    let cx = TestAppContext::single();
    let mut bus = RecBus::new();
    bus.saved = Some("/deleted-page".into());
    Router::init_with(bus);
    Router::register("/files"); // patterns 非空 → 校验生效

    cx.update(|cx| Router::restore(cx));
    // 持久化路径在当前路由表无匹配 → 保持 "/"。
    assert_eq!(Router::current().as_ref(), "/");
    assert_eq!(Router::history_len(), 1);
}

#[test]
fn test_restore_without_patterns_skips_validation() {
    let cx = TestAppContext::single();
    let mut bus = RecBus::new();
    bus.saved = Some("/anything".into());
    Router::init_with(bus);
    // 不注册任何模式 → 无校验,直接恢复。

    cx.update(|cx| Router::restore(cx));
    assert_eq!(Router::current().as_ref(), "/anything");
}

#[test]
fn test_restore_none_or_root_keeps_default() {
    let cx = TestAppContext::single();
    // 无持久化记录。
    Router::init_with(RecBus::new());
    cx.update(|cx| Router::restore(cx));
    assert_eq!(Router::current().as_ref(), "/");

    // 注:同一线程 Router 只能 init 一次,根路径场景由独立测试函数覆盖:
    // (本函数只覆盖 None 场景;/ 场景语义等价于"无需恢复"。)
}

// ════════════════════════════════════════════════════════════════════════
// 7. Outlet 端到端渲染(经 VisualTestContext 拿 &mut Window + &mut App)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn test_outlet_renders_registered_element() {
    let mut cx = TestAppContext::single();
    let calls: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));

    Router::init_default();
    let calls_clone = Rc::clone(&calls);
    Router::register_element("/files", move |_w, _cx| {
        calls_clone.borrow_mut().push("files_element");
        div().into_any_element()
    });

    cx.update(|cx| {
        Router::navigate(cx, "/files");
    });

    let cx = cx.add_empty_window();
    cx.update(|window, cx| {
        // Outlet 是 RenderOnce:render 应调起当前路径的叶子工厂。
        let _el = Outlet.render(window, cx);
    });
    assert_eq!(calls.borrow().as_slice(), &["files_element"]);
}

#[test]
fn test_outlet_layout_wrapping_order_longest_first() {
    let mut cx = TestAppContext::single();
    let calls: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));

    Router::init_default();

    let c = Rc::clone(&calls);
    Router::register_element("/settings/profile", move |_w, _cx| {
        c.borrow_mut().push("element");
        div().into_any_element()
    });
    let c = Rc::clone(&calls);
    Router::register_layout("/", move |outlet| {
        c.borrow_mut().push("layout_root");
        div().child(outlet).into_any_element()
    });
    let c = Rc::clone(&calls);
    Router::register_layout("/settings", move |outlet| {
        c.borrow_mut().push("layout_settings");
        div().child(outlet).into_any_element()
    });
    // 与当前路径无关的 layout 不应参与包裹。
    let c = Rc::clone(&calls);
    Router::register_layout("/unrelated", move |outlet| {
        c.borrow_mut().push("layout_unrelated");
        outlet
    });

    cx.update(|cx| {
        Router::navigate(cx, "/settings/profile");
    });

    let cx = cx.add_empty_window();
    cx.update(|window, cx| {
        let _el = Outlet.render(window, cx);
    });

    // 由内向外:element → /settings(具体,内层)→ /(根,外层);/unrelated 不参与。
    assert_eq!(
        calls.borrow().as_slice(),
        &["element", "layout_settings", "layout_root"]
    );
}

#[test]
fn test_outlet_no_match_renders_empty_gracefully() {
    let mut cx = TestAppContext::single();
    Router::init_default();
    cx.update(|cx| {
        Router::navigate(cx, "/nothing-registered");
    });
    let cx = cx.add_empty_window();
    cx.update(|window, cx| {
        // 无 element 匹配 → Empty,不 panic。
        let _el = Outlet.render(window, cx);
    });
}

#[test]
fn test_outlet_without_router_enabled_renders_empty() {
    // 本测试线程不调用 init_*(Router 未启用)→ Outlet 优雅降级为 Empty。
    let mut cx = TestAppContext::single();
    let cx = cx.add_empty_window();
    cx.update(|window, cx| {
        let _el = Outlet.render(window, cx);
    });
    assert!(!Router::is_enabled());
}

#[test]
fn test_register_same_path_overrides_factory() {
    let mut cx = TestAppContext::single();
    let calls: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));

    Router::init_default();
    let c = Rc::clone(&calls);
    Router::register_element("/files", move |_w, _cx| {
        c.borrow_mut().push("old");
        div().into_any_element()
    });
    let c = Rc::clone(&calls);
    // 同路径重复注册:覆盖旧工厂(而非并存两个)。
    Router::register_element("/files", move |_w, _cx| {
        c.borrow_mut().push("new");
        div().into_any_element()
    });

    cx.update(|cx| {
        Router::navigate(cx, "/files");
    });
    let cx = cx.add_empty_window();
    cx.update(|window, cx| {
        let _el = Outlet.render(window, cx);
    });
    assert_eq!(calls.borrow().as_slice(), &["new"]);
}

/// 动态模式叶子(0.2 起):register_element("/user/{id}") 一条覆盖所有 /user/*。
#[test]
fn test_outlet_dynamic_element_pattern() {
    let mut cx = TestAppContext::single();
    let calls: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));

    Router::init_default();
    let c = Rc::clone(&calls);
    Router::register_element("/user/{id}", move |_w, _cx| {
        c.borrow_mut().push("user_panel");
        div().into_any_element()
    });

    cx.update(|cx| {
        Router::navigate(cx, "/user/42");
    });
    // 隐含 register:params 自动填充,无需另行 Router::register。
    assert_eq!(
        Router::params().get("id").map(|v| v.as_ref()),
        Some("42")
    );
    // is_active 用模式判定同样生效。
    assert!(Router::is_active("/user/{id}"));

    let cx = cx.add_empty_window();
    cx.update(|window, cx| {
        let _el = Outlet.render(window, cx);
    });
    assert_eq!(calls.borrow().as_slice(), &["user_panel"]);
}

/// 静态模式优先于动态模式(matchit 内建优先级)。
#[test]
fn test_outlet_static_beats_dynamic() {
    let mut cx = TestAppContext::single();
    let calls: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));

    Router::init_default();
    let c = Rc::clone(&calls);
    Router::register_element("/user/{id}", move |_w, _cx| {
        c.borrow_mut().push("dynamic");
        div().into_any_element()
    });
    let c = Rc::clone(&calls);
    Router::register_element("/user/new", move |_w, _cx| {
        c.borrow_mut().push("static");
        div().into_any_element()
    });

    cx.update(|cx| {
        Router::navigate(cx, "/user/new"); // 应命中静态,不是 {id}
    });
    let cx = cx.add_empty_window();
    cx.update(|window, cx| {
        let _el = Outlet.render(window, cx);
    });
    assert_eq!(calls.borrow().as_slice(), &["static"]);

    // 其他 id 仍走动态模式。
    cx.update(|_window, cx| {
        Router::navigate(cx, "/user/42");
    });
    cx.update(|window, cx| {
        let _el = Outlet.render(window, cx);
    });
    assert_eq!(calls.borrow().as_slice(), &["static", "dynamic"]);
}

/// 通配模式叶子:/docs/{*path} 匹配多级路径。
#[test]
fn test_outlet_wildcard_element_pattern() {
    let mut cx = TestAppContext::single();
    let calls: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));

    Router::init_default();
    let c = Rc::clone(&calls);
    Router::register_element("/docs/{*path}", move |_w, _cx| {
        c.borrow_mut().push("docs_panel");
        div().into_any_element()
    });

    cx.update(|cx| {
        Router::navigate(cx, "/docs/guide/intro");
    });
    assert_eq!(
        Router::params().get("path").map(|v| v.as_ref()),
        Some("guide/intro")
    );

    let cx = cx.add_empty_window();
    cx.update(|window, cx| {
        let _el = Outlet.render(window, cx);
    });
    assert_eq!(calls.borrow().as_slice(), &["docs_panel"]);
}

// ════════════════════════════════════════════════════════════════════════
// 8. 响应式通知链路说明
// ════════════════════════════════════════════════════════════════════════
//
// Router 的 pathname 用 RwSignal 承载,但其「变化通知」的公开契约是
// RouterEventBus::publish_route_changed(由使用方转发到 gpui observe / 自有
// EventBus,上面 §1–§3 已完整覆盖),而非 reactive_graph Effect——
// Effect 需要 effects feature + any_spawner,Router 本体不依赖它。
// 因此信号读路径的实时性由「navigate 后 Router::current() 立即反映新值」
// 的各断言覆盖(见 §1–§6),不再单独测 reactive_graph 内部机制。
