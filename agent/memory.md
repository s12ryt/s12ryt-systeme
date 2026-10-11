# memory — 項目操作紀錄

## 2026-10-11
- READ 工作區目錄 → 空項目,全新開始
- WRITE agent/question.md (需求問答+驗收標準)
- WRITE agent/項目表.md (結構規劃)
- WRITE agent/deep_todos.md
- WRITE agent/memory.md (本文件)
- 計劃: cargo workspace + TDD 實作
- WRITE Cargo.toml (workspace 根) / crates/systeme-units/* (parser/types/search/specifier + 49 測試)
- WRITE crates/systeme-core/{Cargo.toml,src/lib.rs,src/dep_graph.rs,tests/dep_graph.rs}
- EDIT Cargo.toml (members += systeme-core)
- EDIT agent/deep_todos.md / 項目表.md (依賴圖完成回填)
- WRITE crates/systeme-core/src/transaction.rs + tests/transaction.rs (事務引擎, 28 測試)
- EDIT crates/systeme-core/src/lib.rs (pub mod transaction + re-export)
- EDIT agent/deep_todos.md / 項目表.md (事務引擎完成回填; workspace 102 綠)

## 輪次 4 (2026-10-11)
- 寫入 crates/systeme-core/src/service.rs (Service 執行器配置層 ~500 行)
- 寫入 crates/systeme-core/tests/service.rs (37 測試)
- 編輯 crates/systeme-core/src/lib.rs (pub mod service + re-export)
- 編輯 agent/deep_todos.md (輪次 4 回填)
- 測試: workspace 139 全綠

- 輪次 5: 寫入 crates/systeme-core/src/process.rs、tests/process.rs (19 測試); 編輯 tests/service.rs (matches→assert_eq); workspace 158 全綠
## 輪次 6 寫入 (2026-10-11)
- EDIT crates/systeme-core/src/lib.rs: + pub mod cgroup + re-export (CgroupMode, detect_backend, slice_parent, default_cgroup_path, CgroupBackend, CgroupError, MemCgroup, NoCgroup)
- WRITE crates/systeme-core/src/cgroup.rs (266 行, 前一輪已寫)
- WRITE crates/systeme-core/tests/cgroup.rs (22 測試, 前一輪已寫並確認 RED)
- EDIT crates/systeme-core/src/cgroup.rs: slice_parent 重構 (冗餘邊界條件 → hex_ok 單點判斷)
- EDIT agent/deep_todos.md: 輪次 6 回填
- cargo test --workspace: 180 全綠 (units 49 + core 131)

## 輪次 7 寫入 (2026-10-11)
- WRITE crates/systeme-core/src/manager.rs (管理器核心狀態機 ~600 行)
- WRITE crates/systeme-core/tests/manager.rs (28 測試)
- WRITE crates/systeme-core/src/ipc.rs (IPC 協議)
- WRITE crates/systeme-core/tests/ipc.rs (7 測試)
- EDIT crates/systeme-core/src/lib.rs (pub mod manager + pub mod ipc + re-export)
- EDIT crates/systeme-core/src/dep_graph.rs (DependencyGraph +derive(Clone); 修復編輯事故: 多餘 }+contains 重複定義)
- EDIT agent/deep_todos.md (輪次 7 回填)
- cargo test --workspace: 215 全綠 (units 49 + core 166)

## 輪次 8 寫入記錄 (2026-10-11)
- 新建: crates/systemctl/Cargo.toml、src/lib.rs、src/cli.rs、src/client.rs、src/main.rs、tests/cli.rs(15)、tests/client.rs(13)
- EDIT: 根 Cargo.toml (members += crates/systemctl)、agent/deep_todos.md (輪次8回填)、agent/項目表.md (systemctl 行 ✅)
- 測試: cargo test --workspace — 244 全綠零警告

## 輪次 9 寫入記錄 (2026-10-11)
- W 根 Cargo.toml: members += crates/systeme-manager; workspace.dependencies += tokio 1 (rt-multi-thread/macros/net/time/signal/io-util)
- W crates/systeme-manager/Cargo.toml, src/lib.rs, src/daemon.rs, src/cgroup_fs.rs, src/executor.rs
- W crates/systeme-manager/tests/daemon.rs (22), tests/cgroup_fs.rs (15), tests/executor.rs (3win/5unix)
- W crates/systeme-manager/src/bin/systemd.rs — 守護進程入口 (cfg(unix) tokio 事件循環 / cfg(not(unix)) 提示)
- E agent/deep_todos.md 輪次 9 回填; E agent/項目表.md manager 標 ✅ (systeme-exec 標併入)
- 驗證: cargo test --workspace = 284 全綠零警告

## 輪次 10 寫入記錄 (2026-10-11)
- W .github/workflows/ci.yml — 三 job: test-linux (容器 ubuntu:24.04 全量測試) / integration (daemon --user 非 PID1 啟動+systemctl 冒煙) / test-windows
- E crates/systeme-manager/src/bin/systemd.rs — --user/--system 顯式模式參數; user_search_paths Option 參數修正; 刪冗餘 imports
- E agent/deep_todos.md 輪次 10 回填
- 驗證: cargo build --all-targets 零警告; cargo test --workspace 284 全綠

## 輪次 11 寫入記錄 (2026-10-11)
- E Cargo.toml [workspace.dependencies] += chrono; E crates/systeme-core/Cargo.toml += chrono
- W crates/systeme-core/src/calendar.rs (CalendarSpec/CalendarError/parse_calendar/parse_field/parse_item/parse_val/next_elapse/date_matches)
- W crates/systeme-core/tests/calendar.rs (36); E crates/systeme-core/src/lib.rs (pub mod calendar + re-export)
- 驗證: cargo test --workspace = 320 全綠零警告

### 2026-10-11 輪次 14 寫入
- 新建 crates/systeme-core/src/socket.rs (~190行)、tests/socket.rs (15 測試)
- EDIT crates/systeme-core/src/lib.rs: pub mod socket + re-export (classify_listen, listen_fds_env, socket_service_target, ListenKind, ListenProto, ListenSpec, SocketConfig, DEFAULT_BACKLOG)
- 全程 TDD: RED (E0432) → GREEN (E0308 continue 修復、strip_prefix 斜杠修復) → 15/15 → workspace 372 綠零警告

### 2026-10-11 輪次 15 寫入
- 新建 crates/systeme-core/src/mount.rs (~230行)、src/path.rs (~290行)、tests/mount.rs (11)、tests/path.rs (14)
- EDIT lib.rs: pub mod mount/path + re-export (mount: automount_target/automount_unit_name/mount_unit_name/swap_unit_name/AutoMountConfig/MountConfig/SwapConfig; path: glob_match/path_target/PathConfig/PathProbe/PathScheduler/PathSnapshot/PathTrigger/PathTriggerKind)
- TDD: RED E0432 x2 → GREEN (mount_unit_name 前導斜杠 bug 修復) → 25/25 → workspace 397 綠

### 2026-10-11 輪次 16 (journald 核心)
- 寫入 crates/systeme-journal/Cargo.toml, src/lib.rs, src/entry.rs, src/native.rs, src/syslog.rs (b12 壓縮前完成)
- 寫入 src/rate.rs (RateLimiter 固定窗口), src/store.rs (JournalSink/MemorySink/JsonlSink/JournalStore/read_jsonl) — 本輪 GREEN
- EDIT 根 Cargo.toml: members += "crates/systeme-journal"
- 讀 tests/journal.rs + lib.rs 確認契約後補齊兩模組; 18/18 整合 + 1 內建通過; workspace 416 綠零警告
- 待辦: journalctl 客戶端輪 (JSONL 讀取+過濾+輸出格式); bin 層 unix 接線集中輪
- [寫入] crates/journalctl/Cargo.toml (lib+bin journalctl, dep systeme-journal)
- [寫入] crates/journalctl/tests/cli.rs (21 測試) + tests/query.rs (7 測試) — RED 確認 E0308 前目標缺失錯誤
- [編輯] 根 Cargo.toml: members += "crates/journalctl" (read+edit 工具, 避免 PowerShell -replace 無聲失敗)
- [寫入] crates/journalctl/src/lib.rs + cli.rs (Query/parse_args) + query.rs (run_query) + main.rs (journal_path/read_jsonl/follow)
- [修復] cli.rs `?` 對 Option<u64> 誤用 → Some(..ok_or_else(..)?) ×3
- [驗證] journalctl 28/28; workspace 442 全綠; build --all-targets 零警告
- [追加] agent/deep_todos.md 輪次 17 記錄

### 輪次 18 寫入記錄 (bin 層 unix 接線)
- 寫 crates/systeme-manager/src/executor.rs (StdCommandExecutor 加 pending_fds 欄位 + prepare_socket_fds 覆寫 + inject_fds 接線)
- 寫 crates/systeme-manager/src/daemon.rs (DaemonCore +sockets map; start/stop 分發攔截 .socket/.mount/.automount)
- 寫 crates/systeme-manager/tests/daemon_units.rs (8 測試)
- 改 crates/systeme-manager/tests/executor.rs (+prepare_socket_fds_roundtrip)
- 寫 crates/systeme-journal/tests/datagram.rs (6 測試)
- 改 crates/systeme-journal/src/lib.rs (+process_datagram/journal_path/re-export store 類型)
- 改 crates/systeme-journal/Cargo.toml ([[bin]] journald)
- 寫 crates/systeme-journal/src/bin/journald.rs
- 改 crates/journalctl/src/main.rs (刪本地 journal_path 改用共享)
- 改 crates/systeme-manager/src/bin/systemd.rs (StdCommandExecutor::new() ×3 + PathScheduler/fire_due_paths 接線 + 刪未用 sp)
- 改 crates/systeme-manager/tests/mounts.rs (cfg(unix) use 拆分修 warning)
- 追加 agent/deep_todos.md 輪次 18 記錄
- 驗證: cargo test --workspace = 498 全綠; cargo build --workspace --all-targets 零警告

## 輪次 19 寫入記錄
- 新建: crates/systeme-network/{Cargo.toml, src/lib.rs, src/network.rs, src/link.rs, src/netdev.rs, src/apply.rs, src/bin/systemd-networkd.rs, tests/networkd.rs, tests/apply.rs}
- 編輯: 根 Cargo.toml (members += systeme-network)

## 輪次 20 寫入記錄
- 寫 crates/systeme-network/src/resolved.rs (ResolvConf/parse/render/stub/collect_dns)
- 寫 crates/systeme-network/tests/resolved.rs (11 測試)
- edit crates/systeme-network/src/lib.rs (pub mod resolved + re-export)
- 寫 crates/systeme-network/src/bin/systemd-resolved.rs (DNS stub 轉發器)
- edit crates/systeme-network/Cargo.toml ([[bin]] systemd-resolved)
- 驗證: 557 測試全綠零警告

## 輪次 21 寫入記錄
- 新建 crates/systeme-login (Cargo.toml [[bin]] loginctl; src/lib.rs+sessions.rs+cli.rs+render.rs+bin/loginctl.rs; tests/sessions.rs+cli.rs+render.rs)
- edit 根 Cargo.toml members += systeme-login
- 驗證: 583 測試全綠零警告

## 輪次 22 寫入記錄
- 新建 crates/systeme-udev (Cargo.toml libc+[[bin]] systemd-udevd; src/lib.rs; src/bin/systemd-udevd.rs; tests/udev.rs)
- edit 根 Cargo.toml members += systeme-udev
- 補 UdevDb::records() 方法 (bin 用)
- 驗證: 593 測試全綠零警告

## 輪次 23 寫入記錄
- 新建 crates/systeme-tools (Cargo.toml 兩 [[bin]]; src/lib.rs+escape.rs+analyze.rs; bin×2; tests/tools.rs)
- edit 根 Cargo.toml members += systeme-tools
- 驗證: 607 測試全綠零警告

## 輪次 24 寫入記錄
- EDIT crates/systeme-core/src/lib.rs (doc 列表 : → —)
- EDIT crates/systeme-journal/src/lib.rs (同上)
- EDIT crates/systeme-manager/src/lib.rs (同上)
- EDIT crates/systeme-core/src/service.rs (+allow result_unit_err)
- EDIT crates/systeme-core/src/transaction.rs (doc 空行)
- EDIT .github/workflows/ci.yml (integration job 擴充: timer/socket/journald/escape 冒煙)
- WRITE README.md (新建)

## 2026-10-11 輪次 25 寫入記錄
- EDIT Cargo.toml: tokio features += "sync"
- EDIT crates/systeme-core/Cargo.toml: += [target.'cfg(unix)'.dependencies] libc
- EDIT crates/systeme-manager/src/sockets.rs: setenv 值 format! 字串
- EDIT crates/systeme-manager/src/bin/systemd.rs: set_subreaper() 無參; irrefutable if let → let
- EDIT crates/systeme-network/src/bin/systemd-networkd.rs: match_network 簽名修正 + exec_argv 內 Command import
- EDIT crates/systeme-udev/src/bin/systemd-udevd.rs: 刪 unix-only unused import
- EDIT crates/systeme-journal/src/bin/journald.rs: 刪 unused import
- EDIT tests/sockets.rs: SocketAddrExt import + from_abstract_name().expect + read timeout 500ms
- EDIT tests/daemon_units.rs: ServiceExecutor import cfg 門控 + mount_roundtrip_unix mount_permitted 探測
- EDIT tests/mounts.rs: mount_permitted() helper EPERM 跳過
- EDIT tests/executor.rs: prepare_socket_fds_roundtrip 用真 File fd (holder 防提前 drop)
- git: 5 commits 推送 (build-essential / unix-only fixes / mount skip / read timeout / real fds)
- CI 最終: run 38122286542 三 job 全綠 (test-windows/test-linux/integration)
