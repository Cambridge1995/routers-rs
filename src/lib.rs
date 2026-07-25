//! routers:基于 reactive_graph 的响应式路由器(独立于 framework,任何 gpui 应用可用)。
//!
//! 把"当前显示哪个面板"建模成 pathname 字符串作为单一事实来源,
//! 配合 NavIcon(framework 侧)的 `.to(path)` 实现自动 active 态切换。
//!
//! ## 设计亮点
//!
//! - **零外部业务依赖**:只依赖 `reactive_graph` + `gpui`(中立的第三方)。
//! - **EventBus trait 抽象**:使用方实现 [`RouterEventBus`] trait 决定如何通知,
//!   默认 [`NoopEventBus`] 丢弃事件(独立可用)。
//! - **性能**:navigate 非热路径(用户点击触发),trait object 动态分发开销 ~1ns,无感知。
//!
//! ## 快速上手
//!
//! ```ignore
//! use routers::{Router, RouterEventBus};
//!
//! // 姿势 1:用默认 NoopEventBus(不发布任何事件)
//! Router::init_default();
//! Router::navigate(cx, "/files");
//!
//! // 姿势 2:实现自己的 EventBus
//! struct MyBus;
//! impl RouterEventBus for MyBus {
//!     fn publish_route_changed(&self, cx: &mut gpui::App, path: gpui::SharedString) {
//!         println!("navigated to {path}");
//!     }
//! }
//! Router::init_with(MyBus);
//! ```

mod router;

pub use router::{Router, RouterEventBus, NoopEventBus, Outlet, is_active_simple};

// 统一 re-export,使用方只依赖本 crate,避免版本错位。
// 使用方经 `routers::gpui::*` / `routers::reactive_graph::*` 访问。
pub use gpui;
pub use reactive_graph;
