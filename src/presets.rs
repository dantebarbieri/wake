//! The machines in the house. MACs are each wired NIC's permanent address;
//! a magic packet only reaches a sleeping machine through its wired port.

use std::net::Ipv4Addr;

pub const LAN_BROADCAST: Ipv4Addr = Ipv4Addr::new(192, 168, 50, 255);
pub const WOL_PORT: u16 = 9;

pub struct Preset {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub macs: &'static [[u8; 6]],
    pub ip: Ipv4Addr,
    /// TCP port that answers once the machine is usable, and its name.
    pub check: (u16, &'static str),
    /// BMC that can power the machine on over IPMI.
    pub bmc: Option<Ipv4Addr>,
    /// Shown when the machine doesn't come up.
    pub hint: &'static str,
}

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "totoro",
        aliases: &["pc", "dante-pc", "windows"],
        description: "Windows desktop (wired Ethernet must be plugged in)",
        macs: &[[0x74, 0x56, 0x3c, 0xb4, 0xf5, 0x4e]],
        ip: Ipv4Addr::new(192, 168, 50, 180),
        check: (3389, "RDP"),
        bmc: None,
        hint: "Is its Ethernet cable plugged in? Wi-Fi can't receive a magic packet.",
    },
    Preset {
        name: "no-face",
        aliases: &["noface", "studio", "ai-studio", "mac"],
        description: "Mac Studio (wakes from sleep, not from shutdown)",
        macs: &[[0xbc, 0x89, 0xc1, 0xe2, 0x01, 0x2b]],
        ip: Ipv4Addr::new(192, 168, 50, 80),
        check: (22, "SSH"),
        bmc: None,
        hint: "A Mac that was shut down (not asleep) can't be woken over the network.",
    },
    Preset {
        name: "calcifer",
        aliases: &["server", "homeserver"],
        description: "Homeserver (both bonded NICs; IPMI if configured)",
        // enp66s0f1 (the bond's MAC) and enp66s0f0.
        macs: &[
            [0x9c, 0x6b, 0x00, 0x45, 0x2b, 0xc2],
            [0x9c, 0x6b, 0x00, 0x45, 0x2b, 0xc1],
        ],
        ip: Ipv4Addr::new(192, 168, 50, 100),
        check: (28, "SSH"),
        bmc: Some(Ipv4Addr::new(192, 168, 50, 50)),
        hint: "Power it on from the BMC: set WAKE_IPMI_USER and IPMI_PASSWORD, or use https://192.168.50.50.",
    },
];

pub fn find(name: &str) -> Option<&'static Preset> {
    let name = name.to_ascii_lowercase();
    PRESETS
        .iter()
        .find(|p| p.name == name || p.aliases.contains(&name.as_str()))
}

pub fn format_mac(mac: &[u8; 6]) -> String {
    mac.iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

pub fn parse_mac(value: &str) -> Result<[u8; 6], String> {
    let hex: String = value.chars().filter(|c| *c != ':' && *c != '-').collect();
    let separators: Vec<char> = value.chars().filter(|c| *c == ':' || *c == '-').collect();
    let shape_ok = match separators.len() {
        0 => value.len() == 12,
        5 => {
            separators.iter().all(|c| *c == separators[0])
                && value.len() == 17
                && value
                    .chars()
                    .enumerate()
                    .all(|(i, c)| (i % 3 == 2) == (c == separators[0]))
        }
        _ => false,
    };
    if !shape_ok || hex.len() != 12 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "`{value}` is not a preset or a MAC address (AA:BB:CC:DD:EE:FF)"
        ));
    }
    let mut mac = [0u8; 6];
    for (i, byte) in mac.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
    }
    if mac == [0; 6] || mac[0] & 1 == 1 {
        return Err(format!("`{value}` is not a unicast device MAC"));
    }
    Ok(mac)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_presets_by_alias_case_insensitively() {
        for name in ["Totoro", "PC", "dante-pc", "windows"] {
            assert_eq!(find(name).unwrap().name, "totoro");
        }
        assert_eq!(find("Studio").unwrap().name, "no-face");
        assert_eq!(find("SERVER").unwrap().name, "calcifer");
        assert!(find("toaster").is_none());
    }

    #[test]
    fn names_and_aliases_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for p in PRESETS {
            for n in std::iter::once(&p.name).chain(p.aliases) {
                assert!(seen.insert(*n), "duplicate preset name {n}");
            }
        }
    }

    #[test]
    fn parses_mac_formats() {
        let want = [0x02, 0x11, 0x22, 0x33, 0x44, 0x55];
        for s in [
            "02:11:22:33:44:55",
            "02-11-22-33-44-55",
            "021122334455",
            "02:11:22:33:44:55".to_lowercase().as_str(),
        ] {
            assert_eq!(parse_mac(s).unwrap(), want, "{s}");
        }
    }

    #[test]
    fn rejects_bad_macs() {
        for s in [
            "02:11:22:33:44",
            "02:11-22:33:44:55",
            "0211:22:33:44:55",
            "zz:11:22:33:44:55",
            "00:00:00:00:00:00",
            "01:00:5e:00:00:01",
            "02:11:22:33:44:55:66",
        ] {
            assert!(parse_mac(s).is_err(), "{s}");
        }
    }
}
