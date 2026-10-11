//! journald native 協議解析。
//!
//! 格式: `FIELD=value\n` 行, 條目以空行結束;
//! 含換行等任意字節的值走二進制形式: `FIELD\n` + 8 字節 LE 長度 + 數據 + `\n`。

use crate::entry::{JournalEntry, DEFAULT_PRIORITY};

/// 解析一個 datagram payload, 返回其中的有效條目 (缺 MESSAGE 的丟棄)。
pub fn parse_native(payload: &[u8]) -> Vec<JournalEntry> {
    let mut entries = Vec::new();
    let mut fields: Vec<(String, Vec<u8>)> = Vec::new();
    let mut i = 0usize;

    while i < payload.len() {
        if payload[i] == b'\n' {
            // 空行: 條目結束
            i += 1;
            finish_entry(&mut fields, &mut entries);
            continue;
        }

        let line_end = payload[i..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|p| i + p)
            .unwrap_or(payload.len());
        let line = &payload[i..line_end];

        if let Some(eq) = line.iter().position(|&b| b == b'=') {
            // 文本形式: FIELD=value
            let key = String::from_utf8_lossy(&line[..eq]).into_owned();
            let val = line[eq + 1..].to_vec();
            fields.push((key, val));
            i = if line_end < payload.len() { line_end + 1 } else { line_end };
        } else if line_end + 9 <= payload.len() {
            // 二進制形式: FIELD\n<len u64 LE><data>\n
            let key = String::from_utf8_lossy(line).into_owned();
            let len = u64::from_le_bytes(
                payload[line_end + 1..line_end + 9]
                    .try_into()
                    .expect("9-byte slice is 8 bytes"),
            ) as usize;
            let data_start = line_end + 9;
            let data_end = data_start.saturating_add(len).min(payload.len());
            fields.push((key, payload[data_start..data_end].to_vec()));
            i = if data_end < payload.len() && payload[data_end] == b'\n' {
                data_end + 1
            } else {
                data_end
            };
        } else {
            // 殘段 (無 = 且不夠二進制頭): 跳過本行
            i = if line_end < payload.len() { line_end + 1 } else { line_end };
        }
    }

    // 末尾無空行終結的殘餘 (容忍)
    finish_entry(&mut fields, &mut entries);
    entries
}

fn finish_entry(fields: &mut Vec<(String, Vec<u8>)>, out: &mut Vec<JournalEntry>) {
    if fields.is_empty() {
        return;
    }
    if let Some(e) = entry_from(fields) {
        out.push(e);
    }
    fields.clear();
}

fn entry_from(fields: &[(String, Vec<u8>)]) -> Option<JournalEntry> {
    let mut e = JournalEntry {
        message: String::new(),
        priority: DEFAULT_PRIORITY,
        timestamp_us: 0,
        unit: None,
        pid: None,
        identifier: None,
    };
    let mut has_message = false;
    for (k, v) in fields {
        let val = String::from_utf8_lossy(v).into_owned();
        match k.as_str() {
            "MESSAGE" => {
                e.message = val;
                has_message = true;
            }
            "PRIORITY" => {
                if let Ok(p) = val.trim().parse::<u8>() {
                    if p <= 7 {
                        e.priority = p;
                    }
                }
            }
            "__REALTIME_TIMESTAMP" => {
                if let Ok(t) = val.trim().parse::<u64>() {
                    e.timestamp_us = t;
                }
            }
            "_PID" => {
                if let Ok(p) = val.trim().parse::<u32>() {
                    e.pid = Some(p);
                }
            }
            "_SYSTEMD_UNIT" => e.unit = Some(val),
            "SYSLOG_IDENTIFIER" => e.identifier = Some(val),
            _ => {}
        }
    }
    has_message.then_some(e)
}
