//! 單元目錄搜索與加載。
//!
//! systemd 語義:
//! - 搜索層 (低→高優先): `/usr/lib/systemd/system` < `/usr/local/lib/systemd/system`
//!   < `/run/systemd/system` < `/etc/systemd/system`; 高層同名文件覆蓋低層
//! - `/etc/systemd/system/foo.service` → `/dev/null` 符號鏈接 = **masked**
//! - 符號鏈接 → 其他單元 = **alias**
//! - `foo.service.d/*.conf` override 片段跨層疊加 (低層先, 高層後覆蓋)
//!
//! 文件系統訪問抽象為 [`UnitFs`] trait —— 生產用 [`OsFs`], 測試用 [`MemFs`]
//! (Windows 開發機無法隨意建 symlink, 抽象層保證測試全平台一致)。

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::parser::{parse_unit, ParsedUnit, Section};

/// 文件系統抽象
pub trait UnitFs {
    fn exists(&self, path: &Path) -> bool;
    fn read(&self, path: &Path) -> io::Result<String>;
    fn read_link(&self, path: &Path) -> io::Result<PathBuf>;
    /// 列出目錄內容 (按文件名字典序)
    fn list_dir_sorted(&self, dir: &Path) -> io::Result<Vec<PathBuf>>;
}

/// 真實操作系統文件系統
#[derive(Debug, Default, Clone, Copy)]
pub struct OsFs;

impl UnitFs for OsFs {
    fn exists(&self, path: &Path) -> bool {
        path.symlink_metadata().is_ok()
    }
    fn read(&self, path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
    }
    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        fs::read_link(path)
    }
    fn list_dir_sorted(&self, dir: &Path) -> io::Result<Vec<PathBuf>> {
        let mut v: Vec<PathBuf> = fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .collect();
        v.sort();
        Ok(v)
    }
}

/// 內存文件系統 (測試用)
#[derive(Debug, Default, Clone)]
pub struct MemFs {
    pub nodes: BTreeMap<PathBuf, MemNode>,
}

#[derive(Debug, Clone)]
pub enum MemNode {
    File(String),
    Symlink(PathBuf),
}

impl MemFs {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn file(&mut self, path: &str, content: &str) -> &mut Self {
        self.nodes.insert(PathBuf::from(path), MemNode::File(content.to_string()));
        self
    }
    pub fn symlink(&mut self, path: &str, target: &str) -> &mut Self {
        self.nodes.insert(PathBuf::from(path), MemNode::Symlink(PathBuf::from(target)));
        self
    }
}

impl UnitFs for MemFs {
    fn exists(&self, path: &Path) -> bool {
        self.nodes.contains_key(path)
    }
    fn read(&self, path: &Path) -> io::Result<String> {
        match self.nodes.get(path) {
            Some(MemNode::File(c)) => Ok(c.clone()),
            _ => Err(io::Error::new(io::ErrorKind::NotFound, "not a file")),
        }
    }
    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        match self.nodes.get(path) {
            Some(MemNode::Symlink(t)) => Ok(t.clone()),
            _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "not a symlink")),
        }
    }
    fn list_dir_sorted(&self, dir: &Path) -> io::Result<Vec<PathBuf>> {
        let mut v: Vec<PathBuf> = self
            .nodes
            .keys()
            .filter(|p| p.parent() == Some(dir))
            .cloned()
            .collect();
        v.sort();
        Ok(v)
    }
}

/// 搜索路徑集合 (dirs 為 低→高 優先順序)
#[derive(Debug, Clone)]
pub struct SearchPaths {
    pub dirs: Vec<PathBuf>,
}

/// 系統模式搜索層 (低→高)
pub fn system_search_paths() -> SearchPaths {
    SearchPaths {
        dirs: vec![
            "/usr/lib/systemd/system".into(),
            "/usr/local/lib/systemd/system".into(),
            "/run/systemd/system".into(),
            "/etc/systemd/system".into(),
        ],
    }
}

/// 用戶模式搜索層 (低→高)
pub fn user_search_paths(
    xdg_config_home: Option<&Path>,
    home: Option<&Path>,
) -> SearchPaths {
    let config = xdg_config_home
        .map(|p| p.to_path_buf())
        .or_else(|| home.map(|h| h.join(".config")));
    let mut dirs = vec![
        PathBuf::from("/usr/lib/systemd/user"),
        PathBuf::from("/etc/systemd/user"),
    ];
    if let Some(c) = config {
        dirs.push(c.join("systemd/user"));
    }
    SearchPaths { dirs }
}

/// 找到的單元文件
#[derive(Debug, Clone)]
pub struct FoundUnit {
    /// 請求名 (可能是 alias)
    pub name: String,
    /// alias 解析後的真實單元名
    pub real_name: String,
    /// 真實文件路徑
    pub path: PathBuf,
    /// 是否被 mask (鏈至 /dev/null)
    pub masked: bool,
}

/// 加載結果
#[derive(Debug)]
pub enum Loaded {
    Masked(FoundUnit),
    Unit(FoundUnit, ParsedUnit),
}

/// 按名查找單元文件 (含 alias 解析與 mask 偵測)
pub fn find_unit(name: &str, sp: &SearchPaths, fs: &dyn UnitFs) -> Option<FoundUnit> {
    let mut current = name.to_string();
    let mut visited: Vec<String> = vec![name.to_string()];
    loop {
        let mut follow_alias: Option<String> = None;
        for dir in sp.dirs.iter().rev() {
            // 高 → 低
            let path = dir.join(&current);
            if !fs.exists(&path) {
                continue;
            }
            if let Ok(target) = fs.read_link(&path) {
                let t = target.to_string_lossy().to_string();
                if t == "/dev/null" {
                    return Some(FoundUnit {
                        name: name.to_string(),
                        real_name: current.clone(),
                        path,
                        masked: true,
                    });
                }
                // alias: 跟隨鏈接 (僅當目標 basename 是未訪問的其他單元名)
                if let Some(base) =
                    target.file_name().map(|f| f.to_string_lossy().to_string())
                {
                    if base != current && !visited.contains(&base) {
                        visited.push(base.clone());
                        follow_alias = Some(base);
                        break; // 以 alias 目標名重新從高層找
                    }
                }
                // 鏈接目標不可跟隨 (環或同名) → 視為實體文件
                return Some(FoundUnit {
                    name: name.to_string(),
                    real_name: current.clone(),
                    path,
                    masked: false,
                });
            }
            return Some(FoundUnit {
                name: name.to_string(),
                real_name: current.clone(),
                path,
                masked: false,
            });
        }
        {
            let next = follow_alias?;
            current = next
        }
    }
}

/// 加載單元: 主文件 + 各層 `*.d/*.conf` override 疊加
pub fn load_unit(name: &str, sp: &SearchPaths, fs: &dyn UnitFs) -> Option<Loaded> {
    let found = find_unit(name, sp, fs)?;
    if found.masked {
        return Some(Loaded::Masked(found));
    }
    let text = fs.read(&found.path).ok()?;
    let mut parsed = parse_unit(&text);

    // override: 每層 {dir}/{real_name}.d/*.conf (低→高疊加)
    for dir in &sp.dirs {
        let od = dir.join(format!("{}.d", found.real_name));
        if let Ok(entries) = fs.list_dir_sorted(&od) {
            for conf in entries {
                if conf.extension().and_then(|e| e.to_str()) != Some("conf") {
                    continue;
                }
                if let Ok(text) = fs.read(&conf) {
                    let frag = parse_unit(&text);
                    merge_parsed(&mut parsed, frag);
                }
            }
        }
    }
    Some(Loaded::Unit(found, parsed))
}

/// 疊加 override 片段: 同名 section append entries, 新 section push
pub fn merge_parsed(dst: &mut ParsedUnit, src: ParsedUnit) {
    for sec in src.sections {
        if let Some(existing) = dst.section_mut(&sec.name) {
            existing.entries.extend(sec.entries);
        } else {
            dst.sections.push(Section { name: sec.name, entries: sec.entries });
        }
    }
    dst.warnings.extend(src.warnings);
}
