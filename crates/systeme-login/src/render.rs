//! loginctl 輸出渲染。

use crate::sessions::{Session, Sessions};

/// 會話表: SESSION UID USER SEAT TTY。
pub fn render_sessions_table(s: &Sessions) -> String {
    let rows: Vec<&Session> = s.list();
    let mut out = String::from("SESSION UID USER SEAT TTY\n");
    for sess in rows {
        out.push_str(&format!(
            "{} {} {} {} {}\n",
            sess.id,
            sess.uid,
            sess.user,
            sess.seat.as_deref().unwrap_or("-"),
            sess.tty.as_deref().unwrap_or("-"),
        ));
    }
    out
}

/// 單會話詳情塊。
pub fn render_session_detail(s: &Sessions, id: &str) -> String {
    let Some(sess) = s.get(id) else {
        return format!("Failed to get session: {id}\n");
    };
    let mut out = String::new();
    out.push_str(&format!("   Id={}\n", sess.id));
    out.push_str(&format!("   User={}\n", sess.user));
    out.push_str(&format!("   UID={}\n", sess.uid));
    out.push_str(&format!("   Seat={}\n", sess.seat.as_deref().unwrap_or("-")));
    out.push_str(&format!("   TTY={}\n", sess.tty.as_deref().unwrap_or("-")));
    out.push_str(&format!("   State={}\n", sess.state.as_str()));
    out.push_str(&format!("   LockedHint={}\n", if sess.locked { "yes" } else { "no" }));
    out.push_str(&format!("   IdleHint={}\n", if sess.idle { "yes" } else { "no" }));
    out
}
