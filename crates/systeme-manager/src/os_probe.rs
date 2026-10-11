//! OsProbe — PathProbe 的真實文件系統實現 (跨平台)。

use std::time::UNIX_EPOCH;

use systeme_core::path::{PathProbe, PathSnapshot};
use systeme_core::path::glob_match;

/// 基於 std::fs 的探測器。
#[derive(Debug, Default, Clone, Copy)]
pub struct OsProbe;

impl PathProbe for OsProbe {
    fn snapshot(&self, path: &str) -> PathSnapshot {
        let Ok(md) = std::fs::metadata(path) else {
            return PathSnapshot::default();
        };
        let mtime = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        let dir_nonempty = md.is_dir()
            && std::fs::read_dir(path)
                .map(|mut rd| rd.next().is_some())
                .unwrap_or(false);
        PathSnapshot {
            exists: true,
            mtime,
            dir_nonempty,
        }
    }

    fn glob(&self, pattern: &str) -> bool {
        // 分離 parent 目錄與 basename pattern; 無分隔符 → 當前目錄。
        let (dir, file_pat) = match pattern.rfind(['/', '\\']) {
            Some(i) => (&pattern[..i], &pattern[i + 1..]),
            None => (".", pattern),
        };
        let Ok(rd) = std::fs::read_dir(dir) else {
            return false;
        };
        rd.filter_map(|e| e.ok())
            .any(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(|n| glob_match(file_pat, n))
            })
    }
}
