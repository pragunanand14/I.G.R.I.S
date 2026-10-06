use starship_battery::units::ratio::percent;
use starship_battery::units::time::second;
use starship_battery::{Manager, State};

pub use super::BatteryInfo;

/// Read the first battery. Returns `None` if there is no battery or it can't
/// be read — the UI then shows "No battery" rather than a guessed value.
pub fn read() -> Option<BatteryInfo> {
    let manager = Manager::new().ok()?;
    let mut batteries = manager.batteries().ok()?;
    let battery = batteries.next()?.ok()?;
    let state = match battery.state() {
        State::Charging => "charging",
        State::Discharging => "discharging",
        State::Full => "full",
        State::Empty => "empty",
        State::Paused => "paused",
        State::Unknown => "unknown",
    };
    Some(BatteryInfo {
        percent: battery.state_of_charge().get::<percent>().clamp(0.0, 100.0),
        state,
        time_to_empty_secs: battery.time_to_empty().map(|t| t.get::<second>() as u64),
        time_to_full_secs: battery.time_to_full().map(|t| t.get::<second>() as u64),
    })
}
