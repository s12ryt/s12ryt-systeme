//! 管理器核心狀態機: 模式探測、單元載入/重載、start/stop 事務編排與退出事件回饋。
//!
//! 本模組為**純邏輯層** — 進程的實際 spawn/kill 由 [`ServiceExecutor`] trait
//! 抽象 (守護進程以真實進程實現, 測試以 Mock 記錄)。IPC 協議見 [`crate::ipc`]。
//!
//! 「PID≠1 可啟動」核心語義:
//! - [`RuntimeMode::detect`]: root → 系統模式, 非 root → 用戶模式 (**不要求 PID 1**)
//! - cgroup 不可用時上層以 [`crate::cgroup::NoCgroup`] 降級 + 進程追蹤收養孤兒

use std::collections::{BTreeMap, BTreeSet};

use systeme_units::parser::{parse_unit, ParsedUnit};
use systeme_units::search::{load_unit, Loaded, SearchPaths, UnitFs};
use systeme_units::types::UnitType;

use crate::dep_graph::DependencyGraph;
use crate::process::{plan_kill, waitstatus_to_exit_event, KillPlan, ProcessInfo, WaitStatus};
use crate::service::{
    decide_after_exit, AfterExitAction, ExecCommand, ServiceConfig, ServiceFinalState, ServiceType,
};
use crate::transaction::{JobManager, JobMode, JobType, TransactionError, UnitState};

const SIG_TERM: i32 = 15;
const SIG_KILL: i32 = 9;

// ─── 模式探測 ────────────────────────────────────────────────────────

/// 管理器運行模式。
///
/// 與 systemd 的差異 (刻意為之): 不以 PID 1 為系統模式前提 —
/// 只要是 root 即可跑系統模式, 滿足「移除啟動限制」需求。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    /// 系統實例 (root)
    System,
    /// 用戶實例 (非 root)
    User,
}

impl RuntimeMode {
    pub fn detect(is_root: bool) -> RuntimeMode {
        if is_root {
            RuntimeMode::System
        } else {
            RuntimeMode::User
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            RuntimeMode::System => "system",
            RuntimeMode::User => "user",
        }
    }
}

// ─── 運行時狀態 ──────────────────────────────────────────────────────

/// 單元活躍狀態 (對應 systemd active state; `auto-restart` 對應 sub-state)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveState {
    Inactive,
    Activating,
    Active,
    Deactivating,
    /// 重啟等待中 (RestartPolicy 觸發)
    AutoRestart,
    Failed,
}

impl ActiveState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ActiveState::Inactive => "inactive",
            ActiveState::Activating => "activating",
            ActiveState::Active => "active",
            ActiveState::Deactivating => "deactivating",
            ActiveState::AutoRestart => "auto-restart",
            ActiveState::Failed => "failed",
        }
    }
}

/// 單元運行時數據 (跨 reload 保留)。
#[derive(Debug, Clone)]
pub struct UnitRuntime {
    pub state: ActiveState,
    pub main_pid: Option<i32>,
    /// 本單元追蹤中的進程 (供 KillPlan 計算)
    pub procs: Vec<ProcessInfo>,
    /// 重啟等待剩餘秒數 (None = 無待重啟)
    pub pending_restart_in: Option<f64>,
}

impl UnitRuntime {
    fn new() -> UnitRuntime {
        UnitRuntime {
            state: ActiveState::Inactive,
            main_pid: None,
            procs: Vec::new(),
            pending_restart_in: None,
        }
    }
}

/// 已載入單元條目: 解析結果 + 配置 + 運行時。
#[derive(Debug, Clone)]
pub struct UnitEntry {
    pub name: String,
    /// alias 解析後的真實單元名
    pub real_name: String,
    pub masked: bool,
    pub parsed: ParsedUnit,
    pub config: ServiceConfig,
    pub runtime: UnitRuntime,
}

impl UnitEntry {
    /// [Unit] Description (無則空串)
    pub fn description(&self) -> &str {
        self.parsed
            .section("Unit")
            .and_then(|s| s.get("Description"))
            .unwrap_or("")
    }
}

// ─── 執行器抽象 ──────────────────────────────────────────────────────

/// 進程執行抽象: 守護進程注入真實實現, 測試注入 Mock。
pub trait ServiceExecutor {
    /// 執行命令; 返回新進程 pid
    fn spawn(
        &mut self,
        unit: &str,
        cmd: &ExecCommand,
        env: &[String],
        cwd: Option<&str>,
    ) -> Result<i32, String>;
    /// 對 pid 發信號
    fn signal(&mut self, pid: i32, sig: i32) -> Result<(), String>;
    /// socket 激活 fd 預載: 下一個 spawn 的進程將帶上這些 fd (默認 no-op)。
    fn prepare_socket_fds(&mut self, _fds: &[i32]) {}
}

// ─── 錯誤 ────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum ManagerError {
    #[error("unit not found: {0}")]
    NotFound(String),
    #[error("unit is masked: {0}")]
    Masked(String),
    #[error("failed to start {unit}: {reason}")]
    StartFailed { unit: String, reason: String },
    #[error(transparent)]
    Transaction(#[from] TransactionError),
}

/// load/reload 掃描報告。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LoadReport {
    pub loaded: usize,
    pub masked: usize,
    /// reload 時消失的單元數
    pub dropped: usize,
}

// ─── 管理器核心 ──────────────────────────────────────────────────────

/// 事務引擎的活躍狀態快照 (每次事務前重建)。
struct StateSnapshot(BTreeMap<String, bool>);

impl UnitState for StateSnapshot {
    fn is_active(&self, unit: &str) -> bool {
        self.0.get(unit).copied().unwrap_or(false)
    }
}

pub struct ManagerCore {
    pub mode: RuntimeMode,
    pub graph: DependencyGraph,
    pub units: BTreeMap<String, UnitEntry>,
}

impl ManagerCore {
    pub fn new(mode: RuntimeMode) -> ManagerCore {
        ManagerCore {
            mode,
            graph: DependencyGraph::new(),
            units: BTreeMap::new(),
        }
    }

    pub fn graph_snapshot(&self) -> &DependencyGraph {
        &self.graph
    }

    pub fn unit(&self, name: &str) -> Option<&UnitEntry> {
        self.units.get(name)
    }

    pub fn unit_names(&self) -> Vec<String> {
        self.units.keys().cloned().collect()
    }

    pub fn is_active(&self, unit: &str) -> bool {
        matches!(self.active_state(unit), Some(ActiveState::Active))
    }

    pub fn active_state(&self, unit: &str) -> Option<ActiveState> {
        self.units.get(unit).map(|e| e.runtime.state)
    }

    /// 全量掃描搜索路徑並 (重) 載入所有單元; 運行時狀態按單元名保留。
    pub fn load_all(&mut self, fs: &dyn UnitFs, paths: &SearchPaths) -> LoadReport {
        let old_runtimes: BTreeMap<String, UnitRuntime> = self
            .units
            .iter()
            .map(|(k, v)| (k.clone(), v.runtime.clone()))
            .collect();

        self.units.clear();
        self.graph = DependencyGraph::new();

        let mut report = LoadReport::default();

        // 枚舉所有目錄下的單元名 (去重; 實際載入交給 load_unit 處理優先級/alias/mask/.d)
        let mut names: BTreeSet<String> = BTreeSet::new();
        for dir in &paths.dirs {
            let Ok(entries) = fs.list_dir_sorted(dir) else {
                continue;
            };
            for path in entries {
                let Some(fname) = path.file_name().and_then(|s| s.to_str()) else {
                    continue;
                };
                if UnitType::split_name(fname).is_some() {
                    names.insert(fname.to_string());
                }
            }
        }

        for fname in &names {
            match load_unit(fname, paths, fs) {
                Some(Loaded::Masked(f)) => {
                    let real = f.real_name.clone();
                    self.units.entry(real).or_insert_with(|| {
                        report.masked += 1;
                        let empty = parse_unit("");
                        UnitEntry {
                            name: fname.clone(),
                            real_name: f.real_name.clone(),
                            masked: true,
                            config: ServiceConfig::from_parsed(&empty),
                            parsed: empty,
                            runtime: UnitRuntime::new(),
                        }
                    });
                }
                Some(Loaded::Unit(f, pu)) => {
                    let real = f.real_name.clone();
                    self.graph.add_unit(&real, &pu);
                    if let Some(existing) = self.units.get_mut(&real) {
                        // 同一真實單元被多路徑枚舉兩次 (alias + 本體): 以首次為準
                        // (load_unit 已按優先級給出權威結果, 此分支理論上不觸發)
                        if existing.masked {
                            let rt = existing.runtime.clone();
                            let config = ServiceConfig::from_parsed(&pu);
                            *existing = UnitEntry {
                                name: fname.clone(),
                                real_name: real.clone(),
                                masked: false,
                                parsed: pu,
                                config,
                                runtime: rt,
                            };
                            report.masked = report.masked.saturating_sub(1);
                            report.loaded += 1;
                        }
                    } else {
                        let config = ServiceConfig::from_parsed(&pu);
                        self.units.insert(
                            real,
                            UnitEntry {
                                name: fname.clone(),
                                real_name: f.real_name.clone(),
                                masked: false,
                                parsed: pu,
                                config,
                                runtime: UnitRuntime::new(),
                            },
                        );
                        report.loaded += 1;
                    }
                }
                None => {} // 斷裂 symlink 等: 跳過
            }
        }

        // 恢復運行時狀態 (僅仍存在的單元)
        for (name, entry) in self.units.iter_mut() {
            if let Some(rt) = old_runtimes.get(name) {
                entry.runtime = rt.clone();
            }
        }
        report.dropped = old_runtimes
            .keys()
            .filter(|k| !self.units.contains_key(*k))
            .count();

        report
    }

    /// 重新掃描 (= load_all; 運行時狀態自動保留)
    pub fn reload(&mut self, fs: &dyn UnitFs, paths: &SearchPaths) -> LoadReport {
        self.load_all(fs, paths)
    }

    // ─── 事務執行 ────────────────────────────────────────────────────

    /// 提交事務並返回拓撲序 job 列表
    fn run_transaction(
        &mut self,
        unit: &str,
        jt: JobType,
    ) -> Result<Vec<(String, JobType)>, TransactionError> {
        let snapshot = StateSnapshot(
            self.units
                .iter()
                .map(|(k, v)| (k.clone(), matches!(v.runtime.state, ActiveState::Active)))
                .collect(),
        );
        let mut jm = JobManager::new(self.graph.clone(), Box::new(snapshot));
        jm.submit(unit, jt, JobMode::Replace)?;
        Ok(jm.ordered_jobs())
    }

    /// 啟動單元 (含依賴閉包)。返回實際處理的單元 (拓撲序)。
    pub fn start_unit(
        &mut self,
        unit: &str,
        exec: &mut dyn ServiceExecutor,
    ) -> Result<Vec<String>, ManagerError> {
        let Some(entry) = self.units.get(unit) else {
            return Err(ManagerError::NotFound(unit.to_string()));
        };
        if entry.masked {
            return Err(ManagerError::Masked(unit.to_string()));
        }
        let ordered = self.run_transaction(unit, JobType::Start)?;
        let mut touched = Vec::new();
        for (u, jt) in ordered {
            if self.apply_job(&u, jt, exec)? {
                touched.push(u);
            }
        }
        Ok(touched)
    }

    /// 停止單元 (含反向傳播)。返回實際處理的單元 (依賴者先停)。
    pub fn stop_unit(
        &mut self,
        unit: &str,
        exec: &mut dyn ServiceExecutor,
    ) -> Result<Vec<String>, ManagerError> {
        if !self.units.contains_key(unit) {
            return Err(ManagerError::NotFound(unit.to_string()));
        }
        let ordered = self.run_transaction(unit, JobType::Stop)?;
        let mut touched = Vec::new();
        for (u, jt) in ordered {
            if self.apply_job(&u, jt, exec)? {
                touched.push(u);
            }
        }
        Ok(touched)
    }

    /// 執行單個 job; 返回是否實際處理 (冪等跳過 = false)
    fn apply_job(
        &mut self,
        unit: &str,
        jt: JobType,
        exec: &mut dyn ServiceExecutor,
    ) -> Result<bool, ManagerError> {
        let Some(entry) = self.units.get_mut(unit) else {
            // 依賴圖佔位節點 (引用了未加載的單元): 跳過
            return Ok(false);
        };
        if entry.masked {
            // 直接 start 已在 start_unit 擋下; 此處為依賴拉起 — 跳過不視為成功
            return Ok(false);
        }
        match jt {
            JobType::Start => Self::apply_start(entry, unit, exec),
            JobType::Stop => Ok(Self::apply_stop(entry, unit, exec)),
            JobType::Reload => {
                // ExecReload 支援留待守護進程輪; 保持現狀
                Ok(matches!(entry.runtime.state, ActiveState::Active))
            }
        }
    }

    fn apply_start(
        entry: &mut UnitEntry,
        unit: &str,
        exec: &mut dyn ServiceExecutor,
    ) -> Result<bool, ManagerError> {
        // 冪等: 已活躍則跳過
        if entry.runtime.state == ActiveState::Active {
            return Ok(false);
        }
        let cwd = entry
            .config
            .working_directory
            .as_ref()
            .map(|w| w.path.as_str());

        // 非 service 單元 (target 等): 無進程語義, 直達 Active
        let is_service = UnitType::split_name(unit)
            .map(|(_, t)| matches!(t, UnitType::Service))
            .unwrap_or(true);
        if !is_service {
            entry.runtime.state = ActiveState::Active;
            return Ok(true);
        }

        entry.runtime.state = ActiveState::Activating;

        // ExecStartPre (順序執行; 失敗時除非 '-' 前綴否則中止)
        for cmd in entry.config.exec_start_pre.clone() {
            match exec.spawn(unit, &cmd, &entry.config.environment, cwd) {
                Ok(pid) => entry.runtime.procs.push(ProcessInfo {
                    pid,
                    pgid: pid,
                    is_main: false,
                }),
                Err(reason) => {
                    if !cmd.ignore_failure {
                        entry.runtime.state = ActiveState::Failed;
                        return Err(ManagerError::StartFailed {
                            unit: unit.to_string(),
                            reason,
                        });
                    }
                }
            }
        }

        if entry.config.exec_start.is_empty() {
            // 無 ExecStart: oneshot(+RemainAfterExit) 合法 — 直達終態; 其餘類型視為配置錯誤
            return match entry.config.service_type {
                ServiceType::Oneshot => {
                    if entry.config.remain_after_exit {
                        entry.runtime.state = ActiveState::Active;
                        Ok(true)
                    } else {
                        entry.runtime.state = ActiveState::Inactive;
                        Ok(false)
                    }
                }
                _ => {
                    entry.runtime.state = ActiveState::Failed;
                    Err(ManagerError::StartFailed {
                        unit: unit.to_string(),
                        reason: "no ExecStart configured".to_string(),
                    })
                }
            };
        }

        // ExecStart (末位為主進程)
        let total = entry.config.exec_start.len();
        let mut main_pid = None;
        for (i, cmd) in entry.config.exec_start.iter().enumerate() {
            match exec.spawn(unit, cmd, &entry.config.environment, cwd) {
                Ok(pid) => {
                    let is_main = i + 1 == total;
                    entry.runtime.procs.push(ProcessInfo {
                        pid,
                        pgid: pid,
                        is_main,
                    });
                    if is_main {
                        main_pid = Some(pid);
                    }
                }
                Err(reason) => {
                    entry.runtime.state = ActiveState::Failed;
                    return Err(ManagerError::StartFailed {
                        unit: unit.to_string(),
                        reason,
                    });
                }
            }
        }
        entry.runtime.main_pid = main_pid;
        entry.runtime.pending_restart_in = None;

        // oneshot 等 on_exit 決定終態; 其餘類型 spawn 即 Active
        // (forking/notify 的握手由守護進程層處理, 核心層簡化)
        if !matches!(entry.config.service_type, ServiceType::Oneshot) {
            entry.runtime.state = ActiveState::Active;
        }
        Ok(true)
    }

    fn apply_stop(entry: &mut UnitEntry, unit: &str, exec: &mut dyn ServiceExecutor) -> bool {
        if matches!(
            entry.runtime.state,
            ActiveState::Inactive | ActiveState::Failed
        ) {
            return false;
        }
        entry.runtime.state = ActiveState::Deactivating;

        // ExecStop (失敗不阻斷停止)
        let cwd = entry
            .config
            .working_directory
            .as_ref()
            .map(|w| w.path.as_str());
        for cmd in entry.config.exec_stop.clone() {
            let _ = exec.spawn(unit, &cmd, &entry.config.environment, cwd);
        }

        // 按 KillMode 計劃並發送信號
        match plan_kill(entry.config.kill_mode, &entry.runtime.procs) {
            KillPlan::None => {}
            KillPlan::Pids(pids) => {
                for pid in pids {
                    let _ = exec.signal(pid, SIG_TERM);
                }
            }
            KillPlan::Mixed { term, kill } => {
                for pid in &term {
                    let _ = exec.signal(*pid, SIG_TERM);
                }
                // 同步簡化: 主進程已收 TERM, KILL 階段跳過 term集成員。
                // (真實守護進程按兩階段執行: TERM → 等待 TimeoutStopSec → KILL 殘餘,
                //  那時 kill 集可含未退出的主進程 — 由 daemon 層實現)
                for pid in kill {
                    if !term.contains(&pid) {
                        let _ = exec.signal(pid, SIG_KILL);
                    }
                }
            }
        }

        entry.runtime.procs.clear();
        entry.runtime.main_pid = None;
        entry.runtime.pending_restart_in = None; // 顯式停止取消待重啟
        entry.runtime.state = ActiveState::Inactive;
        true
    }

    // ─── 退出事件回饋 ────────────────────────────────────────────────

    /// 進程退出回饋。返回單元最新狀態:
    /// - `None`: 單元不存在 / 非終態 status (Stopped/Continued) 被忽略
    /// - 子進程退出不改單元狀態, 僅縮減追蹤表
    pub fn on_exit(&mut self, unit: &str, pid: i32, status: WaitStatus) -> Option<ActiveState> {
        let Some(event) = waitstatus_to_exit_event(&status) else {
            return None; // 非終態 (job control) 忽略
        };
        let entry = self.units.get_mut(unit)?;
        entry.runtime.procs.retain(|p| p.pid != pid);

        if entry.runtime.main_pid != Some(pid) {
            // 子進程退出: 單元狀態不變
            return Some(entry.runtime.state);
        }
        entry.runtime.main_pid = None;

        match decide_after_exit(&entry.config, event) {
            AfterExitAction::Restart { after } => {
                entry.runtime.pending_restart_in = after.as_secs();
                entry.runtime.state = ActiveState::AutoRestart;
            }
            AfterExitAction::Final(f) => {
                entry.runtime.state = match f {
                    ServiceFinalState::Active => ActiveState::Active,
                    ServiceFinalState::Dead => ActiveState::Inactive,
                    ServiceFinalState::Failed => ActiveState::Failed,
                };
            }
        }
        Some(entry.runtime.state)
    }
}
