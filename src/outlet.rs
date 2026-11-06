//! 当前路由内容的占位元素。

use crate::Router;
use gpui_kit::{App, Entity, IntoElement, RenderOnce, Window};

/// 渲染指定 [`Router`] 当前匹配的元素与嵌套布局。
///
/// `Outlet` 不拥有第二套路由状态，也不自行订阅变化。持有该元素的父视图应观察
/// 同一个 `Entity<Router>`，在路由更新后调用 `cx.notify()` 触发重渲染。
#[derive(gpui_kit::IntoElement)]
pub struct Outlet {
    router: Entity<Router>,
}

impl Outlet {
    /// 为应用持有的路由实例创建占位元素。
    pub fn new(router: &Entity<Router>) -> Self {
        Self {
            router: router.clone(),
        }
    }
}

impl RenderOnce for Outlet {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let content = self.router.read(cx).outlet_content();
        Router::render_outlet(content, window, cx)
    }
}
