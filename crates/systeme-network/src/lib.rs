//! systeme-network: systemd-networkd 配置解析 (.network/.link/.netdev) 純邏輯層。
//!
//! bin 層真實 netlink/接口操作留待 cfg(unix) 集成; 本 crate 只負責
//! 配置解析、接口匹配 (glob) 與應用計劃生成, 保證跨平台可測。

pub mod apply;
pub mod link;
pub mod netdev;
pub mod network;
pub mod resolved;

pub use apply::{apply_steps, link_apply_argv, list_interface_names, netdev_create_argv, plan_to_argv, ApplyStep};
pub use link::LinkConfig;
pub use netdev::{NetDevConfig, NetDevKind};
pub use network::{match_iface, match_network, network_apply_plan, DhcpMode, NetworkConfig, NetworkMatch};
pub use resolved::{collect_dns, parse_resolv_conf, render_resolv_conf, stub_resolv_conf, ResolvConf};
