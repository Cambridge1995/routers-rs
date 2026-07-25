//! 响应式路由器:类前端 router 的视图切换抽象(独立于 framework,任何 gpui 应用可用)。
//!
//! ## 定位
//!
//! 把"当前显示哪个面板"建模成一个 pathname 字符串(如 `/files`、`/user/123`),
//! 作为应用的**单一事实来源**。配合 NavIcon(在 framework 里)的 `.to(path)` 绑定,
//! 实现「点击图标 = 改一个字符串 = 自动切换 active 态 + 触发主面板切换」。
//!
//! ## 核心能力
//!
//! - **响应式 pathname**:`RwSignal<SharedString, LocalStorage>` 承载,任意位置可读可写。
//! - **动态路由匹配**:经 [`matchit`] 引擎,支持 `/user/{id}` 动态段、`/files/{*path}` 通配段。
//! - **Trie 缓存**:`matchit::Router` 用 `RefCell<Option<...>>` 缓存,首次匹配后零开销。
//! - **history stack**:`navigate` 自动 push 历史,`back/forward` 浏览器标准语义。
//! - **EventBus trait 抽象**:使用方实现 [`RouterEventBus`] 决定如何通知,
//!   默认 [`NoopEventBus`] 丢弃事件(独立可用)。
//! - **持久化钩子**(2026-07-26):[`RouterEventBus::persist_pathname`] /
//!   [`RouterEventBus::restore_pathname`] 由使用方实现存取,配合 [`Router::restore`]
//!   实现「重启恢复上次页面」。routers 本体仍零磁盘依赖。
//!
//! ## 设计依据
//!
//! - 借鉴 gpui-router(React-Router 风格)的思想:声明式路由 + NavLink 自动 active。
//! - 底层用 [`reactive_graph::RwSignal`] 承载 pathname(已实测可在 gpui 主线程工作)。
//! - 事件通知用 trait 抽象(方案 C):可读性好,与 framework trait 风格一致,性能零损失。
//!
//! ## 边界(本轮不做的)
//!
//! - ❌ 不接管 DockArea(主面板切换由使用方订阅事件自行处理;
//!   启动期同步由使用方读 [`Router::current`] 自行处理,见 [`Router::restore`] 文档)。
//! - ❌ 不持久化 history stack(重启后 back/forward 从恢复页重新开始)。
//! - ❌ 单窗口模型(thread-local);多窗口需求出现时再升级为 SyncStorage + 全局 Owner。
//!
//! ## 快速上手
//!
//! ### 姿势 1:独立项目(用默认 NoopEventBus)
//!
//! ```ignore
//! use routers::Router;
//!
//! fn main() {
//!     Application::new().run(|cx| {
//!         Router::init_default();              // 用 NoopEventBus 初始化
//!         Router::register("/files");          // 注册路由模式
//!         Router::navigate(cx, "/files");      // 跳转
//!         let _params = Router::params();      // 读动态参数(本例为空)
//!     });
//! }
//! ```
//!
//! ### 姿势 2:动态路由 + history
//!
//! ```ignore
//! Router::init_default();
//! Router::register("/user/{id}");             // 动态段
//! Router::navigate(cx, "/user/123");
//! assert_eq!(Router::params().get("id"), Some(&"123".into()));
//!
//! Router::back(cx);     // 后退
//! Router::forward(cx);  // 前进
//! ```
//!
//! ### 姿势 3:实现自己的 EventBus
//!
//! ```ignore
//! use routers::{Router, RouterEventBus};
//!
//! struct MyBus;
//! impl RouterEventBus for MyBus {
//!     fn publish_route_changed(&self, cx: &mut gpui::App, path: gpui::SharedString) {
//!         println!("navigated to {path}");
//!     }
//! }
//! Router::init_with(MyBus);
//! ```

use gpui::{AnyElement, App, Empty, IntoElement, RenderOnce, SharedString, Window};
use reactive_graph::owner::{LocalStorage, Owner};
use reactive_graph::prelude::*; // 引入 Get/Set 等 trait 方法
use reactive_graph::signal::RwSignal;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;

// ════════════════════════════════════════════════════════════════════════
// Outlet 相关类型别名
// ════════════════════════════════════════════════════════════════════════

/// 叶子元素工厂:navigate 到某路径时,Outlet 调用它生成该路径的内容元素。
///
/// 用 `Rc` 而非 `Box` 因为 render_outlet 需要把工厂引用 clone 出 RefCell 借用范围
/// (避免生命周期问题)。单线程(Router 是 thread_local),Rc 安全。
type ElementFactory = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;

/// 父布局工厂:接收子内容(outlet 元素),返回包裹后的元素。
///
/// 用 `Rc` 同理。使用方决定"子内容放在父布局的哪个位置"。
type LayoutFactory = Rc<dyn Fn(AnyElement) -> AnyElement>;

// ════════════════════════════════════════════════════════════════════════
// EventBus trait 抽象(方案 C:使用方实现)
// ════════════════════════════════════════════════════════════════════════

/// Router 依赖的事件总线抽象(使用方实现)。
///
/// Router 在 [`Router::navigate`] / [`Router::back`] / [`Router::forward`] 时调用
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

    /// **恢复钩子**:[`Router::restore`] 时调用,返回上次持久化的 pathname。
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

// ════════════════════════════════════════════════════════════════════════
// Router 核心
// ════════════════════════════════════════════════════════════════════════

/// 响应式路由器:pathname 的单一事实来源 + history stack + 动态路由匹配。
///
/// 用 `RwSignal<SharedString, LocalStorage>` 承载当前 pathname(单线程绑定,
/// 适配 gpui 主线程模型)。`Owner` 管理信号生命周期,挂在全局 thread-local 上。
///
/// **不是 gpui Entity**——Router 是 thread-local 静态结构,通过 `Router::current()` 访问。
///
/// ## 内部状态
///
/// - `pathname`:当前路径(响应式,navigate 时自动通知订阅者)。
/// - `history` + `cursor`:历史栈 + 游标(浏览器标准 back/forward 语义)。
/// - `patterns`:已注册的路由模式(如 `/user/{id}`)。
/// - `matchit_cache`:matchit Trie 缓存(首次匹配后零开销)。
/// - `params`:当前路径匹配到的动态参数(如 `{"id": "123"}`)。
pub struct Router {
    /// reactive_graph Owner:管理 signal 的 arena 生命周期。
    _owner: Owner,
    /// 当前 pathname(单一事实来源)。
    pathname: RwSignal<SharedString, LocalStorage>,
    /// 事件总线(trait object,运行时多态)。
    event_bus: Box<dyn RouterEventBus>,
    /// 历史栈:navigate 时 push,back/forward 时移动 cursor。
    history: RefCell<Vec<SharedString>>,
    /// 当前游标:指向 history 中当前 pathname 的位置。
    cursor: RefCell<usize>,
    /// 已注册的路由模式(用于构建 matchit Trie)。
    patterns: RefCell<Vec<SharedString>>,
    /// matchit Trie 缓存:patterns 变化时清空,首次匹配后复用。
    /// 用 `Option` 而非 `OnceCell` 因为 register 后需要重建。
    matchit_cache: RefCell<Option<matchit::Router<Box<str>>>>,
    /// 当前路径的动态参数(navigate 时自动填充)。
    params: RefCell<HashMap<SharedString, SharedString>>,
    /// 父布局注册表:(路径前缀, 工厂)。Outlet 渲染时按最长前缀匹配逐层包裹。
    layouts: RefCell<Vec<(SharedString, LayoutFactory)>>,
    /// 叶子元素注册表:(精确路径, 工厂)。Outlet 渲染时找最具体的匹配。
    elements: RefCell<Vec<(SharedString, ElementFactory)>>,
}

// 全局 Router 实例(单窗口模型;多窗口需求出现时再升级为 SyncStorage + 全局 Owner)。
thread_local! {
    static ROUTER: OnceLock<Router> = OnceLock::new();
}

impl Router {
    /// 用指定 EventBus 实现初始化全局 Router(使用方传自己的 bus 实现)。
    ///
    /// 必须在主线程调用(gpui 单线程 UI 模型)。
    /// 重复调用是幂等的(第二次起 no-op,以第一次的 bus 为准)。
    ///
    /// 初始化后 history 为 `["/"]`,cursor 为 0。
    pub fn init_with<E: RouterEventBus>(event_bus: E) {
        Self::init_with_boxed(Box::new(event_bus));
    }

    /// 用 `Box<dyn RouterEventBus>` 初始化(高级用法,需动态分发时用)。
    pub fn init_with_boxed(event_bus: Box<dyn RouterEventBus>) {
        ROUTER.with(|slot| {
            slot.get_or_init(|| {
                let owner = Owner::new();
                let pathname = owner.with(|| RwSignal::new_local(SharedString::from("/")));
                Router {
                    _owner: owner,
                    pathname,
                    event_bus,
                    history: RefCell::new(vec![SharedString::from("/")]),
                    cursor: RefCell::new(0),
                    patterns: RefCell::new(Vec::new()),
                    matchit_cache: RefCell::new(None),
                    params: RefCell::new(HashMap::new()),
                    layouts: RefCell::new(Vec::new()),
                    elements: RefCell::new(Vec::new()),
                }
            });
        });
    }

    /// 用默认 [`NoopEventBus`] 初始化(独立项目用,不发布任何事件)。
    pub fn init_default() {
        Self::init_with(NoopEventBus);
    }

    /// 检查 Router 是否已启用。
    pub fn is_enabled() -> bool {
        ROUTER.with(|slot| slot.get().is_some())
    }

    /// 读取当前 pathname。
    ///
    /// ⚠️ 未启用时 panic。
    pub fn current() -> SharedString {
        ROUTER.with(|slot| slot.get().expect("Router 未启用").pathname.get())
    }

    /// 启动时恢复上次持久化的 pathname(重启恢复上次页面)。
    ///
    /// 在 `init_with` + 所有 [`register`](Self::register) 完成之后调用一次
    /// (framework 的 install 自动调用;独立项目自行调用)。
    ///
    /// ## 语义(刻意保守)
    ///
    /// - **静默恢复**:直接 set signal,**不 publish RouteChanged、不走 allow_navigate
    ///   守卫、不 push history**(history 重置为 `[restored]`,重启后 back/forward
    ///   从恢复页重新开始)。
    /// - **校验**:若已注册 patterns 非空且恢复路径 matchit 匹配失败(路由表已变,
    ///   页面被删)→ eprintln 警告 + 保持 `"/"`,不恢复。
    /// - params 照常填充(`update_params_for`),恢复后 [`Router::params`] 正确。
    ///
    /// ## 为什么静默(不 publish)
    ///
    /// restore 在开窗前执行(framework install),此时订阅者尚不存在,事件必然丢失。
    /// 主面板启动同步由使用方装配时读 [`Router::current`] 自行处理
    /// (「Router 不接管 DockArea」边界不变)。
    ///
    /// ⚠️ 未启用时 panic。
    pub fn restore(cx: &mut App) {
        ROUTER.with(|slot| {
            let Some(router) = slot.get() else {
                panic!("Router::restore 在未启用时调用(需先 Router::init_*)");
            };
            let event_bus_ptr: *const dyn RouterEventBus = &*router.event_bus;
            // SAFETY: 单线程,Router 存活期间 event_bus 不变。
            let event_bus: &dyn RouterEventBus = unsafe { &*event_bus_ptr };
            let Some(saved) = event_bus.restore_pathname(cx) else {
                return; // 使用方未实现持久化,或无记录:保持默认 "/"。
            };
            // 空串或根路径:无需恢复(初始即 "/")。
            if saved.as_ref().is_empty() || saved.as_ref() == "/" {
                return;
            }
            // 校验:已注册 patterns 非空且 matchit 匹配失败 → 路由表已变,回退 "/"。
            {
                let patterns = router.patterns.borrow();
                if !patterns.is_empty() {
                    let mut cache = router.matchit_cache.borrow_mut();
                    let trie = cache.get_or_insert_with(|| build_matchit(&patterns));
                    if trie.at(saved.as_ref()).is_err() {
                        eprintln!(
                            "routers: 持久化路径 {:?} 在当前路由表无匹配,保持 \"/\"",
                            saved
                        );
                        return;
                    }
                }
            }
            // 静默恢复:不 publish、不走守卫、history 重置为 [saved]。
            router.update_params_for(&saved);
            *router.history.borrow_mut() = vec![saved.clone()];
            *router.cursor.borrow_mut() = 0;
            router.pathname.set(saved);
        })
    }

    /// 注册路由模式(支持动态段 `{id}` / 通配段 `{*splat}`)。
    ///
    /// 必须在第一次 navigate 之前调用。注册后:
    /// - [`Router::navigate`] 时会尝试用 matchit 匹配并提取动态参数。
    /// - [`Router::is_active`] 用 matchit 语义判定(支持动态段)。
    ///
    /// ```ignore
    /// Router::register("/files");
    /// Router::register("/user/{id}");        // 动态段
    /// Router::register("/static/{*path}");   // 通配段
    /// ```
    ///
    /// **缓存失效**:每次 register 会清空 matchit Trie 缓存,下次匹配时重建。
    /// 所以建议在应用启动时一次性注册所有模式,避免运行期频繁 register。
    pub fn register(pattern: impl Into<SharedString>) {
        ROUTER.with(|slot| {
            if let Some(router) = slot.get() {
                let pattern = pattern.into();
                // 去重(避免重复注册导致 matchit insert 冲突)。
                let mut patterns = router.patterns.borrow_mut();
                if !patterns.iter().any(|p| p == &pattern) {
                    patterns.push(pattern);
                    // 清空 Trie 缓存,下次匹配时重建。
                    *router.matchit_cache.borrow_mut() = None;
                }
            } else {
                panic!("Router::register 在未启用时调用(需先 Router::init_*)");
            }
        });
    }

    /// 注册**叶子元素**:某具体路径对应的内容工厂。
    ///
    /// Outlet 渲染时,若当前路径**精确匹配**此注册,调用工厂生成元素。
    /// 用于"每个路由对应一个面板"场景。
    ///
    /// ```ignore
    /// Router::register_element("/files", |window, cx| FilesPanel::render(window, cx));
    /// Router::register_element("/search", |window, cx| SearchPanel::render(window, cx));
    /// ```
    ///
    /// 工厂签名 `Fn(&mut Window, &mut App) -> AnyElement`——接收 window + cx
    /// 因为面板通常需要它们构造(Entity/焦点等)。
    pub fn register_element<F>(path: impl Into<SharedString>, factory: F)
    where
        F: Fn(&mut Window, &mut App) -> AnyElement + 'static,
    {
        ROUTER.with(|slot| {
            if let Some(router) = slot.get() {
                let path = path.into();
                let mut elements = router.elements.borrow_mut();
                // 去重:同路径覆盖旧工厂。
                if let Some(existing) = elements.iter_mut().find(|(p, _)| p == &path) {
                    existing.1 = Rc::new(factory);
                } else {
                    elements.push((path, Rc::new(factory)));
                }
            } else {
                panic!("Router::register_element 在未启用时调用(需先 Router::init_*)");
            }
        });
    }

    /// 注册**父布局**:某路径前缀对应的布局工厂。
    ///
    /// Outlet 渲染时,所有"是当前路径前缀"的 layout 会**按最长前缀优先**逐层包裹
    /// (类 React Router 嵌套路由)。
    ///
    /// 工厂接收 `outlet: AnyElement`(当前子内容),使用方决定它放在父布局的哪个位置:
    /// ```ignore
    /// Router::register_layout("/", |outlet: AnyElement| -> AnyElement {
    ///     div()
    ///       .child(NavBar::new())   // 父布局常驻内容
    ///       .child(outlet)          // ← 子内容(Outlet 自动塞)
    ///       .into_any_element()
    /// });
    /// ```
    ///
    /// **嵌套示例**:`navigate("/settings/profile")` 时,
    /// 若注册了 `/` 和 `/settings` 两个 layout,渲染顺序(由内向外):
    /// `/settings/profile` element → `/settings` layout 包裹 → `/` layout 包裹。
    pub fn register_layout<F>(path: impl Into<SharedString>, factory: F)
    where
        F: Fn(AnyElement) -> AnyElement + 'static,
    {
        ROUTER.with(|slot| {
            if let Some(router) = slot.get() {
                let path = path.into();
                let mut layouts = router.layouts.borrow_mut();
                // 去重:同路径覆盖旧工厂。
                if let Some(existing) = layouts.iter_mut().find(|(p, _)| p == &path) {
                    existing.1 = Rc::new(factory);
                } else {
                    layouts.push((path, Rc::new(factory)));
                }
            } else {
                panic!("Router::register_layout 在未启用时调用(需先 Router::init_*)");
            }
        });
    }

    /// 渲染当前 Outlet 内容(Outlet 元素内部调用)。
    ///
    /// 算法(最长前缀匹配 + 由内向外包裹):
    /// 1. 在 `elements` 中找精确匹配当前路径的工厂,生成叶子元素;无匹配则用 `Empty`。
    /// 2. 找所有"是当前路径前缀"的 layout,按路径长度**降序**排序(长的=具体的在前)。
    /// 3. 逐个用 layout 工厂包裹:element = layout(outlet=element)。
    ///
    /// **若 Router 未启用**返回 Empty(避免 panic,Outlet 优雅降级)。
    pub(crate) fn render_outlet(window: &mut Window, cx: &mut App) -> AnyElement {
        // 第 1 步:从全局 Router 收集所需数据(leaf factory + layout factories 的 Rc clone)。
        // 用 Rc::clone 把工厂引用拿出 RefCell 借用范围,避免生命周期问题。
        let (leaf_factory, mut layout_factories): (
            Option<ElementFactory>,
            Vec<(SharedString, LayoutFactory)>,
        ) = ROUTER.with(|slot| {
            let Some(router) = slot.get() else {
                return (None, Vec::new());
            };
            let current = router.pathname.get();

            // 1. 找精确匹配的叶子工厂(clone Rc)。
            let leaf = router
                .elements
                .borrow()
                .iter()
                .find(|(p, _)| p == &current)
                .map(|(_, f)| Rc::clone(f));

            // 2. 收集所有前缀匹配的 layout(路径 + clone Rc)。
            let layouts: Vec<(SharedString, LayoutFactory)> = router
                .layouts
                .borrow()
                .iter()
                .filter(|(p, _)| is_prefix_of(p.as_ref(), current.as_ref()))
                .map(|(p, f)| (p.clone(), Rc::clone(f)))
                .collect();
            (leaf, layouts)
        });

        // 第 2 步:生成叶子元素(借用已结束,可安全调工厂)。
        let mut element: AnyElement = if let Some(factory) = leaf_factory {
            factory(window, cx)
        } else {
            Empty {}.into_any_element()
        };

        // 第 3 步:按"路径长度降序"逐层包裹。
        // 长的(具体的)在前 → 它先包裹 element(成为最内层 layout)。
        // 短的(抽象的)在后 → 它最后包裹(成为最外层)。
        layout_factories.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        for (_, factory) in layout_factories {
            element = factory(element);
        }
        element
    }

    /// 读取当前路径的动态参数(navigate 后自动填充)。
    ///
    /// 如 `register("/user/{id}")` + `navigate("/user/123")` → `params().get("id") == "123"`。
    ///
    /// 未注册任何模式或当前路径未匹配到时返回空 HashMap。
    pub fn params() -> HashMap<SharedString, SharedString> {
        ROUTER.with(|slot| slot.get().expect("Router 未启用").params.borrow().clone())
    }

    /// 跳转到指定 pathname。
    ///
    /// 流程(主线程同步执行):
    /// 1. **守卫检查**:调 `event_bus.allow_navigate(cx, from, to)`。
    ///    - 放行 → 继续;拒绝 → 返回 `false`,**什么也不做**(pathname/history/params 不变)。
    /// 2. **执行**(守卫放行后):
    ///    - matchit 匹配 + params 填充。
    ///    - history push(截断 forward 历史)。
    ///    - `signal.set(path)`。
    ///    - `event_bus.publish_route_changed(cx, path)`。
    ///
    /// 返回值:
    /// - `true` = 导航已执行(守卫放行)。
    /// - `false` = 导航被守卫取消(使用方可在业务代码里弹框,确认后调 [`navigate_force`](Self::navigate_force))。
    ///
    /// ⚠️ 未启用时 panic。
    pub fn navigate(cx: &mut App, path: impl Into<SharedString>) -> bool {
        Self::navigate_inner(cx, path, /*check_guard=*/ true)
    }

    /// 强制跳转,**跳过 allow_navigate 守卫**。
    ///
    /// 适用于"用户已在弹框里确认"场景(若不跳守卫,会再次触发守卫取消)。
    ///
    /// ⚠️ 未启用时 panic。
    pub fn navigate_force(cx: &mut App, path: impl Into<SharedString>) -> bool {
        Self::navigate_inner(cx, path, /*check_guard=*/ false)
    }

    /// navigate 内部实现(守卫检查可选,供 navigate / navigate_force 共用)。
    fn navigate_inner(cx: &mut App, path: impl Into<SharedString>, check_guard: bool) -> bool {
        let path = path.into();
        ROUTER.with(|slot| {
            let Some(router) = slot.get() else {
                panic!("Router::navigate 在未启用时调用(需先 Router::init_*)");
            };
            let from = router.pathname.get();

            // ① 守卫检查(可选)。
            if check_guard {
                let event_bus_ptr: *const dyn RouterEventBus = &*router.event_bus;
                let event_bus: &dyn RouterEventBus = unsafe { &*event_bus_ptr };
                match event_bus.allow_navigate(cx, from.clone(), path.clone()) {
                    Ok(true) => {}
                    Ok(false) => {
                        eprintln!("routers: navigate {:?} → {:?} 被 allow_navigate 守卫拒绝(Ok(false))", from, path);
                        return false;
                    }
                    Err(msg) => {
                        eprintln!("routers: navigate {:?} → {:?} 被 allow_navigate 守卫拒绝(Err({}))", from, path, msg);
                        return false;
                    }
                }
            }

            // ② 执行导航。
            router.update_params_for(&path);
            router.push_history(path.clone());
            router.pathname.set(path.clone());
            let event_bus_ptr: *const dyn RouterEventBus = &*router.event_bus;
            // SAFETY: 单线程,Router 存活期间 event_bus 不变。
            let event_bus: &dyn RouterEventBus = unsafe { &*event_bus_ptr };
            event_bus.publish_route_changed(cx, path.clone());
            // 持久化钩子(默认 no-op;使用方实现后此处写盘)。
            event_bus.persist_pathname(cx, &path);
            true
        })
    }

    /// 后退一步(若 cursor > 0)。返回是否成功。
    ///
    /// 成功时:cursor--,更新 pathname,**不截断 forward 历史**(可再 forward 回去)。
    /// 被守卫拒绝时返回 `false`(什么也不做)。
    pub fn back(cx: &mut App) -> bool {
        Self::move_cursor(cx, -1, /*check_guard=*/ true)
    }

    /// 前进一步(若 cursor < history.len()-1)。返回是否成功。
    ///
    /// 成功时:cursor++,更新 pathname,**不截断**。
    /// 被守卫拒绝时返回 `false`(什么也不做)。
    pub fn forward(cx: &mut App) -> bool {
        Self::move_cursor(cx, 1, /*check_guard=*/ true)
    }

    /// 强制后退,**跳过守卫**。
    pub fn back_force(cx: &mut App) -> bool {
        Self::move_cursor(cx, -1, /*check_guard=*/ false)
    }

    /// 强制前进,**跳过守卫**。
    pub fn forward_force(cx: &mut App) -> bool {
        Self::move_cursor(cx, 1, /*check_guard=*/ false)
    }

    /// 能否后退(cursor > 0)。
    pub fn can_go_back() -> bool {
        ROUTER.with(|slot| {
            slot.get()
                .map(|r| *r.cursor.borrow() > 0)
                .unwrap_or(false)
        })
    }

    /// 能否前进(cursor < history.len()-1)。
    pub fn can_go_forward() -> bool {
        ROUTER.with(|slot| {
            slot.get()
                .map(|r| {
                    let cursor = *r.cursor.borrow();
                    cursor + 1 < r.history.borrow().len()
                })
                .unwrap_or(false)
        })
    }

    /// history 长度(调试用)。
    pub fn history_len() -> usize {
        ROUTER.with(|slot| {
            slot.get()
                .map(|r| r.history.borrow().len())
                .unwrap_or(0)
        })
    }

    /// 当前游标位置(调试用)。
    pub fn cursor() -> usize {
        ROUTER.with(|slot| {
            slot.get()
                .map(|r| *r.cursor.borrow())
                .unwrap_or(0)
        })
    }

    /// 判定 target 是否匹配当前 pathname(用 matchit 语义)。
    ///
    /// 若 target 是已注册模式(经 [`Router::register`]),用 matchit 精确匹配:
    /// - `/user/{id}` 模式对当前 pathname `/user/123` → true。
    /// - `/user` 模式对 `/user/123` → false(段数不匹配)。
    ///
    /// 若 target 未注册,降级为前缀匹配(用 [`is_active_simple`])。
    ///
    /// ⚠️ 此方法是**纯读**(不修改 Router state),可在 render 时安全调用。
    pub fn is_active(target: &str) -> bool {
        ROUTER.with(|slot| {
            let Some(router) = slot.get() else {
                return false;
            };
            let current = router.pathname.get();
            let patterns = router.patterns.borrow();
            // 若 target 是已注册模式,用 matchit 匹配。
            if patterns.iter().any(|p| p == target) {
                let mut cache = router.matchit_cache.borrow_mut();
                let trie = cache.get_or_insert_with(|| build_matchit(&patterns));
                if let Ok(matched) = trie.at(&current) {
                    // 匹配到的模式(value 存的就是模式字符串)与 target 相同才算 active。
                    return matched.value.as_ref() == target;
                }
                return false;
            }
            // 未注册模式:降级为前缀匹配。
            drop(patterns);
            is_active_simple(&current, target, false)
        })
    }

    // ────────────────────────────────────────────────────────────────────
    // 内部辅助
    // ────────────────────────────────────────────────────────────────────

    /// 移动 cursor(back/forward 共用)。
    /// delta=-1 后退,+1 前进。check_guard=false 时跳过守卫。
    fn move_cursor(cx: &mut App, delta: isize, check_guard: bool) -> bool {
        ROUTER.with(|slot| {
            let Some(router) = slot.get() else {
                return false;
            };
            // 先算出 new_cursor(不修改 state,用于守卫检查的 to 参数)。
            let current_cursor = *router.cursor.borrow();
            let new_cursor_isize = (current_cursor as isize) + delta;
            if new_cursor_isize < 0 {
                return false;
            }
            let history_len = router.history.borrow().len();
            let new_cursor = new_cursor_isize as usize;
            if new_cursor >= history_len {
                return false;
            }
            // 取出 from / to(用于守卫检查)。
            let from = router.pathname.get();
            let to = router.history.borrow()[new_cursor].clone();

            // ① 守卫检查(可选)。
            if check_guard {
                let event_bus_ptr: *const dyn RouterEventBus = &*router.event_bus;
                let event_bus: &dyn RouterEventBus = unsafe { &*event_bus_ptr };
                match event_bus.allow_navigate(cx, from.clone(), to.clone()) {
                    Ok(true) => {}
                    Ok(false) => {
                        eprintln!(
                            "routers: {:?} back/forward {:?} → {:?} 被守卫拒绝(Ok(false))",
                            delta, from, to
                        );
                        return false;
                    }
                    Err(msg) => {
                        eprintln!(
                            "routers: {:?} back/forward {:?} → {:?} 被守卫拒绝(Err({}))",
                            delta, from, to, msg
                        );
                        return false;
                    }
                }
            }

            // ② 执行 cursor 移动。
            *router.cursor.borrow_mut() = new_cursor;
            router.update_params_for(&to);
            router.pathname.set(to.clone());

            // 通知订阅者。
            let event_bus_ptr: *const dyn RouterEventBus = &*router.event_bus;
            let event_bus: &dyn RouterEventBus = unsafe { &*event_bus_ptr };
            event_bus.publish_route_changed(cx, to.clone());
            // 持久化钩子(默认 no-op;使用方实现后此处写盘)。
            event_bus.persist_pathname(cx, &to);
            true
        })
    }

    /// 用 matchit 匹配 path,更新 params 字段(若未注册任何模式则清空 params)。
    ///
    /// 在 navigate/back/forward 时调用,确保 render 读 params 时拿到的是当前路径的参数。
    fn update_params_for(&self, path: &SharedString) {
        let patterns = self.patterns.borrow();
        if patterns.is_empty() {
            // 未注册任何模式:params 为空。
            self.params.borrow_mut().clear();
            return;
        }
        // 构建/复用 Trie 缓存。
        let mut cache = self.matchit_cache.borrow_mut();
        let trie = cache.get_or_insert_with(|| build_matchit(&patterns));
        // 匹配并提取参数。
        let mut params = self.params.borrow_mut();
        params.clear();
        if let Ok(matched) = trie.at(path.as_ref()) {
            for (key, value) in matched.params.iter() {
                params.insert(key.to_string().into(), value.to_string().into());
            }
        }
    }

    /// push 到 history(截断 cursor 之后的 forward 历史)。
    ///
    /// 这是浏览器标准语义:navigate 后,之前的 forward 历史被丢弃。
    fn push_history(&self, path: SharedString) {
        let mut history = self.history.borrow_mut();
        let mut cursor = self.cursor.borrow_mut();
        // 截断 cursor 之后的 forward 历史。
        history.truncate(*cursor + 1);
        // 若新路径与当前相同,不重复 push(避免 back/forward 死循环)。
        if history.last() == Some(&path) {
            return;
        }
        history.push(path);
        *cursor = history.len() - 1;
    }
}

/// 构建 matchit Trie(把 patterns 全部 insert,value 存模式字符串本身)。
///
/// **为什么 value 存模式字符串**:matchit 0.8 的 `Match.value` 是 insert 时存入的值,
/// 不暴露匹配到的模式字符串。为了让 [`Router::is_active`] 能判断"当前路径匹配到哪个模式",
/// 我们把模式字符串本身作为 value 存入,匹配后用 `matched.value == target` 比较。
///
/// 失败的模式(语法错误/冲突)被跳过 + eprintln 警告(不 panic)。
/// 判断 `prefix` 是否是 `path` 的前缀(用于 Outlet 嵌套匹配)。
///
/// 规则(类 React Router):
/// - `"/"` 是所有路径的前缀(根布局总是匹配)。
/// - `"/settings"` 是 `/settings` 和 `/settings/profile` 的前缀。
/// - `"/settings"` **不是** `/settings-account` 的前缀(段边界)。
/// - `"/settings/"` 与 `"/settings"` 等价(尾斜杠忽略)。
fn is_prefix_of(prefix: &str, path: &str) -> bool {
    if prefix == "/" {
        return true; // 根布局总是匹配。
    }
    let prefix = prefix.trim_end_matches('/');
    let path = path.trim_end_matches('/');
    if path == prefix {
        return true;
    }
    path.strip_prefix(prefix)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// 构建 matchit Trie(把 patterns 全部 insert,value 存模式字符串本身)。
///
/// **为什么 value 存模式字符串**:matchit 0.8 的 `Match.value` 是 insert 时存入的值,
/// 不暴露匹配到的模式字符串。为了让 [`Router::is_active`] 能判断"当前路径匹配到哪个模式",
/// 我们把模式字符串本身作为 value 存入,匹配后用 `matched.value == target` 比较。
///
/// 失败的模式(语法错误/冲突)被跳过 + eprintln 警告(不 panic)。
fn build_matchit(patterns: &[SharedString]) -> matchit::Router<Box<str>> {
    let mut trie = matchit::Router::new();
    for pattern in patterns {
        let value: Box<str> = pattern.as_ref().into();
        if let Err(e) = trie.insert(pattern.as_ref(), value) {
            eprintln!("routers: 跳过无效路由模式 {:?}: {}", pattern, e);
        }
    }
    trie
}

/// NavIcon active 判定:**前缀匹配**(类 React Router NavLink 默认语义)。
///
/// - `target == "/"` 或 `exact == true`:要求 pathname 与 target 完全相等。
/// - 否则:`/files` 在 `/files/sub` 时也算 active(前缀匹配 + 段边界)。
///
/// 段边界:`/files` 不会让 `/files-and-more` active(避免误判)。
///
/// **与 [`Router::is_active`] 的区别**:
/// - `is_active_simple`:字符串前缀匹配,简单直观,NavIcon 默认用这个。
/// - `Router::is_active`:matchit 精确匹配(支持动态段),需先 register 模式。
pub fn is_active_simple(current: &str, target: &str, exact: bool) -> bool {
    if target == "/" || exact {
        return current == target;
    }
    if current == target {
        return true;
    }
    current
        .strip_prefix(target)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

// ════════════════════════════════════════════════════════════════════════
// Outlet 元素(路由器自动管理的内容占位符)
// ════════════════════════════════════════════════════════════════════════

/// **Outlet**:路由器自动管理的"内容占位符"元素。
///
/// 它代表"当前路由应该显示的内容"。路径变化时(经父视图重渲染触发),
/// Outlet 自动 render 当前匹配的子元素(经 [`Router::render_outlet`]),
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

#[cfg(test)]
mod tests {
    //! 纯函数单测(不依赖 gpui App / 全局 Router)。
    //!
    //! navigate / back / forward / 守卫 / 持久化 / restore / Outlet 的**真实运行时行为**
    //! 由 `tests/e2e.rs`(gpui TestAppContext 端到端集成测试)覆盖,这里只保留
    //! 纯算法函数(`is_active_simple` / `build_matchit` / `is_prefix_of`)的单测。
    use super::*;

    // ══════════════════════════════════════════════════════════════════
    // is_active_simple(NavIcon 默认的前缀匹配语义)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_root_only_active_on_exact_root() {
        assert!(is_active_simple("/", "/", false));
        assert!(!is_active_simple("/files", "/", false));
    }

    #[test]
    fn test_prefix_match_includes_descendants() {
        assert!(is_active_simple("/files/sub", "/files", false));
        assert!(is_active_simple("/files/", "/files", false));
        assert!(is_active_simple("/files", "/files", false));
    }

    #[test]
    fn test_exact_match_strict() {
        assert!(is_active_simple("/files", "/files", true));
        assert!(!is_active_simple("/files/sub", "/files", true));
    }

    #[test]
    fn test_segment_boundary_not_false_positive() {
        assert!(!is_active_simple("/files-and-more", "/files", false));
        assert!(!is_active_simple("/files2", "/files", false));
    }

    // ══════════════════════════════════════════════════════════════════
    // build_matchit(动态路由匹配引擎)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_matchit_dynamic_segment_match() {
        let trie = build_matchit(&[
            "/home".into(),
            "/user/{id}".into(),
            "/files/{*path}".into(),
        ]);
        // 静态匹配。
        assert!(trie.at("/home").is_ok());
        // 动态段:匹配并提取参数。
        let matched = trie.at("/user/123").unwrap();
        assert_eq!(matched.params.get("id"), Some("123"));
        // 通配段:匹配多级路径。
        let matched = trie.at("/files/a/b/c").unwrap();
        assert_eq!(matched.params.get("path"), Some("a/b/c"));
    }

    #[test]
    fn test_matchit_value_stores_pattern_string() {
        // value 存模式字符串本身,用于 is_active 比较。
        let trie = build_matchit(&["/user/{id}".into()]);
        let matched = trie.at("/user/123").unwrap();
        assert_eq!(matched.value.as_ref(), "/user/{id}");
    }

    #[test]
    fn test_matchit_no_match_returns_err() {
        let trie = build_matchit(&["/home".into()]);
        assert!(trie.at("/nonexistent").is_err());
    }

    #[test]
    fn test_matchit_segment_count_must_match() {
        // /user/{id} 只匹配 1 段,/user/123/456 不匹配。
        let trie = build_matchit(&["/user/{id}".into()]);
        assert!(trie.at("/user/123").is_ok());
        assert!(trie.at("/user/123/456").is_err());
    }

    #[test]
    fn test_build_matchit_skips_invalid_pattern() {
        // 无效模式(语法错误)被跳过,不 panic。
        let trie = build_matchit(&["/valid".into(), "{invalid".into()]);
        assert!(trie.at("/valid").is_ok());
    }

    // ══════════════════════════════════════════════════════════════════
    // is_prefix_of(Outlet 嵌套 layout 的前缀匹配语义)
    // ══════════════════════════════════════════════════════════════════

    /// 根布局 "/" 总是匹配。
    #[test]
    fn test_is_prefix_of_root_matches_all() {
        assert!(is_prefix_of("/", "/"));
        assert!(is_prefix_of("/", "/files"));
        assert!(is_prefix_of("/", "/settings/profile"));
    }

    /// 具体前缀匹配。
    #[test]
    fn test_is_prefix_of_concrete_prefix() {
        assert!(is_prefix_of("/settings", "/settings"));
        assert!(is_prefix_of("/settings", "/settings/profile"));
        assert!(is_prefix_of("/settings", "/settings/"));
    }

    /// 段边界(避免 /settings 匹配 /settings-account)。
    #[test]
    fn test_is_prefix_of_segment_boundary() {
        assert!(!is_prefix_of("/settings", "/settings-account"));
        assert!(!is_prefix_of("/settings", "/settings2"));
        assert!(!is_prefix_of("/user", "/users"));
    }
}
