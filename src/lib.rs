//! 为 GPUI Kit 桌面应用提供显式状态所有的路由组件。
//!
//! 应用通过 `Entity<Router>` 持有路由状态。路径、参数、历史、选中态和
//! `Outlet` 内容均从同一个实例读取，不需要线程局部全局单例或额外页面状态。
//!
//! ```no_run
//! use routers::gpui_kit::{App, AppContext, Entity, IntoElement, ParentElement};
//! use routers::{Outlet, Router};
//!
//! fn build(cx: &mut App) -> Entity<Router> {
//!     let router = cx.new(|_| Router::new());
//!     router.update(cx, |router, _| {
//!         router.register_element("/user/{id}", |route, _, _| {
//!             routers::gpui_kit::div()
//!                 .child(format!("用户 {}", route.param("id").unwrap()))
//!                 .into_any_element()
//!         });
//!     });
//!     router.update(cx, |router, cx| {
//!         router.navigate(cx, "/user/42");
//!     });
//!     router
//! }
//! ```

mod delegate;
mod matching;
mod outlet;
mod router;

pub use delegate::{NoopRouterDelegate, RouterDelegate};
pub use matching::is_active_simple;
pub use outlet::Outlet;
pub use router::{Route, RouteChanged, Router};

/// 与路由组件使用同一套 GPUI 类型，避免应用混入其他 GPUI 版本。
pub use gpui_kit;
