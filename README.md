# routers

[![Crates.io](https://img.shields.io/crates/v/routers-rs.svg)](https://crates.io/crates/routers-rs)
[![Documentation](https://docs.rs/routers-rs/badge.svg)](https://docs.rs/routers-rs)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](./LICENSE-MIT)

面向 GPUI Kit 桌面应用的路由组件。应用通过 `Entity<Router>` 显式持有当前路径、动态参数和历史栈，`Outlet` 从同一个实例渲染内容，不需要线程局部全局单例或另一套页面状态。

## 功能

- 静态路径、`{id}` 动态段和 `{*splat}` 通配路径。
- 命令式导航、前进、后退、历史截断和同路径去重。
- 精确模式选中态，以及带段边界的前缀选中态。
- 放行、静默拒绝、带原因拒绝三种守卫结果，以及确认后的强制导航。
- 当前路径持久化、启动恢复和失效路径回退。
- `Outlet` 内容渲染与按段边界匹配的嵌套布局。
- 原生 `RouteChanged` 事件和实体观察通知。
- 路由表及元素匹配缓存的明确失效规则。

## 依赖与安装

| crate | 版本 | 用途 |
|---|---|---|
| `gpui-kit` | 0.9.0 | 统一的 GPUI 类型、实体和元素体系 |
| `matchit` | 0.9 | 静态、动态和通配路径匹配 |

发布包名是 `routers-rs`，库名保持 `routers`：

```toml
[dependencies]
routers = { package = "routers-rs", version = "1.0.0" }
gpui-kit = "0.9.0"
```

组件同时提供 `routers::gpui_kit` 重导出。应用应让直接依赖和路由组件使用兼容的 GPUI Kit 版本，不要另行引入旧版 `gpui`。

## 快速上手

### 1. 创建并持有 Router

```rust
use routers::gpui_kit::{App, AppContext, Entity};
use routers::Router;

fn create_router(cx: &mut App) -> Entity<Router> {
    let router = cx.new(|_| Router::new());
    router.update(cx, |router, _| {
        router.register("/files");
        router.register("/docs/{*path}");
        router.register_element("/user/{id}", |route, _, _| {
            routers::gpui_kit::div()
                .child(format!("用户 {}", route.param("id").unwrap()))
                .into_any_element()
        });
    });
    router
}
```

`Router::new()` 使用无守卫、无持久化的默认委托。需要这些能力时改用 `Router::with_delegate`。

### 2. 导航与读取状态

```rust
router.update(cx, |router, cx| {
    assert!(router.navigate(cx, "/user/42"));
});

let state = router.read(cx);
assert_eq!(state.current().as_ref(), "/user/42");
assert_eq!(state.params().get("id").map(|id| id.as_ref()), Some("42"));
assert!(state.is_active("/user/{id}"));
```

导航失败只表示被守卫拒绝。失败时路径、参数、历史、事件和持久化均不变。

### 3. 在历史中移动

```rust
router.update(cx, |router, cx| {
    router.navigate(cx, "/files");
    router.navigate(cx, "/search");
    assert!(router.back(cx));     // 回到 /files
    assert!(router.forward(cx));  // 回到 /search
});
```

后退后导航到新路径会截断前进历史。导航到当前路径不会重复写入历史。`back_force`、`forward_force` 和 `navigate_force` 仅用于应用已经完成用户确认的场景。

### 4. 用 Outlet 渲染页面

```rust
use routers::Outlet;

impl Render for MainView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().child(Outlet::new(&self.router))
    }
}
```

元素工厂接收 `&Route`、`&mut Window` 和 `&mut App`：

```rust
router.update(cx, |router, _| {
    router.register_element("/files", |route, window, cx| {
        FilesPanel::new(route, window, cx).into_any_element()
    });
});
```

`register_element` 会同时注册路径模式。相同模式的重复注册替换旧工厂。未匹配到元素时，`Outlet` 渲染空内容。

### 5. 观察变化，不复制页面状态

```rust
struct MainView {
    router: Entity<Router>,
    _subscriptions: Vec<Subscription>,
}

impl MainView {
    fn new(router: Entity<Router>, cx: &mut Context<Self>) -> Self {
        let observation = cx.observe(&router, |_, _, cx| cx.notify());
        let event = cx.subscribe(&router, |_, _, event: &RouteChanged, _| {
            println!("进入 {}", event.pathname());
        });
        Self {
            router,
            _subscriptions: vec![observation, event],
        }
    }
}
```

`Outlet` 自身不订阅，也不保存当前页面。父视图观察同一个 `Entity<Router>` 即可。成功导航和恢复都会提交 `RouteChanged`；GPUI 可以在实体更新结束后再向订阅者投递事件。

### 6. 嵌套布局

```rust
router.update(cx, |router, _| {
    router.register_layout("/", |outlet| {
        div().child(NavBar::new()).child(outlet).into_any_element()
    });
    router.register_layout("/settings", |outlet| {
        div().child(SettingsSidebar::new()).child(outlet).into_any_element()
    });
});
```

当前路径为 `/settings/profile` 时，内容先由 `/settings` 布局包裹，再由 `/` 布局包裹。`/settings` 不会匹配 `/settings-account`。

### 7. 路由守卫

```rust
use routers::{RouterDelegate, RouteChanged};
use routers::gpui_kit::{App, SharedString};

struct AppDelegate;

impl RouterDelegate for AppDelegate {
    fn allow_navigate(
        &self,
        _cx: &mut App,
        _from: &SharedString,
        to: &SharedString,
    ) -> Result<bool, SharedString> {
        if to.starts_with("/admin") && !is_logged_in() {
            return Err("需要登录".into());
        }
        Ok(true)
    }
}

let router = cx.new(|_| Router::with_delegate(AppDelegate));
```

- `Ok(true)`：放行。
- `Ok(false)`：静默拒绝。
- `Err(reason)`：带原因拒绝，组件输出警告。

守卫只负责判定，不要在守卫内对同一个路由器再次调用导航。确认后由调用方使用 `navigate_force`。

### 8. 持久化与恢复

```rust
impl RouterDelegate for AppDelegate {
    fn persist_pathname(&self, _cx: &mut App, path: &SharedString) {
        settings.set_current_path(path.to_string());
    }

    fn restore_pathname(&self, _cx: &App) -> Option<SharedString> {
        settings.current_path().map(Into::into)
    }
}

router.update(cx, |router, cx| {
    router.restore(cx);
});
```

应在路由注册完成后、正常导航开始前恢复。恢复遵循以下顺序：

1. 从委托读取保存路径。
2. 若路由表非空，验证保存路径仍可匹配。
3. 更新参数和路径，将历史重置为恢复路径。
4. 提交 `RouteChanged` 和实体通知，不执行守卫，不重复持久化。

没有记录、空路径和根路径保持初始状态；失效路径安全回退 `/`。持久化钩子在导航提交事件和通知后同步执行，但 GPUI 的事件订阅回调可能稍后运行。重型 I/O 应由应用放入自己的后台写队列，路由组件不另建存储通道或线程。

## 选中态

```rust
router.read(cx).is_active("/user/{id}");
is_active_simple("/files/sub", "/files", false); // true
is_active_simple("/files2", "/files", false);    // false
is_active_simple("/files/sub", "/files", true);  // false
```

已注册目标使用实际命中的模式判定。未注册目标使用带段边界的前缀匹配；根路径 `/` 始终要求精确匹配。

## 缓存失效

- `register` 新增模式后，路径匹配缓存失效。
- `register_element` 新增或替换工厂后，元素匹配缓存失效，并同步注册路径模式。
- 缓存均在下一次读取或渲染时重建。
- 重复注册同一路径模式不会重复插入匹配器。

## 可运行示例

```bash
cargo run --example router_demo
```

示例覆盖静态和动态导航、参数、选中态、前进后退、守卫与强制导航、嵌套布局、事件以及临时目录中的路径恢复。示例窗口使用 GPUI Kit 组件和语义主题令牌。

## 验证

```bash
cargo fmt --all -- --check
cargo test --all-targets
cargo test --doc
cargo clippy --all-targets -- -D warnings
cargo build --example router_demo
```

测试覆盖纯匹配函数，以及基于 GPUI Kit `TestAppContext` 的实例隔离、动态导航、历史、守卫、事件、恢复、缓存失效和 Outlet 渲染。真实桌面窗口验收请运行上述示例。

## 从 0.x 迁移

| 0.x | 1.0 |
|---|---|
| `Router::init_*()` 和线程局部全局状态 | `cx.new(|_| Router::new())`，应用保存 `Entity<Router>` |
| `RouterEventBus` | `RouterDelegate` 负责守卫与持久化；通知改用 `RouteChanged` |
| `Router::navigate(cx, path)` 等静态调用 | `router.update(cx, \|router, cx\| router.navigate(cx, path))` |
| `Router::current()` / `params()` 全局读取 | `router.read(cx).current()` / `params()` |
| 页面工厂 `|window, cx|` | `|route, window, cx|`，参数从 `route` 读取 |
| `Outlet::new()` | `Outlet::new(&router)` |
| `reactive_graph` 信号 | GPUI 实体通知与事件 |

1.0 是有意的不兼容升级，迁移后不应保留手工转发路由事件或同步第二套页面状态的代码。

## 致谢

受 [@justjavac](https://github.com/justjavac) 的 [gpui-router](https://github.com/justjavac/gpui-router) 的路径匹配与 Outlet 思路启发。

## License

按你的选择采用以下任一许可证：

- [MIT License](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)
