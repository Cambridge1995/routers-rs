//! 通过公开 API 验证导航、历史、守卫、恢复、事件和 Outlet。

use std::cell::RefCell;
use std::rc::Rc;

use routers::gpui_kit::{
    App, AppContext, Entity, IntoElement, ParentElement, RenderOnce, SharedString, TestAppContext,
    div,
};
use routers::{Outlet, RouteChanged, Router, RouterDelegate};

type PersistedPaths = Rc<RefCell<Vec<SharedString>>>;
type GuardCalls = Rc<RefCell<Vec<(SharedString, SharedString)>>>;

struct RecDelegate {
    persisted: PersistedPaths,
    guard_calls: GuardCalls,
    blocked_err: Vec<&'static str>,
    blocked_silent: Vec<&'static str>,
    saved: Option<SharedString>,
}

impl RecDelegate {
    fn new() -> Self {
        Self {
            persisted: Rc::new(RefCell::new(Vec::new())),
            guard_calls: Rc::new(RefCell::new(Vec::new())),
            blocked_err: Vec::new(),
            blocked_silent: Vec::new(),
            saved: None,
        }
    }

    fn handles(&self) -> (PersistedPaths, GuardCalls) {
        (Rc::clone(&self.persisted), Rc::clone(&self.guard_calls))
    }
}

impl RouterDelegate for RecDelegate {
    fn allow_navigate(
        &self,
        _cx: &mut App,
        from: &SharedString,
        to: &SharedString,
    ) -> Result<bool, SharedString> {
        self.guard_calls
            .borrow_mut()
            .push((from.clone(), to.clone()));
        if self.blocked_err.iter().any(|prefix| to.starts_with(prefix)) {
            return Err(format!("守卫拒绝：{to}").into());
        }
        if self
            .blocked_silent
            .iter()
            .any(|prefix| to.starts_with(prefix))
        {
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

fn paths(values: &RefCell<Vec<SharedString>>) -> Vec<String> {
    values.borrow().iter().map(ToString::to_string).collect()
}

fn new_router(cx: &mut App) -> Entity<Router> {
    cx.new(|_| Router::new())
}

fn router_with_delegate<D: RouterDelegate>(cx: &mut App, delegate: D) -> Entity<Router> {
    cx.new(|_| Router::with_delegate(delegate))
}

fn navigate(cx: &mut App, router: &Entity<Router>, path: &'static str) -> bool {
    router.update(cx, |router, cx| router.navigate(cx, path))
}

fn navigate_force(cx: &mut App, router: &Entity<Router>, path: &'static str) -> bool {
    router.update(cx, |router, cx| router.navigate_force(cx, path))
}

fn back(cx: &mut App, router: &Entity<Router>) -> bool {
    router.update(cx, |router, cx| router.back(cx))
}

fn forward(cx: &mut App, router: &Entity<Router>) -> bool {
    router.update(cx, |router, cx| router.forward(cx))
}

#[gpui_kit::test]
fn default_state(cx: &mut TestAppContext) {
    let router = cx.update(new_router);
    cx.update(|cx| {
        let router = router.read(cx);
        assert_eq!(router.current().as_ref(), "/");
        assert_eq!(router.history_len(), 1);
        assert_eq!(router.cursor(), 0);
        assert!(!router.can_go_back());
        assert!(!router.can_go_forward());
    });
}

#[gpui_kit::test]
fn instances_are_independent(cx: &mut TestAppContext) {
    let (first, second) = cx.update(|cx| (new_router(cx), new_router(cx)));
    cx.update(|cx| {
        navigate(cx, &first, "/files");
        navigate(cx, &second, "/search");
        assert_eq!(first.read(cx).current().as_ref(), "/files");
        assert_eq!(second.read(cx).current().as_ref(), "/search");
        assert_eq!(first.read(cx).history_len(), 2);
        assert_eq!(second.read(cx).history_len(), 2);
    });
}

#[gpui_kit::test]
fn navigate_updates_state_and_persists(cx: &mut TestAppContext) {
    let delegate = RecDelegate::new();
    let (persisted, _) = delegate.handles();
    let router = cx.update(|cx| router_with_delegate(cx, delegate));
    cx.update(|cx| {
        assert!(navigate(cx, &router, "/files"));
        assert!(navigate(cx, &router, "/search"));
        let state = router.read(cx);
        assert_eq!(state.current().as_ref(), "/search");
        assert_eq!(state.history_len(), 3);
        assert_eq!(state.cursor(), 2);
        assert!(state.can_go_back());
        assert!(!state.can_go_forward());
    });
    assert_eq!(paths(&persisted), vec!["/files", "/search"]);
}

#[gpui_kit::test]
fn same_path_does_not_duplicate_history(cx: &mut TestAppContext) {
    let router = cx.update(new_router);
    cx.update(|cx| {
        assert!(navigate(cx, &router, "/files"));
        assert!(navigate(cx, &router, "/files"));
        assert_eq!(router.read(cx).history_len(), 2);
        assert_eq!(router.read(cx).cursor(), 1);
    });
}

#[gpui_kit::test]
fn back_forward_semantics(cx: &mut TestAppContext) {
    let delegate = RecDelegate::new();
    let (persisted, _) = delegate.handles();
    let router = cx.update(|cx| router_with_delegate(cx, delegate));
    cx.update(|cx| {
        navigate(cx, &router, "/a");
        navigate(cx, &router, "/b");
        navigate(cx, &router, "/c");
        assert!(back(cx, &router));
        assert_eq!(router.read(cx).current().as_ref(), "/b");
        assert!(back(cx, &router));
        assert_eq!(router.read(cx).current().as_ref(), "/a");
        assert!(router.read(cx).can_go_forward());
        assert!(forward(cx, &router));
        assert_eq!(router.read(cx).current().as_ref(), "/b");
        assert_eq!(router.read(cx).history_len(), 4);
        assert_eq!(router.read(cx).cursor(), 2);
    });
    assert_eq!(paths(&persisted), vec!["/a", "/b", "/c", "/b", "/a", "/b"]);
}

#[gpui_kit::test]
fn back_forward_boundaries(cx: &mut TestAppContext) {
    let router = cx.update(new_router);
    cx.update(|cx| {
        assert!(!back(cx, &router));
        assert!(!forward(cx, &router));
        navigate(cx, &router, "/a");
        assert!(!forward(cx, &router));
        assert!(back(cx, &router));
        assert!(!back(cx, &router));
        assert_eq!(router.read(cx).current().as_ref(), "/");
        assert_eq!(router.read(cx).cursor(), 0);
    });
}

#[gpui_kit::test]
fn navigate_after_back_truncates_forward_history(cx: &mut TestAppContext) {
    let router = cx.update(new_router);
    cx.update(|cx| {
        navigate(cx, &router, "/a");
        navigate(cx, &router, "/b");
        back(cx, &router);
        navigate(cx, &router, "/c");
        let state = router.read(cx);
        assert_eq!(state.current().as_ref(), "/c");
        assert_eq!(state.history_len(), 3);
        assert_eq!(state.cursor(), 2);
        assert!(!state.can_go_forward());
    });
}

#[gpui_kit::test]
fn guard_rejects_with_reason(cx: &mut TestAppContext) {
    let mut delegate = RecDelegate::new();
    delegate.blocked_err = vec!["/admin"];
    let (persisted, _) = delegate.handles();
    let router = cx.update(|cx| router_with_delegate(cx, delegate));
    cx.update(|cx| {
        assert!(!navigate(cx, &router, "/admin/panel"));
        assert_eq!(router.read(cx).current().as_ref(), "/");
        assert_eq!(router.read(cx).history_len(), 1);
        assert!(persisted.borrow().is_empty());
        assert!(navigate(cx, &router, "/files"));
    });
}

#[gpui_kit::test]
fn guard_silently_rejects(cx: &mut TestAppContext) {
    let mut delegate = RecDelegate::new();
    delegate.blocked_silent = vec!["/dirty"];
    let (persisted, _) = delegate.handles();
    let router = cx.update(|cx| router_with_delegate(cx, delegate));
    cx.update(|cx| {
        assert!(!navigate(cx, &router, "/dirty-page"));
        assert_eq!(router.read(cx).current().as_ref(), "/");
    });
    assert!(persisted.borrow().is_empty());
}

#[gpui_kit::test]
fn force_navigation_bypasses_guard(cx: &mut TestAppContext) {
    let mut delegate = RecDelegate::new();
    delegate.blocked_err = vec!["/admin"];
    let (persisted, guard_calls) = delegate.handles();
    let router = cx.update(|cx| router_with_delegate(cx, delegate));
    cx.update(|cx| {
        assert!(navigate_force(cx, &router, "/admin/panel"));
        assert_eq!(router.read(cx).current().as_ref(), "/admin/panel");
    });
    assert_eq!(paths(&persisted), vec!["/admin/panel"]);
    assert!(guard_calls.borrow().is_empty());
}

#[gpui_kit::test]
fn history_guard_and_force_variants(cx: &mut TestAppContext) {
    let mut delegate = RecDelegate::new();
    delegate.blocked_err = vec!["/a"];
    let router = cx.update(|cx| router_with_delegate(cx, delegate));
    cx.update(|cx| {
        navigate_force(cx, &router, "/a");
        navigate(cx, &router, "/b");
        assert!(!back(cx, &router));
        assert_eq!(router.read(cx).current().as_ref(), "/b");
        assert!(router.update(cx, |router, cx| router.back_force(cx)));
        assert_eq!(router.read(cx).current().as_ref(), "/a");
        assert!(forward(cx, &router));
        assert!(router.update(cx, |router, cx| router.back_force(cx)));
        assert!(router.update(cx, |router, cx| router.forward_force(cx)));
        assert_eq!(router.read(cx).current().as_ref(), "/b");
    });
}

#[gpui_kit::test]
fn guard_receives_current_and_target(cx: &mut TestAppContext) {
    let delegate = RecDelegate::new();
    let (_, guard_calls) = delegate.handles();
    let router = cx.update(|cx| router_with_delegate(cx, delegate));
    cx.update(|cx| {
        navigate(cx, &router, "/one");
        navigate(cx, &router, "/two");
        back(cx, &router);
    });
    let calls: Vec<(String, String)> = guard_calls
        .borrow()
        .iter()
        .map(|(from, to)| (from.to_string(), to.to_string()))
        .collect();
    assert_eq!(
        calls,
        vec![
            ("/".to_string(), "/one".to_string()),
            ("/one".to_string(), "/two".to_string()),
            ("/two".to_string(), "/one".to_string()),
        ]
    );
}

#[gpui_kit::test]
fn dynamic_params_follow_navigation(cx: &mut TestAppContext) {
    let router = cx.update(new_router);
    cx.update(|cx| {
        router.update(cx, |router, _| router.register("/user/{id}"));
        navigate(cx, &router, "/user/123");
        assert_eq!(
            router.read(cx).params().get("id").map(|v| v.as_ref()),
            Some("123")
        );
        navigate(cx, &router, "/user/abc");
        assert_eq!(
            router.read(cx).params().get("id").map(|v| v.as_ref()),
            Some("abc")
        );
    });
}

#[gpui_kit::test]
fn wildcard_params_include_remaining_segments(cx: &mut TestAppContext) {
    let router = cx.update(new_router);
    cx.update(|cx| {
        router.update(cx, |router, _| router.register("/files/{*path}"));
        navigate(cx, &router, "/files/docs/rs/main.rs");
        assert_eq!(
            router.read(cx).params().get("path").map(|v| v.as_ref()),
            Some("docs/rs/main.rs")
        );
    });
}

#[gpui_kit::test]
fn unmatched_path_clears_params(cx: &mut TestAppContext) {
    let router = cx.update(new_router);
    cx.update(|cx| {
        router.update(cx, |router, _| router.register("/user/{id}"));
        navigate(cx, &router, "/user/123");
        navigate(cx, &router, "/elsewhere");
        assert!(router.read(cx).params().is_empty());
    });
}

#[gpui_kit::test]
fn params_follow_back_and_forward(cx: &mut TestAppContext) {
    let router = cx.update(new_router);
    cx.update(|cx| {
        router.update(cx, |router, _| router.register("/user/{id}"));
        navigate(cx, &router, "/user/1");
        navigate(cx, &router, "/user/2");
        back(cx, &router);
        assert_eq!(
            router.read(cx).params().get("id").map(|v| v.as_ref()),
            Some("1")
        );
        forward(cx, &router);
        assert_eq!(
            router.read(cx).params().get("id").map(|v| v.as_ref()),
            Some("2")
        );
    });
}

#[gpui_kit::test]
fn registering_new_pattern_invalidates_match_cache(cx: &mut TestAppContext) {
    let router = cx.update(new_router);
    cx.update(|cx| {
        router.update(cx, |router, _| router.register("/files"));
        navigate(cx, &router, "/files");
        assert!(router.read(cx).is_active("/files"));
        router.update(cx, |router, _| router.register("/user/{id}"));
        navigate(cx, &router, "/user/42");
        assert!(router.read(cx).is_active("/user/{id}"));
        assert_eq!(
            router.read(cx).params().get("id").map(|v| v.as_ref()),
            Some("42")
        );
    });
}

#[gpui_kit::test]
fn active_uses_registered_pattern(cx: &mut TestAppContext) {
    let router = cx.update(new_router);
    cx.update(|cx| {
        router.update(cx, |router, _| {
            router.register("/files");
            router.register("/user/{id}");
        });
        navigate(cx, &router, "/user/123");
        assert!(router.read(cx).is_active("/user/{id}"));
        assert!(!router.read(cx).is_active("/files"));
        navigate(cx, &router, "/files");
        assert!(router.read(cx).is_active("/files"));
        assert!(!router.read(cx).is_active("/user/{id}"));
    });
}

#[gpui_kit::test]
fn active_unregistered_target_uses_segment_prefix(cx: &mut TestAppContext) {
    let router = cx.update(new_router);
    cx.update(|cx| {
        navigate(cx, &router, "/docs/guide/intro");
        assert!(router.read(cx).is_active("/docs"));
        assert!(!router.read(cx).is_active("/docs-account"));
    });
}

#[gpui_kit::test]
fn duplicate_registration_is_harmless(cx: &mut TestAppContext) {
    let router = cx.update(new_router);
    cx.update(|cx| {
        router.update(cx, |router, _| {
            router.register("/files");
            router.register("/files");
        });
        assert!(navigate(cx, &router, "/files"));
        assert!(router.read(cx).is_active("/files"));
    });
}

#[gpui_kit::test]
fn restore_valid_path_resets_history_without_persisting(cx: &mut TestAppContext) {
    let mut delegate = RecDelegate::new();
    delegate.saved = Some("/user/42".into());
    let (persisted, guard_calls) = delegate.handles();
    let router = cx.update(|cx| router_with_delegate(cx, delegate));
    cx.update(|cx| {
        router.update(cx, |router, _| router.register("/user/{id}"));
        assert!(router.update(cx, |router, cx| router.restore(cx)));
        let state = router.read(cx);
        assert_eq!(state.current().as_ref(), "/user/42");
        assert_eq!(state.params().get("id").map(|v| v.as_ref()), Some("42"));
        assert_eq!(state.history_len(), 1);
        assert!(!state.can_go_back());
        assert!(!state.can_go_forward());
    });
    assert!(persisted.borrow().is_empty());
    assert!(guard_calls.borrow().is_empty());
}

#[gpui_kit::test]
fn restore_invalid_path_falls_back_to_root(cx: &mut TestAppContext) {
    let mut delegate = RecDelegate::new();
    delegate.saved = Some("/deleted-page".into());
    let router = cx.update(|cx| router_with_delegate(cx, delegate));
    cx.update(|cx| {
        router.update(cx, |router, _| router.register("/files"));
        assert!(!router.update(cx, |router, cx| router.restore(cx)));
        assert_eq!(router.read(cx).current().as_ref(), "/");
        assert_eq!(router.read(cx).history_len(), 1);
    });
}

#[gpui_kit::test]
fn restore_without_patterns_accepts_saved_path(cx: &mut TestAppContext) {
    let mut delegate = RecDelegate::new();
    delegate.saved = Some("/anything".into());
    let router = cx.update(|cx| router_with_delegate(cx, delegate));
    cx.update(|cx| {
        assert!(router.update(cx, |router, cx| router.restore(cx)));
        assert_eq!(router.read(cx).current().as_ref(), "/anything");
    });
}

#[gpui_kit::test]
fn restore_without_record_keeps_default(cx: &mut TestAppContext) {
    let router = cx.update(|cx| router_with_delegate(cx, RecDelegate::new()));
    cx.update(|cx| {
        assert!(!router.update(cx, |router, cx| router.restore(cx)));
        assert_eq!(router.read(cx).current().as_ref(), "/");
    });
}

#[gpui_kit::test]
fn restore_root_keeps_default(cx: &mut TestAppContext) {
    let mut delegate = RecDelegate::new();
    delegate.saved = Some("/".into());
    let router = cx.update(|cx| router_with_delegate(cx, delegate));
    cx.update(|cx| {
        assert!(!router.update(cx, |router, cx| router.restore(cx)));
        assert_eq!(router.read(cx).current().as_ref(), "/");
        assert_eq!(router.read(cx).history_len(), 1);
    });
}

#[gpui_kit::test]
fn persistence_is_synchronous_and_route_event_is_delivered(cx: &mut TestAppContext) {
    struct OrderDelegate {
        calls: Rc<RefCell<Vec<String>>>,
    }
    impl RouterDelegate for OrderDelegate {
        fn persist_pathname(&self, _cx: &mut App, path: &SharedString) {
            self.calls.borrow_mut().push(format!("persist:{path}"));
        }
    }

    let calls = Rc::new(RefCell::new(Vec::new()));
    let router = cx.update(|cx| {
        router_with_delegate(
            cx,
            OrderDelegate {
                calls: Rc::clone(&calls),
            },
        )
    });
    let event_calls = Rc::clone(&calls);
    let _subscriber = cx.update(|cx| {
        cx.new(|cx| {
            cx.subscribe(&router, move |_, _, event: &RouteChanged, _| {
                event_calls
                    .borrow_mut()
                    .push(format!("event:{}", event.pathname()));
            })
            .detach();
        })
    });
    cx.update(|cx| {
        navigate(cx, &router, "/files");
        assert_eq!(calls.borrow().as_slice(), &["persist:/files"]);
    });
    assert_eq!(
        calls.borrow().as_slice(),
        &["persist:/files", "event:/files"]
    );
}

#[gpui_kit::test]
fn restore_emits_route_event(cx: &mut TestAppContext) {
    let mut delegate = RecDelegate::new();
    delegate.saved = Some("/files".into());
    let (persisted, _) = delegate.handles();
    let router = cx.update(|cx| router_with_delegate(cx, delegate));
    let restored = Rc::new(RefCell::new(Vec::new()));
    let restored_in_event = Rc::clone(&restored);
    let _subscriber = cx.update(|cx| {
        cx.new(|cx| {
            cx.subscribe(&router, move |_, _, event: &RouteChanged, _| {
                restored_in_event
                    .borrow_mut()
                    .push(event.pathname().to_string());
            })
            .detach();
        })
    });
    cx.update(|cx| {
        assert!(router.update(cx, |router, cx| router.restore(cx)));
    });
    assert_eq!(restored.borrow().as_slice(), &["/files"]);
    assert!(persisted.borrow().is_empty());
}

#[gpui_kit::test]
fn outlet_renders_registered_element(cx: &mut TestAppContext) {
    let router = cx.update(new_router);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let calls_in_factory = Rc::clone(&calls);
    cx.update(|cx| {
        router.update(cx, |router, _| {
            router.register_element("/files", move |_, _, _| {
                calls_in_factory.borrow_mut().push("files");
                div().into_any_element()
            });
        });
        navigate(cx, &router, "/files");
    });
    let window = cx.add_empty_window();
    window.update(|window, cx| {
        drop(Outlet::new(&router).render(window, cx));
    });
    assert_eq!(calls.borrow().as_slice(), &["files"]);
}

#[gpui_kit::test]
fn outlet_wraps_layouts_from_specific_to_root(cx: &mut TestAppContext) {
    let router = cx.update(new_router);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let element_calls = Rc::clone(&calls);
    let root_calls = Rc::clone(&calls);
    let settings_calls = Rc::clone(&calls);
    cx.update(|cx| {
        router.update(cx, |router, _| {
            router.register_element("/settings/profile", move |_, _, _| {
                element_calls.borrow_mut().push("element");
                div().into_any_element()
            });
            router.register_layout("/", move |outlet| {
                root_calls.borrow_mut().push("root");
                div().child(outlet).into_any_element()
            });
            router.register_layout("/settings", move |outlet| {
                settings_calls.borrow_mut().push("settings");
                div().child(outlet).into_any_element()
            });
            router.register_layout("/unrelated", |outlet| outlet);
        });
        navigate(cx, &router, "/settings/profile");
    });
    let window = cx.add_empty_window();
    window.update(|window, cx| {
        drop(Outlet::new(&router).render(window, cx));
    });
    assert_eq!(calls.borrow().as_slice(), &["element", "settings", "root"]);
}

#[gpui_kit::test]
fn outlet_without_match_renders_empty(cx: &mut TestAppContext) {
    let router = cx.update(new_router);
    cx.update(|cx| navigate(cx, &router, "/nothing-registered"));
    let window = cx.add_empty_window();
    window.update(|window, cx| {
        drop(Outlet::new(&router).render(window, cx));
    });
}

#[gpui_kit::test]
fn registering_same_element_replaces_factory(cx: &mut TestAppContext) {
    let router = cx.update(new_router);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let old_calls = Rc::clone(&calls);
    let new_calls = Rc::clone(&calls);
    cx.update(|cx| {
        router.update(cx, |router, _| {
            router.register_element("/files", move |_, _, _| {
                old_calls.borrow_mut().push("old");
                div().into_any_element()
            });
            router.register_element("/files", move |_, _, _| {
                new_calls.borrow_mut().push("new");
                div().into_any_element()
            });
        });
        navigate(cx, &router, "/files");
    });
    let window = cx.add_empty_window();
    window.update(|window, cx| {
        drop(Outlet::new(&router).render(window, cx));
    });
    assert_eq!(calls.borrow().as_slice(), &["new"]);
}

#[gpui_kit::test]
fn outlet_dynamic_element_supplies_route_snapshot(cx: &mut TestAppContext) {
    let router = cx.update(new_router);
    let seen = Rc::new(RefCell::new(None));
    cx.update(|cx| {
        let seen = Rc::clone(&seen);
        router.update(cx, |router, _| {
            router.register_element("/user/{id}", move |route, _, _| {
                *seen.borrow_mut() = route.param("id").map(ToString::to_string);
                div().into_any_element()
            });
        });
        navigate(cx, &router, "/user/42");
    });
    let window = cx.add_empty_window();
    window.update(|window, cx| {
        drop(Outlet::new(&router).render(window, cx));
    });
    assert_eq!(seen.borrow().as_deref(), Some("42"));
}

#[gpui_kit::test]
fn new_static_element_invalidates_dynamic_match_cache(cx: &mut TestAppContext) {
    let router = cx.update(new_router);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let dynamic_calls = Rc::clone(&calls);
    cx.update(|cx| {
        router.update(cx, |router, _| {
            router.register_element("/user/{id}", move |_, _, _| {
                dynamic_calls.borrow_mut().push("dynamic");
                div().into_any_element()
            });
        });
        navigate(cx, &router, "/user/42");
    });
    let window = cx.add_empty_window();
    window.update(|window, cx| {
        drop(Outlet::new(&router).render(window, cx));
        let static_calls = Rc::clone(&calls);
        router.update(cx, |router, _| {
            router.register_element("/user/new", move |_, _, _| {
                static_calls.borrow_mut().push("static");
                div().into_any_element()
            });
        });
        navigate(cx, &router, "/user/new");
        drop(Outlet::new(&router).render(window, cx));
    });
    assert_eq!(calls.borrow().as_slice(), &["dynamic", "static"]);
}

#[gpui_kit::test]
fn outlet_wildcard_element_matches_nested_path(cx: &mut TestAppContext) {
    let router = cx.update(new_router);
    let seen = Rc::new(RefCell::new(None));
    cx.update(|cx| {
        let seen = Rc::clone(&seen);
        router.update(cx, |router, _| {
            router.register_element("/docs/{*path}", move |route, _, _| {
                *seen.borrow_mut() = route.param("path").map(ToString::to_string);
                div().into_any_element()
            });
        });
        navigate(cx, &router, "/docs/guide/intro");
    });
    let window = cx.add_empty_window();
    window.update(|window, cx| {
        drop(Outlet::new(&router).render(window, cx));
    });
    assert_eq!(seen.borrow().as_deref(), Some("guide/intro"));
}

#[gpui_kit::test]
fn outlets_use_their_own_router_instances(cx: &mut TestAppContext) {
    let (first, second) = cx.update(|cx| (new_router(cx), new_router(cx)));
    let calls = Rc::new(RefCell::new(Vec::new()));
    let first_calls = Rc::clone(&calls);
    let second_calls = Rc::clone(&calls);
    cx.update(|cx| {
        first.update(cx, |router, _| {
            router.register_element("/first", move |_, _, _| {
                first_calls.borrow_mut().push("first");
                div().into_any_element()
            });
        });
        second.update(cx, |router, _| {
            router.register_element("/second", move |_, _, _| {
                second_calls.borrow_mut().push("second");
                div().into_any_element()
            });
        });
        navigate(cx, &first, "/first");
        navigate(cx, &second, "/second");
    });
    let window = cx.add_empty_window();
    window.update(|window, cx| {
        drop(Outlet::new(&first).render(window, cx));
        drop(Outlet::new(&second).render(window, cx));
    });
    assert_eq!(calls.borrow().as_slice(), &["first", "second"]);
}
