# Changelog

本项目的所有显著变更都将记录在此文件。

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/),
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [0.1.1] - 2026-07-26

### 新增

- `register_element` 支持**动态路由模式**(0.1.0 只能精确路径):动态段 `{id}`、通配段 `{*splat}`,matchit 语义且静态模式优先;一条 `/user/{id}` 注册即可覆盖所有 `/user/*`
- `register_element` 隐含 `Router::register`:元素模式自动登记进路由模式表,动态路径的 `params()` 提取与 `is_active()` 判定无需重复注册
- 叶子元素匹配引擎升级为独立缓存 Trie(注册时失效、首次渲染重建,渲染路径零重复构建)

## [0.1.0] - 2026-07-26

首个版本。受 [gpui-router](https://github.com/justjavac/gpui-router) 启发,
在其「pathname 单一事实来源 + 声明式匹配 + matchit 引擎」思想之上,
补齐桌面应用所需的历史栈、路由守卫、持久化与缓存优化。

### 新增

- **核心**:`Router`(thread-local 全局单例),`RwSignal<SharedString>` 承载 pathname 作单一事实来源(基于 `reactive_graph`)
- **导航**:`navigate` / `navigate_force`,返回值标识是否被守卫拦截
- **历史栈**:`back` / `forward` / `back_force` / `forward_force` / `can_go_back` / `can_go_forward` / `history_len` / `cursor`,浏览器标准语义(back 后 navigate 截断 forward 历史,同路径不重复入栈)
- **动态路由**:`register` 注册模式,支持静态段 / 动态段 `{id}` / 通配段 `{*splat}`(matchit Trie,O(1) 匹配 + Trie 缓存,注册时自动失效重建)
- **参数提取**:`params()`,navigate / back / forward / restore 时自动填充
- **路由守卫**:`RouterEventBus::allow_navigate` 三态语义(`Ok(true)` 放行 / `Ok(false)` 静默取消 / `Err(msg)` 带错误取消),覆盖 navigate / back / forward
- **持久化**:`persist_pathname` / `restore_pathname` trait 钩子 + `Router::restore()` 静默恢复(不发布事件、不走守卫、history 重置;恢复路径经路由表校验,失效回退 `/`)
- **事件抽象**:`RouterEventBus` trait(通知方式由使用方决定),默认 `NoopEventBus` 零装配可跑
- **Outlet**:`Outlet::new()` 一行接入(`RenderOnce` + `IntoElement`),路由变化自动渲染当前内容;`register_element` 注册叶子内容工厂
- **嵌套布局**:`register_layout` 按最长前缀自动层层包裹(类 React Router 嵌套路由)
- **active 判定**:`Router::is_active`(matchit 精确语义)+ `is_active_simple`(前缀/精确,NavLink 语义,含段边界保护)
- **re-export**:`routers::gpui` / `routers::reactive_graph`,使用方只依赖本 crate
- **示例**:`examples/router_demo.rs`,一个窗口演示全部能力(导航高亮 / 动态参数 / back-forward / 嵌套布局 / 守卫拦截与强制进入 / 持久化恢复)
- **测试**:39 个测试——12 个纯函数单测 + 27 个基于 gpui `TestAppContext` 的端到端集成测试
- **文档**:README(功能对比 / 依赖 / 安装 / 八节使用教程 / 守卫禁忌 / 异步持久化配方)

[0.1.0]: https://github.com/Cambridge1995/routers-rs/releases/tag/v0.1.0
