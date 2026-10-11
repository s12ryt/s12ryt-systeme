//! systeme-udev: udevd 基礎 — uevent 解析/設備資料庫/trigger 計劃。
//!
//! 真實 netlink KOBJECT_UEVENT 監聽留 bin 層 cfg(unix); 本 crate
//! 提供 uevent 文本解析 (\0 分隔 KEY=VALUE)、設備資料庫 (add/change/remove)
//! 與 sysfs 掃描 (--trigger 資料來源)。

use std::collections::BTreeMap;
use std::path::Path;

/// 一條 uevent。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uevent {
    pub action: Option<String>,
    pub devpath: Option<String>,
    pub subsystem: Option<String>,
    pub devname: Option<String>,
    /// 其餘環境變量
    pub extra: BTreeMap<String, String>,
}

/// 解析 uevent 文本: `\0` 分隔的 KEY=VALUE 序列, 雙 `\0` 結尾。
/// 必須含 DEVPATH 否則 None (無法定位設備)。
pub fn parse_uevent(text: &str) -> Option<Uevent> {
    let mut ev = Uevent {
        action: None,
        devpath: None,
        subsystem: None,
        devname: None,
        extra: BTreeMap::new(),
    };
    let mut saw_field = false;
    for part in text.split('\0') {
        if part.is_empty() {
            continue;
        }
        saw_field = true;
        if let Some((k, v)) = part.split_once('=') {
            match k {
                "ACTION" => ev.action = Some(v.to_string()),
                "DEVPATH" => ev.devpath = Some(v.to_string()),
                "SUBSYSTEM" => ev.subsystem = Some(v.to_string()),
                "DEVNAME" => ev.devname = Some(v.to_string()),
                _ => {
                    ev.extra.insert(k.to_string(), v.to_string());
                }
            }
        }
    }
    if !saw_field || !text.contains('\0') && !text.contains('=') {
        return None;
    }
    ev.devpath.as_ref()?;
    Some(ev)
}

/// 資料庫條目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRecord {
    pub devpath: String,
    pub subsystem: Option<String>,
    pub devname: Option<String>,
}

/// 設備資料庫: 按 DEVPATH 索引。
#[derive(Debug, Default)]
pub struct UdevDb {
    map: BTreeMap<String, DeviceRecord>,
}

impl UdevDb {
    pub fn new() -> UdevDb {
        UdevDb::default()
    }

    /// 套用一條 uevent: remove→刪除, 其餘 (add/change/bind/...)→upsert。
    pub fn apply(&mut self, ev: &Uevent) {
        let Some(devpath) = &ev.devpath else { return };
        if ev.action.as_deref() == Some("remove") {
            self.map.remove(devpath);
            return;
        }
        self.map.insert(
            devpath.clone(),
            DeviceRecord {
                devpath: devpath.clone(),
                subsystem: ev.subsystem.clone(),
                devname: ev.devname.clone(),
            },
        );
    }

    pub fn get(&self, devpath: &str) -> Option<&DeviceRecord> {
        self.map.get(devpath)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// 全部記錄 (devpath 排序)。
    pub fn records(&self) -> Vec<&DeviceRecord> {
        self.map.values().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// 指定子系統的全部設備。
    pub fn by_subsystem(&self, subsystem: &str) -> Vec<&DeviceRecord> {
        self.map
            .values()
            .filter(|d| d.subsystem.as_deref() == Some(subsystem))
            .collect()
    }

    /// 生成 trigger 計劃: 對庫內設備重放 ACTION=<action> 的 uevent 文本。
    pub fn trigger_plan(&self, action: &str) -> Vec<String> {
        self.map
            .values()
            .map(|d| {
                let mut s = format!("ACTION={action}\0DEVPATH={}", d.devpath);
                if let Some(sub) = &d.subsystem {
                    s.push_str(&format!("\0SUBSYSTEM={sub}"));
                }
                if let Some(name) = &d.devname {
                    s.push_str(&format!("\0DEVNAME={name}"));
                }
                s.push_str("\0\0");
                s
            })
            .collect()
    }
}

/// 遞歸掃描 sysfs 根, 收集所有 `uevent` 文件內容 (排序確定性)。
pub fn scan_sysfs(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut children: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        children.sort();
        for child in children {
            if child.is_dir() {
                stack.push(child);
            } else if child.file_name().is_some_and(|n| n == "uevent") {
                if let Ok(text) = std::fs::read_to_string(&child) {
                    out.push(text);
                }
            }
        }
    }
    out.sort();
    out
}
