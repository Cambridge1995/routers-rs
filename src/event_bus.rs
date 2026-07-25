//! 事件总线抽象:Router 的通知/守卫/持久化钩子(使用方实现)。
//!
//! 见 [`RouterEventBus`] 文档;默认实现 [`NoopEventBus`] 丢弃所有事件。

use gpui::{App, SharedString};

// ════════════════════════════════════════════════════════════════════════
// EventBus trait 抽象(方案 C:使用方实现)
// ════════════════════════════════════════════════════════════════════════

/// Router 依赖的事件总线抽象(使用方实现)。
///
/// Router 在 [`crate::Router::navigate`] / [`crate::Router::back`] / [`crate::Router::forward`] 时调用
/// [`RouterEventBus::publish_route_changed`],让使用方决定如何通知订阅者
/// (如转发到自己的 EventBus、gpui 原生 emit 等)。
///
/// 默认实现 [`NoopEventBus`] 丢弃所有事件——让 Router 不注入任何 bus 也能独立运行。
/// framework 提供基于其 EventBus 的实现;其他 gpui 应用可自定义。
///
/// ## 为什么用 trait 而非回调
///
/// - **可读性**:`impl RouterEventBus for MyBus` 比 `set_on_change(|cx, path| ...)`
///   更直白,新手一眼看懂。
/// - **类型安全**:trait 方法签名明确,避免闭包签名写错。
/// - **可扩展**:未来加新事件(如 `publish_route_will_change`),trait 加方法即可。
/// - **可测试**:trait 易 mock。
///
/// ## 性能
///
/// trait object(`Box<dyn RouterEventBus>`)动态分发单次开销 ~1ns,
/// navigate 非热路径(用户点击触发),性能与回调方案完全一致。
pub trait RouterEventBus: 'static {
    /// 发布路由变化事件(navigate/back/forward 时调用)。
    ///
    /// `cx` 让使用方能把事件转发到 gpui 全局状态(如 framework EventBus)。
    fn publish_route_changed(&self, cx: &mut App, path: SharedString);

    /// **路由守卫**:navigate/back/forward **之前**调用,判定是否允许导航。
    ///
    /// 返回值语义:
    /// - `Ok(true)` —— 放行,导航继续执行。
    /// - `Ok(false)` —— 静默取消(普通业务拒绝,如"未保存修改")。
    /// - `Err(msg)` —— 带错误信息取消(如"权限不足:需要登录"),msg 会 eprintln。
    ///
    /// **默认实现** `Ok(true)` —— 不覆盖 = 无守卫,所有导航放行。
    /// 现有 `NoopEventBus` / `FrameworkEventBus` 自动继承,**无需修改**。
    ///
    /// ## 使用方覆盖示例
    ///
    /// ### 场景 1:权限检查
    /// ```ignore
    /// fn allow_navigate(&self, _cx: &mut App, _from: SharedString, to: SharedString)
    ///     -> Result<bool, SharedString> {
    ///     if to.starts_with("/admin") && !is_logged_in() {
    ///         return Err("需要登录".into());
    ///     }
    ///     Ok(true)
    /// }
    /// ```
    ///
    /// ### 场景 2:未保存修改提示弹框
    /// ```ignore
    /// fn allow_navigate(&self, cx: &mut App, _from: SharedString, _to: SharedString)
    ///     -> Result<bool, SharedString> {
    ///     let dirty = SharedStates::global(cx).get::<EditorState>("editor").unwrap().read(cx).dirty;
    ///     if dirty { Ok(false) } else { Ok(true) }
    /// }
    /// // 调用侧:
    /// if !Router::navigate(cx, "/other") {
    ///     show_confirm_dialog("有未保存的修改,是否离开?",
    ///         on_yes: |cx| Router::navigate_force(cx, "/other"));
    /// }
    /// ```
    ///
    /// ## ⚠️ 重要:守卫内禁止调 Router::navigate/back/forward(会死循环)。
    /// 守卫只负责**判定**(返回 bool),不负责**执行**导航。
    fn allow_navigate(
        &self,
        _cx: &mut App,
        _from: SharedString,
        _to: SharedString,
    ) -> Result<bool, SharedString> {
        Ok(true)
    }

    /// **持久化钩子**:navigate/back/forward **成功后**调用(在
    /// [`publish_route_changed`](Self::publish_route_changed) 之后)。
    ///
    /// 使用方在此把 path 写盘(或写内存/数据库——routers 不关心存储介质,
    /// 本体保持零磁盘依赖)。navigate 是低频用户操作(点击触发),写盘开销可忽略。
    ///
    /// **默认实现** no-op —— 不覆盖 = 不持久化,`NoopEventBus` 自动继承,**无需修改**。
    fn persist_pathname(&self, _cx: &mut App, _path: &SharedString) {}

    /// **恢复钩子**:[`crate::Router::restore`] 时调用,返回上次持久化的 pathname。
    ///
    /// 返回 `None` = 无持久化记录(或读取失败),Router 保持默认 `"/"`。
    ///
    /// **默认实现** `None` —— 不覆盖 = 不恢复,`NoopEventBus` 自动继承,**无需修改**。
    fn restore_pathname(&self, _cx: &App) -> Option<SharedString> {
        None
    }
}

/// 默认实现:丢弃所有事件。
///
/// 让 Router 独立可用——使用方不实现 [`RouterEventBus`] 也能跑(只是没人收到通知)。
/// 适合独立项目(不需要事件通知)或测试场景。
pub struct NoopEventBus;
impl RouterEventBus for NoopEventBus {
    fn publish_route_changed(&self, _cx: &mut App, _path: SharedString) {
        // 故意空实现:丢弃事件。
    }
}
