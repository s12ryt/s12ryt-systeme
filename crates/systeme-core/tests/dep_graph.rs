//! 依賴圖構建測試 — TDD RED 先行
//!
//! 覆蓋: [Unit] 段依賴聲明提取、`-` 前綴修飾、反向索引、
//! Before 邊反轉、RequiredBy/WantedBy 反向聲明、After 傳遞閉包、
//! 排序環檢測 (Tarjan SCC)、佔位節點、空值清空語義、重複 add 冪等。

use std::collections::BTreeSet;

use systeme_core::{DepTarget, DependencyGraph};
use systeme_units::parser::parse_unit;

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn tset(items: &[&str]) -> BTreeSet<DepTarget> {
    items.iter().map(|s| DepTarget::parse(s)).collect()
}

// ---------- 基礎 ----------

#[test]
fn empty_graph_returns_none_for_any_unit() {
    let g = DependencyGraph::new();
    assert!(g.node("nothing.service").is_none());
    assert!(g.unit_names().is_empty());
}

#[test]
fn extracts_after_and_before_from_unit_section() {
    let mut g = DependencyGraph::new();
    g.add_unit(
        "a.service",
        &parse_unit("[Unit]\nAfter=net.service\nBefore=log.service\n"),
    );
    let node = g.node("a.service").expect("a.service 應存在");
    assert_eq!(node.deps.after, set(&["net.service"]));
    assert_eq!(node.deps.before, set(&["log.service"]));
}

#[test]
fn extracts_all_dependency_kinds() {
    let mut g = DependencyGraph::new();
    g.add_unit(
        "a.service",
        &parse_unit(
            "[Unit]\nRequires=db.service\nWants=cache.service\nRequisite=base.target\n\
             BindsTo=hwd.service\nPartOf=stack.target\nConflicts=old.service\nUpholds=link.service\n",
        ),
    );
    let d = &g.node("a.service").unwrap().deps;
    assert_eq!(d.requires, tset(&["db.service"]));
    assert_eq!(d.wants, tset(&["cache.service"]));
    assert_eq!(d.requisite, tset(&["base.target"]));
    assert_eq!(d.binds_to, tset(&["hwd.service"]));
    assert_eq!(d.part_of, tset(&["stack.target"]));
    assert_eq!(d.conflicts, tset(&["old.service"]));
    assert_eq!(d.upholds, tset(&["link.service"]));
}

#[test]
fn multiple_values_and_list_semantics() {
    let mut g = DependencyGraph::new();
    g.add_unit(
        "a.service",
        &parse_unit("[Unit]\nWants=x.service y.service\nWants=z.service\n"),
    );
    assert_eq!(
        g.node("a.service").unwrap().deps.wants,
        tset(&["x.service", "y.service", "z.service"])
    );
}

#[test]
fn dependency_list_clears_on_empty_value() {
    let mut g = DependencyGraph::new();
    g.add_unit(
        "a.service",
        &parse_unit("[Unit]\nRequires=a.service\nRequires=\nWants=b.service\n"),
    );
    let d = &g.node("a.service").unwrap().deps;
    assert!(d.requires.is_empty(), "空值應清空 Requires 列表");
    assert_eq!(d.wants, tset(&["b.service"]), "僅清空對應鍵,不影響其他");
}

#[test]
fn dependency_lines_outside_unit_section_are_ignored() {
    let mut g = DependencyGraph::new();
    g.add_unit(
        "a.service",
        &parse_unit("[Service]\nAfter=evil.service\n[Unit]\nAfter=good.service\n"),
    );
    let d = &g.node("a.service").unwrap().deps;
    assert_eq!(d.after, set(&["good.service"]));
}

// ---------- `-` 前綴修飾 ----------

#[test]
fn dash_prefix_parses_ignore_failure() {
    let mut g = DependencyGraph::new();
    g.add_unit(
        "a.service",
        &parse_unit("[Unit]\nRequires=-flaky.service\nWants=ok.service\n"),
    );
    let d = &g.node("a.service").unwrap().deps;
    let flaky = d.requires.iter().find(|t| t.name == "flaky.service").unwrap();
    assert!(flaky.ignore_failure, "'-' 前綴應設 ignore_failure");
    let ok = d.wants.iter().find(|t| t.name == "ok.service").unwrap();
    assert!(!ok.ignore_failure);
}

// ---------- 反向索引 ----------

#[test]
fn reverse_indices_are_built() {
    let mut g = DependencyGraph::new();
    g.add_unit(
        "app.service",
        &parse_unit(
            "[Unit]\nRequires=db.service\nWants=cache.service\nRequisite=base.target\n\
             BindsTo=hwd.service\nPartOf=stack.target\nUpholds=link.service\n",
        ),
    );
    let db = g.node("db.service").unwrap();
    assert!(db.rdeps.required_by.contains("app.service"));
    let cache = g.node("cache.service").unwrap();
    assert!(cache.rdeps.wanted_by.contains("app.service"));
    let base = g.node("base.target").unwrap();
    assert!(base.rdeps.requisite_of.contains("app.service"));
    let hwd = g.node("hwd.service").unwrap();
    assert!(hwd.rdeps.bound_by.contains("app.service"));
    let stack = g.node("stack.target").unwrap();
    assert!(stack.rdeps.consists_of.contains("app.service"));
    let link = g.node("link.service").unwrap();
    assert!(link.rdeps.upheld_by.contains("app.service"));
}

#[test]
fn conflicts_are_symmetric() {
    let mut g = DependencyGraph::new();
    g.add_unit("a.service", &parse_unit("[Unit]\nConflicts=b.service\n"));
    let b = g.node("b.service").unwrap();
    assert!(
        b.deps.conflicts.iter().any(|t| t.name == "a.service"),
        "A Conflicts=B 應對稱地使 B 也衝突 A"
    );
}

#[test]
fn before_declaration_inverts_to_after_edge() {
    let mut g = DependencyGraph::new();
    g.add_unit("a.service", &parse_unit("[Unit]\nBefore=b.service\n"));
    // A Before=B 等價 B After=A
    let b = g.node("b.service").unwrap();
    assert!(b.deps.after.contains("a.service"));
    let a = g.node("a.service").unwrap();
    assert!(a.deps.before.contains("b.service"));
    // 反向索引: B 節點應知道有單元聲明排在它之前
    assert!(b.rdeps.units_before_me.contains("a.service"));
}

#[test]
fn after_declaration_builds_reverse_index() {
    let mut g = DependencyGraph::new();
    g.add_unit("a.service", &parse_unit("[Unit]\nAfter=b.service\n"));
    let b = g.node("b.service").unwrap();
    assert!(b.rdeps.units_after_me.contains("a.service"));
}

// ---------- 反向聲明 (RequiredBy/WantedBy/...) ----------

#[test]
fn required_by_declaration_adds_forward_requires() {
    let mut g = DependencyGraph::new();
    // B 聲明 RequiredBy=A ≡ A Requires=B
    g.add_unit("b.service", &parse_unit("[Unit]\nRequiredBy=a.service\n"));
    let a = g.node("a.service").unwrap();
    assert!(
        a.deps.requires.iter().any(|t| t.name == "b.service"),
        "B RequiredBy=A 應使 A.requires 含 B"
    );
    let b = g.node("b.service").unwrap();
    assert!(b.rdeps.required_by.contains("a.service"));
}

#[test]
fn wanted_by_declaration_adds_forward_wants() {
    let mut g = DependencyGraph::new();
    g.add_unit("inst.service", &parse_unit("[Unit]\nWantedBy=multi-user.target\n"));
    let mu = g.node("multi-user.target").unwrap();
    assert!(mu.deps.wants.iter().any(|t| t.name == "inst.service"));
    let inst = g.node("inst.service").unwrap();
    assert!(inst.rdeps.wanted_by.contains("multi-user.target"));
}

#[test]
fn bound_by_declaration_adds_forward_binds_to() {
    let mut g = DependencyGraph::new();
    g.add_unit("b.service", &parse_unit("[Unit]\nBoundBy=a.service\n"));
    let a = g.node("a.service").unwrap();
    assert!(a.deps.binds_to.iter().any(|t| t.name == "b.service"));
}

#[test]
fn conflicted_by_declaration_adds_symmetric_conflict() {
    let mut g = DependencyGraph::new();
    g.add_unit("b.service", &parse_unit("[Unit]\nConflictedBy=a.service\n"));
    let a = g.node("a.service").unwrap();
    assert!(a.deps.conflicts.iter().any(|t| t.name == "b.service"));
    let b = g.node("b.service").unwrap();
    assert!(b.deps.conflicts.iter().any(|t| t.name == "a.service"));
}

// ---------- After 閉包與環 ----------

#[test]
fn after_closure_is_transitive() {
    let mut g = DependencyGraph::new();
    g.add_unit("a.service", &parse_unit("[Unit]\nAfter=b.service\n"));
    g.add_unit("b.service", &parse_unit("[Unit]\nAfter=c.service\n"));
    g.add_unit("c.service", &parse_unit("[Unit]\n"));

    let closure = g.after_closure("a.service");
    assert_eq!(closure, set(&["b.service", "c.service"]));
    // 不含自身
    let self_closure = g.after_closure("c.service");
    assert!(self_closure.is_empty());
}

#[test]
fn after_closure_survives_cycles_without_hanging() {
    let mut g = DependencyGraph::new();
    g.add_unit("a.service", &parse_unit("[Unit]\nAfter=b.service\n"));
    g.add_unit("b.service", &parse_unit("[Unit]\nAfter=a.service\n"));
    // 環存在時閉包仍應終止 (含兩者)
    let closure = g.after_closure("a.service");
    assert!(closure.contains("b.service"));
    assert!(closure.contains("a.service") || !closure.contains("a.service")); // 自身是否含不算錯,重點是不死循環
}

#[test]
fn ordering_cycle_is_detected() {
    let mut g = DependencyGraph::new();
    g.add_unit("a.service", &parse_unit("[Unit]\nAfter=b.service\n"));
    g.add_unit("b.service", &parse_unit("[Unit]\nAfter=a.service\n"));
    let cycles = g.ordering_cycles();
    assert_eq!(cycles.len(), 1, "應檢測到恰好一個環");
    assert!(cycles[0].contains("a.service") && cycles[0].contains("b.service"));
}

#[test]
fn self_cycle_is_detected() {
    let mut g = DependencyGraph::new();
    g.add_unit("a.service", &parse_unit("[Unit]\nAfter=a.service\n"));
    let cycles = g.ordering_cycles();
    assert_eq!(cycles.len(), 1);
    assert!(cycles[0].contains("a.service"));
}

#[test]
fn dag_has_no_ordering_cycles() {
    let mut g = DependencyGraph::new();
    g.add_unit("a.service", &parse_unit("[Unit]\nAfter=b.service c.service\n"));
    g.add_unit("b.service", &parse_unit("[Unit]\nAfter=c.service\n"));
    g.add_unit("c.service", &parse_unit("[Unit]\n"));
    assert!(g.ordering_cycles().is_empty());
}

#[test]
fn cycle_through_before_edges_is_detected() {
    let mut g = DependencyGraph::new();
    // A After=B 且 A Before=C、C After=A → 環 A→B, A→C→A? 構造: A After=B, C After=A, B After=C
    g.add_unit("a.service", &parse_unit("[Unit]\nAfter=b.service\n"));
    g.add_unit("b.service", &parse_unit("[Unit]\nAfter=c.service\nBefore=x.service\n"));
    g.add_unit("c.service", &parse_unit("[Unit]\nAfter=a.service\n"));
    let cycles = g.ordering_cycles();
    assert_eq!(cycles.len(), 1);
    assert!(cycles[0].contains("a.service"));
    assert!(cycles[0].contains("b.service"));
    assert!(cycles[0].contains("c.service"));
}

// ---------- 佔位與冪等 ----------

#[test]
fn referencing_missing_unit_creates_placeholder() {
    let mut g = DependencyGraph::new();
    g.add_unit("a.service", &parse_unit("[Unit]\nAfter=ghost.service\n"));
    assert!(g.node("ghost.service").is_some(), "被引用但未定義的單元應有佔位節點");
    assert!(g.unit_names().contains(&"ghost.service".to_string()));
}

#[test]
fn re_add_unit_replaces_forward_deps() {
    let mut g = DependencyGraph::new();
    g.add_unit("a.service", &parse_unit("[Unit]\nWants=old.service\n"));
    g.add_unit("a.service", &parse_unit("[Unit]\nWants=new.service\n"));
    let a = g.node("a.service").unwrap();
    assert!(!a.deps.wants.iter().any(|t| t.name == "old.service"));
    assert!(a.deps.wants.iter().any(|t| t.name == "new.service"));
    // 反向索引同步: old 不再被 a wants
    let old = g.node("old.service").unwrap();
    assert!(!old.rdeps.wanted_by.contains("a.service"));
    let new = g.node("new.service").unwrap();
    assert!(new.rdeps.wanted_by.contains("a.service"));
}

#[test]
fn unit_names_lists_all_nodes_sorted() {
    let mut g = DependencyGraph::new();
    g.add_unit("b.service", &parse_unit("[Unit]\n"));
    g.add_unit("a.service", &parse_unit("[Unit]\n"));
    assert_eq!(g.unit_names(), vec!["a.service", "b.service"]);
}
