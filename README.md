# routers

[![Crates.io](https://img.shields.io/crates/v/routers-rs.svg)](https://crates.io/crates/routers-rs)
[![Documentation](https://docs.rs/routers-rs/badge.svg)](https://docs.rs/routers-rs)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](./LICENSE-MIT)

基于 [reactive_graph](https://crates.io/crates/reactive_graph) 的**响应式路由器**,为 [gpui](https://gpui.rs) 桌面应用而生。

把「当前显示哪个面板」建模成一个 URL pathname(`/files`、`/user/123`…)作为**单一事实来源**,点击导航只是 `Router::navigate(cx, "/path")` 一下,路由器自动匹配、提取参数、渲染对应内容——声明式路由的 React Router 体验,原生桌面性能。

```rust
Router::init_default();
Router::register("/user/{id}");
Router::register_element("/files", |w, cx| FilesPanel::render(w, cx));

Router::navigate(cx, "/user/42");
Router::params().get("id");  // → Some("42")

// 视图里一行接入,路由变化自动换内容:
div().child(Outlet::new())
```

## 功能一览

- 🧭 **命令式导航**:`navigate` / `navigate_force`,返回值告知是否被守卫拦截
- ⏪ **历史栈**:`back` / `forward` / `can_go_back` / `can_go_forward`,浏览器标准语义(back 后 navigate 自动截断 forward 历史,同路径不重复入栈)
- 🧩 **动态路由**:静态段、动态段 `{id}`、通配段 `{*splat}`,matchit Trie O(1) 匹配,`params()` 自动提取
- 🛡 **路由守卫**:`allow_navigate` 返回 `Ok(true)` 放行 / `Ok(false)` 静默取消 / `Err(msg)` 带错误取消,force 系列可绕过
- 💾 **持久化/恢复**:`persist_pathname` / `restore_pathname` trait 钩子,重启自动回到上次页面(带路由表校验)
- 🪆 **嵌套布局**:`register_layout` 按最长前缀自动层层包裹(类 React Router 嵌套路由)
- 🎯 **active 判定**:`Router::is_active`(matchit 精确语义)+ `is_active_simple`(前缀/精确,NavLink 语义)
- 🔌 **事件抽象**:`RouterEventBus` trait,通知方式由你决定;默认 `NoopEventBus` 零装配可跑
- 📦 **零业务依赖**:只依赖 gpui + reactive_graph + matchit,任何 gpui 应用可直接使用

## 与 gpui-router 的关系:创新点与技术优化

本项目受 [@justjavac](https://github.com/justjavac) 的 [gpui-router](https://github.com/justjavac/gpui-router) 启发(致谢见文末),在其「pathname 单一事实来源 + 声明式匹配 + matchit」的核心思想之上,针对桌面应用场景做了以下演进:

| 维度 | gpui-router | routers |
|------|-------------|---------|
| 状态承载 | gpui `Global` | **`RwSignal`(reactive_graph)**——细粒度响应式信号,状态核心不绑死 gpui 全局模型 |
| 历史栈 | 无(刻意极简) | **完整 history stack + back/forward**,桌面应用(IDE 类)用户期望的「后退」开箱即有 |
| 路由守卫 | 无 | **`allow_navigate` 三态守卫**(放行/静默取消/带错误取消)+ force 绕过系列 |
| 持久化 | 无 | **persist/restore 钩子**,重启恢复上次页面;恢复路径经路由表校验,失效自动回退 `/` |
| Trie 构建 | 每次匹配重建 matchit Router | **Trie 缓存**:注册时失效、首次匹配后零开销|
| 事件通知 | 耦合在 Global 更新 | **`RouterEventBus` trait 抽象**:转发到自己的 EventBus / gpui emit / 直接丢弃,使用方自选 |
| 嵌套布局 | 声明式 `<Route layout>` 树 | **注册表 + 最长前缀自动包裹**:`register_layout("/settings")` 即可,无需维护树形结构 |
| active 语义 | NavLink 前缀匹配 | **双语义**:已注册模式走 matchit 精确判定,未注册降级前缀匹配 |
| 测试 | 少量 | **39 个测试**,含基于 gpui `TestAppContext` 的真实窗口端到端集成测试 |

一句话:gpui-router 是「小而美」的声明式路由(≈700 行),routers 在此思想之上补齐了桌面应用需要的**历史栈、守卫、持久化、缓存**四块拼图,并把状态核心换成了响应式信号。

## 依赖

| crate | 版本 | 用途 |
|-------|------|------|
| `gpui` | 0.2.2 | UI 框架(`Window` / `App` / 元素体系) |
| `reactive_graph` | 0.2 | 响应式信号(leptos 官方独立 crate;桌面目标不拉 `web-sys`) |
| `matchit` | 0.8 | 路由匹配引擎(Trie,支持 `{id}` / `{*splat}`) |

`gpui` 与 `reactive_graph` 均从 routers 根部 re-export(`routers::gpui::*` / `routers::reactive_graph::*`),使用方只依赖本 crate 即可,避免版本错位。

## 安装

crate 名是 **`routers-rs`**(crates.io 上 `routers` 已被占用),lib 名仍是 `routers`,
代码里 `use routers::...` 不受影响。

```toml
[dependencies]
routers = { package = "routers-rs", git = "https://github.com/Cambridge1995/routers-rs" }
# 或(crates.io 发布后):
routers = { package = "routers-rs", version = "0.1.0" }
```

## 使用教程

### 1. 初始化

```rust
use routers::{Router, RouterEventBus};
use routers::gpui::{App, SharedString};

// 姿势一:默认 NoopEventBus(不发布任何事件,独立可跑)
Router::init_default();

// 姿势二:实现自己的 EventBus(推荐,完整能力)
struct MyBus;
impl RouterEventBus for MyBus {
    fn publish_route_changed(&self, cx: &mut App, path: SharedString) {
        // 转发到你自己的事件总线 / gpui emit / 日志……
        println!("navigated to {path}");
    }
}
Router::init_with(MyBus);
```

`init_*` 是幂等的(重复调用 no-op,以第一次为准)。

### 2. 注册路由 + 导航

```rust
// 注册模式(支持动态段 / 通配段),建议启动时一次性注册完
Router::register("/files");
Router::register("/user/{id}");      // 动态段
Router::register("/docs/{*path}");   // 通配段

// 导航
Router::navigate(cx, "/user/42");

// 读取当前路径与参数
Router::current();            // → "/user/42"
Router::params().get("id");   // → Some("42")
```

### 3. 历史栈(back / forward)

```rust
Router::navigate(cx, "/files");
Router::navigate(cx, "/search");

Router::can_go_back();     // → true
Router::back(cx);          // → 回到 /files
Router::forward(cx);       // → 回到 /search

// back 之后 navigate 新路径,forward 历史按浏览器语义截断
Router::back(cx);
Router::navigate(cx, "/settings");
Router::can_go_forward();  // → false
```

### 4. Outlet:自动渲染当前路由内容

```rust
// 每个路由注册一个内容工厂;支持动态模式(matchit 语义,静态优先)
Router::register_element("/files", |window, cx| FilesPanel::render(window, cx));
Router::register_element("/user/{id}", |window, cx| UserPanel::render(window, cx));
// 一条动态注册覆盖所有 /user/*:
// navigate("/user/42") → UserPanel,且 Router::params()["id"] == "42"
// (元素模式隐含 register,无需另行 Router::register)

// 视图 render 里一行接入,路由变化时 Outlet 自动渲染当前内容
impl Render for MainView {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child(Outlet::new())
    }
}
```

### 5. 嵌套布局(最长前缀自动包裹)

```rust
// 根布局:常驻(所有路径)
Router::register_layout("/", |outlet| {
    div().child(NavBar::new()).child(outlet).into_any_element()
});
// 仅 /settings/* 路径出现的子布局
Router::register_layout("/settings", |outlet| {
    div().child(SettingsSidebar::new()).child(outlet).into_any_element()
});

// navigate("/settings/profile") 自动渲染:
//   NavBar > SettingsSidebar > ProfilePanel
// 父布局不动,只换 Outlet 内容。
```

### 6. 路由守卫

```rust
impl RouterEventBus for MyBus {
    fn publish_route_changed(&self, _cx: &mut App, _path: SharedString) {}

    fn allow_navigate(&self, _cx: &mut App, _from: SharedString, to: SharedString)
        -> Result<bool, SharedString>
    {
        if to.starts_with("/admin") && !is_logged_in() {
            return Err("需要登录".into());  // 带错误信息取消
        }
        Ok(true)   // 放行;Ok(false) = 静默取消
    }
}

// 调用侧:navigate 返回 false = 被拦截,一切状态不变
if !Router::navigate(cx, "/admin/panel") {
    show_confirm_dialog("需要登录,仍要前往?", |cx| {
        Router::navigate_force(cx, "/admin/panel")  // force 系列跳过守卫
    });
}
```

### ⚠️ 守卫的禁忌:函数体内禁止再调导航

守卫开启时按 back / navigate **完全没问题**——守卫只做一次判定,拒绝就返回 `false`,一切状态不变。

禁忌是在 `allow_navigate` 的**函数体内部**再调 `Router::navigate` / `back` / `forward`:

```rust
// ❌ 错误示范:守卫里执行导航
fn allow_navigate(&self, cx: &mut App, from: SharedString, to: SharedString)
    -> Result<bool, SharedString>
{
    if to.starts_with("/admin") {
        Router::navigate(cx, "/login");  // ← 致命:这次 navigate 又触发守卫
        //   → 守卫里又 navigate → 又触发守卫 → …… 无限递归直到栈溢出
    }
    Ok(true)
}
```

调用链是:`back()` → 触发守卫 → 守卫里又调 `back()`/`navigate()` → 又触发守卫 → 死循环。

**正确分工**:守卫只负责**纯判定**(看 from/to,返回放行/拒绝);「拦截后转去别的页面」这类逻辑写在**调用侧**:

```rust
// ✅ 守卫外:navigate 返回 false 后再决定跳转
if !Router::navigate(cx, "/admin/panel") {
    Router::navigate(cx, "/login");  // 安全:此时不在守卫调用栈内
}
```

`back_force` / `forward_force` 与 `navigate_force` 一样跳过守卫。

### 7. 持久化与重启恢复

```rust
impl RouterEventBus for MyBus {
    fn publish_route_changed(&self, _cx: &mut App, _path: SharedString) {}

    // 导航成功后自动调用:写盘(或任何存储介质,routers 本体零磁盘依赖)
    fn persist_pathname(&self, _cx: &mut App, path: &SharedString) {
        let _ = std::fs::write("route.json", path.as_str());
    }

    // Router::restore 时调用:返回上次持久化的路径
    fn restore_pathname(&self, _cx: &App) -> Option<SharedString> {
        std::fs::read_to_string("route.json").ok().map(Into::into)
    }
}

// 启动时:init + register 完成后调用一次
Router::restore(cx);
```

恢复是**静默**的(不发布事件、不走守卫、history 重置为 `[恢复路径]`);若恢复路径在当前路由表无匹配(页面已删除),自动回退 `/` 并打印警告。

### 持久化是同步还是异步?

`persist_pathname` 的**调用**是同步的,但它只是一声**通知**——函数体里做不做 IO 由你决定。担心同步写盘拖慢主线程(如 Windows 杀软把偶发写盘拖到 ms 级),在 hook 里只做 `channel.send()`,IO 交给你自己的后台线程即可:

```rust
use std::sync::mpsc::{channel, Sender};

struct AsyncBus {
    tx: Sender<String>,  // 持久化请求经 channel 发给后台线程
}

impl AsyncBus {
    fn new() -> Self {
        let (tx, rx) = channel::<String>();
        // 你自己的读写线程:routers 只发消息,IO 全在这里
        std::thread::spawn(move || {
            while let Ok(path) = rx.recv() {
                let _ = std::fs::write("route.json", &path);  // 重 IO 不阻塞主线程
            }
        });
        Self { tx }
    }
}

impl RouterEventBus for AsyncBus {
    fn publish_route_changed(&self, _cx: &mut App, _path: SharedString) {}

    fn persist_pathname(&self, _cx: &mut App, path: &SharedString) {
        // 同步 hook 里只做一件事:发消息(ns 级),channel 保序,last-write-wins
        let _ = self.tx.send(path.to_string());
    }
}
```

**为什么 routers 不内置异步线程**:

- **存储介质未知**——写文件 / 数据库 / 网络是使用方的事,routers 替你建线程就违背了「零业务依赖」;
- **`restore_pathname` 天然同步**——启动时必须在首帧渲染前拿到路径,只能同步读;但它只在启动执行**一次**,一次小文件读(μs 级)完全无感;
- **频率现实**——navigate 是用户点击驱动的低频操作,同步写几十字节小文件(OS 缓存命中 ~10-100μs)远低于一帧预算;上面 channel 模式只是给「存储较重」的场景一个零成本规避姿势。

### 8. active 判定(导航高亮)

```rust
// matchit 精确语义:已注册模式用模式判定
Router::is_active("/user/{id}");   // 当前 /user/42 → true
Router::is_active("/files");       // 当前 /files/sub → false(段数不匹配)

// 前缀语义(类 NavLink 默认行为):无需注册,含段边界保护
routers::is_active_simple("/files/sub", "/files", false);  // → true
routers::is_active_simple("/files2", "/files", false);     // → false(段边界)
routers::is_active_simple("/files/sub", "/files", true);   // → false(exact)
```

### 完整可运行示例

```bash
cargo run --example router_demo
```

一个窗口看全所有能力:导航高亮、动态参数、back/forward、嵌套布局、守卫拦截/强制进入、持久化恢复(写系统临时目录,重启自动回到上次页面)。

## 测试

```bash
cargo test
```

39 个测试:12 个纯函数单测(`is_active_simple` / `build_matchit` / `is_prefix_of`)+ 27 个基于 gpui `TestAppContext` 的端到端集成测试(真实 App/Window 驱动 navigate / back / forward / 守卫 / 参数 / 持久化 / restore / Outlet 渲染)。

## 致谢

感谢 **[@justjavac](https://github.com/justjavac)(迷渡)** 的 [gpui-router](https://github.com/justjavac/gpui-router)——「pathname 单一事实来源 + 声明式匹配 + Outlet 占位 + matchit 引擎」的核心思想直接启发了本项目。routers 站在它的肩膀上,补齐了桌面应用所需的历史栈、路由守卫、持久化与缓存优化。

## License

采用 Rust 社区标准的双协议,由你任选其一:

- [MIT License](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)

(`MIT OR Apache-2.0`)
