//! cgroup v2/v1 文件系統後端。
//!
//! cgroup 層級在文件系統上就是目錄樹: v2 統一根 `/sys/fs/cgroup`、
//! 進程清單文件 `cgroup.procs`; v1 各控制器各自層級、進程清單 `tasks`。
//! 本後端以 root 可注入的普通文件操作實現, 因此可以在臨時目錄中
//! 跨平台測試; 生產部署 root 指向真實掛載點。
//!
//! 信號發送經 [`CgroupFsBackend::kill_group_with`] 注入, 使發送路徑
//! 可測; trait 方法 `kill_group` 用 libc::kill (僅 unix)。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use systeme_core::cgroup::{CgroupBackend, CgroupError, CgroupMode};

/// cgroup 文件系統後端 (v2 或 v1, 由構造函數決定)。
pub struct CgroupFsBackend {
    /// 層級根 (v2: /sys/fs/cgroup; v1: 如 /sys/fs/cgroup/memory)
    root: PathBuf,
    /// 進程清單文件名: v2 = "cgroup.procs", v1 = "tasks"
    procs_file: &'static str,
}

impl CgroupFsBackend {
    /// v2 統一層級後端
    pub fn new_v2(root: impl Into<PathBuf>) -> Self {
        CgroupFsBackend {
            root: root.into(),
            procs_file: "cgroup.procs",
        }
    }

    /// v1 控制器層級後端 (root 指向該控制器的掛載點)
    pub fn new_v1(root: impl Into<PathBuf>) -> Self {
        CgroupFsBackend {
            root: root.into(),
            procs_file: "tasks",
        }
    }

    /// 相對組路徑 → root 下的絕對路徑。
    /// 剝前導 '/' 防止 `PathBuf::join` 的絕對路徑覆蓋陷阱。
    fn abs(&self, rel: &str) -> PathBuf {
        self.root.join(rel.trim_start_matches('/'))
    }

    /// 組的進程清單文件絕對路徑
    fn procs_path(&self, rel: &str) -> PathBuf {
        self.abs(rel).join(self.procs_file)
    }

    /// 對組內全體進程經注入的發送函數發信號; 返回被發信號的 pid 列表。
    ///
    /// 組不存在 → `Ok(空)` (與 MemCgroup 語義一致: 無進程可殺)。
    pub fn kill_group_with(
        &mut self,
        path: &str,
        sig: i32,
        kill_fn: &mut dyn FnMut(i32, i32) -> Result<(), String>,
    ) -> Result<Vec<i32>, CgroupError> {
        let pids = match self.list_pids(path) {
            Err(CgroupError::NotFound(_)) => return Ok(Vec::new()),
            other => other?,
        };
        for pid in &pids {
            kill_fn(*pid, sig).map_err(CgroupError::Backend)?;
        }
        Ok(pids)
    }
}

impl CgroupBackend for CgroupFsBackend {
    fn mode(&self) -> CgroupMode {
        if self.procs_file == "cgroup.procs" {
            CgroupMode::V2
        } else {
            CgroupMode::V1
        }
    }

    fn create_group(&mut self, path: &str) -> Result<(), CgroupError> {
        // 逐層建立 (父鏈); 已存在冪等
        let abs = self.abs(path);
        let mut cur = self.root.clone();
        ensure_dir(&cur)?;
        for part in abs.strip_prefix(&self.root).unwrap_or_else(|_| Path::new("")) {
            // 防禦: 路徑分量不允許逃逸
            if part == ".." {
                return Err(CgroupError::Backend("invalid path component '..'".into()));
            }
            cur.push(part);
            ensure_dir(&cur)?;
        }
        Ok(())
    }

    fn group_exists(&self, path: &str) -> bool {
        self.abs(path).is_dir()
    }

    fn attach_pid(&mut self, path: &str, pid: i32) -> Result<(), CgroupError> {
        let procs = self.procs_path(path);
        if !self.abs(path).is_dir() {
            return Err(CgroupError::NotFound(path.to_string()));
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&procs)
            .map_err(|e| CgroupError::Backend(e.to_string()))?;
        writeln!(f, "{pid}").map_err(|e| CgroupError::Backend(e.to_string()))?;
        Ok(())
    }

    fn list_pids(&self, path: &str) -> Result<Vec<i32>, CgroupError> {
        let abs = self.abs(path);
        if !abs.is_dir() {
            return Err(CgroupError::NotFound(path.to_string()));
        }
        let procs = abs.join(self.procs_file);
        if !procs.exists() {
            // 空組 (尚無 procs 文件)
            return Ok(Vec::new());
        }
        let text =
            fs::read_to_string(&procs).map_err(|e| CgroupError::Backend(e.to_string()))?;
        let mut pids: Vec<i32> = text
            .lines()
            .filter_map(|l| l.trim().parse::<i32>().ok())
            .collect();
        pids.sort_unstable();
        pids.dedup();
        Ok(pids)
    }

    fn kill_group(&mut self, path: &str, sig: i32) -> Result<Vec<i32>, CgroupError> {
        #[cfg(unix)]
        {
            self.kill_group_with(path, sig, &mut |pid, s| {
                systeme_core::process::sys::kill_pid(pid, s).map_err(|e| e.to_string())
            })
        }
        #[cfg(not(unix))]
        {
            let _ = (path, sig);
            Err(CgroupError::Backend(
                "signal delivery unsupported on this platform".into(),
            ))
        }
    }

    fn remove_group(&mut self, path: &str) -> Result<(), CgroupError> {
        let abs = self.abs(path);
        if !abs.is_dir() {
            return Err(CgroupError::NotFound(path.to_string()));
        }
        // 只能刪空組 (與內核 rmdir 語義一致); 非空 → Backend 錯
        fs::remove_dir(&abs).map_err(|e| CgroupError::Backend(e.to_string()))
    }
}

fn ensure_dir(p: &Path) -> Result<(), CgroupError> {
    if p.is_dir() {
        Ok(())
    } else if p.exists() {
        Err(CgroupError::Backend(format!(
            "{} exists but is not a directory",
            p.display()
        )))
    } else {
        fs::create_dir(p).map_err(|e| CgroupError::Backend(e.to_string()))
    }
}
