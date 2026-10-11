//! Service 執行器: 配置提取、命令行解析與退出後決策。
//!
//! 本模組是「純邏輯層」: 不 spawn 進程, 只負責
//! - 從 [`ParsedUnit`] 的 `[Service]` 段提取 [`ServiceConfig`] (未知值降級為警告)
//! - systemd 命令行語法解析 [`ExecCommand`] (引號/轉義/前綴旗標)
//! - systemd 時長語法 [`parse_time_span`]
//! - 主進程退出後的重啟/最終狀態決策 [`decide_after_exit`]
//!
//! 實際進程執行由後續 systeme-exec crate 承擔。

use systeme_units::ParsedUnit;

// ─── 類型定義 ────────────────────────────────────────────────────────

/// Service 型別 ([Service] Type= 指令)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceType {
    Simple,
    Forking,
    Exec,
    Notify,
    Oneshot,
    DBus,
}

impl ServiceType {
    /// 解析 Type= 值; 未知值返回 None (調用方降級為默認 Simple 並警告)
    pub fn parse(v: &str) -> Option<ServiceType> {
        match v.trim().to_ascii_lowercase().as_str() {
            "simple" => Some(ServiceType::Simple),
            "forking" => Some(ServiceType::Forking),
            "exec" => Some(ServiceType::Exec),
            "notify" => Some(ServiceType::Notify),
            "oneshot" => Some(ServiceType::Oneshot),
            "dbus" => Some(ServiceType::DBus),
            _ => None,
        }
    }
}

/// 重啟策略 ([Service] Restart= 指令)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartPolicy {
    No,
    OnSuccess,
    OnFailure,
    OnAbnormal,
    OnAbort,
    OnWatchdog,
    Always,
}

impl RestartPolicy {
    pub fn parse(v: &str) -> Option<RestartPolicy> {
        match v.trim().to_ascii_lowercase().as_str() {
            "no" => Some(RestartPolicy::No),
            "on-success" => Some(RestartPolicy::OnSuccess),
            "on-failure" => Some(RestartPolicy::OnFailure),
            "on-abnormal" => Some(RestartPolicy::OnAbnormal),
            "on-abort" => Some(RestartPolicy::OnAbort),
            "on-watchdog" => Some(RestartPolicy::OnWatchdog),
            "always" => Some(RestartPolicy::Always),
            _ => None,
        }
    }
}

/// 殺進程模式 ([Service] KillMode= 指令)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillMode {
    /// 殺整個 cgroup (默認)
    ControlGroup,
    /// 主進程殺全部、其餘只殺主進程
    Mixed,
    /// 只殺主進程
    Process,
    /// 殺主進程同組進程
    ProcessGroup,
    /// 不殺任何進程
    None,
}

impl KillMode {
    pub fn parse(v: &str) -> Option<KillMode> {
        match v.trim().to_ascii_lowercase().as_str() {
            "control-group" => Some(KillMode::ControlGroup),
            "mixed" => Some(KillMode::Mixed),
            "process" => Some(KillMode::Process),
            "process-group" => Some(KillMode::ProcessGroup),
            "none" => Some(KillMode::None),
            _ => None,
        }
    }
}

/// systemd 時長值: 秒 (浮點) 或無限
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TimeSpan {
    Finite(f64),
    Infinity,
}

impl TimeSpan {
    pub fn as_secs(&self) -> Option<f64> {
        match self {
            TimeSpan::Finite(s) => Some(*s),
            TimeSpan::Infinity => None,
        }
    }
}

/// 解析 systemd 時長語法: `90` `90s` `100ms` `1min 30s` `infinity`
///
/// 單位: us/usec, ms/msec, s/sec (默認), m/min, h/hr, d, w。多段以空白分隔相加。
// 調用方只關心通過/失敗, 不需要錯誤細節, 故意使用單元錯誤類型。
#[allow(clippy::result_unit_err)]
pub fn parse_time_span(v: &str) -> Result<TimeSpan, ()> {
    let v = v.trim();
    if v.eq_ignore_ascii_case("infinity") {
        return Ok(TimeSpan::Infinity);
    }
    if v.is_empty() {
        return Err(());
    }
    let mut total = 0.0f64;
    for part in v.split_whitespace() {
        // 拆出前導數字 (含小數點) 與單位後綴
        let digits_end = part
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(part.len());
        let (num_str, unit) = part.split_at(digits_end);
        let n: f64 = num_str.parse().map_err(|_| ())?;
        let mult = match unit.to_ascii_lowercase().as_str() {
            "" | "s" | "sec" | "secs" | "seconds" => 1.0,
            "ms" | "msec" | "msecs" | "milliseconds" => 1e-3,
            "us" | "usec" | "usecs" | "microseconds" => 1e-6,
            "m" | "min" | "mins" | "minutes" => 60.0,
            "h" | "hr" | "hrs" | "hours" => 3600.0,
            "d" | "days" => 86400.0,
            "w" | "weeks" => 604800.0,
            _ => return Err(()),
        };
        total += n * mult;
    }
    Ok(TimeSpan::Finite(total))
}

// ─── ExecCommand 解析 ────────────────────────────────────────────────

/// 解析後的可執行命令行
#[derive(Debug, Clone, PartialEq)]
pub struct ExecCommand {
    /// 參數列表; argv[0] 為可執行文件路徑
    pub argv: Vec<String>,
    /// `-` 前綴: 非零退出視為成功
    pub ignore_failure: bool,
    /// `+` 前綴: 以完整權限執行 (不切換用戶/不加權限限制)
    pub full_privileges: bool,
    /// `:` 前綴: 執行時不做變量展開
    pub no_env_expand: bool,
}

/// 命令行解析錯誤
#[derive(Debug, Clone, PartialEq)]
pub enum ExecParseError {
    /// 空命令
    EmptyCommand,
    /// 可執行文件不是絕對路徑
    NonAbsolutePath(String),
    /// 引號未閉合
    UnterminatedQuote(char),
}

impl ExecCommand {
    /// 解析一行 systemd 命令行 (ExecStart/ExecStop 等指令的值)。
    ///
    /// 語法 (對齊 systemd.service COMMAND LINES):
    /// - 前綴旗標 (可組合, 位於行首): `-` 忽略失敗, `+` 完整權限,
    ///   `:` 不展開變量, `!`/`!!` 權限語義 (本層忽略)
    /// - `"..."` 雙引號: 支持 `\\ \" \' \s \t \n \r` 轉義, 其餘 `\x` 保留字面;
    ///   `$VAR` 展開延後到執行時
    /// - `'...'` 單引號: 一切字面
    /// - 引號外 `\c` = 字面 c
    /// - 空引號 `""` 構成空參數
    pub fn parse(line: &str) -> Result<ExecCommand, ExecParseError> {
        let mut rest = line.trim();
        let mut cmd = ExecCommand {
            argv: Vec::new(),
            ignore_failure: false,
            full_privileges: false,
            no_env_expand: false,
        };

        // 前綴旗標: 僅行首的 -, +, :, ! 有旗標語義
        loop {
            let first = rest.chars().next();
            match first {
                Some('-') => cmd.ignore_failure = true,
                Some('+') => cmd.full_privileges = true,
                Some(':') => cmd.no_env_expand = true,
                Some('!') => {} // 權限語義, 純邏輯層不追蹤
                _ => break,
            }
            rest = &rest[1..];
        }

        cmd.argv = tokenize(rest)?;
        if cmd.argv.is_empty() {
            return Err(ExecParseError::EmptyCommand);
        }
        // '+' 前綴時允許簡單文件名 (執行層走 PATH 搜索); 否則必須絕對路徑
        if !cmd.full_privileges && !cmd.argv[0].starts_with('/') {
            return Err(ExecParseError::NonAbsolutePath(cmd.argv[0].clone()));
        }
        Ok(cmd)
    }
}

/// 引號感知分詞 (ExecCommand::parse 的核心)
fn tokenize(line: &str) -> Result<Vec<String>, ExecParseError> {
    #[derive(PartialEq)]
    enum St {
        Bare,
        Double,
        Single,
    }
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut st = St::Bare;
    let mut started = false; // 當前 token 是否已開始 (含空引號)

    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match st {
            St::Bare => match c {
                '"' => {
                    st = St::Double;
                    started = true;
                }
                '\'' => {
                    st = St::Single;
                    started = true;
                }
                '\\' => {
                    // 引號外: 反斜杠 + 任意字符 = 該字符字面 (行尾孤反斜杠保留字面)
                    if let Some(&n) = chars.peek() {
                        chars.next();
                        cur.push(n);
                        started = true;
                    } else {
                        cur.push('\\');
                        started = true;
                    }
                }
                c if c.is_whitespace() => {
                    if started {
                        tokens.push(std::mem::take(&mut cur));
                        started = false;
                    }
                }
                c => {
                    cur.push(c);
                    started = true;
                }
            },
            St::Double => match c {
                '"' => st = St::Bare,
                '\\' => {
                    // 雙引號內: \\ \" \' \s \t \n \r 轉義, 其餘 \x 保留字面
                    match chars.next() {
                        Some('\\') => cur.push('\\'),
                        Some('"') => cur.push('"'),
                        Some('\'') => cur.push('\''),
                        Some('s') => cur.push(' '),
                        Some('t') => cur.push('\t'),
                        Some('n') => cur.push('\n'),
                        Some('r') => cur.push('\r'),
                        Some(other) => {
                            cur.push('\\');
                            cur.push(other);
                        }
                        None => cur.push('\\'),
                    }
                }
                c => cur.push(c),
            },
            St::Single => match c {
                '\'' => st = St::Bare,
                c => cur.push(c),
            },
        }
    }
    if st == St::Double {
        return Err(ExecParseError::UnterminatedQuote('"'));
    }
    if st == St::Single {
        return Err(ExecParseError::UnterminatedQuote('\''));
    }
    if started {
        tokens.push(cur);
    }
    Ok(tokens)
}

// ─── ServiceConfig 提取 ──────────────────────────────────────────────

/// 工作目錄 (前導 `-` = 目錄不存在時不報錯)
#[derive(Debug, Clone, PartialEq)]
pub struct WorkingDirectory {
    pub path: String,
    pub ignore_missing: bool,
}

/// 從 `[Service]` 段提取的服務配置
#[derive(Debug, Clone, PartialEq)]
pub struct ServiceConfig {
    pub service_type: ServiceType,
    pub exec_start: Vec<ExecCommand>,
    pub exec_start_pre: Vec<ExecCommand>,
    pub exec_start_post: Vec<ExecCommand>,
    pub exec_stop: Vec<ExecCommand>,
    pub exec_stop_post: Vec<ExecCommand>,
    pub restart: RestartPolicy,
    /// 默認 100ms
    pub restart_sec: TimeSpan,
    pub remain_after_exit: bool,
    /// 默認 90s
    pub timeout_start_sec: TimeSpan,
    /// 默認 90s
    pub timeout_stop_sec: TimeSpan,
    pub kill_mode: KillMode,
    pub user: Option<String>,
    pub group: Option<String>,
    pub working_directory: Option<WorkingDirectory>,
    /// Environment= 條目 (每條 KEY=VALUE; 一行多條已拆開)
    pub environment: Vec<String>,
    /// EnvironmentFile= 原始值 (保留 `-` 前綴 = 不存在不報錯, 執行層處理)
    pub environment_files: Vec<String>,
    /// 非致命降級警告
    pub warnings: Vec<String>,
}

/// systemd 默認時長
pub const DEFAULT_TIMEOUT_SECS: f64 = 90.0;
pub const DEFAULT_RESTART_SECS: f64 = 0.1;

impl ServiceConfig {
    /// 從解析後的單元文件提取 [Service] 配置。
    ///
    /// 高兼容語義: 未知枚舉值、畸形時長、壞命令行均不致命 —
    /// 降級為默認值/跳過並記錄到 `warnings`。
    pub fn from_parsed(pu: &ParsedUnit) -> ServiceConfig {
        let mut warnings = Vec::new();
        let section = pu.section("Service");

        let get = |key: &str| section.and_then(|s| s.get(key));

        let service_type = match get("Type") {
            None => ServiceType::Simple,
            Some(v) => match ServiceType::parse(v) {
                Some(t) => t,
                None => {
                    warnings.push(format!("Unknown Type value {v:?}, falling back to simple"));
                    ServiceType::Simple
                }
            },
        };

        let restart = match get("Restart") {
            None => RestartPolicy::No,
            Some(v) => match RestartPolicy::parse(v) {
                Some(r) => r,
                None => {
                    warnings.push(format!("Unknown Restart value {v:?}, falling back to no"));
                    RestartPolicy::No
                }
            },
        };

        let restart_sec = parse_span_or(get("RestartSec"), DEFAULT_RESTART_SECS, &mut warnings);
        let timeout_start_sec =
            parse_span_or(get("TimeoutStartSec"), DEFAULT_TIMEOUT_SECS, &mut warnings);
        let timeout_stop_sec =
            parse_span_or(get("TimeoutStopSec"), DEFAULT_TIMEOUT_SECS, &mut warnings);

        let kill_mode = match get("KillMode") {
            None => KillMode::ControlGroup,
            Some(v) => match KillMode::parse(v) {
                Some(k) => k,
                None => {
                    warnings.push(format!(
                        "Unknown KillMode value {v:?}, falling back to control-group"
                    ));
                    KillMode::ControlGroup
                }
            },
        };

        let mut cfg = ServiceConfig {
            service_type,
            exec_start: Vec::new(),
            exec_start_pre: Vec::new(),
            exec_start_post: Vec::new(),
            exec_stop: Vec::new(),
            exec_stop_post: Vec::new(),
            restart,
            restart_sec,
            remain_after_exit: section.map(|s| s.get_flag("RemainAfterExit")).unwrap_or(false),
            timeout_start_sec,
            timeout_stop_sec,
            kill_mode,
            user: get("User").map(|s| s.to_string()),
            group: get("Group").map(|s| s.to_string()),
            working_directory: get("WorkingDirectory").map(|v| {
                if let Some(stripped) = v.strip_prefix('-') {
                    WorkingDirectory {
                        path: stripped.to_string(),
                        ignore_missing: true,
                    }
                } else {
                    WorkingDirectory {
                        path: v.to_string(),
                        ignore_missing: false,
                    }
                }
            }),
            environment: Vec::new(),
            environment_files: Vec::new(),
            warnings,
        };

        let mut collect_exec = |key: &str| -> Vec<ExecCommand> {
            let mut out = Vec::new();
            if let Some(s) = section {
                for raw in s.get_list(key) {
                    match ExecCommand::parse(raw) {
                        Ok(cmd) => out.push(cmd),
                        Err(e) => cfg
                            .warnings
                            .push(format!("Invalid {key} line {raw:?}: {e:?} (skipped)")),
                    }
                }
            }
            out
        };
        cfg.exec_start = collect_exec("ExecStart");
        cfg.exec_start_pre = collect_exec("ExecStartPre");
        cfg.exec_start_post = collect_exec("ExecStartPost");
        cfg.exec_stop = collect_exec("ExecStop");
        cfg.exec_stop_post = collect_exec("ExecStopPost");

        // Environment= 一行可含多條 (空格分隔, 可帶引號) — 引號感知拆分
        if let Some(s) = section {
            for raw in s.get_all("Environment") {
                match tokenize(raw) {
                    Ok(parts) if !parts.is_empty() => cfg.environment.extend(parts),
                    Ok(_) => cfg
                        .warnings
                        .push("Empty Environment= entry ignored".to_string()),
                    Err(e) => cfg
                        .warnings
                        .push(format!("Invalid Environment= line {raw:?}: {e:?} (skipped)")),
                }
            }
            cfg.environment_files = s.get_list("EnvironmentFile").iter().map(|s| s.to_string()).collect();
        }

        cfg
    }
}

fn parse_span_or(v: Option<&str>, default: f64, warnings: &mut Vec<String>) -> TimeSpan {
    match v {
        None => TimeSpan::Finite(default),
        Some(s) => match parse_time_span(s) {
            Ok(t) => t,
            Err(()) => {
                warnings.push(format!("Invalid time span {s:?}, falling back to {default}s"));
                TimeSpan::Finite(default)
            }
        },
    }
}

// ─── 退出後決策 ──────────────────────────────────────────────────────

/// 主進程退出/異常事件分類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitEvent {
    /// 退出碼 0
    CleanExit,
    /// 退出碼非 0 (不潔退出)
    UncleanExit,
    /// 被信號殺死 (不潔停止)
    Killed,
    /// 啟動超時
    StartTimeout,
    /// 看門狗超時
    Watchdog,
}

/// 服務終態 (不再重啟時)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceFinalState {
    /// 乾淨退出且不保持
    Dead,
    /// 失敗
    Failed,
    /// RemainAfterExit=yes 且乾淨退出
    Active,
}

/// 退出後動作
#[derive(Debug, Clone, PartialEq)]
pub enum AfterExitAction {
    /// 等待 `after` 後重啟 (單元先進 activating 狀態)
    Restart { after: TimeSpan },
    /// 進入終態
    Final(ServiceFinalState),
}

/// 依 systemd.service(5) 語義決定主進程退出後的動作。
///
/// RestartPolicy × ExitEvent 矩陣:
/// - no: 永不;always: 恆是
/// - on-success: 僅乾淨退出
/// - on-failure: 一切非乾淨 (不潔退出/信號/超時/看門狗)
/// - on-abnormal: 信號/啟動超時/看門狗
/// - on-abort: 信號/看門狗
/// - on-watchdog: 僅看門狗
pub fn decide_after_exit(cfg: &ServiceConfig, event: ExitEvent) -> AfterExitAction {
    use ExitEvent::*;
    let should_restart = match (cfg.restart, event) {
        (RestartPolicy::No, _) => false,
        (RestartPolicy::Always, _) => true,
        (RestartPolicy::OnSuccess, CleanExit) => true,
        (RestartPolicy::OnSuccess, _) => false,
        (RestartPolicy::OnFailure, CleanExit) => false,
        (RestartPolicy::OnFailure, _) => true,
        (RestartPolicy::OnAbnormal, Killed | StartTimeout | Watchdog) => true,
        (RestartPolicy::OnAbnormal, _) => false,
        (RestartPolicy::OnAbort, Killed | Watchdog) => true,
        (RestartPolicy::OnAbort, _) => false,
        (RestartPolicy::OnWatchdog, Watchdog) => true,
        (RestartPolicy::OnWatchdog, _) => false,
    };
    if should_restart {
        return AfterExitAction::Restart {
            after: cfg.restart_sec,
        };
    }
    AfterExitAction::Final(match event {
        CleanExit => {
            if cfg.remain_after_exit {
                ServiceFinalState::Active
            } else {
                ServiceFinalState::Dead
            }
        }
        _ => ServiceFinalState::Failed,
    })
}
