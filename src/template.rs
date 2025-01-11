use crate::{
    dto::{
        mesh_packet::{MeshPacket as MeshPacketDto, Payload},
        GatewayPacketInfo, NodeSelectResult, PlotData, StatsSelectResult,
    },
    proto::meshtastic::config::device_config::Role,
    util::capitalize,
};
use askama::Template;
use chrono::prelude::*;
use chrono::TimeDelta;

const SVG_ICONS_CONTENT: &str = include_str!(env!("SVG_ICONS_PATH"));
const GIT_VERSION: &str = env!("VERGEN_GIT_DESCRIBE");

#[derive(Template)]
#[template(path = "index.html")]
pub(crate) struct IndexTemplate {}

#[derive(Template)]
#[template(path = "_stats.html")]
pub(crate) struct StatsTemplate {
    pub stats: StatsSelectResult,
}

#[derive(Template)]
#[template(path = "_node.html")]
pub(crate) struct NodeTemplate {
    pub node: NodeSelectResult,
    pub geom: Option<String>,
}

#[derive(Template)]
#[template(path = "_packet.html")]
pub(crate) struct PacketTemplate {
    pub packet: MeshPacketDto,
}

#[derive(Template)]
#[template(path = "_position_details.html")]
pub(crate) struct PositionDetailsTemplate {
    pub packet: MeshPacketDto,
}

#[derive(Template)]
#[template(path = "_node_details.html")]
pub(crate) struct NodeDetailsTemplate {
    pub node: NodeSelectResult,
    pub plots: Vec<PlotData>,
    pub gateway_packet_info: Vec<GatewayPacketInfo>,
    pub selected_node: Option<String>,
}

fn logo() -> String {
    "<svg viewBox=\"0 0 512 256\" class=\"logo\"><use xlink:href=\"#icon-meshstellar\"></use></svg>"
        .to_string()
}

fn icon(icon_name: &str) -> String {
    format!(
        "<svg viewBox=\"0 0 24 24\" class=\"icon icon-{}\"><use xlink:href=\"#icon-{}\"></use></svg>",
        icon_name,
        icon_name
    )
}

fn role_name(role: &i64) -> String {
    Role::try_from(*role as i32)
        .map(|role| capitalize(role.as_str_name().replace('_', " ").as_str()))
        .unwrap_or_else(|_| "Unknown role".to_string())
}

fn format_mesh_gps(latitude: &f64, longitude: &f64) -> String {
    format!("{:.7}, {:.7}", latitude, longitude)
}

fn format_timestamp(nanos: &i64) -> String {
    Utc.timestamp_nanos(*nanos).to_rfc3339()
}

fn get_battery_info(voltage: &Option<f64>, battery_level: &Option<i64>) -> (String, String) {
    if let (Some(voltage), Some(battery_level)) = (voltage, battery_level) {
        let info_string = format!("{}% {:.2}V", battery_level, voltage);
        let icon = match battery_level {
            0..=4 => "battery-alert",
            5..=14 => "battery-outline",
            15..=34 => "battery-low",
            35..=79 => "battery-medium",
            80..=100 => "battery-high",
            101 => return ("power-plug".into(), format!("{:.2}V", voltage)),
            _ => "battery-unknown",
        };
        (icon.into(), info_string)
    } else {
        ("battery-unknown".into(), "".into())
    }
}

fn format_duration_sec(duration_sec: &i64) -> String {
    let duration = TimeDelta::seconds(*duration_sec);

    // Calculate days, hours, minutes, and seconds
    let days = duration.num_days();
    let hours = duration.num_hours() % 24;
    let minutes = duration.num_minutes() % 60;
    let seconds = duration.num_seconds() % 60;

    let mut time_str = String::new();

    if days > 0 {
        time_str.push_str(&format!("{} days", days));
    }
    if hours > 0 {
        if !time_str.is_empty() {
            time_str.push_str(", ");
        }
        time_str.push_str(&format!("{} hours", hours));
    }
    if minutes > 0 {
        if !time_str.is_empty() {
            time_str.push_str(", ");
        }
        time_str.push_str(&format!("{} minutes", minutes));
    }
    if seconds > 0 || time_str.is_empty() {
        // Include seconds if everything else is zero
        if !time_str.is_empty() {
            time_str.push_str(", ");
        }
        time_str.push_str(&format!("{} seconds", seconds));
    }

    time_str
}

mod filters {
    use askama::Error::Fmt;
    use num_traits::{NumCast, Signed};
    use std::fmt::{self, LowerHex};

    pub fn hex<T>(num: &T) -> ::askama::Result<String>
    where
        T: LowerHex,
    {
        let s = format!("{:x}", num);
        Ok(s)
    }

    /// Absolute value
    pub fn abs_ref<T>(number: &T) -> ::askama::Result<T>
    where
        T: Signed,
    {
        Ok(number.abs())
    }

    pub fn into_f64_ref<T>(number: &&T) -> ::askama::Result<f64>
    where
        T: NumCast,
    {
        number.to_f64().ok_or(Fmt(fmt::Error))
    }
}
