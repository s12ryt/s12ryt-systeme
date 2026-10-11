# 項目需求問答記錄（TDD 驗收標準）

> 日期: 2026-10-11
> 需求: 用 Rust 全面復刻 systemd 各項功能,支持 PID 不為 1 啟動,移除啟動限制但保留原有功能

## 用戶決策

### Q1: 主要運行環境?
**A: 容器版 Linux**。測試可透過 GitHub 公開倉庫跑 Actions workflow。

### Q2: 第一版組件範圍?
**A: 全部**:
- 核心管理器 (init + systemctl + 單元解析 + 依賴事務 + cgroup + journald 替代)
- 單元類型全家桶 (service/socket/timer/target/mount/automount/path/swap/slice/scope/device)
- 網絡組件 (networkd / resolved)
- 登入/設備組件 (logind / udevd)

### Q3: 單元文件兼容程度?
**A: 高兼容**。能直接解析運行 `/usr/lib/systemd/system` 下現有單元 (nginx/sshd/docker 等),
未知指令/未知欄位**降級為警告**而非報錯。

### Q4: 「PID 不為 1 + 移除啟動限制」場景?
**A: 雙模式自適應**:
- 有 root → 系統模式 (system.slice、/run、全系統服務),但 PID 隨意
- 無 root → 自動降級用戶模式 (XDG_RUNTIME_DIR)
- cgroup 不可用時 → 退回進程組追蹤 (subreaper + /proc 掃描)

### Q5: 測試與開發環境?
**A: GitHub Actions workflow**。容器 (ubuntu:24.04) 內跑集成測試,容器內 PID 1 非 systemd,
正好驗證「PID 不為 1」場景。

## 技術決策 (由上述推導,重大項目補充確認項)

- 語言: Rust (2021 edition), 異步運行時 tokio, serde/serde_json
- 二進制命名: `systemd` / `systemctl` / `journalctl` 等 drop-in 兼容名
- IPC: 自有 Unix socket 協議 (JSON 行協議), D-Bus 兼容為後期目標
- 子進程收養: prctl(PR_SET_CHILD_SUBREAPER) 使非 PID1 進程可收養孤兒
- cgroup 後端抽象: cgroupv2 > cgroupv1 > 純進程追蹤 三級降級

## 驗收標準 (TDD)

1. 所有功能須有對應自動化測試 (單元測試 + GitHub Actions 容器集成測試)
2. 解析器測試需用真實 systemd 單元文件片段驗證高兼容
3. 集成測試必須在「PID 1 非本管理器」環境下驗證啟動/停止/重啟/依賴/計時器
4. 無 root / 無 cgroup 環境必須能正常降級運行 (CI 中模擬)
5. 每輪疊代: RED (先寫失敗測試) → GREEN (實作通過) → 重構
