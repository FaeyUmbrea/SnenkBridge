use snenk_bridge_service::tracking::client::TrackingClientType;

pub const COLOR_RED: (u8, u8, u8) = (0xcc, 0x44, 0x44);
pub const COLOR_YELLOW: (u8, u8, u8) = (0xcc, 0x99, 0x44);
pub const COLOR_ORANGE: (u8, u8, u8) = (0xdd, 0x77, 0x33);
pub const COLOR_GREEN: (u8, u8, u8) = (0x5d, 0xba, 0x7d);

pub fn color(rgb: (u8, u8, u8)) -> slint::Color {
    slint::Color::from_argb_u8(255, rgb.0, rgb.1, rgb.2)
}

/// Convert a Slint i32 combo-box index to usize. Negative values become 0.
pub fn slint_idx(index: i32) -> usize {
    usize::try_from(index).unwrap_or(0)
}

pub fn tracking_client_type(index: i32) -> TrackingClientType {
    match index {
        1 => TrackingClientType::IFacialMocap,
        _ => TrackingClientType::VTubeStudio,
    }
}

pub fn timeout_ms(val: &str) -> u64 {
    val.parse::<u64>().unwrap_or(3000)
}
