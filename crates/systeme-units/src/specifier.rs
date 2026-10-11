//! systemd 說明符 (specifier) 展開與變量展開。
//!
//! 支援: `%% %n %N %p %P %i %I %j %J %f %t %S %C %L %h %u %U %g %G %m %b %H %v`
//! 未知說明符 **保留原樣並記警告** (高兼容原則)。

use crate::types::UnitType;

/// 展開上下文 —— 由 manager 依運行模式 (系統/用戶) 填充, 便於測試
#[derive(Debug, Clone, Default)]
pub struct SpecContext {
    pub runtime_dir: String,    // %t  /run 或 $XDG_RUNTIME_DIR
    pub state_dir: String,      // %S  /var/lib 或 ~/.local/state
    pub cache_dir: String,      // %C  /var/cache 或 ~/.cache
    pub log_dir: String,        // %L  /var/log 或 ~/.local/state/log
    pub home: String,           // %h  服務用戶 home
    pub user: String,           // %u
    pub uid: u32,               // %U
    pub group: String,          // %g
    pub gid: u32,               // %G
    pub machine_id: String,     // %m
    pub boot_id: String,        // %b
    pub hostname: String,       // %H
    pub kernel_release: String, // %v
}

/// 展開結果: 新字串 + 未知說明符警告
#[derive(Debug, Clone, PartialEq)]
pub struct Expansion {
    pub text: String,
    pub warnings: Vec<String>,
}

/// 單元名拆解: prefix / instance
/// "nginx.service" → ("nginx", None); "foo@bar.service" → ("foo", Some("bar"))
pub fn split_instance(unit_name: &str) -> (String, Option<String>) {
    let (prefix, instance) = match unit_name.split_once('@') {
        Some((p, rest)) => (p.to_string(), Some(rest.to_string())),
        None => (unit_name.to_string(), None),
    };
    // 去掉類型後綴
    let prefix = match UnitType::split_name(&prefix) {
        Some((p, _)) => p.to_string(),
        None => prefix,
    };
    let instance = instance.map(|i| match UnitType::split_name(&i) {
        Some((p, _)) => p.to_string(),
        None => i,
    });
    (prefix, instance)
}

/// systemd unescape: "\\xHH" → 字符 (嚴格模式, 用於 %N)
pub fn unescape_unit_str(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 <= bytes.len() && bytes[i + 1] == b'x' {
            let hex = &s[i + 2..i + 4];
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v as char);
                i += 4;
                continue;
            }
        }
        out.push(s[i..].chars().next().unwrap());
        i += s[i..].chars().next().unwrap().len_utf8();
    }
    out
}

/// 路徑語義 unescape (用於 %P %I %J %f):
/// "\\xHH" 先解碼 (故 "\\x2d" 保留為字面 '-'), 其餘裸 '-' → '/' (路徑分隔)
pub fn unescape_unit_path(s: &str) -> String {
    let strict = unescape_unit_str(s);
    // 標記 \x2d 的位置: 先把 \x2d 換成佔位, 替換 '-' 後再還原
    const PLACEHOLDER: char = '\u{1}'; // 佔位符, 不出現於合法單元名
    let mut marked = String::with_capacity(strict.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 <= bytes.len() && bytes[i + 1] == b'x' {
            let hex = &s[i + 2..i + 4];
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                if v as char == '-' {
                    marked.push(PLACEHOLDER);
                    i += 4;
                    continue;
                }
                marked.push(v as char);
                i += 4;
                continue;
            }
        }
        marked.push(s[i..].chars().next().unwrap());
        i += s[i..].chars().next().unwrap().len_utf8();
    }
    marked
        .chars()
        .map(|c| if c == '-' { '/' } else if c == PLACEHOLDER { '-' } else { c })
        .collect()
}

/// systemd escape (instance 風格): '/' → '-', 非字母數字/常見保留字符 → "\\xHH"
/// path_mode: 額外將 '-' → "\\x2d"、':' → "\\x3a" (mount 路徑轉義語義)
pub fn escape_unit_str(s: &str, path_mode: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '/' => out.push('-'),
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '.' | '@' => out.push(c),
            '-' | ':' if !path_mode => out.push(c),
            c if !c.is_ascii() => out.push(c), // 非 ASCII 保留
            c => out.push_str(&format!("\\x{:02x}", c as u32)),
        }
    }
    out
}

/// 展開說明符
pub fn expand_specifiers(input: &str, unit_name: &str, ctx: &SpecContext) -> Expansion {
    let (prefix, instance) = split_instance(unit_name);
    let prefix_unescaped = unescape_unit_path(&prefix); // %P/%J 路徑語義
    let instance_unescaped = instance.as_deref().map(unescape_unit_path); // %I 路徑語義

    let mut out = String::with_capacity(input.len());
    let mut warnings = Vec::new();
    let mut it = input.chars().peekable();
    while let Some(c) = it.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let Some(spec) = it.next() else {
            out.push('%'); // 結尾孤兒 %
            break;
        };
        match spec {
            '%' => out.push('%'),
            'n' => out.push_str(unit_name),
            'N' => out.push_str(&unescape_unit_str(unit_name)),
            'p' => out.push_str(&prefix),
            'P' => out.push_str(&prefix_unescaped),
            'i' => out.push_str(instance.as_deref().unwrap_or("")),
            'I' => out.push_str(instance_unescaped.as_deref().unwrap_or("")),
            'j' => {
                let comp = final_component(&prefix_unescaped);
                out.push_str(&escape_unit_str(&comp, false));
            }
            'J' => out.push_str(&final_component(&prefix_unescaped)),
            'f' => {
                if let Some(inst) = &instance_unescaped {
                    out.push('/');
                    out.push_str(inst);
                }
            }
            't' => out.push_str(&ctx.runtime_dir),
            'S' => out.push_str(&ctx.state_dir),
            'C' => out.push_str(&ctx.cache_dir),
            'L' => out.push_str(&ctx.log_dir),
            'h' => out.push_str(&ctx.home),
            'u' => out.push_str(&ctx.user),
            'U' => out.push_str(&ctx.uid.to_string()),
            'g' => out.push_str(&ctx.group),
            'G' => out.push_str(&ctx.gid.to_string()),
            'm' => out.push_str(&ctx.machine_id),
            'b' => out.push_str(&ctx.boot_id),
            'H' => out.push_str(&ctx.hostname),
            'v' => out.push_str(&ctx.kernel_release),
            other => {
                // 未知說明符: 保留原樣 + 警告 (高兼容)
                out.push('%');
                out.push(other);
                warnings.push(format!("unknown specifier %{other}"));
            }
        }
    }
    Expansion { text: out, warnings }
}

fn final_component(s: &str) -> String {
    match s.rsplit_once('/') {
        Some((_, last)) => last.to_string(),
        None => s.to_string(),
    }
}

/// 展開 `$VAR` / `${VAR}` / `$$` (systemd 環境變量語法, 用於 Exec* 等)
pub fn expand_env_vars(input: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        match chars.peek().copied() {
            Some('$') => {
                chars.next();
                out.push('$');
            }
            Some('{') => {
                chars.next();
                let mut name = String::new();
                let mut closed = false;
                for ch in chars.by_ref() {
                    if ch == '}' {
                        closed = true;
                        break;
                    }
                    name.push(ch);
                }
                if closed && !name.is_empty() {
                    if let Some(v) = lookup(&name) {
                        out.push_str(&v);
                    }
                } else {
                    // 未閉合或空名: 原樣保留
                    out.push_str("${");
                    out.push_str(&name);
                    if closed {
                        out.push('}');
                    }
                }
            }
            Some(first) if first.is_ascii_alphabetic() || first == '_' => {
                let mut name = String::new();
                name.push(first);
                chars.next();
                while let Some(&ch) = chars.peek() {
                    if ch.is_ascii_alphanumeric() || ch == '_' {
                        name.push(ch);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if let Some(v) = lookup(&name) {
                    out.push_str(&v);
                }
            }
            _ => out.push('$'), // 孤兒 $ 或 $數字: 保留
        }
    }
    out
}
