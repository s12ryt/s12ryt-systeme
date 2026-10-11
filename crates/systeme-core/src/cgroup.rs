//! cgroup 抽象層: 後端探測、slice 層級語義、cgroup 路徑計算與抽象後端 trait。
//!
//! 三級降級策略 (對應「PID≠1 / 無 root 也能跑」需求):
//! 1. v2 unified (`/sys/fs/cgroup` 可寫, 有 `cgroup.controllers`)
//! 2. v1 legacy (controller 各自掛載)
//! 3. none — [`NoCgroup`] no-op 後端, 管理器退回 subreaper + 進程追蹤
//!
//! 本模組為純邏輯層, 跨平台可測; 真實文件系統後端 (v2/v1) 於 manager
//! 集成輪以 `#[cfg(unix)]` 實現, 由 CI 容器驗證。

use std::collections::{BTreeMap, BTreeSet};

// ─── 後端模式與探測 ──────────────────────────────────────────────────

/// cgroup 後端模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CgroupMode {
    /// cgroup v2 unified hierarchy
    V2,
    /// cgroup v1 legacy controllers
    V1,
    /// 無 cgroup 可用 (容器受限 / 非 root 無權限) — 退回進程追蹤
    None,
}

/// 依文件系統探測結果選擇後端 (v2 優先於 v1, 都沒有則 None)。
///
/// `exists` 為文件存在性探測回調 (生產環境 std::fs::metadata; 測試注入)。
pub fn detect_backend(exists: &dyn Fn(&str) -> bool) -> CgroupMode {
    if exists("/sys/fs/cgroup/cgroup.controllers") {
        CgroupMode::V2
    } else if exists("/sys/fs/cgroup/memory") {
        CgroupMode::V1
    } else {
        CgroupMode::None
    }
}

// ─── slice 層級語義 ──────────────────────────────────────────────────

/// 計算 slice 單元的父 slice 名。
///
/// systemd 語義: `a-b-c.slice` 的父是 `a-b.slice`; 頂層 `a.slice` 的父是
/// 根 `-.slice`; 根無父。`\xHH` 轉義序列中的字符是名稱字面量,
/// 不參與 `-` 分隔 (例如 `a\x2db.slice` 是頂層 slice)。
///
/// 輸入應為合法 `.slice` 單元名; `-.slice` 返回 None。
pub fn slice_parent(name: &str) -> Option<String> {
    if !name.ends_with(".slice") || name == "-.slice" {
        return None;
    }
    let stem = &name[..name.len() - ".slice".len()];
    // 掃描找最後一個「未轉義」的 '-'
    let bytes = stem.as_bytes();
    let mut last_dash: Option<usize> = None;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                // \xHH: 跳過整個轉義序列 (不足 4 字節或非 hex 則視為普通字符)
                let hex_ok = bytes.get(i + 1) == Some(&b'x')
                    && bytes.len() >= i + 4
                    && bytes[i + 2].is_ascii_hexdigit()
                    && bytes[i + 3].is_ascii_hexdigit();
                i += if hex_ok { 4 } else { 1 };
            }
            b'-' => {
                last_dash = Some(i);
                i += 1;
            }
            _ => i += 1,
        }
    }
    match last_dash {
        Some(pos) => Some(format!("{}.slice", &stem[..pos])),
        None => Some("-.slice".to_string()),
    }
}

// ─── cgroup 路徑計算 ─────────────────────────────────────────────────

/// slice 單元的 cgroup 相對路徑 (遞歸父鏈)。`-.slice` → ""。
fn slice_path(name: &str) -> Option<String> {
    if name == "-.slice" {
        return Some(String::new());
    }
    if !is_safe_unit_name(name) || !name.ends_with(".slice") {
        return None;
    }
    let parent = slice_parent(name)?;
    let parent_path = slice_path(&parent)?;
    Some(format!("{parent_path}/{name}"))
}

/// 單元名安全檢查 (路徑計算前的最小驗證)
fn is_safe_unit_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains("..")
}

/// 計算單元的 cgroup 相對路徑。
///
/// - `.slice` 單元: 遞歸父鏈 (`a-b.slice` → `/a.slice/a-b.slice`)
/// - 其他單元 (service/scope/...): 掛在 `slice_override` (默認 `system.slice`) 之下
///
/// 不安全的名稱 (空 / 含 `/` / 含 `..`) 返回 None。
pub fn default_cgroup_path(unit: &str, slice_override: Option<&str>) -> Option<String> {
    if !is_safe_unit_name(unit) {
        return None;
    }
    if unit.ends_with(".slice") {
        return slice_path(unit);
    }
    let base_slice = slice_override.unwrap_or("system.slice");
    let base = slice_path(base_slice)?;
    Some(format!("{base}/{unit}"))
}

// ─── 抽象後端 trait ──────────────────────────────────────────────────

/// cgroup 操作錯誤
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CgroupError {
    #[error("cgroup not found: {0}")]
    NotFound(String),
    #[error("cgroup backend error: {0}")]
    Backend(String),
}

/// cgroup 後端抽象。生產實現: v2/v1 文件系統後端 (cfg(unix));
/// 測試實現: [`MemCgroup`]; 降級實現: [`NoCgroup`]。
pub trait CgroupBackend {
    fn mode(&self) -> CgroupMode;
    /// 創建組 (含父鏈); 已存在冪等 Ok
    fn create_group(&mut self, path: &str) -> Result<(), CgroupError>;
    fn group_exists(&self, path: &str) -> bool;
    /// 將進程移入組
    fn attach_pid(&mut self, path: &str, pid: i32) -> Result<(), CgroupError>;
    /// 組內全體 pid (升序)
    fn list_pids(&self, path: &str) -> Result<Vec<i32>, CgroupError>;
    /// 對組內全體進程發信號; 返回被發信號的 pid 列表
    fn kill_group(&mut self, path: &str, sig: i32) -> Result<Vec<i32>, CgroupError>;
    /// 刪除組 (組須存在; 組內進程由調用方先處理)
    fn remove_group(&mut self, path: &str) -> Result<(), CgroupError>;
}

// ─── MemCgroup: 內存假後端 (測試用) ──────────────────────────────────

/// 純內存 cgroup 模擬後端: 驗證管理器與抽象層交互的測試替身。
#[derive(Debug, Default, Clone)]
pub struct MemCgroup {
    groups: BTreeMap<String, BTreeSet<i32>>,
    signals: Vec<(i32, i32)>,
}

impl MemCgroup {
    pub fn new() -> MemCgroup {
        MemCgroup::default()
    }

    /// 已發送的信號記錄 (pid, sig), 依發送序
    pub fn signal_log(&self) -> &[(i32, i32)] {
        &self.signals
    }

    /// 逐層確保父組存在
    fn ensure_parents(&mut self, path: &str) {
        let mut acc = String::new();
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        for p in parts {
            acc.push('/');
            acc.push_str(p);
            self.groups.entry(acc.clone()).or_default();
        }
    }
}

impl CgroupBackend for MemCgroup {
    fn mode(&self) -> CgroupMode {
        CgroupMode::V1 // 模擬後端的 mode 無關緊要, 標 V1 供區分
    }

    fn create_group(&mut self, path: &str) -> Result<(), CgroupError> {
        self.ensure_parents(path);
        Ok(())
    }

    fn group_exists(&self, path: &str) -> bool {
        self.groups.contains_key(path)
    }

    fn attach_pid(&mut self, path: &str, pid: i32) -> Result<(), CgroupError> {
        self.groups
            .get_mut(path)
            .ok_or_else(|| CgroupError::NotFound(path.to_string()))?
            .insert(pid);
        Ok(())
    }

    fn list_pids(&self, path: &str) -> Result<Vec<i32>, CgroupError> {
        self.groups
            .get(path)
            .map(|s| s.iter().copied().collect())
            .ok_or_else(|| CgroupError::NotFound(path.to_string()))
    }

    fn kill_group(&mut self, path: &str, sig: i32) -> Result<Vec<i32>, CgroupError> {
        // 殺不存在的組 = 無進程可殺, Ok(空)
        let Some(pids) = self.groups.get(path) else {
            return Ok(Vec::new());
        };
        let pids: Vec<i32> = pids.iter().copied().collect();
        for pid in &pids {
            self.signals.push((*pid, sig));
        }
        Ok(pids)
    }

    fn remove_group(&mut self, path: &str) -> Result<(), CgroupError> {
        self.groups
            .remove(path)
            .map(|_| ())
            .ok_or_else(|| CgroupError::NotFound(path.to_string()))
    }
}

// ─── NoCgroup: 無 cgroup 降級後端 ────────────────────────────────────

/// 無 cgroup 環境的 no-op 後端: 一切操作成功但無實際效果。
///
/// `kill_group` 返回空列表 — 調用方 (管理器) 應改用 subreaper +
/// 進程追蹤對服務進程發信號。這是「移除啟動限制」的降級核心。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoCgroup;

impl CgroupBackend for NoCgroup {
    fn mode(&self) -> CgroupMode {
        CgroupMode::None
    }

    fn create_group(&mut self, _path: &str) -> Result<(), CgroupError> {
        Ok(())
    }

    fn group_exists(&self, _path: &str) -> bool {
        false
    }

    fn attach_pid(&mut self, _path: &str, _pid: i32) -> Result<(), CgroupError> {
        Ok(())
    }

    fn list_pids(&self, _path: &str) -> Result<Vec<i32>, CgroupError> {
        Ok(Vec::new())
    }

    fn kill_group(&mut self, _path: &str, _sig: i32) -> Result<Vec<i32>, CgroupError> {
        Ok(Vec::new())
    }

    fn remove_group(&mut self, _path: &str) -> Result<(), CgroupError> {
        Ok(())
    }
}
