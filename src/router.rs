//! 显式实例路由状态、历史、匹配与 Outlet 渲染。

use crate::delegate::{NoopRouterDelegate, RouterDelegate};
use crate::matching::{build_matchit, is_active_simple, is_prefix_of};
use gpui_kit::{AnyElement, App, Context, Empty, EventEmitter, IntoElement, SharedString, Window};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

type ElementFactory = Rc<dyn Fn(&Route, &mut Window, &mut App) -> AnyElement>;
type LayoutFactory = Rc<dyn Fn(AnyElement) -> AnyElement>;

pub(crate) struct OutletContent {
    route: Route,
    leaf_factory: Option<ElementFactory>,
    layout_factories: Vec<(SharedString, LayoutFactory)>,
}

/// 当前路径及其动态参数的只读快照。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pathname: SharedString,
    params: HashMap<SharedString, SharedString>,
}

impl Route {
    /// 当前完整路径。
    pub fn pathname(&self) -> &SharedString {
        &self.pathname
    }

    /// 当前路径匹配到的全部动态参数。
    pub fn params(&self) -> &HashMap<SharedString, SharedString> {
        &self.params
    }

    /// 读取一个动态参数。
    pub fn param(&self, name: &str) -> Option<&SharedString> {
        self.params.get(name)
    }
}

/// 路由路径成功变化后由 `Router` 发出的事件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteChanged {
    route: Route,
}

impl RouteChanged {
    /// 变化后的路径和参数。
    pub fn route(&self) -> &Route {
        &self.route
    }

    /// 变化后的完整路径。
    pub fn pathname(&self) -> &SharedString {
        self.route.pathname()
    }
}

/// 应用显式持有的路由状态。
///
/// 通过 `cx.new(|_| Router::new())` 创建，并由应用保存返回的
/// `Entity<Router>`。路径、历史和参数只存在于该实例中；多个窗口可以持有互不干扰的实例。
pub struct Router {
    delegate: Box<dyn RouterDelegate>,
    pathname: SharedString,
    history: Vec<SharedString>,
    cursor: usize,
    patterns: Vec<SharedString>,
    matchit_cache: RefCell<Option<matchit::Router<Box<str>>>>,
    params: HashMap<SharedString, SharedString>,
    layouts: Vec<(SharedString, LayoutFactory)>,
    elements: Vec<(SharedString, ElementFactory)>,
    elements_trie: RefCell<Option<matchit::Router<usize>>>,
}

impl Default for Router {
    fn default() -> Self {
        Self::with_delegate(NoopRouterDelegate)
    }
}

impl EventEmitter<RouteChanged> for Router {}

impl Router {
    /// 创建不持久化、无守卫的路由器，初始路径为 `/`。
    pub fn new() -> Self {
        Self::default()
    }

    /// 创建使用应用委托的路由器。
    pub fn with_delegate<D: RouterDelegate>(delegate: D) -> Self {
        Self::with_boxed_delegate(Box::new(delegate))
    }

    /// 创建使用动态委托的路由器。
    pub fn with_boxed_delegate(delegate: Box<dyn RouterDelegate>) -> Self {
        Self {
            delegate,
            pathname: SharedString::from("/"),
            history: vec![SharedString::from("/")],
            cursor: 0,
            patterns: Vec::new(),
            matchit_cache: RefCell::new(None),
            params: HashMap::new(),
            layouts: Vec::new(),
            elements: Vec::new(),
            elements_trie: RefCell::new(None),
        }
    }

    /// 当前完整路径。
    pub fn current(&self) -> &SharedString {
        &self.pathname
    }

    /// 当前路径及参数快照。
    pub fn route(&self) -> Route {
        Route {
            pathname: self.pathname.clone(),
            params: self.params.clone(),
        }
    }

    /// 当前路径的动态参数。
    pub fn params(&self) -> &HashMap<SharedString, SharedString> {
        &self.params
    }

    /// 注册静态、动态 `{id}` 或通配 `{*splat}` 路由模式。
    ///
    /// 重复模式会被去重。每次新增模式都会使匹配缓存失效，下次读取时重建。
    pub fn register(&mut self, pattern: impl Into<SharedString>) {
        let pattern = pattern.into();
        if !self.patterns.contains(&pattern) {
            self.patterns.push(pattern);
            *self.matchit_cache.borrow_mut() = None;
        }
    }

    /// 注册一个路径模式对应的内容工厂，同时注册该模式本身。
    ///
    /// 相同模式的重复注册会替换旧工厂，并使元素匹配缓存失效。
    pub fn register_element<F>(&mut self, path: impl Into<SharedString>, factory: F)
    where
        F: Fn(&Route, &mut Window, &mut App) -> AnyElement + 'static,
    {
        let path = path.into();
        if let Some((_, existing)) = self
            .elements
            .iter_mut()
            .find(|(pattern, _)| pattern == &path)
        {
            *existing = Rc::new(factory);
        } else {
            self.elements.push((path.clone(), Rc::new(factory)));
        }
        *self.elements_trie.borrow_mut() = None;
        self.register(path);
    }

    /// 注册按路径段前缀匹配的父布局。
    ///
    /// 多层布局按路径从具体到宽泛的顺序包裹内容；`/settings` 不会误匹配
    /// `/settings-account`。
    pub fn register_layout<F>(&mut self, path: impl Into<SharedString>, factory: F)
    where
        F: Fn(AnyElement) -> AnyElement + 'static,
    {
        let path = path.into();
        if let Some((_, existing)) = self.layouts.iter_mut().find(|(prefix, _)| prefix == &path) {
            *existing = Rc::new(factory);
        } else {
            self.layouts.push((path, Rc::new(factory)));
        }
    }

    /// 从委托恢复上次路径。
    ///
    /// 应在注册完成后、正常导航开始前调用。恢复不执行守卫，也不重复持久化；
    /// 成功时历史重置为恢复路径，并发出 [`RouteChanged`] 和实体通知。
    /// 没有记录、记录为空或记录为 `/` 时保持初始状态；已注册路由但保存路径失效时回退 `/`。
    pub fn restore(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(saved) = self.delegate.restore_pathname(cx) else {
            return false;
        };
        if saved.as_ref().is_empty() || saved.as_ref() == "/" {
            return false;
        }
        if !self.patterns.is_empty() && !self.matches_registered_path(saved.as_ref()) {
            eprintln!("routers: 持久化路径 {saved:?} 在当前路由表无匹配，保持 \"/\"");
            return false;
        }

        self.update_params_for(&saved);
        self.history = vec![saved.clone()];
        self.cursor = 0;
        self.pathname = saved;
        self.finish_change(cx, false);
        true
    }

    /// 经守卫判定后导航到新路径。
    ///
    /// 成功时依次更新参数、历史和路径，发出 [`RouteChanged`]，通知实体观察者，
    /// 最后调用持久化委托。被守卫拒绝时不修改任何状态，也不发出通知或写入。
    pub fn navigate(&mut self, cx: &mut Context<Self>, path: impl Into<SharedString>) -> bool {
        self.navigate_inner(cx, path, true)
    }

    /// 跳过守卫导航，供用户在应用确认流程中明确确认后使用。
    pub fn navigate_force(
        &mut self,
        cx: &mut Context<Self>,
        path: impl Into<SharedString>,
    ) -> bool {
        self.navigate_inner(cx, path, false)
    }

    /// 经守卫判定后退一步；边界或拒绝时返回 `false`。
    pub fn back(&mut self, cx: &mut Context<Self>) -> bool {
        self.move_cursor(cx, -1, true)
    }

    /// 经守卫判定前进一步；边界或拒绝时返回 `false`。
    pub fn forward(&mut self, cx: &mut Context<Self>) -> bool {
        self.move_cursor(cx, 1, true)
    }

    /// 跳过守卫后退一步。
    pub fn back_force(&mut self, cx: &mut Context<Self>) -> bool {
        self.move_cursor(cx, -1, false)
    }

    /// 跳过守卫前进一步。
    pub fn forward_force(&mut self, cx: &mut Context<Self>) -> bool {
        self.move_cursor(cx, 1, false)
    }

    /// 是否可以后退。
    pub fn can_go_back(&self) -> bool {
        self.cursor > 0
    }

    /// 是否可以前进。
    pub fn can_go_forward(&self) -> bool {
        self.cursor + 1 < self.history.len()
    }

    /// 历史记录数量。
    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    /// 当前历史游标。
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// 判定导航目标的选中状态。
    ///
    /// 已注册目标使用匹配到的具体模式判定；未注册目标退化为带段边界的前缀匹配。
    pub fn is_active(&self, target: &str) -> bool {
        if self.patterns.iter().any(|pattern| pattern == target) {
            let mut cache = self.matchit_cache.borrow_mut();
            let trie = cache.get_or_insert_with(|| build_matchit(&self.patterns));
            return trie
                .at(self.pathname.as_ref())
                .is_ok_and(|matched| matched.value.as_ref() == target);
        }
        is_active_simple(&self.pathname, target, false)
    }

    pub(crate) fn outlet_content(&self) -> OutletContent {
        let leaf_factory = {
            let mut cache = self.elements_trie.borrow_mut();
            let trie = cache.get_or_insert_with(|| {
                let mut trie = matchit::Router::new();
                for (index, (pattern, _)) in self.elements.iter().enumerate() {
                    if let Err(error) = trie.insert(pattern.as_ref(), index) {
                        eprintln!("routers: 跳过无效元素路由 {pattern:?}: {error}");
                    }
                }
                trie
            });
            trie.at(self.pathname.as_ref())
                .ok()
                .map(|matched| Rc::clone(&self.elements[*matched.value].1))
        };

        let mut layout_factories: Vec<(SharedString, LayoutFactory)> = self
            .layouts
            .iter()
            .filter(|(prefix, _)| is_prefix_of(prefix, &self.pathname))
            .map(|(prefix, factory)| (prefix.clone(), Rc::clone(factory)))
            .collect();
        layout_factories.sort_by_key(|entry| std::cmp::Reverse(entry.0.len()));
        OutletContent {
            route: self.route(),
            leaf_factory,
            layout_factories,
        }
    }

    pub(crate) fn render_outlet(
        content: OutletContent,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let mut element = content.leaf_factory.map_or_else(
            || Empty.into_any_element(),
            |factory| factory(&content.route, window, cx),
        );
        for (_, factory) in content.layout_factories {
            element = factory(element);
        }
        element
    }

    fn navigate_inner(
        &mut self,
        cx: &mut Context<Self>,
        path: impl Into<SharedString>,
        check_guard: bool,
    ) -> bool {
        let path = path.into();
        if check_guard && !self.allow_navigate(cx, &path) {
            return false;
        }

        self.update_params_for(&path);
        self.push_history(path.clone());
        self.pathname = path;
        self.finish_change(cx, true);
        true
    }

    fn move_cursor(&mut self, cx: &mut Context<Self>, delta: isize, check_guard: bool) -> bool {
        let Some(new_cursor) = self.cursor.checked_add_signed(delta) else {
            return false;
        };
        if new_cursor >= self.history.len() {
            return false;
        }
        let path = self.history[new_cursor].clone();
        if check_guard && !self.allow_navigate(cx, &path) {
            return false;
        }

        self.cursor = new_cursor;
        self.update_params_for(&path);
        self.pathname = path;
        self.finish_change(cx, true);
        true
    }

    fn allow_navigate(&self, cx: &mut Context<Self>, path: &SharedString) -> bool {
        match self.delegate.allow_navigate(cx, &self.pathname, path) {
            Ok(true) => true,
            Ok(false) => false,
            Err(reason) => {
                eprintln!(
                    "routers: 从 {:?} 到 {:?} 的导航被守卫拒绝: {reason}",
                    self.pathname, path
                );
                false
            }
        }
    }

    fn finish_change(&self, cx: &mut Context<Self>, persist: bool) {
        cx.emit(RouteChanged {
            route: self.route(),
        });
        cx.notify();
        if persist {
            self.delegate.persist_pathname(cx, &self.pathname);
        }
    }

    fn matches_registered_path(&self, path: &str) -> bool {
        let mut cache = self.matchit_cache.borrow_mut();
        let trie = cache.get_or_insert_with(|| build_matchit(&self.patterns));
        trie.at(path).is_ok()
    }

    fn update_params_for(&mut self, path: &SharedString) {
        self.params.clear();
        if self.patterns.is_empty() {
            return;
        }

        let mut cache = self.matchit_cache.borrow_mut();
        let trie = cache.get_or_insert_with(|| build_matchit(&self.patterns));
        if let Ok(matched) = trie.at(path.as_ref()) {
            for (key, value) in matched.params.iter() {
                self.params.insert(key.into(), value.into());
            }
        }
    }

    fn push_history(&mut self, path: SharedString) {
        self.history.truncate(self.cursor + 1);
        if self.history.last() == Some(&path) {
            return;
        }
        self.history.push(path);
        self.cursor = self.history.len() - 1;
    }
}
