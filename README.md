# s12ryt-systeme

用 Rust 全面復刻 [systemd](https://systemd.io/) 的核心功能, 但**不要求 PID 1** — 守護進程可以作為普通進程在任何環境啟動 (開發機、容器、CI、無 root 環境)。

```
$ ./systemd --user &          # 非 PID 1 啟動, 自動探測用戶模式
$ systemctl list-units
$ systemctl start my.service
```

## 核心設計

### 1. PID≠1 可啟動
- 不檢查 PID 1 身份; 以 `--user` / `--system` 顯式指定或按 euid 自動探測
- 進程收養改用 `PR_SET_CHILD_SUBREAPER` (subreaper), 不依賴 init 身份
- 容器 (ubuntu:24.04, PID 1 為 bash/sh) 即項目的一級測試環境 (見 CI)

### 2. 雙模式自適應
| 條件 | 行為 |
|---|---|
| root + cgroup v2/v1 可用 | 系統模式, cgroup 管理進程組 |
| 無 root | 用戶模式 (`~/.config/systemd/user` 搜索路徑) |
| cgroup 不可用 | 降級為 subreaper + 進程表追蹤 (NoCgroup 後端, kill 樹仍可用) |

### 3. 高兼容單元文件
- 直接解析現有 systemd 單元: `[Section]` 段頭合併、`Key=Value`、行尾 `\` 續行、cunescape 轉義、`#/;` 註釋
- 未知指令/未知枚舉值 → **警告降級, 不報錯** (高兼容策略)
- 說明符全支持: `%n %N %p %i %j %f %t %S %C %L %h %u %U %g %m %b %H %v %%` 與 `$VAR`/`${VAR}` 變量展開
- 目錄層級 `/etc > /run > /usr/local/lib > /usr/lib`, mask (`→/dev/null` 符號鏈接)、alias、`.d/*.conf` drop-in 疊加

### 4. 二進制名 drop-in 兼容
發布的二進制名與 systemd 家族一致: `systemd` `systemctl` `journalctl` `journald` `loginctl` `systemd-networkd` `systemd-resolved` `systemd-udevd` `systemd-escape` `systemd-analyze`

## Crates

| Crate | 職責 |
|---|---|
| `systeme-units` | 單元文件解析器、目錄搜索/加載、說明符展開 |
| `systeme-core` | 依賴圖、事務引擎、Service 配置、進程管理純邏輯、cgroup 抽象、管理器狀態機、IPC 協議、Calendar/Timer/Socket/Mount/Path 配置 |
| `systeme-manager` | DaemonCore (IPC→調度)、真實進程執行器、cgroup fs 後端、timer/path 調度器、mount/socket 真實操作、`systemd` 守護進程 |
| `systemctl` | `systemctl` CLI (start/stop/restart/status/list-units/is-active/cat/daemon-reload/mask...) |
| `systeme-journal` | journald 核心 (native/syslog 協議、rate limit、JSONL 存儲) + `journald` 守護進程 |
| `journalctl` | `journalctl` CLI (-u/-p/--since/--until/-n/-r/-f/-o short\|json\|cat) |
| `systeme-network` | networkd (.network/.link/.netdev 解析+應用)、resolved (resolv.conf/stub) + 兩個 bin |
| `systeme-login` | logind 會話/席位管理 + `loginctl` |
| `systeme-udev` | uevent 解析、sysfs 掃描、UdevDb + `systemd-udevd` |
| `systeme-tools` | `systemd-escape` / `systemd-analyze` |

## 功能矩陣

**單元類型**: service (simple/forking/exec/notify/oneshot/dbus 配置層) · socket (stream/dgram/fifo/abstract, LISTEN_FDS fd 傳遞) · timer (OnCalendar/OnBootSec/OnUnitActiveSec/OnUnitInactiveSec) · mount/automount/swap · path (Exists/ExistsGlob/Changed/Modified/DirectoryNotEmpty) · target/slice/scope/device

**依賴語義**: After/Before/Requires/Wants/Requisite/BindsTo/PartOf/Conflicts/Upholds + 反向聲明 (RequiredBy/WantedBy/...), Tarjan SCC 環檢測, Kahn 拓撲 job 排序 (啟動正向/停止反向), JobMode Fail/Replace 兩階段合併

**Service 生命週期**: ExecStartPre/ExecStart/ExecStop/ExecStopPost、`-`/`+`/`:` 前綴、Restart 矩陣 (No/OnSuccess/OnFailure/OnAbnormal/OnAbort/OnWatchdog/Always)、KillMode (control-group/mixed/process/process-group/none)、RemainAfterExit、subreaper 收割 + AutoRestart

**Calendar**: `Mon..Fri 9:00`、`*-01-01`、`*/15`、`Mon..Fri`、特殊值 (daily/hourly/weekly/monthly/...)、範圍+步進、`next_elapse` 逐日掃描

## 快速開始

```bash
cargo build --release

# 用戶模式啟動 (無需 root, 非 PID 1)
export SYSTEME_SOCKET=/tmp/systeme/private.sock
./target/release/systemd --user &

./target/release/systemctl start my.service
./target/release/systemctl status my.service
./target/release/journalctl -u my.service -n 20
```

環境變量: `SYSTEME_SOCKET` (IPC socket 路徑) · `SYSTEME_JOURNAL` (日誌文件) · `SYSTEME_DEV_LOG` (journald 監聽) · `SYSTEMD_NETWORK_DIR` · `SYSTEME_DNS` · `SYSTEME_RESOLV_CONF`

## 測試

```bash
cargo test --workspace    # 607 項測試 (Windows 純邏輯層 / Linux 全量含 cfg(unix))
cargo clippy --workspace --all-targets   # 零警告
```

CI (GitHub Actions, `.github/workflows/ci.yml`):
1. **test-linux** — ubuntu:24.04 容器內全量構建+測試 (PID 1 非 systemd = 目標環境)
2. **integration** — release 構建後守護進程以普通進程啟動, systemctl 對接冒煙: oneshot/sleeper 服務、timer 到期觸發、socket 激活綁定、journald /dev/log、systemd-escape、優雅退出
3. **test-windows** — Windows 純邏輯層回歸 (跨平台抽象層保證)

## 與 systemd 的差異 (簡化項)

- IPC 用 JSON 行協議 over Unix socket (D-Bus 未實現)
- `Type=notify` 的 sd_notify 握手、`Type=forking` 的 PID 推斷留待守護進程層增強
- socket 激活為提前啟動目標服務 (惰性 accept 觸發簡化)
- logind/udevd 為基礎版 (會話管理/uevent db, 無 PAM/規則引擎)
- networkd 通過 `ip` 命令應用 (非裸 netlink)
