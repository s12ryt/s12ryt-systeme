//! 依賴圖: 從解析後的單元文件提取 [Unit] 段依賴聲明並構建雙向索引圖。
//!
//! 模仿真 systemd 的語義:
//! - `After=`/`Before=` 為純排序邊 (不拉起); `Before=` 聲明反轉為目標的 After 邊
//! - `Requires=`/`Wants=`/`Requisite=`/`BindsTo=`/`PartOf=`/`Conflicts=`/`Upholds=` 為依賴邊
//! - `Conflicts` 對稱 (A Conflicts=B ⇒ B 衝突 A)
//! - 反向聲明 `RequiredBy=`/`WantedBy=`/`BoundBy=`/`ConflictedBy=`/`UpheldBy=`
//!   (B RequiredBy=A ≡ A Requires=B)
//! - 依賴值 `-` 前綴 = 容忍失敗 (ignore_failure)
//! - 被引用但未定義的單元自動生成佔位節點

use std::collections::{BTreeMap, BTreeSet};

use systeme_units::parser::ParsedUnit;

/// 依賴種類 (供查詢/事務引擎使用)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DepKind {
    Requires,
    Wants,
    Requisite,
    BindsTo,
    PartOf,
    Conflicts,
    Upholds,
}

/// 依賴目標: 單元名 + `-` 前綴修飾 (容忍失敗)。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DepTarget {
    pub name: String,
    /// `Requires=-foo.service` 中的 `-`: 依賴失敗不阻擋本單元。
    pub ignore_failure: bool,
}

impl DepTarget {
    pub fn parse(raw: &str) -> Self {
        match raw.strip_prefix('-') {
            Some(rest) => Self {
                name: rest.to_string(),
                ignore_failure: true,
            },
            None => Self {
                name: raw.to_string(),
                ignore_failure: false,
            },
        }
    }

    pub fn plain(name: &str) -> Self {
        Self {
            name: name.to_string(),
            ignore_failure: false,
        }
    }
}

impl From<&str> for DepTarget {
    fn from(raw: &str) -> Self {
        Self::parse(raw)
    }
}

/// 正向依賴聲明 (單元文件 [Unit] 段直接寫出的內容)。
#[derive(Debug, Default, Clone)]
pub struct DirectDeps {
    /// 本單元需排在其後啟動的單元 (純排序)。
    pub after: BTreeSet<String>,
    /// 本單元需排在其之前啟動的單元 (聲明語法; 建圖時同時反轉寫入目標的 after)。
    pub before: BTreeSet<String>,
    pub requires: BTreeSet<DepTarget>,
    pub wants: BTreeSet<DepTarget>,
    pub requisite: BTreeSet<DepTarget>,
    pub binds_to: BTreeSet<DepTarget>,
    pub part_of: BTreeSet<DepTarget>,
    /// 衝突 (建圖時對稱填充到目標)。
    pub conflicts: BTreeSet<DepTarget>,
    pub upholds: BTreeSet<DepTarget>,
}

/// 反向依賴索引 (由圖構建時自動維護, 不可直接聲明)。
#[derive(Debug, Default, Clone)]
pub struct ReverseDeps {
    /// 誰 Requires 本單元。
    pub required_by: BTreeSet<String>,
    /// 誰 Wants 本單元。
    pub wanted_by: BTreeSet<String>,
    /// 誰 Requisite 本單元。
    pub requisite_of: BTreeSet<String>,
    /// 誰 BindsTo 本單元。
    pub bound_by: BTreeSet<String>,
    /// 誰 PartOf 本單元 (停止/重啟傳播給它們)。
    pub consists_of: BTreeSet<String>,
    /// 誰 Upholds 本單元。
    pub upheld_by: BTreeSet<String>,
    /// 哪些單元聲明瞭 After=本單元 (即它們排在本單元之後)。
    pub units_after_me: BTreeSet<String>,
    /// 哪些單元聲明瞭 Before=本單元 (即它們排在本單元之前)。
    pub units_before_me: BTreeSet<String>,
}

/// 圖節點。
#[derive(Debug, Default, Clone)]
pub struct UnitNode {
    pub name: String,
    pub deps: DirectDeps,
    pub rdeps: ReverseDeps,
}

/// 依賴圖。
#[derive(Debug, Default, Clone)]
pub struct DependencyGraph {
    nodes: BTreeMap<String, UnitNode>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// 所有節點名 (有序)。
    pub fn unit_names(&self) -> Vec<String> {
        self.nodes.keys().cloned().collect()
    }

    pub fn node(&self, name: &str) -> Option<&UnitNode> {
        self.nodes.get(name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.nodes.contains_key(name)
    }

    /// 取或建佔位節點。
    fn entry(&mut self, name: &str) -> &mut UnitNode {
        self.nodes.entry(name.to_string()).or_insert_with(|| UnitNode {
            name: name.to_string(),
            ..UnitNode::default()
        })
    }

    /// 從解析後的單元文件添加/覆蓋一個單元的依賴聲明。
    ///
    /// 同時處理正向 (After/Requires/...) 與反向聲明 (RequiredBy/WantedBy/...)。
    /// 冪等: 重複 add 同名單元會先清除舊正向邊 (含對目標反向索引的清理) 再重寫。
    /// 值應已由上層完成說明符展開 (本層只吃純字符串)。
    pub fn add_unit(&mut self, name: &str, parsed: &ParsedUnit) {
        let deps = Self::extract(name, parsed);
        self.set_unit_deps(name, deps);
        self.add_reverse_declarations(name, parsed);
    }

    /// 直接注入依賴 (程序化構建/測試用)。
    pub fn set_unit_deps(&mut self, name: &str, deps: DirectDeps) {
        // 冪等覆蓋: 清除舊正向邊在目標上的反向痕跡。
        if let Some(old) = self.nodes.get(name) {
            let old = old.clone();
            self.clear_forward_traces(name, &old.deps);
        }

        // Before= 聲明反轉: A Before=B ⇒ B 獲得 after 邊 A + units_before_me 索引。
        for target in &deps.before {
            let t = self.entry(target);
            t.deps.after.insert(name.to_string());
            t.rdeps.units_before_me.insert(name.to_string());
        }

        // After= 邊反向索引: A After=B ⇒ B.rdeps.units_after_me += A。
        for target in &deps.after {
            let t = self.entry(target);
            t.rdeps.units_after_me.insert(name.to_string());
        }

        // 依賴集合 → 目標反向索引。
        self.link_set(name, &deps.requires, |t| &mut t.rdeps.required_by);
        self.link_set(name, &deps.wants, |t| &mut t.rdeps.wanted_by);
        self.link_set(name, &deps.requisite, |t| &mut t.rdeps.requisite_of);
        self.link_set(name, &deps.binds_to, |t| &mut t.rdeps.bound_by);
        self.link_set(name, &deps.part_of, |t| &mut t.rdeps.consists_of);
        self.link_set(name, &deps.upholds, |t| &mut t.rdeps.upheld_by);

        // conflicts 對稱。
        for target in &deps.conflicts {
            let t = self.entry(&target.name);
            t.deps.conflicts.insert(DepTarget::plain(name));
        }

        // 寫回本節點完整 deps。
        let node = self.entry(name);
        node.deps = deps;
    }
    fn clear_forward_traces(&mut self, name: &str, old: &DirectDeps) {
        for t in &old.requires {
            if let Some(n) = self.nodes.get_mut(&t.name) {
                n.rdeps.required_by.remove(name);
            }
        }
        for t in &old.wants {
            if let Some(n) = self.nodes.get_mut(&t.name) {
                n.rdeps.wanted_by.remove(name);
            }
        }
        for t in &old.requisite {
            if let Some(n) = self.nodes.get_mut(&t.name) {
                n.rdeps.requisite_of.remove(name);
            }
        }
        for t in &old.binds_to {
            if let Some(n) = self.nodes.get_mut(&t.name) {
                n.rdeps.bound_by.remove(name);
            }
        }
        for t in &old.part_of {
            if let Some(n) = self.nodes.get_mut(&t.name) {
                n.rdeps.consists_of.remove(name);
            }
        }
        for t in &old.upholds {
            if let Some(n) = self.nodes.get_mut(&t.name) {
                n.rdeps.upheld_by.remove(name);
            }
        }
        for t in &old.after {
            if let Some(n) = self.nodes.get_mut(t) {
                n.rdeps.units_after_me.remove(name);
            }
        }
        for t in &old.before {
            if let Some(n) = self.nodes.get_mut(t) {
                n.deps.after.remove(name);
                n.rdeps.units_before_me.remove(name);
            }
        }
        for t in &old.conflicts {
            if let Some(n) = self.nodes.get_mut(&t.name) {
                n.deps.conflicts.remove(&DepTarget::plain(name));
            }
        }
    }

    fn link_set<F>(&mut self, name: &str, targets: &BTreeSet<DepTarget>, idx: F)
    where
        F: Fn(&mut UnitNode) -> &mut BTreeSet<String>,
    {
        for t in targets {
            let n = self.entry(&t.name);
            idx(n).insert(name.to_string());
        }
    }

    /// 從 [Unit] 段提取依賴聲明。
    fn extract(name: &str, parsed: &ParsedUnit) -> DirectDeps {
        let _ = name;
        let section = match parsed.section("Unit") {
            Some(s) => s,
            None => return DirectDeps::default(),
        };

        // systemd 依賴列表語義: 每行可含多個空白分隔的單元名。
        let strs = |key: &str| -> Vec<String> {
            section
                .get_list(key)
                .into_iter()
                .flat_map(|v| v.split_whitespace().map(|s| s.to_string()).collect::<Vec<_>>())
                .filter(|v| !v.is_empty())
                .collect()
        };

        DirectDeps {
            after: strs("After").into_iter().collect(),
            before: strs("Before").into_iter().collect(),
            requires: strs("Requires").into_iter().map(|v| DepTarget::parse(&v)).collect(),
            wants: strs("Wants").into_iter().map(|v| DepTarget::parse(&v)).collect(),
            requisite: strs("Requisite").into_iter().map(|v| DepTarget::parse(&v)).collect(),
            binds_to: strs("BindsTo").into_iter().map(|v| DepTarget::parse(&v)).collect(),
            part_of: strs("PartOf").into_iter().map(|v| DepTarget::parse(&v)).collect(),
            conflicts: strs("Conflicts").into_iter().map(|v| DepTarget::parse(&v)).collect(),
            upholds: strs("Upholds").into_iter().map(|v| DepTarget::parse(&v)).collect(),
        }
    }

    // ---------- 反向聲明 (RequiredBy= 等) ----------

    /// 處理反向聲明鍵 (RequiredBy/WantedBy/BoundBy/ConflictedBy/UpheldBy)。
    /// B 聲明 RequiredBy=A 等價於 A 的文件寫 Requires=B。
    fn add_reverse_declarations(&mut self, name: &str, parsed: &ParsedUnit) {
        let Some(section) = parsed.section("Unit") else {
            return;
        };

        // 同依賴列表語義: 空白分隔多值。
        let strs = |key: &str| -> Vec<String> {
            section
                .get_list(key)
                .into_iter()
                .flat_map(|v| v.split_whitespace().map(|s| s.to_string()).collect::<Vec<_>>())
                .filter(|v| !v.is_empty())
                .collect()
        };

        for src in strs("RequiredBy") {
            let n = self.entry(&src);
            n.deps.requires.insert(DepTarget::plain(name));
            let me = self.entry(name);
            me.rdeps.required_by.insert(src);
        }
        for src in strs("WantedBy") {
            let n = self.entry(&src);
            n.deps.wants.insert(DepTarget::plain(name));
            let me = self.entry(name);
            me.rdeps.wanted_by.insert(src);
        }
        for src in strs("BoundBy") {
            let n = self.entry(&src);
            n.deps.binds_to.insert(DepTarget::plain(name));
            let me = self.entry(name);
            me.rdeps.bound_by.insert(src);
        }
        for src in strs("UpheldBy") {
            let n = self.entry(&src);
            n.deps.upholds.insert(DepTarget::plain(name));
            let me = self.entry(name);
            me.rdeps.upheld_by.insert(src);
        }
        for src in strs("ConflictedBy") {
            // 衝突對稱
            let n = self.entry(&src);
            n.deps.conflicts.insert(DepTarget::plain(name));
            let me = self.entry(name);
            me.deps.conflicts.insert(DepTarget::plain(&src));
        }
    }

    // ---------- 查詢 ----------

    /// After 傳遞閉包: 本單元之後啟動所需知道的「先於本單元」的全體。
    /// A After=B, B After=C ⇒ closure(A) = {B, C}。防環。
    pub fn after_closure(&self, name: &str) -> BTreeSet<String> {
        let mut visited = BTreeSet::new();
        let mut stack: Vec<String> = Vec::new();
        if let Some(n) = self.nodes.get(name) {
            stack.extend(n.deps.after.iter().cloned());
        }
        while let Some(cur) = stack.pop() {
            if !visited.insert(cur.clone()) {
                continue;
            }
            if let Some(n) = self.nodes.get(&cur) {
                for next in &n.deps.after {
                    if !visited.contains(next) {
                        stack.push(next.clone());
                    }
                }
            }
        }
        visited
    }

    /// 檢測排序環 (After/Before 邊構成的有向圖中的強連通分量, size>1 或自環)。
    /// 返回每個環的成員集合 (Tarjan SCC)。
    pub fn ordering_cycles(&self) -> Vec<BTreeSet<String>> {
        // 迭代版 Tarjan SCC
        #[derive(Clone, Copy)]
        struct Frame<'a> {
            v: &'a str,
            iter_pos: usize,
        }

        let mut index = 0usize;
        let mut indices: BTreeMap<&str, usize> = BTreeMap::new();
        let mut lowlink: BTreeMap<&str, usize> = BTreeMap::new();
        let mut on_stack: BTreeSet<&str> = BTreeSet::new();
        let mut scc_stack: Vec<&str> = Vec::new();
        let mut result: Vec<BTreeSet<String>> = Vec::new();

        // 鄰接表: after 邊
        let succ = |v: &str| -> Vec<&str> {
            self.nodes
                .get(v)
                .map(|n| n.deps.after.iter().map(|s| s.as_str()).collect())
                .unwrap_or_default()
        };

        let vertices: Vec<&str> = self.nodes.keys().map(|s| s.as_str()).collect();

        for root in vertices {
            if indices.contains_key(root) {
                continue;
            }
            let mut call_stack: Vec<Frame> = vec![Frame {
                v: root,
                iter_pos: 0,
            }];
            while let Some(frame) = call_stack.last_mut() {
                let v = frame.v;
                let neighbors = succ(v);
                if frame.iter_pos == 0 {
                    // 首次訪問
                    indices.insert(v, index);
                    lowlink.insert(v, index);
                    index += 1;
                    scc_stack.push(v);
                    on_stack.insert(v);
                }
                let mut recursed = false;
                while frame.iter_pos < neighbors.len() {
                    let w = neighbors[frame.iter_pos];
                    frame.iter_pos += 1;
                    if !indices.contains_key(w) {
                        call_stack.push(Frame {
                            v: w,
                            iter_pos: 0,
                        });
                        recursed = true;
                        break;
                    } else if on_stack.contains(w) {
                        let wi = indices[&w];
                        if wi < lowlink[&v] {
                            lowlink.insert(v, wi);
                        }
                    }
                }
                if recursed {
                    continue;
                }
                // v 的所有鄰居處理完畢
                let vl = lowlink[&v];
                let vi = indices[&v];
                if vl == vi {
                    let mut component = BTreeSet::new();
                    while let Some(w) = scc_stack.pop() {
                        on_stack.remove(&w);
                        component.insert(w.to_string());
                        if w == v {
                            break;
                        }
                    }
                    // size>1 是環; size==1 需檢查自環 (v after v)
                    if component.len() > 1 {
                        result.push(component);
                    } else if let Some(only) = component.iter().next() {
                        if succ(only).contains(&only.as_str()) {
                            result.push(component);
                        }
                    }
                }
                // 回溯: 更新父幀 lowlink
                call_stack.pop();
                if let Some(parent) = call_stack.last_mut() {
                    let pv = parent.v;
                    let pl = lowlink[&pv];
                    let vl2 = lowlink[&v];
                    if vl2 < pl {
                        lowlink.insert(pv, vl2);
                    }
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dep_target_parse_variants() {
        assert!(DepTarget::parse("-x").ignore_failure);
        assert_eq!(DepTarget::parse("-x").name, "x");
        assert!(!DepTarget::parse("x").ignore_failure);
        assert_eq!(DepTarget::parse("--x").name, "-x", "僅剝第一個 '-'");
    }
}
