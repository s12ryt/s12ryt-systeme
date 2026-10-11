//! systemd 單元文件解析器 (INI 變體語法)。
//!
//! 語法規則 (對齊 systemd conf-parser 行為):
//! - `[Section]` 段頭; 同名段多次出現自動合併
//! - `Key=Value`; `Key` 單獨出現為布爾旗標 (等價 true)
//! - 行尾反斜杠 `\` 續行, 拼接時剝除下一行前導空白
//! - `#` 與 `;` 行首註釋; 行內 `#` 屬於值的一部分
//! - 值做 systemd cunescape: `\s \t \n \r \\ \" \' \a \b \f \v \xHH`
//! - 值前導/尾端空白剝除
//! - 列表指令可重複出現; 空值 (`Key=`) 語義為「清空此前列表」(由 `get_list` fold 實現)
//! - 未知段/未知鍵 **不報錯**, 收集為 `ParseWarning` (高兼容要求)

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ParsedUnit {
    /// 依出現順序的段 (同名段已合併)
    pub sections: Vec<Section>,
    /// 非致命解析警告 (未知/畸形行)
    pub warnings: Vec<ParseWarning>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Section {
    pub name: String,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub key: String,
    pub value: String,
    /// 1-based 行號 (續行取首行)
    pub line: usize,
    /// true = 沒有等號的布爾旗標形式 (`Key` 單獨出現)
    pub is_flag: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseWarning {
    pub line: usize,
    pub message: String,
}

impl Entry {
    /// 布爾語義: 旗標形式恆真; 有值時按 systemd 布爾詞解析
    pub fn as_bool(&self) -> bool {
        if self.is_flag {
            return true;
        }
        parse_bool(&self.value).unwrap_or(false)
    }
}

/// systemd 布爾詞法: 1/yes/true/on 為真, 0/no/false/off 為假 (大小寫不敏感)
pub fn parse_bool(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "1" | "yes" | "true" | "on" => Some(true),
        "0" | "no" | "false" | "off" => Some(false),
        _ => None,
    }
}

impl Section {
    /// 該鍵最後一次出現的原始值 (無則 None)
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .rev()
            .find(|e| e.key == key)
            .map(|e| e.value.as_str())
    }

    /// 該鍵的旗標語義 (最後一次出現)
    pub fn get_flag(&self, key: &str) -> bool {
        self.entries
            .iter()
            .rev()
            .find(|e| e.key == key)
            .map(|e| e.as_bool())
            .unwrap_or(false)
    }

    /// 該鍵所有原始值 (依序)
    pub fn get_all(&self, key: &str) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| e.key == key)
            .map(|e| e.value.as_str())
            .collect()
    }

    /// 列表語義: 依序 fold, 遇空值 (`Key=`) 清空此前累積 (systemd 列表重置語義)
    pub fn get_list(&self, key: &str) -> Vec<&str> {
        let mut out = Vec::new();
        for e in &self.entries {
            if e.key != key {
                continue;
            }
            if e.is_flag {
                continue; // 列表指令的旗標形式無意義, 忽略
            }
            if e.value.is_empty() {
                out.clear();
            } else {
                out.push(e.value.as_str());
            }
        }
        out
    }
}

impl ParsedUnit {
    /// 按名查找段
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }

    pub fn section_mut(&mut self, name: &str) -> Option<&mut Section> {
        self.sections.iter_mut().find(|s| s.name == name)
    }
}

/// 解析單元文件文本。永不失敗 —— 畸形內容進 warnings。
pub fn parse_unit(text: &str) -> ParsedUnit {
    let mut unit = ParsedUnit::default();
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);

    let mut current_section: Option<usize> = None; // index into unit.sections
    let mut logical = String::new();
    let mut logical_line_no = 0usize;
    let mut in_continuation = false;

    for (idx, raw_line) in text.split('\n').enumerate() {
        let line_no = idx + 1;
        // 容忍 CRLF
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);

        if in_continuation {
            logical.push_str(line.trim_start());
        } else {
            logical.clear();
            logical.push_str(line);
            logical_line_no = line_no;
        }

        // 續行判定: 尾部奇數個反斜杠 → 待續
        let trailing_bs = logical.chars().rev().take_while(|&c| c == '\\').count();
        if trailing_bs % 2 == 1 {
            logical.pop(); // 移除續行反斜杠
            in_continuation = true;
            continue;
        }
        in_continuation = false;

        let line_ref = logical.as_str();
        let trimmed = line_ref.trim();

        // 空行 / 註釋
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }

        // 段頭
        if trimmed.starts_with('[') {
            if !trimmed.ends_with(']') {
                unit.warnings.push(ParseWarning {
                    line: logical_line_no,
                    message: format!("malformed section header (missing ']'): {trimmed}"),
                });
                continue;
            }
            let name = &trimmed[1..trimmed.len() - 1];
            if name.is_empty() {
                unit.warnings.push(ParseWarning {
                    line: logical_line_no,
                    message: "empty section name".to_string(),
                });
                continue;
            }
            if name.chars().any(char::is_whitespace) {
                unit.warnings.push(ParseWarning {
                    line: logical_line_no,
                    message: format!("section name contains whitespace: [{name}]"),
                });
            }
            if let Some(i) = unit.sections.iter().position(|s| s.name == name) {
                current_section = Some(i);
            } else {
                unit.sections.push(Section {
                    name: name.to_string(),
                    entries: Vec::new(),
                });
                current_section = Some(unit.sections.len() - 1);
            }
            continue;
        }

        // 鍵值 / 旗標
        let Some(sec) = current_section else {
            unit.warnings.push(ParseWarning {
                line: logical_line_no,
                message: format!("directive outside of any section: {trimmed}"),
            });
            continue;
        };

        let (key, value, is_flag) = match trimmed.split_once('=') {
            Some((k, v)) => {
                let key = k.trim();
                if key.is_empty() {
                    unit.warnings.push(ParseWarning {
                        line: logical_line_no,
                        message: format!("empty key for value: ={v}"),
                    });
                    continue;
                }
                (key, v.trim(), false)
            }
            None => {
                let key = trimmed;
                if key.chars().any(char::is_whitespace) {
                    unit.warnings.push(ParseWarning {
                        line: logical_line_no,
                        message: format!("line is neither directive nor flag: {key}"),
                    });
                    continue;
                }
                (key, "", true)
            }
        };

        unit.sections[sec].entries.push(Entry {
            key: key.to_string(),
            value: cunescape(value),
            line: logical_line_no,
            is_flag,
        });
    }

    if in_continuation {
        unit.warnings.push(ParseWarning {
            line: logical_line_no,
            message: "file ends inside a line continuation".to_string(),
        });
    }
    unit
}

/// systemd cunescape: \s \t \n \r \a \b \f \v \\ \" \' \xHH
/// 未知轉義序列保留原樣 (含反斜杠)。
fn cunescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('s') => out.push(' '),
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('a') => out.push('\u{07}'),
            Some('b') => out.push('\u{08}'),
            Some('f') => out.push('\u{0c}'),
            Some('v') => out.push('\u{0b}'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('\'') => out.push('\''),
            Some('x') => {
                let h1 = it.peek().and_then(|c| c.to_digit(16));
                if let Some(h1) = h1 {
                    it.next();
                    let h2 = it.peek().and_then(|c| c.to_digit(16));
                    let code = if let Some(h2) = h2 {
                        it.next();
                        h1 * 16 + h2
                    } else {
                        h1
                    };
                    out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                } else {
                    out.push_str("\\x"); // 無 hex 跟隨, 保留原樣
                }
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'), // 結尾孤兒反斜杠
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cunescape_basics() {
        assert_eq!(cunescape("a\\sb"), "a b");
        assert_eq!(cunescape("\\x41"), "A");
        assert_eq!(cunescape("end\\"), "end\\");
        assert_eq!(cunescape("keep\\q"), "keep\\q");
    }
}
