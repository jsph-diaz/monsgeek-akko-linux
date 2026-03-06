// Device Registry - Pattern-based HID device matching
//
// Matches any VID=0x3151 device with vendor usage page + correct usage/interface_number.
// No PID whitelist needed — all RY5088/AT32F405 keyboards share the same interface layout.

use super::constants;
use super::interface::{HidInterface, InterfaceType};
use std::sync::OnceLock;

/// Registry of known HID device interfaces
pub struct DeviceRegistry;

impl DeviceRegistry {
    /// Find the FEATURE interface for a device (constructs on the fly)
    pub fn find_feature_interface(&self, vid: u16, pid: u16) -> Option<HidInterface> {
        if vid == constants::VENDOR_ID {
            Some(HidInterface::feature(vid, pid))
        } else {
            None
        }
    }

    /// Find the INPUT interface for a device (constructs on the fly)
    pub fn find_input_interface(&self, vid: u16, pid: u16) -> Option<HidInterface> {
        if vid == constants::VENDOR_ID {
            Some(HidInterface::input(vid, pid))
        } else {
            None
        }
    }

    /// Check if a VID/PID is a known device (VID-based)
    pub fn is_known_device(&self, vid: u16, _pid: u16) -> bool {
        vid == constants::VENDOR_ID
    }

    /// Find interface matching a hidapi DeviceInfo
    pub fn find_matching(&self, info: &hidapi::DeviceInfo) -> Option<HidInterface> {
        if info.vendor_id() != constants::VENDOR_ID {
            return None;
        }
        if !constants::is_vendor_usage_page(info.usage_page()) {
            return None;
        }
        let iface_type = if info.usage() == constants::USAGE_FEATURE
            && info.interface_number() == constants::INTERFACE_FEATURE
        {
            InterfaceType::Feature
        } else if info.usage() == constants::USAGE_INPUT
            && info.interface_number() == constants::INTERFACE_INPUT
        {
            InterfaceType::Input
        } else {
            return None;
        };
        Some(HidInterface {
            vid: info.vendor_id(),
            pid: info.product_id(),
            usage: info.usage(),
            usage_page: info.usage_page(),
            interface_number: info.interface_number(),
            interface_type: iface_type,
        })
    }
}

impl Default for DeviceRegistry {
    fn default() -> Self {
        Self
    }
}

// Global singleton registry
static REGISTRY: OnceLock<DeviceRegistry> = OnceLock::new();

/// Get the global device registry
pub fn device_registry() -> &'static DeviceRegistry {
    REGISTRY.get_or_init(|| DeviceRegistry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_feature_interface() {
        let reg = device_registry();
        let iface = reg.find_feature_interface(0x3151, 0x5030);
        assert!(iface.is_some());
        let iface = iface.unwrap();
        assert_eq!(iface.usage, 0x02);
        assert_eq!(iface.interface_number, 2);
    }

    #[test]
    fn test_find_input_interface() {
        let reg = device_registry();
        let iface = reg.find_input_interface(0x3151, 0x5030);
        assert!(iface.is_some());
        let iface = iface.unwrap();
        assert_eq!(iface.usage, 0x01);
        assert_eq!(iface.interface_number, 1);
    }

    #[test]
    fn test_unknown_pid_still_matches() {
        let reg = device_registry();
        // FUN 60 Pro (PID 0x502d) should match by VID alone
        assert!(reg.find_feature_interface(0x3151, 0x502d).is_some());
        assert!(reg.find_input_interface(0x3151, 0x502d).is_some());
        assert!(reg.is_known_device(0x3151, 0x502d));
    }

    #[test]
    fn test_non_monsgeek_rejected() {
        let reg = device_registry();
        assert!(reg.find_feature_interface(0x1234, 0x5678).is_none());
        assert!(!reg.is_known_device(0x1234, 0x5678));
    }
}
