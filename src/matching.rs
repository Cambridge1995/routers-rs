//! 路由匹配引擎封装:matchit Trie 构建 + 前缀/active 判定纯函数。

use gpui::SharedString;


/// 判断 `prefix` 是否是 `path` 的前缀(用于 Outlet 嵌套匹配)。
///
/// 规则(类 React Router):
/// - `"/"` 是所有路径的前缀(根布局总是匹配)。
/// - `"/settings"` 是 `/settings` 和 `/settings/profile` 的前缀。
/// - `"/settings"` **不是** `/settings-account` 的前缀(段边界)。
/// - `"/settings/"` 与 `"/settings"` 等价(尾斜杠忽略)。
pub(crate) fn is_prefix_of(prefix: &str, path: &str) -> bool {
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
/// 不暴露匹配到的模式字符串。为了让 [`crate::Router::is_active`] 能判断"当前路径匹配到哪个模式",
/// 我们把模式字符串本身作为 value 存入,匹配后用 `matched.value == target` 比较。
///
/// 失败的模式(语法错误/冲突)被跳过 + eprintln 警告(不 panic)。
pub(crate) fn build_matchit(patterns: &[SharedString]) -> matchit::Router<Box<str>> {
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
/// **与 [`crate::Router::is_active`] 的区别**:
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
