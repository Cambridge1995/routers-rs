//! Outlet:路由器自动管理的内容占位符元素。

use crate::Router;
use gpui::{App, IntoElement, RenderOnce, Window};

// ════════════════════════════════════════════════════════════════════════
// Outlet 元素(路由器自动管理的内容占位符)
// ════════════════════════════════════════════════════════════════════════

/// **Outlet**:路由器自动管理的"内容占位符"元素。
///
/// 它代表"当前路由应该显示的内容"。路径变化时(经父视图重渲染触发),
/// Outlet 自动 render 当前匹配的子元素(经 [`crate::Router::render_outlet`]),
/// **父布局不动**。
///
/// ## 工作原理
///
/// Outlet 是 gpui [`RenderOnce`] 元素。它**不自己订阅路由变化事件**,
/// 而是随父视图重渲染——navigate 时 framework MainView 已订阅 RouteChanged
/// 触发 `cx.notify()` → 父视图重渲染 → Outlet.render() 被调用 → 拿最新内容。
///
/// ## 使用示例
///
/// ### 场景 1:简单父布局 + Outlet(IDE 经典布局)
///
/// ```ignore
/// // Plugin::build 里注册根布局:
/// Router::register_layout("/", |outlet: AnyElement| -> AnyElement {
///     div()
///       .child(NavBar::new())    // 左侧导航(常驻)
///       .child(outlet)           // ← 子内容(Outlet 自动塞)
///       .into_any_element()
/// });
/// Router::register_element("/files", |w, cx| FilesPanel::render(w, cx));
///
/// // 主视图 render:
/// impl Render for MainView {
///     fn render(&mut self, w, cx) -> impl IntoElement {
///         Outlet::new()   // ★ 一行接入
///     }
/// }
/// ```
///
/// ### 场景 2:多层嵌套
///
/// ```ignore
/// Router::register_layout("/", |o| TitleBar::new().child(o).into());
/// Router::register_layout("/settings", |o| SettingsSidebar::new().child(o).into());
/// Router::register_element("/settings/profile", |w, cx| ProfilePanel::render(w, cx));
/// // navigate("/settings/profile") 自动渲染:
/// // TitleBar > SettingsSidebar > ProfilePanel
/// ```
#[derive(gpui::IntoElement)]
pub struct Outlet;

impl Outlet {
    /// 创建一个 Outlet 元素。
    ///
    /// 放进任意父布局或主视图,路由变化时自动 render 当前子内容。
    pub fn new() -> Self {
        Outlet
    }
}

impl Default for Outlet {
    fn default() -> Self {
        Outlet
    }
}

impl RenderOnce for Outlet {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        // 委托给 Router::render_outlet,获取当前匹配的内容。
        // 若 Router 未启用,render_outlet 返回 Empty(优雅降级)。
        Router::render_outlet(window, cx)
    }
}
