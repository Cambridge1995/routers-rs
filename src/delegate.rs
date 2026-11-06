//! 路由守卫与持久化委托。

use gpui_kit::{App, SharedString};

/// 由应用提供路由守卫和持久化行为。
///
/// 路由变化通知不使用本委托，而是通过 `Router` 发出的
/// [`crate::RouteChanged`] 事件传递。
pub trait RouterDelegate: 'static {
    /// 在导航、后退或前进修改状态之前判定是否放行。
    ///
    /// - `Ok(true)`：放行。
    /// - `Ok(false)`：静默拒绝。
    /// - `Err(reason)`：带原因拒绝，路由组件会输出警告。
    ///
    /// 实现只应做判定，不应在同一个 `Router` 上再次发起导航。
    fn allow_navigate(
        &self,
        _cx: &mut App,
        _from: &SharedString,
        _to: &SharedString,
    ) -> Result<bool, SharedString> {
        Ok(true)
    }

    /// 在成功导航提交 [`crate::RouteChanged`] 事件和实体通知后同步调用。
    ///
    /// GPUI 可以向订阅者延迟投递事件，因此本方法可能先于订阅回调执行。
    /// 恢复路径时不会调用本方法，避免启动时重复写入相同配置。
    fn persist_pathname(&self, _cx: &mut App, _path: &SharedString) {}

    /// 在 [`crate::Router::restore`] 时返回上次保存的路径。
    fn restore_pathname(&self, _cx: &App) -> Option<SharedString> {
        None
    }
}

/// 放行所有导航且不持久化路径的默认委托。
pub struct NoopRouterDelegate;

impl RouterDelegate for NoopRouterDelegate {}
