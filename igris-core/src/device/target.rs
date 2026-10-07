//! Choosing a device from the user's words: "on my laptop", "on my PC",
//! "here", "on my phone", or a device's name. When more than one device fits,
//! IGRIS asks instead of guessing.

use super::hub::ThisDevice;
use super::identity::Platform;
use super::registry::Device;

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// This device.
    Here,
    Remote(Box<Device>),
}

fn is_computer(p: Platform) -> bool {
    matches!(p, Platform::Windows | Platform::Linux | Platform::Macos)
}

/// Resolve `phrase` against this device and the active trusted devices.
/// The error is a sentence for the user (and the model).
pub fn resolve(phrase: &str, me: &ThisDevice, devices: &[Device]) -> Result<Target, String> {
    let p = phrase.trim().to_lowercase();
    let p = p.trim_start_matches("on ").trim_start_matches("my ").trim_start_matches("the ").trim();
    let active: Vec<&Device> = devices.iter().filter(|d| d.active()).collect();
    let names = || {
        let mut n: Vec<String> = active.iter().map(|d| format!("\"{}\" ({})", d.name, d.platform.as_str())).collect();
        n.push(format!("\"{}\" (this device)", me.name));
        n.join(", ")
    };
    if p.is_empty() {
        return Err(format!("Which device? You have: {}.", names()));
    }
    if matches!(p, "here" | "this device" | "this one" | "this computer" | "this pc" | "this phone" | "local") {
        return Ok(Target::Here);
    }
    // Exact name or id first.
    if p == me.name.to_lowercase() || p == me.device_id {
        return Ok(Target::Here);
    }
    if let Some(d) = active.iter().find(|d| d.name.to_lowercase() == p || d.device_id == p) {
        return Ok(Target::Remote(Box::new((*d).clone())));
    }
    let computer_words = ["pc", "computer", "laptop", "desktop", "windows", "windows pc", "mac", "workstation"];
    let phone_words = ["phone", "mobile", "android", "cell", "smartphone"];
    let wants: Option<fn(Platform) -> bool> = if computer_words.contains(&p) {
        Some(is_computer)
    } else if phone_words.contains(&p) {
        Some(Platform::is_phone)
    } else {
        None
    };
    let Some(kind) = wants else {
        return Err(format!("I don't know a device called \"{phrase}\". You have: {}.", names()));
    };
    let mut fits: Vec<&Device> = active.iter().copied().filter(|d| kind(d.platform)).collect();
    // "laptop"/"desktop" narrow by name when possible.
    if fits.len() > 1 && matches!(p, "laptop" | "desktop") {
        let named: Vec<&Device> = fits.iter().copied().filter(|d| d.name.to_lowercase().contains(p)).collect();
        if !named.is_empty() {
            fits = named;
        }
    }
    match (kind(me.platform), fits.len()) {
        // "my phone" said on the phone (and no other phone) means here.
        (true, 0) => Ok(Target::Here),
        (false, 0) => Err(if active.is_empty() {
            "No other devices are paired with this IGRIS yet. Pair one in Settings → Devices.".to_string()
        } else {
            format!("None of your paired devices is a {p}. You have: {}.", names())
        }),
        (false, 1) => Ok(Target::Remote(Box::new(fits[0].clone()))),
        _ => Err(format!(
            "More than one device fits \"{phrase}\": {}. Which one?",
            fits.iter()
                .map(|d| format!("\"{}\"", d.name))
                .chain(kind(me.platform).then(|| format!("\"{}\" (this device)", me.name)))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::identity::Identity;

    fn this(name: &str, platform: Platform) -> ThisDevice {
        ThisDevice { device_id: Identity::generate(name, platform).device_id, name: name.into(), platform, capabilities: vec![], key_protection: "none".into() }
    }

    fn dev(name: &str, platform: Platform) -> Device {
        let id = Identity::generate(name, platform);
        let p = id.public();
        Device {
            device_id: p.device_id,
            owner_id: "own_x".into(),
            name: name.into(),
            platform,
            signing_key: p.signing_key,
            kx_key: p.kx_key,
            capabilities: vec![],
            trusted: true,
            revoked_at: None,
            last_seen: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn natural_phrases_pick_the_right_device() {
        let phone = this("Pixel", Platform::Android);
        let devices = vec![dev("My PC", Platform::Windows)];
        assert!(matches!(resolve("on my laptop", &phone, &devices), Ok(Target::Remote(d)) if d.name == "My PC"));
        assert!(matches!(resolve("my pc", &phone, &devices), Ok(Target::Remote(_))));
        assert!(matches!(resolve("windows", &phone, &devices), Ok(Target::Remote(_))));
        assert_eq!(resolve("here", &phone, &devices), Ok(Target::Here));
        assert_eq!(resolve("my phone", &phone, &devices), Ok(Target::Here));
        assert!(matches!(resolve("My PC", &phone, &devices), Ok(Target::Remote(_))));
        assert!(resolve("toaster", &phone, &devices).is_err());
    }

    #[test]
    fn ambiguity_asks_and_revoked_devices_dont_count() {
        let phone = this("Pixel", Platform::Android);
        let mut devices = vec![dev("Work laptop", Platform::Windows), dev("Gaming desktop", Platform::Windows)];
        let e = resolve("computer", &phone, &devices).unwrap_err();
        assert!(e.contains("Work laptop") && e.contains("Gaming desktop") && e.contains("Which one"), "{e}");
        // "laptop" narrows by name.
        assert!(matches!(resolve("laptop", &phone, &devices), Ok(Target::Remote(d)) if d.name == "Work laptop"));
        devices[1].revoked_at = Some("now".into());
        assert!(matches!(resolve("computer", &phone, &devices), Ok(Target::Remote(d)) if d.name == "Work laptop"));
        assert!(resolve("pc", &phone, &[]).unwrap_err().contains("No other devices"));
    }
}
