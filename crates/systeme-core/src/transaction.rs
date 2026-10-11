//! 事務引擎: job 遞歸展開、合併/替換語義、cycle 檢測與運行排序。
//!
//! 模仿真 systemd 的 job 語義:
//! - `Start` 展開: Requires/Wants/BindsTo/Upholds 目標拉起 Start job;
//!   Requisite 目標必須已 active 否則事務失敗 (不拉起);
//!   Conflicts 目標加入 Stop job
//! - `Stop` 傳播: 聲明 Requires/BindsTo/PartOf 指向本單元的單元 (required_by/bound_by/consists_of)
//!   一併加入 Stop job;Wants 為弱依賴不傳播
//! - 合併語義: 同單元同類型 job 冪等合併;不同類型在 `Replace` 模式下新替換舊,
//!   `Fail` 模式 (systemctl 默認) 下整個事務失敗且隊列不變
//! - 環檢測: 依賴展開環 → `DependencyCycle`;After/Before 排序環 → `OrderingCycle`
//! - 運行排序: 啟動 job 按 After 邊正向拓撲 (被依賴者先),
//!   停止 job 反向 (依賴者先停);混合 Stop/Start 對無排序約束

use std::collections::{BTreeMap, BTreeSet};

use crate::dep_graph::DependencyGraph;

/// job 類型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobType {
    Start,
    Stop,
    Reload,
}

impl JobType {
    pub fn is_stop(&self) -> bool {
        matches!(self, JobType::Stop)
    }
}

/// 與已裝 job 衝突時的處理模式 (對應 systemctl `--job-mode=`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobMode {
    /// 衝突 → 整個事務失敗 (默認)。
    Fail,
    /// 衝突 → 新 job 替換舊 job。
    Replace,
}

/// 單元當前狀態查詢 (由管理器運行時提供;測試用固定集合)。
pub trait UnitState {
    fn is_active(&self, unit: &str) -> bool;
}

/// 事務錯誤。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TransactionError {
    /// 依賴展開中遇到環 (如 A Requires B, B Requires A)。
    #[error("依賴環: {path:?}")]
    DependencyCycle { path: Vec<String> },

    /// After/Before 排序環涉及本事務。
    #[error("排序環: {members:?}")]
    OrderingCycle { members: Vec<String> },

    /// 與已裝 job 同單元不同類型衝突 (Fail 模式)。
    #[error("job 衝突: {unit} 已有 {existing:?} 又要求 {new:?}")]
    JobConflict {
        unit: String,
        existing: JobType,
        new: JobType,
    },

    /// Requisite 目標未 active。
    #[error("{unit} 的 Requisite 依賴 {requisite} 未運行")]
    RequisiteNotMet { unit: String, requisite: String },
}

/// job 隊列管理器: 接收 submit、展開依賴閉包、維護已裝 job、提供運行排序。
pub struct JobManager {
    graph: DependencyGraph,
    state: Box<dyn UnitState>,
    /// 已裝 job (unit → 類型)。
    jobs: BTreeMap<String, JobType>,
}

impl JobManager {
    pub fn new(graph: DependencyGraph, state: Box<dyn UnitState>) -> Self {
        Self {
            graph,
            state,
            jobs: BTreeMap::new(),
        }
    }

    /// 提交一個 job 請求: 遞歸展開依賴閉包後按 `mode` 與已裝隊列合併。
    ///
    /// Fail 模式下任何衝突都會使本次 submit 整體失敗且隊列保持不變
    /// (兩階段: 先全量驗證, 再一次性套用)。
    pub fn submit(
        &mut self,
        unit: &str,
        jt: JobType,
        mode: JobMode,
    ) -> Result<(), TransactionError> {
        // 1. 展開事務閉包。
        let mut txn: BTreeMap<String, JobType> = BTreeMap::new();
        let mut path: Vec<String> = Vec::new();
        self.expand(unit, jt, &mut txn, &mut path)?;

        // 2. 排序環涉及檢查。
        for scc in self.graph.ordering_cycles() {
            if scc.iter().any(|u| txn.contains_key(u)) {
                return Err(TransactionError::OrderingCycle {
                    members: scc.into_iter().collect(),
                });
            }
        }

        // 3. 兩階段合併: 先計劃後套用, 保證 Fail 衝突時零殘留。
        enum Action {
            Keep,
            Overwrite(JobType),
        }
        let mut plan: Vec<(String, Action)> = Vec::new();
        for (u, &new) in &txn {
            match self.jobs.get(u) {
                Some(&old) if old == new => plan.push((u.clone(), Action::Keep)),
                Some(&old) => match mode {
                    JobMode::Fail => {
                        return Err(TransactionError::JobConflict {
                            unit: u.clone(),
                            existing: old,
                            new,
                        })
                    }
                    JobMode::Replace => plan.push((u.clone(), Action::Overwrite(new))),
                },
                None => plan.push((u.clone(), Action::Overwrite(new))),
            }
        }
        for (u, a) in plan {
            if let Action::Overwrite(t) = a {
                self.jobs.insert(u, t);
            }
        }
        Ok(())
    }

    /// 已裝 job 快照 (字典序)。
    pub fn jobs(&self) -> Vec<(String, JobType)> {
        self.jobs.iter().map(|(u, &t)| (u.clone(), t)).collect()
    }

    /// 移除一個 job (運行完成後由執行層調用)。
    pub fn remove_job(&mut self, unit: &str) -> Option<JobType> {
        self.jobs.remove(unit)
    }

    /// 運行排序 (拓撲):
    /// - 兩個非 Stop job: After 邊正向 (被依賴者先)
    /// - 兩個 Stop job: After 邊反向 (依賴者先停)
    /// - Stop 與非 Stop 混合對: 無排序約束
    ///
    /// 同層無約束時按單元名字典序, 保證輸出確定性。
    pub fn ordered_jobs(&self) -> Vec<(String, JobType)> {
        let units: Vec<&str> = self.jobs.keys().map(|s| s.as_str()).collect();

        // 邊 a→b 表示「a 先於 b 運行」。
        let mut adj: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for (i, &u1) in units.iter().enumerate() {
            for &u2 in units.iter().skip(i + 1) {
                let t1 = self.jobs[u1];
                let t2 = self.jobs[u2];
                if t1.is_stop() != t2.is_stop() {
                    continue; // 混合對無約束
                }
                let after = |u: &str, v: &str| {
                    self.graph
                        .node(u)
                        .map(|n| n.deps.after.contains(v))
                        .unwrap_or(false)
                };
                if t1.is_stop() {
                    // 停止序反向: u1 after u2 ⇒ u1 先停。
                    if after(u1, u2) {
                        adj.entry(u1).or_default().insert(u2);
                    }
                    if after(u2, u1) {
                        adj.entry(u2).or_default().insert(u1);
                    }
                } else {
                    // 啟動序正向: u1 after u2 ⇒ u2 先啟動。
                    if after(u1, u2) {
                        adj.entry(u2).or_default().insert(u1);
                    }
                    if after(u2, u1) {
                        adj.entry(u1).or_default().insert(u2);
                    }
                }
            }
        }

        // Kahn 拓撲, BTreeSet ready 保證字典序確定性。
        let mut indeg: BTreeMap<&str, usize> = units.iter().map(|&u| (u, 0)).collect();
        for outs in adj.values() {
            for &o in outs {
                *indeg.entry(o).or_insert(0) += 1;
            }
        }
        let mut ready: BTreeSet<&str> = indeg
            .iter()
            .filter(|(_, &d)| d == 0)
            .map(|(&u, _)| u)
            .collect();
        let mut out: Vec<(String, JobType)> = Vec::new();
        while let Some(&u) = ready.iter().next() {
            ready.remove(&u);
            out.push((u.to_string(), self.jobs[u]));
            if let Some(outs) = adj.remove(u) {
                for o in outs {
                    let d = indeg.get_mut(o).expect("入度表中必有");
                    *d -= 1;
                    if *d == 0 {
                        ready.insert(o);
                    }
                }
            }
        }

        // 防禦: 理論上排序環已在 submit 擋下;若仍出現 (如防禦性場景),
        // 殘餘節點按字典序附加而非死鎖。
        if out.len() < units.len() {
            let done: BTreeSet<String> = out.iter().map(|(u, _)| u.clone()).collect();
            for &u in &units {
                if !done.contains(u) {
                    out.push((u.to_string(), self.jobs[u]));
                }
            }
        }
        out
    }

    // ---------- 事務閉包展開 ----------

    /// 遞歸展開 (帶 DFS 路徑環檢測與同類冪等剪枝)。
    fn expand(
        &self,
        unit: &str,
        jt: JobType,
        jobs: &mut BTreeMap<String, JobType>,
        path: &mut Vec<String>,
    ) -> Result<(), TransactionError> {
        if path.iter().any(|p| p == unit) {
            let mut cycle = path.clone();
            cycle.push(unit.to_string());
            return Err(TransactionError::DependencyCycle { path: cycle });
        }
        match jobs.get(unit) {
            Some(&t) if t == jt => return Ok(()), // 冪等剪枝
            Some(&t) => {
                // 事務內部衝突 (如同時被拉起又被衝突停止)。
                return Err(TransactionError::JobConflict {
                    unit: unit.to_string(),
                    existing: t,
                    new: jt,
                });
            }
            None => {}
        }
        jobs.insert(unit.to_string(), jt);
        path.push(unit.to_string());

        let result = match jt {
            JobType::Start => self.expand_start_deps(unit, jobs, path),
            JobType::Stop => self.expand_stop_deps(unit, jobs, path),
            JobType::Reload => Ok(()),
        };
        path.pop();
        result
    }

    fn expand_start_deps(
        &self,
        unit: &str,
        jobs: &mut BTreeMap<String, JobType>,
        path: &mut Vec<String>,
    ) -> Result<(), TransactionError> {
        let Some(node) = self.graph.node(unit) else {
            return Ok(()); // 未定義單元: 無依賴可展開
        };

        // 拉起類依賴 (ignore_failure 標記是執行層語義, 這裡統一建 job)。
        for t in &node.deps.requires {
            self.expand(&t.name, JobType::Start, jobs, path)?;
        }
        for t in &node.deps.wants {
            self.expand(&t.name, JobType::Start, jobs, path)?;
        }
        for t in &node.deps.binds_to {
            self.expand(&t.name, JobType::Start, jobs, path)?;
        }
        for t in &node.deps.upholds {
            self.expand(&t.name, JobType::Start, jobs, path)?;
        }

        // Requisite: 只檢查已 active, 不拉起。
        for t in &node.deps.requisite {
            if !self.state.is_active(&t.name) {
                return Err(TransactionError::RequisiteNotMet {
                    unit: unit.to_string(),
                    requisite: t.name.clone(),
                });
            }
        }

        // Conflicts: 目標加入 Stop job。
        for t in &node.deps.conflicts {
            self.expand(&t.name, JobType::Stop, jobs, path)?;
        }
        Ok(())
    }

    fn expand_stop_deps(
        &self,
        unit: &str,
        jobs: &mut BTreeMap<String, JobType>,
        path: &mut Vec<String>,
    ) -> Result<(), TransactionError> {
        let Some(node) = self.graph.node(unit) else {
            return Ok(());
        };
        // 停止傳播: required_by (誰 Requires 我) / bound_by (誰 BindsTo 我) /
        // consists_of (誰 PartOf 我)。Wants 不傳播。
        let mut targets: BTreeSet<&str> = BTreeSet::new();
        targets.extend(node.rdeps.required_by.iter().map(|s| s.as_str()));
        targets.extend(node.rdeps.bound_by.iter().map(|s| s.as_str()));
        targets.extend(node.rdeps.consists_of.iter().map(|s| s.as_str()));
        for v in targets {
            self.expand(v, JobType::Stop, jobs, path)?;
        }
        Ok(())
    }
}
