//! systeme-core: 依賴圖、事務引擎與單元運行時核心。
//!
//! 本 crate 承載管理器的「大腦」:
//! - [`dep_graph`] — 從解析後的單元文件構建依賴圖 (After/Before/Requires/...)
//! - [`transaction`] — 事務引擎 (job 展開/合併/排序/環檢測)
//! - [`service`] — Service 執行器 (配置提取、命令行解析、退出後決策)
//! - [`process`] — 進程管理 (信號表、waitpid 解碼、KillPlan、Unix 系統層)
//! - [`cgroup`] — cgroup 抽象層 (v2/v1/none 三級降級、slice 層級、路徑計算)
//! - [`manager`] — 管理器核心狀態機 (load/reload、start/stop 編排、退出回饋)
//! - [`ipc`] — 守護進程/客戶端共享的 IPC 協議類型
//! - [`calendar`] — systemd.time(7) Calendar 事件語法解析與下次觸發計算
//! - [`timer`] — Timer 單元配置解析與錨點式下次觸發計算
//! - [`socket`] — Socket 單元配置解析、Listen 目標分類、fd 傳遞環境構造
//! - [`mount`] — mount / automount / swap 單元配置與單元名推導
//! - [`path`] — Path 單元配置、glob 匹配、觸發調度器

pub mod calendar;
pub mod cgroup;
pub mod dep_graph;
pub mod ipc;
pub mod manager;
pub mod mount;
pub mod path;
pub mod process;
pub mod service;
pub mod socket;
pub mod timer;
pub mod transaction;

pub use calendar::{next_elapse, parse_calendar, CalendarError, CalendarSpec};
pub use cgroup::{
    default_cgroup_path, detect_backend, slice_parent, CgroupBackend, CgroupError, CgroupMode,
    MemCgroup, NoCgroup,
};
pub use dep_graph::{DepKind, DepTarget, DependencyGraph, DirectDeps, ReverseDeps, UnitNode};
pub use ipc::{unit_info_of, Request, Response, UnitInfo};
pub use manager::{
    ActiveState, LoadReport, ManagerCore, ManagerError, RuntimeMode, ServiceExecutor, UnitEntry,
    UnitRuntime,
};
pub use mount::{
    automount_target, automount_unit_name, mount_unit_name, swap_unit_name, AutoMountConfig,
    MountConfig, SwapConfig,
};
pub use path::{
    glob_match, path_target, PathConfig, PathProbe, PathScheduler, PathSnapshot, PathTrigger,
    PathTriggerKind,
};
pub use service::{
    decide_after_exit, parse_time_span, AfterExitAction, ExitEvent, ExecCommand, ExecParseError,
    KillMode, RestartPolicy, ServiceConfig, ServiceFinalState, ServiceType, TimeSpan,
    DEFAULT_RESTART_SECS, DEFAULT_TIMEOUT_SECS,
};
pub use socket::{
    classify_listen, listen_fds_env, socket_service_target, ListenKind, ListenProto, ListenSpec,
    SocketConfig, DEFAULT_BACKLOG,
};
pub use timer::{next_timer_fire, TimerAnchors, TimerConfig, DEFAULT_ACCURACY_SECS};
pub use transaction::{JobManager, JobMode, JobType, TransactionError, UnitState};
