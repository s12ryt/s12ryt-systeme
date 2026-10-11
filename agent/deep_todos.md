# deep_todos — 完整歷史任務紀錄

## 輪次 1 (2026-10-11)
- [x] 需求訪談: 5 問 5 答 (見 question.md)
- [x] 建立 agent 四文件
- [x] cargo workspace 初始化 (members: systeme-units, systeme-core)
- [x] 單元解析器 TDD — 23 集成測試 + 3 單元測試
- [x] 單元目錄搜索與加載 TDD — 11 測試 (UnitFs/MemFs 抽象、四級搜索路徑、mask、alias、.d override)
- [x] 說明符展開 TDD — 12 測試 (%n %N %i %I %j %f、兩種 unescape、escape、split_instance、env vars)

## 輪次 2 (2026-10-11)
- [x] systeme-core crate 建立並加入 workspace members
- [x] 依賴圖構建 TDD — 24 集成測試 + 1 單元測試 (RED 先行: 先寫測試確認編譯失敗再實作)
  - DepTarget{-前綴 ignore_failure}、DirectDeps{after/before/requires/wants/requisite/binds_to/part_of/conflicts/upholds}
  - ReverseDeps{required_by/wanted_by/requisite_of/bound_by/consists_of/upheld_by/units_after_me/units_before_me}
  - Before 聲明反轉為目標 after 邊、Conflicts 對稱、RequiredBy/WantedBy/BoundBy/UpheldBy/ConflictedBy 反向聲明
  - 空白分詞多值語義 (Wants=a b)、空值清空 (get_list fold)、冪等覆蓋 (clear_forward_traces)
  - after_closure 傳遞閉包防環、ordering_cycles 迭代版 Tarjan SCC (size>1 或自環)
  - 佔位節點 (引用未定義單元自動建)
  - 修過的 bug: set_unit_deps 曾用 mem::take 拿走集合寫回空 deps; 反向聲明曾漏維護本單元 rdeps
- [x] 事務引擎 TDD — 28 集成測試 (RED 先行: E0432 模組不存在確認後實作)
  - JobType{Start,Stop,Reload}/JobMode{Fail,Replace}/UnitState trait/TransactionError (thiserror)
  - submit 遞歸展開: Requires/Wants/BindsTo/Upholds 拉起 Start;Requisite 只檢查 active 不拉起;Conflicts 加 Stop
  - Stop 傳播: required_by/bound_by/consists_of (Wants 不傳播;Requires 傳播方向=「誰 Requires 我」)
  - 兩階段合併 (先全量驗證後套用): Fail 衝突隊列零殘留;Replace 新替換舊
  - 環檢測: DependencyCycle (DFS path) / OrderingCycle (Tarjan SCC 涉及檢查)
  - ordered_jobs: Kahn 拓撲;非 Stop 對 After 正向 (被依賴者先)、Stop 對反向 (依賴者先停)、混合對無約束;BTreeSet ready 字典序確定性
  - 修過的問題: closure 捕獲 adj 的生命週期 E0521 (改直接 insert);done 借用衝突 E0502 (改 clone String);測試 tuple variant 模式改 struct variant;stop_replaces_full_chain 測試場景曾寫反 (a Requires=b 停 a 不傳播 b,正確是 b Requires=a 停 a 才傳播)

## 輪次 4 (2026-10-11)
- [x] Service 執行器配置層 TDD (systeme-core/src/service.rs) — 37 集成測試 (RED 先行: E0432 確認後實作)
  - ServiceType{Simple/Forking/Exec/Notify/Oneshot/DBus}.parse (未知→None);RestartPolicy{No/OnSuccess/OnFailure/OnAbnormal/OnAbort/OnWatchdog/Always};KillMode{ControlGroup/Mixed/Process/ProcessGroup/None}
  - ExecCommand::parse: 前綴旗標 -/+/:(!/!! 權限語義不追蹤)可組合;雙引號 \\ \" \' \s \t \n \r 轉義 (未識別保留字面);單引號全字面;引號外 \c=字面c;空引號=空參數;相鄰引號段拼接;argv[0] 需絕對路徑 (full_privileges 時允許文件名走 PATH);ExecParseError{EmptyCommand,NonAbsolutePath,UnterminatedQuote}
  - parse_time_span: 純數字=秒;s/sec/ms/us/m/min/h/d/w 後綴;空白分段相加 ("1min 30s"=90);infinity;壞值 Err
  - ServiceConfig::from_parsed: Type/Restart/KillMode 未知值→警告+默認;Exec* 多行保序、壞行→警告跳過;Environment 一行多條引號感知拆分;EnvironmentFile 保留 '-' 前綴;WorkingDirectory=-/path→ignore_missing;RestartSec 默認 100ms;TimeoutStart/StopSec 默認 90s
  - decide_after_exit(cfg,event): systemd RestartPolicy×ExitEvent{CleanExit/UncleanExit/Killed/StartTimeout/Watchdog} 矩陣 (on-abort 不含 timeout;on-abnormal 不含 unclean exit);重啟→Restart{after};終態: CleanExit+remain_after_exit→Active、CleanExit→Dead、其餘→Failed
  - 修過的問題: warnings move 後借用 E0382 (Environment 段改 push cfg.warnings);"10us" f64 乘法精度 9.99e-6≠1e-5 (測試改近似比較 abs<1e-12)

## 累計測試: 139 全綠 (units 49 + core 90: dep_graph 25 + transaction 28 + service 37)

## 輪次 5 (2026-10-11)
- [x] 進程管理純邏輯層 TDD (systeme-core/src/process.rs) — 19 集成測試 (RED: E0432 先行)
  - 信號表: signal_name 1-31 (Linux 通用信號);signal_from_name 接受 TERM/SIGTERM/大小寫/數字字串;垃圾返回 None
  - decode_wait_status: libc 標準位布局 (低7位信號/0x80 core/8-15位 exit code;0xffff=Continued;低字節 0x7f=Stopped) → WaitStatus{Exited,Signaled{core},Stopped,Continued}
  - waitstatus_to_exit_event: Exited 0→CleanExit、≠0→UncleanExit、Signaled→Killed、Stopped/Continued→None (非終態)
  - plan_kill(mode, procs): KillPlan{None,Pids,Mixed{term,kill}};ControlGroup=全體、Process=主進程、ProcessGroup=同 pgid、Mixed TERM 主進程/KILL 全體、None 不殺;pid 升序確定性;無 is_main 防禦返回 None
  - sys 子模組 (#[cfg(unix)]): set_subreaper (PR_SET_CHILD_SUBREAPER=36)、kill_pid、send_term→HashSet<pid>、send_kill (超時升級 SIGKILL)、resolve_signal — CI 容器驗證
  - 修過的問題: 私有 use 導致 E0603 (改 pub use);use+pub use 同名 E0252 (只留 pub use)
- [x] 修 service.rs 測試瑕疵: matches!(x, expect) 變量作綁定 pattern 恆真 → assert_eq! (ServiceType 已 derive PartialEq)

## 累計測試: 158 全綠 (units 49 + core 109: dep_graph 25 + transaction 28 + service 37 + process 19)

## 輪次 6 (2026-10-11)
- [x] cgroup 抽象層 TDD (systeme-core/src/cgroup.rs) — 22 集成測試 (RED: E0432 先行)
  - CgroupMode{V2,V1,None};detect_backend(&dyn Fn(&str)->bool): /sys/fs/cgroup/cgroup.controllers 存在→V2 (hybrid 優先) / memory 目錄→V1 / 否則 None
  - slice_parent: 最後一個「未轉義」'-' 分隔;a-b-c.slice→a-b.slice→…→-.slice;根 -.slice→None;\xHH 轉義內的 '-' 是字面量 (a\x2db.slice 是頂層)
  - slice_path 遞歸父鏈;default_cgroup_path(unit, slice_override): .slice 單元遞歸;其他掛 system.slice (可覆蓋) 下;不安全名 (空/含'/'/'..') →None
  - CgroupBackend trait {mode/create_group 冪等含父鏈/group_exists/attach_pid/list_pids 升序/kill_group 返回 pid 列表/remove_group}
  - MemCgroup 內存測試後端 (groups BTreeMap + signal_log);kill 不存在組→Ok(空);attach/remove 不存在→Err(NotFound)
  - NoCgroup no-op 降級後端 (kill_group 返回空 — 管理器退回 subreaper+進程追蹤,即「PID≠1/無 root 也能跑」的降級核心)
  - 真實 v2/v1 文件系統後端留待 manager 輪 #[cfg(unix)] 實現 (Windows 開發機看不見 /sys/fs,由 CI 容器驗證)
  - 重構: slice_parent 邊界條件 `i+3 < len+1` 冗餘寫法 → 統一 hex_ok (bytes.get(i+1)==x && len>=i+4 && 兩位 hexdigit) 單點判斷
- [x] lib.rs 接入 pub mod cgroup + re-export 全部公開 API

## 累計測試: 180 全綠 (units 49 + core 131: dep_graph 25 + transaction 28 + service 37 + process 19 + cgroup 22)

## 輪次 7 (2026-10-11)
- [x] 管理器核心狀態機 TDD (systeme-core/src/manager.rs) — 28 集成測試 (RED: E0432 ×2 先行)
  - RuntimeMode{System,User}::detect(is_root) — root→System、非 root→User,**不檢查 PID 1** (移除啟動限制核心)
  - ActiveState{Inactive/Activating/Active/Deactivating/AutoRestart/Failed}.as_str
  - UnitRuntime{state, main_pid, procs, pending_restart_in} 手寫 new (無 Default derive); UnitEntry{name, real_name, masked, parsed, config, runtime}.description()
  - trait ServiceExecutor{spawn/signal} 進程抽象 — 測試 Mock 記錄 spawns/signals 可注入 fail_marker
  - ManagerError{NotFound/Masked/StartFailed/Transaction(#[from])}; LoadReport{loaded,masked,dropped}
  - ManagerCore{mode, graph, units} 全 pub: load_all (枚舉目錄→去重→按真名入表; masked 條目空 config; old_runtimes 快照→重建→恢復運行時); reload=load_all
  - run_transaction: StateSnapshot 實現 UnitState → JobManager::new(graph.clone(), snapshot) → submit(Replace) → ordered_jobs
  - start_unit/stop_unit: 前置檢查→事務→逐 job apply_job 返回 touched 拓撲序
  - apply_start: 冪等;非 service(target 等)直達 Active 零 spawn;ExecStartPre 順序 ('-' 前綴失敗繼續、無前綴失敗→Failed);無 ExecStart: oneshot+remain→Active/oneshot→Inactive/其他→Err;ExecStart 逐行 spawn (末位 is_main, pgid=pid)→非 oneshot 即 Active (forking/notify 握手留 daemon 層)
  - apply_stop: ExecStop (失敗不阻斷)→plan_kill→Pids 全 TERM;Mixed: term 集 TERM、kill 集排除 term 成員後 KILL (同步簡化;兩階段 TERM→等→KILL 殘餘由 daemon 層)
  - on_exit: procs 移除 pid;非 main 退出不改狀態;main 退出→decide_after_exit: Restart{after}→pending_restart_in+AutoRestart;Final (Dead→Inactive/Failed)
- [x] IPC 協議 TDD (systeme-core/src/ipc.rs) — 7 集成測試
  - Request{ListUnits/Status{unit}/Start{unit}/Stop/Restart/Reload/IsActive/Cat/Enable/Disable/Mask/Unmask{unit}/DaemonReload} serde enum
  - UnitInfo{name,description,load_state,active_state,main_pid}; Response{Ok/Units/UnitStatus/Active/Cat{name,text}/Error{message}/NotImplemented{request}}
  - JSON 行協議 (D-Bus 後期);unit_info_of(&UnitEntry)->UnitInfo
- 修過的 bug: dep_graph 編輯事故 (多餘 }+contains 重複定義,恢復並保留 derive(Clone));UnitRuntime 無 Default→手寫 new;masked 條目 config=from_parsed(parse_unit(""));ipc 測試 Status 是 struct variant 不能裸字串反序列化→改 unit variant DaemonReload;reload_preserves 期望值修正 (b 先 spawn 佔 pid1);KillMode=mixed 同步執行 main 同收 TERM+KILL→manager Mixed 分支 KILL 排除 term 成員

## 累計測試: 215 全綠 (units 49 + core 166: dep_graph 25 + transaction 28 + service 37 + process 19 + cgroup 22 + manager 28 + ipc 7)

### 待辦 (守護進程二進制,下一階段)
- [ ] 真正守護進程 crate (tokio + Unix socket 監聽 + 真實 executor spawn + subreaper 事件循環 + cgroup v2/v1 文件系統後端 cfg(unix)) — manager 純邏輯層已就緒
- [x] systemctl CLI (輪次 8 完成,見下)
- [ ] CI workflow (.github/workflows 仍未建!)

## 輪次 8 (2026-10-11)
- [x] systemctl CLI TDD — 新 crate `crates/systemctl` (package/bin 名均 systemctl,二進制 drop-in 兼容);lib+bin 雙 target;29 測試 (cli 15 + client 13 + lib 1),RED (E0432 ×2) 先行
  - cli.rs: Command enum (Start/Stop/Restart/Reload{unit}/Status{unit:Option}/ListUnits/IsActive/Cat/Enable/Disable/Mask/Unmask{unit}/DaemonReload/Help/Version);parse_args: --version 旗標、'-' 開頭參數容忍跳過 (高兼容)、無參→Help;default_suffix: UnitType::split_name 無已知後綴→補 .service (foo.bar→foo.bar.service)
  - client.rs: trait IpcTransport{request};run_command→(String,exit_code): Ok→0、Units→動態列寬表格、UnitStatus→●/× status 塊 (Loaded/Active/Main PID)、Active→exit 0/3、Cat→文本、Error/NotImplemented→1、傳輸失敗→"Failed to connect to daemon"+1;Help/Version 零 IPC
  - UnixSocketTransport #[cfg(unix)]: JSON 行協議 (寫+\n+flush / BufReader read_line);socket_path: $SYSTEME_SOCKET > euid0→/run/systemd/private > $XDG_RUNTIME_DIR/systemd/private
  - main.rs: cfg(unix) parse→connect→run→ExitCode (parse 錯 exit 2;連不上且 Help/Version 本地輸出);cfg(not(unix)) 僅 help/version
  - 修過: macro_rules! 定義順序問題改普通 impl;#[cfg(unix)] use 修 unused import;tests/manager.rs 遺留 unused import 刪除;空殼文件 write 被拒改 edit
- [x] 根 Cargo.toml members += crates/systemctl

## 累計測試: 244 全綠零警告 (units 49 + core 166 + systemctl 29)

## 輪次 9 (2026-10-11)
- [x] 守護進程組裝 crate `crates/systeme-manager` TDD — 40 測試 (daemon 22 + cgroup_fs 15 + executor 3win/5unix), RED (E0583 ×3) 先行; workspace members += manager; workspace deps += tokio(1, rt-multi-thread/macros/net/time/signal/io-util)
  - daemon.rs: DaemonCore{manager, search_paths 全 pub} + handle(&Request, &mut dyn ServiceExecutor, &dyn UnitFs)→Response — ListUnits/Status/Start/Stop/Restart(stop+start, inactive 冪等→等價 start)/IsActive(僅 Active)/Cat(load_unit→Masked 報錯/原文)/DaemonReload(load_all 保留運行態)/Enable·Disable·Mask·Unmask·Reload(unit)→NotImplemented
  - cgroup_fs.rs: CgroupFsBackend{root, procs_file("cgroup.procs"v2 /"tasks"v1)};abs() 剝前導'/' 防 join 覆蓋;create_group 逐層冪等+'..'防禦;attach_pid 組須存在;list_pids 排序去重;remove_group 非空→Backend;kill_group_with 注入信號 fn 可跨平台測;組不存在 kill→Ok(空) 與 MemCgroup 一致;trait kill_group cfg(unix) 用 sys::kill_pid
  - executor.rs: StdCommandExecutor — Command::new(argv0).args(rest)+env+cwd+stdin null+cfg(unix) pre_exec setsid;返回 pid 不持 Child (收割歸 subreaper 循環)
  - 修過: Backend(m) partial move E0382→Backend(ref m);MemFs::symlink(path,target) 參數順序寫反致 evil.service 沒建 3 測試失敗;ExecCommand 4 字段字面量全列
- [x] 守護進程二進制 `crates/systeme-manager/src/bin/systemd.rs` (~250 行, cfg(unix) 事件循環)
  - tokio multi-thread runtime;--version 打印;--system/--user 等參數忽略 (模式自動探測: euid==0→system_search_paths / 否則 user_search_paths(XDG_CONFIG_HOME||HOME/.config, HOME))
  - set_subreaper best-effort 失敗僅警告;cgroup 模式探測打印 (v2/v1/none 降級提示)
  - socket_path 與 systemctl client 同邏輯: $SYSTEME_SOCKET > euid0→/run/systemd/private > $XDG_RUNTIME_DIR/systemd/private;父目錄 create_dir_all;殘留 socket 清理 (NotFound 容忍)
  - 事件循環 select!: accept→spawn handle_conn (JSON 行: BufReader lines→Request→core.lock→handle→Response+\n 寫回;惡意 JSON→Error response) / mpsc ChildExit(pid,raw)→unit_of_pid(main_pid 或 procs 含 pid)→on_exit→狀態變更打印 / 1s tick→pending_restart_in 遞減到期→start_unit (AutoRestart 生效) / SIGTERM·SIGINT 優雅退出+socket 清理
  - waitpid(-1,WNOHANG) 收割線程 200ms 輪詢→unbounded channel;發送端關閉自動退線程
  - tokio::sync::Mutex 跨 await;Arc<Mutex<DaemonCore>> 共享
  - cfg(not(unix)): 僅 --version,否則提示需 Unix 退出 1
  - Windows 編譯驗證通過 (unix 分支待 CI Linux 驗證)
- [x] agent 文件同步;修過: DaemonCore::new 簽名是 (ManagerCore, SearchPoints) 非 (mode, sp) → ManagerCore::new(mode) 先建

## 累計測試: 284 全綠零警告 (units 49 + core 166 + systemctl 29 + manager 40: daemon 22 + cgroup_fs 15 + executor 3)

### 下一階段
- [ ] **CI workflow (.github/workflows) — 最高優先**: unix 分支代碼 (bin/systemd.rs、sys 模組、UnixSocketTransport) 從未在 Linux 編譯過;容器 ubuntu:24.04 驗證 cargo test + 啟動 daemon (PID≠1) + systemctl 對接集成測試
- [ ] Calendar/timer → socket 激活 → 其餘單元類型 → journald/journalctl → networkd/resolved → logind/udevd → 輔助工具 → README

## 輪次 10 (2026-10-11)
- [x] CI workflow `.github/workflows/ci.yml` — 三 job:
  - test-linux: ubuntu-latest + container ubuntu:24.04 (PID 1 非 systemd = 項目目標環境);rustup 裝 stable;cargo build --workspace --all-targets + cargo test --workspace (全部 cfg(unix) 分支/測試首次在 Linux 編譯執行);RUSTFLAGS=-D warnings
  - integration (needs test-linux): release 構建 → 冒煙腳本: SYSTEME_SOCKET=/tmp/systeme-test/private.sock;寫 hi.service (oneshot+RemainAfterExit+ExecStart=/bin/sh -c 'echo hello-from-hi') 與 sleeper.service (/bin/sleep 300) 到 ~/.config/systemd/user;`systemd --user &` (非 PID 1) → kill -0 存活檢查 → systemctl list-units/start hi/sleep 1 (等收割線程 200ms 輪詢)/is-active hi=active/status → start sleeper/is-active/stop → is-active 期望 exit 3 → daemon-reload → cat hi | grep hello-from-hi → kill -TERM daemon → wait → socket 已清理斷言
  - test-windows: windows-latest 純邏輯層回歸 (dtolnay/rust-toolchain@stable)
- [x] bin/systemd.rs 增强: --user/--system 顯式模式參數 (force_user/force_system 覆蓋自動探測;與真 systemd --user 語義對齊);User 模式即使 root 也用 user_search_paths (CI 容器 root 跑 --user 的關鍵)
- [x] 設計檢查: parser 整行 trim 後判段頭 (heredoc 前導縮進安全);load_all 對不存在目錄 continue (容器無 /usr/lib/systemd/user 安全);ExecStart 單引號 token 進 argv;stop 後 socket 清理斷言失敗會暴露
- [x] 修過: user_search_paths(xdg_config_home, home) 參數是 Option<&Path> 非 &Path (改 .ok().map(PathBuf).as_deref());刪除 bin 冗餘 BTreeMap/UnixStream imports (避免 -D warnings 殺 CI);Actions 容器默認 root 跑 → daemon 需 --user 強制 (否則進系統模式搜索路徑對不上)
- [x] 驗證: cargo build --workspace --all-targets 零警告;cargo test --workspace 284 全綠;clippy 有 ~12 個非阻塞建議 (CI 不跑 clippy;留作後續清理輪)

## 累計測試: 284 全綠零警告 (units 49 + core 166 + systemctl 29 + manager 40)

### 下一階段
- [ ] 用戶推送 GitHub 觸發 CI 首跑,依結果修 unix 分支問題 (本地 Windows 無法驗證 cfg(unix) 編譯)
- [ ] Calendar/timer → socket 激活 → 其餘單元類型 → journald/journalctl → networkd/resolved → logind/udevd → 輔助工具 → README

## 輪次 11 (2026-10-11)
- [x] TDD Calendar 事件解析器 (systeme-core/src/calendar.rs + tests/calendar.rs, 36/36):
  - CalendarSpec{dow(0=Mon..6=Sun)/year/month/day/hour/minute/second: Option<Vec<_>>}, None=*
  - parse_calendar: 特殊值 daily/hourly/weekly/monthly/yearly|annually/quarterly/semiannually 直接展開;一般 [DOW][DATE][TIME] 按 token 形式識別 (含':'或純數字=時間、含'-'或'*'=日期、字母=星期),位置無關,重複部分報錯,UTC 後綴忽略
  - 字段語法: */單值/..b/..b/n/*/n//n(到字段上限)/逗號列表;月名 Jan..Dec、星期名 Mon..Sun 大小寫不敏感 (前綴匹配接受全名);值域檢查 (year 1970-9999, dow 1-7, month 1-12, day 1-31, hour 0-23, min/sec 0-59), 步進 0 拒絕
  - 缺省補值: DATE Y-M-D 1-3 段 (缺月/日補 1 單值)、TIME H:M:S 1-3 段 (缺分/秒補 0 單值)、時間整體省略=00:00:00;* 段傳播為 None 不展開
  - next_elapse(spec, after): after+1s 起逐日掃 (上限 20000 天),date_matches(年月日+dow 集合) 通過後三層小時/分/秒候選找第一個 >= 當日已過時刻;嚴格大於 after;固定過去日期→None;Feb 29 自動跳平年
  - 修過: chrono 0.4.45 方法名 num_days_from_monday 非 days_from_monday;parse_item 初版 *//n 終點用 u32::MAX 致值域檢查失敗→加 min/max 參數;DATE/TIME 段內 * 誤展開成值集→改 None 傳播
  - 新依賴: chrono 0.4 (default-features=false, clock+std) 加入 workspace.dependencies 與 systeme-core
- 驗證: cargo test --workspace = 320 全綠零警告 (units 49 + core 202 + systemctl 29 + manager 40)

## 輪次 12 (2026-10-11)
- [x] TDD timer 單元 (systeme-core/src/timer.rs + tests/timer.rs, 22/22; RED: E0432 先行):
  - TimerConfig{on_calendar: Vec<CalendarSpec>, on_boot_sec/on_startup_sec/on_unit_active_sec/on_unit_inactive_sec: Option<TimeSpan>, unit: Option<String>, accuracy_sec: TimeSpan, randomized_delay_sec: TimeSpan, persistent: bool, warnings: Vec<String>} derive(Debug,Clone,PartialEq) + 手寫 Default
  - from_parsed: missing [Timer] 段→默認無警告;OnCalendar 多行多值逐條 parse_calendar 失敗警告跳過;TimeSpan 字段 infinity/壞值→警告+None (parse_span_field);AccuracySec 默認 Finite(60.0)、RandomizedDelaySec 默認 Finite(0.0) 壞值保持默認;Unit 空值忽略;Persistent=get_flag
  - TimerAnchors{boot, unit_active, unit_inactive, daemon_startup: Option<NaiveDateTime>} derive(Default)
  - next_timer_fire(cfg, now, anchors) -> Option<NaiveDateTime>: 嚴格 > now;calendar 逐 spec next_elapse;四對 (span,anchor) 錨點+時長候選 (Finite 才算, checked_add_signed 毫秒);錨點缺失/候選過期→忽略;取最早
  - 修過: **lib.rs edit 事故** — 用猜測的 service re-export 列表做前綴替換 oldString,與實際內容不對齊→重複拼接+引入不存在的 WaitStatus/漏 ServiceFinalState;修復=先讀全文精確對齊。教訓: edit 前 oldString 必須與文件實際內容精確對齊,不確定時先讀
- 驗證: cargo test --workspace = **342 全綠零警告** (units 49 + core 224: lib 1+calendar 36+cgroup 22+dep_graph 24+ipc 7+manager 28+process 19+service 37+timer 22 + systemctl 29 + manager 40)

### 下一輪
- [ ] manager timer 集成: bin/systemd.rs tick 擴展 — 掃 .timer 單元 TimerConfig::from_parsed、維護 next_fire 快取、到期 start_unit(Unit= 或同名 .service)、觸發後更新 unit_active/inactive 錨點
- [ ] 之後: socket 激活 → 其餘單元類型 → journald/journalctl → networkd/resolved → logind/udevd → 輔助工具 → README+clippy

## 輪次 13 (2026-10-11)
- [x] TDD manager timer 集成 (systeme-manager/src/timers.rs + tests/timers.rs, 15/15; RED: E0432 先行):
  - TimerScheduler{boot, daemon_startup, per_timer: BTreeMap<String,TimerTracking>, unit_anchors, last_states} 純邏輯可跨平台測
  - TimerTracking{cfg, target, next: NextFire}; NextFire{NotComputed/At(t)/Never}: NotComputed 初始化時做「過期立即觸發」檢查;Never=已算無未來觸發點 (config 變化或錨點失效才重算)
  - due_timers(&ManagerCore, now)->Vec<(timer,target)>: (1) 邊沿檢測 update_edges — 對比上次觀察 ActiveState::Active 布爾,false→true 記 active_at/true→false 記 inactive_at;首次觀察 active 近似記 now (不覆蓋外部 record_activation 的精確值);(2) prune_tracked 清已移除單元;(3) 逐 timer (BTreeMap 字典序): TimerConfig::from_parsed 即場解析,cfg/target 變化→重置追蹤;NotComputed: expired_span_candidate (錨點+span<=now,含等於) 或 calendar_due_now (next_timer_fire(now-1s)<=now — 首追蹤恰在 elapse 點立即觸發,繞過嚴格>語義) → At(now) 立即 due;否則 next_timer_fire → At/Never;At(t) 且 t<=now → due+觸發後重算 (嚴格>now 不重複)
  - record_activation/deactivation 公開 API (錨點記錄+失效依賴該 target 的 timer→NotComputed;目標重啟邊沿自動重新武裝 timer)
  - systemd 語義驗證: OnBootSec 錨點過期立即觸發一次後 Never (不重複);OnUnitActiveSec 目標重啟後重新武裝;masked timer 跳過;daemon-reload 換 calendar 重置
  - detect_boot: /proc/uptime 第一字段秒 → now-uptime;失敗用 now 近似
  - 修過: E0502 anchors_for(&self) 與 per_timer get_mut 借用重疊 → anchors 提前算好再取可變引用;測試 daily 誤以為 10:00 觸發 (實為每天 00:00) → 改明確 *-*-* 10:00:00;首次 tick 恰等於觸發時刻被嚴格>語義跳到明天 → calendar_due_now(now-1s) 檢查
- [x] bin/systemd.rs 接線 (cfg(unix)): run() 建 TimerScheduler::new(detect_boot, now);tick 分支 fire_due_timers — due_timers → 逐 (timer,target) eprintln+manager.start_unit;chrono::Local::now() 做時鐘 (calendar daily 語義用本地時區)
- 驗證: cargo test --workspace = **357 全綠零警告** (units 49 + core 224 + systemctl 29 + manager 55: daemon 22 + cgroup_fs 15 + executor 3 + timers 15)

## 輪次 14: socket 激活 core 純邏輯層 (TDD) — workspace 372 綠
- crates/systeme-core/src/socket.rs + tests/socket.rs (15/15)
- SocketConfig::from_parsed: 直接遍歷 [Socket] 段原始 entries (Section.entries pub) 實現跨 ListenStream/Datagram/FIFO/Special 保序; 空值清空同 key 累積 (retain proto); Accept 壞值→警告+false; Backlog 壞值→警告+128; Service 空值忽略
- ListenProto{Stream,Datagram,Fifo,Special}; ListenKind{Port(u16),AddrPort{addr,port}(rsplit_once ':' 右段 u16, 左剝[]),Path(完整含/),Abstract(剝@),Network(原值兜底: 99999/a:notaport/空)}; classify_listen 公開
- socket_service_target: Service= 優先否則剝 .socket→.service; listen_fds_env(n)= [("LISTEN_FDS", n)] (fd 從 3 起 sd_listen_fds 協議, LISTEN_PID 執行時填)
- DEFAULT_BACKLOG=128; lib.rs re-export 全套
- 修: (1) match 分支 Backlog 缺 continue → E0308 () vs ListenProto; (2) strip_prefix('/') 丟前導斜杠 → Path("run/f") 改 starts_with+完整值
- 真實監聽/fd 傳遞/Accept=yes per-connection spawn 留 bin 層 cfg(unix) 下輪

## 輪次 15: mount/automount/swap/path 單元 core 層 (TDD) — workspace 397 綠
- crates/systeme-core/src/mount.rs + tests/mount.rs (11/11)
  - MountConfig (What/Where/Type/Options 逗號拆分多行累積/LazyUnmount 壞值警告/DirectoryMode); SwapConfig (Priority 壞值警告+None); AutoMountConfig (TimeoutIdleSec TimeSpan)
  - path_body: 剝前導+尾斜杠再 escape_unit_str(path_mode), 根→\"-\"; mount_unit_name/swap_unit_name/automount_unit_name; automount_target (.automount→.mount)
  - 修: escape_unit_str 把前導 / 轉 '-' 是錯的, systemd 語義剝前導 / → path_body 先 strip_prefix
- crates/systeme-core/src/path.rs + tests/path.rs (14/14)
  - PathTriggerKind{Exists,ExistsGlob,Changed,Modified,DirectoryNotEmpty}; 遍歷原始 entries 保序+空值清空同 kind; path_target (Unit= 或 .path→.service)
  - glob_match 雙指針回溯 (* 任意序列/? 單字符); PathSnapshot{exists,mtime,dir_nonempty}; PathProbe trait{snapshot/glob}
  - PathScheduler: Watch{Level(bool) 電平 false 起步=激活已滿足即觸發 / Stamp(Option<u64>) mtime 邊沿首輪 baseline}; due_paths 只掃 active .path 單元; cfg/target 變化重置; prune 消失單元; 字典序
- target/slice/scope/device 無配置文件 — manager apply_start 已直達 Active (既有行為)
- 決策: bin 層 unix 接線 (socket 監聽+fd 傳遞、mount(2)、path OsProbe、automount) 集中一輪統一寫 (全部 cfg(unix) 只能編譯驗證, 靠 CI 修)

## 輪次 16: journald 核心 (TDD) — 新 crate systeme-journal — workspace 416 綠
- crates/systeme-journal: Cargo.toml (chrono/serde/serde_json ws + dev tempfile); 根 Cargo.toml members 加入
- src/entry.rs: JournalEntry{message, priority:u8, timestamp_us:u64, unit/pid/identifier} derive(Serialize/Deserialize); DEFAULT_PRIORITY=6; priority_name(p)->&'static str [emerg..debug] (min(7) 鉗制)
- src/native.rs: parse_native(&[u8])->Vec<JournalEntry> — journald native 協議: 逐字節掃; 空行=條目結束; 行含 '=' → 文本 FIELD=value; 無 '=' 且剩餘 >=9 → 二進制 FIELD\n+8LE 長度+data+\n; 殘段跳過; 末尾殘餘容忍。entry_from: MESSAGE 必需否則丟棄; PRIORITY 0-7 才接受否則默認 6; __REALTIME_TIMESTAMP (u64 微秒)/_PID/_SYSTEMD_UNIT/SYSLOG_IDENTIFIER
- src/syslog.rs: parse_syslog(line, default_year)->Option<JournalEntry> — RFC3164: '<PRI>' 剝 PRI%8 severity; 'Mmm dd hh:mm:ss host tag[pid]: msg' splitn 解析; MONTHS 表; timestamp = NaiveDate::from_ymd_opt(year,..)+and_hms_opt→and_utc().timestamp_micros(); split_tag: find(': ') 剝 msg, strip_suffix(']')+find('[') 拆 name/pid; 畸形返回 None
- src/rate.rs: RateLimiter{burst, interval_ms, window_start_ms, used} 固定窗口; allow(now_ms): now >= window_start+interval → 滾動重置; used<burst → 放行計數; 否則拒絕
- src/store.rs: trait JournalSink{append}; MemorySink{entries: Rc<RefCell<Vec>>} derive(Default,Clone) (clone 共享觀察); JsonlSink{file}::create (create+append); JournalStore{sink, limiters: BTreeMap<String,RateLimiter>, burst, interval_ms, dropped_count}: submit(&entry, now_ms) — source key = unit→identifier→"_unknown" 每源獨立限流, 拒絕 dropped_count+1 返回 false; dropped() 查詢; read_jsonl(&Path)->Vec<JournalEntry> (open 失敗→空, 壞行 filter_map 跳過)
- tests/journal.rs 18/18 + rate.rs 內建 1 = 19 測試; RED 曾確認 E0433
- 驗證: cargo test --workspace = 416 全綠零警告 (units 49 + core 264: lib1+calendar36+cgroup22+dep_graph24+ipc7+manager28+mount11+path14+process19+service37+socket15+timer22+transaction28 + systemctl 29 + journal 19 + manager 55)
- 注意: /dev/log datagram socket 監聽 + journald daemon 二進制留 bin 層輪; journalctl 客戶端下一輪 (讀 JSONL + 過濾 -u/-p/--since + 輸出格式)

## 輪次 17: journalctl 完整實作 (TDD) — 新 crate `crates/journalctl` — workspace 442 綠
- crates/systemctl 同構: Cargo.toml (lib+bin 同名 journalctl, deps: systeme-journal path); 根 Cargo.toml members 加入 crates/journalctl (read+edit 工具)
- src/cli.rs: Query{units, priority_max, since, until, format, lines, reverse, follow, patterns} derive(Debug,Clone,PartialEq) + Default (Short/None/false); parse_args(&[String])->Result<Query,String>: split_once('=') 同時支援 -u name / --unit=name; -u/--unit 多次累積; -p/--priority 壞值 Err; --since/--until parse_datetime_arg 壞日期 Err; -o/--output 壞格式 Err; -n/--lines 非數字 Err; -r/-f 布爾; --no-pager 忽略; 未知 - 開頭旗標容忍跳過; 位置參數→patterns。value_of 閉包: 行內值或吃下一參數, 缺值 Err("missing value for {flag}")
- src/query.rs: run_query(&[JournalEntry], &Query)->String — JournalFilter 過濾→lines 尾部截取 (split_off)→reverse 倒序→format_entry 逐行 +'\n' collect; 無匹配空串
- src/main.rs: journal_path() = $SYSTEME_JOURNAL > $XDG_RUNTIME_DIR/systeme/journal.jsonl > /run/systeme/journal.jsonl; read_jsonl→空且非 follow→exit 1; run_query 打印; follow: 500ms 輪詢重讀打印增量 (跨平台簡化); 解析錯誤 exit 2
- systeme-journal filter.rs: JournalFilter{units, priority_max, since_us, until_us, patterns}+matches() AND 語義 (units 命中 unit 或 identifier; patterns 全部小寫子串); parse_priority (數字 0-7/名稱 panic·error·warn 別名); parse_datetime_arg (4 種日期時間格式+純日期→00:00:00, UTC 微秒)
- systeme-journal format.rs: OutputFormat{Short,Json,Cat}::parse; format_entry: Short "Oct 11 10:00:01 tag[pid]: msg" (identifier 優先 unit 兜底, 無 host 簡化); Json=serde 一行; Cat=message
- 修過: cli.rs 初版 `?` 對 Option<u64> 誤用 (q.until = parse..? 直接賦) — E0308, 改 Some(..ok_or_else(..)?)
- 測試: tests/cli.rs 21 (默認/兩形式/多次/p 數字名稱壞值缺值/since until iso/壞日期/o json cat 壞值/n/r/f/no-pager/位置 pattern/組合/Default 對齊) + tests/query.rs 7 (unit 過濾/tail3/reverse/short/json/cat/空) + systeme-journal tests/journalctl.rs 26 (filter/format 層) = crate 間接計 28 直屬
- 驗證: cargo test --workspace = 442 全綠 0 failed (journalctl 28 + systemctl 29 + core 236 + journal 45 + manager 55 + units 49); build --workspace --all-targets 零警告
- journald /dev/log daemon 監聽 + journalctl -f inotify 留 bin 層接線輪

## 輪次 18: bin 層 unix 接線集中輪 (TDD) — workspace 498 綠
- **StdCommandExecutor 加 pending_fds 欄位** (executor.rs): pub Vec<i32>; prepare_socket_fds 覆寫存入; cfg(unix) spawn 分支 sockets::inject_fds(&mut c, &pending_fds) (與 setsid pre_exec 共存); spawn 成功後全平台 clear (fd 消費不洩漏); 結構體從 unit struct 改帶欄位, new() 改 {pending_fds: Vec::new()}, bin/systemd.rs 3 處 `StdCommandExecutor` 值用法改 `::new()`
- **DaemonCore sockets 攔截** (daemon.rs): +pub sockets: BTreeMap<String, SocketActivation>; handle Start/Stop/Restart 改走 start()/stop() 分發: unit.ends_with(".socket"/".mount"/".automount") 攔截, 其餘原樣 manager.start_unit/stop_unit; Restart = stop+start (stop 非 Ok 提前返回)
- start_socket: not_found/masked 前置; sockets.contains_key 冪等 Ok; SocketConfig::from_parsed(entry.parsed)→bind_socket_unit 失敗 Error; 成功→manager.start_unit(unit) (非 service 直達 Active)→insert sockets→socket_service_target→exec.prepare_socket_fds(fds)→manager.start_unit(target) (提前啟動簡化, 惰性激活留 watcher); 事務閉包內 .socket 依賴不攔截 (簡化)
- start_mount: real_mount(MountConfig::from_parsed) 失敗 Error 不標 Active; 成功→manager.start_unit
- start_automount: 簡化 — automount_target(unit) 對應 .mount 存在則一併 start_mount, 自身 start_unit 直達 Active
- stop: .socket→sockets.remove (drop listeners) + manager.stop_unit; .mount→Active 才 real_umount (失敗 Error 狀態保持) 否則冪等; .automount 僅標 Inactive
- **journald bin** (systeme-journal): Cargo.toml [[bin]] journald; src/bin/journald.rs cfg(unix): UnixDatagram bind /dev/log (失敗退回 $XDG_RUNTIME_DIR/systeme-dev-log; $SYSTEME_DEV_LOG 覆蓋) → process_datagram → JournalStore(JsonlSink(journal_path()), burst 100/30s); --version; 非 unix 提示退出
- **lib.rs 共享函數** (systeme-journal): process_datagram(payload, default_year, store, now_ms)->bool (先 syslog 後 native, submit 限流); journal_path() = $SYSTEME_JOURNAL > $XDG_RUNTIME_DIR/systeme/journal.jsonl > /run/systeme/journal.jsonl; journalctl/src/main.rs 刪本地重複改用共享
- **bin/systemd.rs path 接線**: use systeme_core::path::PathScheduler + systeme_manager::OsProbe; main loop tick 分支 += fire_due_paths (due_paths(&manager, &OsProbe) → start_unit(target)); 順手刪 handle_conn 未使用 sp 變量 (CI -D warnings 會炸)
- 修過: tests/mounts.rs Windows unused import (real_mount/real_umount 只在 cfg(unix) 測試用) → use 拆 cfg; JournalStore::new burst 參數是 usize 測試簽名對齊
- 測試: tests/daemon_units.rs 8 (全平台 6: socket/mount not_found+masked+missing What+automount start/stop+stop inactive mount 冪等; cfg(unix) 2: socket_start_binds_and_activates_service + socket_restart_rebinds 待 CI; cfg(not(unix)) socket_start_unsupported_platform_errors) + executor.rs +1 (prepare_socket_fds_roundtrip) + journal tests/datagram.rs 6 (syslog/native/garbage/限流/多條/JSONL 落盤)
- 結果: cargo test --workspace = **498 全綠零警告** (units 49 + core 264 + systemctl 29 + journal 51: lib1+datagram6+journal18+journalctl26 + journalctl 28 + manager 77: cgroup_fs15+daemon22+daemon_units8+executor4+mounts4+os_probe9+timers15); build --workspace --all-targets 零警告
- 待 CI (unix 專屬): daemon_units 2 socket 測試 + mounts 3 真掛載 + sockets 8 監聽測試

## 輪次 19: networkd (crates/systeme-network, 48 測試)

- 純邏輯層 (Windows 可測): src/network.rs (NetworkMatch/DhcpMode/NetworkConfig/match_iface AND-OR glob 匹配/network_apply_plan 順序 dhcp→address→route→dns); src/link.rs (LinkConfig + parse_size_bytes k/m/g 後綴); src/netdev.rs (NetDevConfig + NetDevKind 11 種); src/apply.rs (plan_to_argv→ip 命令向量/list_interface_names(/sys/class/net)/apply_steps 首個匹配/netdev_create_argv/link_apply_argv)
- bin systemd-networkd (cfg(unix)): $SYSTEMD_NETWORK_DIR >/etc/systemd/network, *.netdev 先建 → *.link 鏈路層 → *.network 地址路由; --dry-run; DHCP/DNS 行打印 skip; 非 unix 提示退出
- 測試: tests/networkd.rs 29 + tests/apply.rs 17 + lib 2 = 48; workspace 546 全綠零警告
- 修: DhcpMode 需 derive(Default)+#[default] No; bin 的 Command/PathBuf/config_dir 需 cfg(unix) 否則 Windows unused warning

## 輪次 20: resolved (systeme-network/src/resolved.rs + bin/systemd-resolved.rs)
- RED: tests/resolved.rs 11 測試 E0432 確認後實作一次 GREEN
- src/resolved.rs: ResolvConf{nameservers,search,options} derive(Default,PartialEq,Eq); parse_resolv_conf (行 trim, #/; 註釋跳過, nameserver/search|domain/options, 未知指令忽略); stub_resolv_conf()→[127.0.0.53]; render_resolv_conf (頭註釋+nameserver/search/options 行); collect_dns(&[NetworkConfig]) BTreeSet 去重保序
- lib.rs: pub mod resolved + re-export 全套
- bin/systemd-resolved.rs + Cargo.toml [[bin]]: cfg(unix) upstream_dns ( 逗號分隔 > 配置目錄 .network DNS= 聚合 collect_dns > fallback 8.8.8.8/1.1.1.1); 寫  或 /run/systemd/resolve/stub-resolv.conf (render stub); bind 127.0.0.53:53 失敗退 127.0.0.1:5353; loop recv_from→forward (逐上游 bind 0.0.0.0:0+connect+send+2s timeout+recv) → send_to 客戶端; --version/-V; 非 unix 提示
- 驗證: cargo test --workspace 557 全綠 (network 59: lib2+networkd29+apply17+resolved11); build --all-targets 零警告
- systemd 真實語義確認: stub 監聽 127.0.0.53:53, stub-resolv.conf 指向自身, 上游失敗逐個嘗試

## 輪次 21: logind 基礎 (crates/systeme-login, 26 測試)
- RED: tests/sessions.rs (14) + tests/cli.rs (9) E0433 確認後實作
- src/sessions.rs: SessionState{Online,Active,Closing}::as_str; Session{id,user,uid,seat,tty,state,locked,idle}; Sessions{next_id 單調,BTreeMap}: create(user,uid,seat,tty)→新會話 Active+同席位舊 Active 降 Online; close→移除+同席位最新非 Closing 提升 Active (next_back); active_on_seat/list(BTreeMap 序)/sessions_of_user/set_locked/set_idle; find_seat_of_session
- src/lib.rs SeatKind::classify: "seat"+非空純數字→Physical 其餘 Virtual (seat-1/seat/autovt 均 Virtual)
- src/cli.rs: Command{ListSessions,SessionStatus,ShowUser,Lock,Unlock,Terminate,Version}; parse_args 未知旗標容忍、無參→ListSessions、--version、未知命令 Err; exit 2
- src/render.rs: render_sessions_table (SESSION UID USER SEAT TTY 表)、render_session_detail (Id/User/UID/Seat/TTY/State/LockedHint/IdleHint 塊) — GREEN 補測 tests/render.rs (3)
- bin loginctl: 查詢命令對本地空 Sessions 渲染 (守護進程 IPC 集成留後續), 查詢失敗 exit 1
- 修過: positional Vec<&String> match 解構出 &&String → to_string(); 測試函數名與導入衝突重命名; 測試語義兩處 (首會話應 Active 非 Online; seat-1 非 seat<數字> 應 Virtual); render LockedHint 大小寫; unused import SessionState
- 驗證: workspace 583 全綠零警告 (557+26)

## 輪次 22: udevd 基礎 (crates/systeme-udev, 10 測試)
- RED: tests/udev.rs E0433 確認後實作
- src/lib.rs: Uevent{action,devpath,subsystem,devname,extra:BTreeMap}; parse_uevent (\0 分隔 KEY=VALUE, 必須含 DEVPATH 否則 None); DeviceRecord; UdevDb{BTreeMap<devpath,record>}: apply (remove→刪, 其餘 upsert)/get/len/records/by_subsystem/trigger_plan(action) (重放 ACTION=<action> 文本); scan_sysfs(root) 遞歸收集 uevent 文件 (顯式 stack 迭代, 排序確定性)
- bin systemd-udevd (Cargo.toml [[bin]] + libc 依賴): cfg(unix) --trigger=掃 /sys 重建 db 打印設備; netlink_socket() libc 裸調用 AF_NETLINK/SOCK_RAW|CLOEXEC/NETLINK_KOBJECT_UEVENT(15)/bind nl_groups=1, fd 包 FromRawFd 借管; loop recv→parse→apply→打印; 不可用退提示; --version/-V; 非 unix 提示 exit 1
- 驗證: workspace 593 全綠零警告 (583+10)

## 輪次 23: 輔助工具 systemd-escape / systemd-analyze (crates/systeme-tools, 14 測試)
- RED: tests/tools.rs E0432 確認後實作一次 GREEN
- src/escape.rs run(args, flags): 轉義: 參數 '/' 開頭→路徑模式 (trim_end '/'、剝前導 '/'、escape_unit_str(true)); 否則模板名模式 (false); --suffix=X 加 .X; --template=fsck@.service split_once('@') 組合 prefix@body+suffix; 反轉義 --unescape: 剝 --suffix 後綴或 --instance 取 @ 後部分→unescape_unit_path→補前導 '/'; 空參→空串
- src/analyze.rs: format_duration (<1s→"50ms"; >=60s→"1min 1s"; 整數→"3s"; 否則 "3.2s"); blame(entries) 降序 join; run(args) blame/time/critical-chain/dot 無守護進程提示 + 未知命令
- bin systemd-escape + systemd-analyze (Cargo.toml 兩 [[bin]]; --version/-V; 旗標/位置分離)
- 複用 systeme-units::specifier::{escape_unit_str, unescape_unit_path} 零重複代碼
- 驗證: workspace 607 全綠零警告 (593+14)

## 輪次 24: clippy 清理 + CI 擴充 + README (最終輪)
- clippy 零警告達成: cargo clippy --fix 自動修 3 文件後手動修 8 條 — core/journal/manager lib.rs doc 列表 `[x]:` 改 `[x] —` (link reference lint); service.rs parse_time_span +#[allow(clippy::result_unit_err)] (API 刻意單元錯誤); transaction.rs ordered_jobs doc 列表後加空行 (list item continuation lint); journalctl tests/query.rs 6 處 Default 賦值改 struct 更新語法 + query() 輔助去 mut
- CI integration job 擴充: +greet.timer (OnBootSec=1s 過期立即觸發) / greet.service / app.socket (ListenStream=/tmp/xdg/app.sock) / app.service 單元; 冒煙段: start timer→sleep 3→is-active greet.service; start app.socket→test -S→is-active app.service→stop; journald bin 啟動+kill -0+test -S dev-log+TERM; systemd-escape --suffix=mount /sys/fs/cgroup = sys-fs-cgroup.mount; cp 改全文件
- README.md 新建: 項目簡介/PID≠1 設計/雙模式表/高兼容說明/crates 表/功能矩陣/快速開始/環境變量/測試與 CI 三 job/與 systemd 差異簡化項
- 驗證: cargo clippy --workspace --all-targets 零警告 (exit 0); cargo test --workspace 607 全綠 0 failed; cargo build --workspace --all-targets 零警告 (exit 0)
