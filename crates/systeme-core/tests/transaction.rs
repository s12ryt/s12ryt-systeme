//! 事務引擎測試: job 遞歸展開、合併/替換語義、cycle 檢測、破壞性事務、運行排序。

use std::collections::BTreeSet;

use systeme_core::transaction::{JobManager, JobMode, JobType, TransactionError, UnitState};
use systeme_core::DependencyGraph;
use systeme_units::parser::parse_unit;

// ---------- 測試輔助 ----------

/// 固定 active 集合的狀態源 (測試用)。
struct FixedState {
    active: BTreeSet<String>,
}

impl UnitState for FixedState {
    fn is_active(&self, unit: &str) -> bool {
        self.active.contains(unit)
    }
}

fn state(active: &[&str]) -> Box<dyn UnitState> {
    Box::new(FixedState {
        active: active.iter().map(|s| s.to_string()).collect(),
    })
}

fn mk_graph(defs: &[(&str, &str)]) -> DependencyGraph {
    let mut g = DependencyGraph::new();
    for (name, text) in defs {
        g.add_unit(name, &parse_unit(text));
    }
    g
}

fn mgr(defs: &[(&str, &str)], active: &[&str]) -> JobManager {
    JobManager::new(mk_graph(defs), state(active))
}

fn jobs_of(m: &JobManager) -> Vec<(String, JobType)> {
    m.jobs().into_iter().collect()
}

fn ordered_of(m: &JobManager) -> Vec<(String, JobType)> {
    m.ordered_jobs()
}

// ---------- 基本展開 ----------

#[test]
fn single_start_no_deps() {
    let mut m = mgr(&[("a.service", "[Unit]\nDescription=x\n")], &[]);
    m.submit("a.service", JobType::Start, JobMode::Fail).unwrap();
    assert_eq!(
        jobs_of(&m),
        vec![("a.service".into(), JobType::Start)]
    );
}

#[test]
fn requires_pulls_start() {
    let mut m = mgr(
        &[("a.service", "[Unit]\nRequires=b.service\n")],
        &[],
    );
    m.submit("a.service", JobType::Start, JobMode::Fail).unwrap();
    assert_eq!(
        jobs_of(&m),
        vec![
            ("a.service".into(), JobType::Start),
            ("b.service".into(), JobType::Start),
        ]
    );
}

#[test]
fn wants_pulls_start() {
    let mut m = mgr(&[("a.service", "[Unit]\nWants=b.service\n")], &[]);
    m.submit("a.service", JobType::Start, JobMode::Fail).unwrap();
    assert_eq!(
        jobs_of(&m),
        vec![
            ("a.service".into(), JobType::Start),
            ("b.service".into(), JobType::Start),
        ]
    );
}

#[test]
fn binds_to_pulls_start() {
    let mut m = mgr(&[("a.service", "[Unit]\nBindsTo=b.service\n")], &[]);
    m.submit("a.service", JobType::Start, JobMode::Fail).unwrap();
    assert!(m.jobs().contains(&("b.service".to_string(), JobType::Start)));
}

#[test]
fn upholds_pulls_start() {
    let mut m = mgr(&[("a.service", "[Unit]\nUpholds=b.service\n")], &[]);
    m.submit("a.service", JobType::Start, JobMode::Fail).unwrap();
    assert!(m.jobs().contains(&("b.service".to_string(), JobType::Start)));
}

#[test]
fn chain_closure_three_levels() {
    let mut m = mgr(
        &[
            ("a.service", "[Unit]\nRequires=b.service\n"),
            ("b.service", "[Unit]\nRequires=c.service\n"),
        ],
        &[],
    );
    m.submit("a.service", JobType::Start, JobMode::Fail).unwrap();
    assert_eq!(m.jobs().len(), 3);
    assert!(m.jobs().contains(&("c.service".to_string(), JobType::Start)));
}

#[test]
fn reverse_declaration_required_by_pulls() {
    // b 聲明 RequiredBy=a ⇒ a Requires=b
    let mut m = mgr(&[("b.service", "[Unit]\nRequiredBy=a.service\n")], &[]);
    m.submit("a.service", JobType::Start, JobMode::Fail).unwrap();
    assert!(m.jobs().contains(&("b.service".to_string(), JobType::Start)));
}

// ---------- Requisite ----------

#[test]
fn requisite_inactive_fails() {
    let mut m = mgr(&[("a.service", "[Unit]\nRequisite=b.service\n")], &[]);
    let err = m
        .submit("a.service", JobType::Start, JobMode::Fail)
        .unwrap_err();
    match err {
        TransactionError::RequisiteNotMet { unit, requisite } => {
            assert_eq!(unit, "a.service");
            assert_eq!(requisite, "b.service");
        }
        other => panic!("期望 RequisiteNotMet, 得到 {other:?}"),
    }
}

#[test]
fn requisite_active_ok_and_not_pulled() {
    let mut m = mgr(&[("a.service", "[Unit]\nRequisite=b.service\n")], &["b.service"]);
    m.submit("a.service", JobType::Start, JobMode::Fail).unwrap();
    // Requisite 只檢查已 active, 不會拉起目標
    assert_eq!(m.jobs().len(), 1);
}

// ---------- Conflicts ----------

#[test]
fn conflicts_stops_other() {
    let mut m = mgr(&[("a.service", "[Unit]\nConflicts=b.service\n")], &[]);
    m.submit("a.service", JobType::Start, JobMode::Fail).unwrap();
    assert!(m.jobs().contains(&("b.service".to_string(), JobType::Stop)));
    assert!(m.jobs().contains(&("a.service".to_string(), JobType::Start)));
}

// ---------- Stop 傳播 ----------

#[test]
fn stop_propagates_required_by() {
    // b Requires=a ⇒ 停 a 時 b 也要停
    let mut m = mgr(
        &[("b.service", "[Unit]\nRequires=a.service\n")],
        &[],
    );
    m.submit("a.service", JobType::Stop, JobMode::Fail).unwrap();
    assert!(m.jobs().contains(&("a.service".to_string(), JobType::Stop)));
    assert!(m.jobs().contains(&("b.service".to_string(), JobType::Stop)));
}

#[test]
fn stop_propagates_bound_by() {
    // a BindsTo=b ⇒ 停 b 時 a 也要停 (a 在 b 的 bound_by 索引)
    let mut m = mgr(&[("a.service", "[Unit]\nBindsTo=b.service\n")], &[]);
    m.submit("b.service", JobType::Stop, JobMode::Fail).unwrap();
    assert!(m.jobs().contains(&("a.service".to_string(), JobType::Stop)));
    assert!(m.jobs().contains(&("b.service".to_string(), JobType::Stop)));
}

#[test]
fn stop_propagates_part_of() {
    // x PartOf=a ⇒ 停 a 時 x 也要停
    let mut m = mgr(&[("x.service", "[Unit]\nPartOf=a.service\n")], &[]);
    m.submit("a.service", JobType::Stop, JobMode::Fail).unwrap();
    assert!(m.jobs().contains(&("x.service".to_string(), JobType::Stop)));
}

#[test]
fn wanted_by_no_stop_propagation() {
    // Wants 是弱依賴: 停 a 不傳播到 b
    let mut m = mgr(&[("b.service", "[Unit]\nWants=a.service\n")], &[]);
    m.submit("a.service", JobType::Stop, JobMode::Fail).unwrap();
    assert_eq!(m.jobs().len(), 1);
}

// ---------- 合併 / 替換 / 衝突 ----------

#[test]
fn merge_same_type_idempotent() {
    let mut m = mgr(&[("a.service", "[Unit]\n")], &[]);
    m.submit("a.service", JobType::Start, JobMode::Fail).unwrap();
    m.submit("a.service", JobType::Start, JobMode::Fail).unwrap();
    assert_eq!(m.jobs().len(), 1);
}

#[test]
fn replace_mode_swaps_job_type() {
    let mut m = mgr(&[("a.service", "[Unit]\n")], &[]);
    m.submit("a.service", JobType::Start, JobMode::Fail).unwrap();
    m.submit("a.service", JobType::Stop, JobMode::Replace).unwrap();
    assert_eq!(
        jobs_of(&m),
        vec![("a.service".into(), JobType::Stop)]
    );
}

#[test]
fn fail_mode_conflict_errors() {
    let mut m = mgr(&[("a.service", "[Unit]\n")], &[]);
    m.submit("a.service", JobType::Start, JobMode::Fail).unwrap();
    let err = m
        .submit("a.service", JobType::Stop, JobMode::Fail)
        .unwrap_err();
    match err {
        TransactionError::JobConflict { unit, existing, new } => {
            assert_eq!(unit, "a.service");
            assert_eq!(existing, JobType::Start);
            assert_eq!(new, JobType::Stop);
        }
        other => panic!("期望 JobConflict, 得到 {other:?}"),
    }
    // 原有 job 不受影響
    assert!(m.jobs().contains(&("a.service".to_string(), JobType::Start)));
}

#[test]
fn stop_replaces_full_chain() {
    // b Requires=a; 先 start b (拉入 a), 再 stop a (Replace)
    // ⇒ stop a 傳播到 required_by={b}, 兩者皆變 Stop
    let mut m = mgr(&[("b.service", "[Unit]\nRequires=a.service\n")], &[]);
    m.submit("b.service", JobType::Start, JobMode::Fail).unwrap();
    assert_eq!(m.jobs().len(), 2, "start b 應拉入 a");
    m.submit("a.service", JobType::Stop, JobMode::Replace).unwrap();
    assert!(m.jobs().contains(&("a.service".to_string(), JobType::Stop)));
    assert!(m.jobs().contains(&("b.service".to_string(), JobType::Stop)));
    assert_eq!(m.jobs().len(), 2);
}

#[test]
fn reload_job_type_and_replace() {
    let mut m = mgr(&[("a.service", "[Unit]\n")], &[]);
    m.submit("a.service", JobType::Reload, JobMode::Fail).unwrap();
    assert_eq!(jobs_of(&m), vec![("a.service".into(), JobType::Reload)]);
    // 不同類型在 Replace 模式下: 新替換舊
    m.submit("a.service", JobType::Start, JobMode::Replace).unwrap();
    assert_eq!(jobs_of(&m), vec![("a.service".into(), JobType::Start)]);
}

#[test]
fn fail_conflict_leaves_queue_untouched() {
    // Fail 衝突時整個 submit 失敗, 不應殘留半個事務
    let mut m = mgr(
        &[
            ("a.service", "[Unit]\nRequires=c.service\n"),
            ("b.service", "[Unit]\n"),
        ],
        &[],
    );
    m.submit("a.service", JobType::Start, JobMode::Fail).unwrap();
    let before = jobs_of(&m);
    let err = m.submit("c.service", JobType::Stop, JobMode::Fail);
    assert!(matches!(err, Err(TransactionError::JobConflict { .. })));
    assert_eq!(jobs_of(&m), before, "衝突失敗後隊列不變");
}

// ---------- 環檢測 ----------

#[test]
fn dependency_cycle_detected() {
    let mut m = mgr(
        &[
            ("a.service", "[Unit]\nRequires=b.service\n"),
            ("b.service", "[Unit]\nRequires=a.service\n"),
        ],
        &[],
    );
    let err = m
        .submit("a.service", JobType::Start, JobMode::Fail)
        .unwrap_err();
    match err {
        TransactionError::DependencyCycle { path } => {
            assert!(path.first().map(|s| s == "a.service").unwrap_or(false));
            assert!(path.contains(&"a.service".to_string()));
            assert!(path.contains(&"b.service".to_string()));
        }
        other => panic!("期望 DependencyCycle, 得到 {other:?}"),
    }
}

#[test]
fn ordering_cycle_detected() {
    let mut m = mgr(
        &[
            ("a.service", "[Unit]\nAfter=b.service\n"),
            ("b.service", "[Unit]\nAfter=a.service\n"),
        ],
        &[],
    );
    let err = m
        .submit("a.service", JobType::Start, JobMode::Fail)
        .unwrap_err();
    match err {
        TransactionError::OrderingCycle { members } => {
            assert!(members.contains(&"a.service".to_string()));
            assert!(members.contains(&"b.service".to_string()));
        }
        other => panic!("期望 OrderingCycle, 得到 {other:?}"),
    }
}

// ---------- 運行排序 ----------

#[test]
fn ordered_start_respects_after() {
    // a After=b 且 a Requires=b ⇒ b 先啟動
    let mut m = mgr(
        &[("a.service", "[Unit]\nRequires=b.service\nAfter=b.service\n")],
        &[],
    );
    m.submit("a.service", JobType::Start, JobMode::Fail).unwrap();
    let order: Vec<String> = ordered_of(&m).into_iter().map(|(u, _)| u).collect();
    assert_eq!(order, vec!["b.service", "a.service"]);
}

#[test]
fn ordered_stop_reversed() {
    // a After=b ⇒ 啟動序 b→a, 停止序 a→b
    let mut m = mgr(
        &[
            ("a.service", "[Unit]\nAfter=b.service\n"),
            ("b.service", "[Unit]\n"),
        ],
        &[],
    );
    m.submit("a.service", JobType::Stop, JobMode::Fail).unwrap();
    m.submit("b.service", JobType::Stop, JobMode::Fail).unwrap();
    let order: Vec<String> = ordered_of(&m).into_iter().map(|(u, _)| u).collect();
    assert_eq!(order, vec!["a.service", "b.service"]);
}

#[test]
fn mixed_jobs_no_order_panic() {
    // stop a + start b 混合: 無排序約束, 不 panic, 確定性輸出 (字典序)
    let mut m = mgr(
        &[
            ("a.service", "[Unit]\nAfter=b.service\n"),
            ("b.service", "[Unit]\n"),
        ],
        &[],
    );
    m.submit("a.service", JobType::Stop, JobMode::Fail).unwrap();
    m.submit("b.service", JobType::Start, JobMode::Fail).unwrap();
    let order = ordered_of(&m);
    assert_eq!(order.len(), 2);
    assert_eq!(order[0].0, "a.service");
    assert_eq!(order[1].0, "b.service");
}

#[test]
fn ordered_chain_three_levels() {
    // a Requires+After b, b Requires+After c ⇒ 順序 c, b, a
    let mut m = mgr(
        &[
            ("a.service", "[Unit]\nRequires=b.service\nAfter=b.service\n"),
            ("b.service", "[Unit]\nRequires=c.service\nAfter=c.service\n"),
        ],
        &[],
    );
    m.submit("a.service", JobType::Start, JobMode::Fail).unwrap();
    let order: Vec<String> = ordered_of(&m).into_iter().map(|(u, _)| u).collect();
    assert_eq!(order, vec!["c.service", "b.service", "a.service"]);
}

#[test]
fn independent_roots_deterministic_order() {
    // 無排序邊的 job 按字典序
    let mut m = mgr(
        &[("zz.service", "[Unit]\n"), ("aa.service", "[Unit]\n")],
        &[],
    );
    m.submit("zz.service", JobType::Start, JobMode::Fail).unwrap();
    m.submit("aa.service", JobType::Start, JobMode::Fail).unwrap();
    let order: Vec<String> = ordered_of(&m).into_iter().map(|(u, _)| u).collect();
    assert_eq!(order, vec!["aa.service", "zz.service"]);
}

// ---------- 未定義單元 ----------

#[test]
fn submit_unknown_unit_still_queues() {
    // 圖中不存在的單元: 無依賴可展開, job 照建 (執行層再報錯)
    let mut m = mgr(&[], &[]);
    m.submit("ghost.service", JobType::Start, JobMode::Fail).unwrap();
    assert_eq!(jobs_of(&m), vec![("ghost.service".into(), JobType::Start)]);
}
