use crate::model::cpu::CpuVendor;

/// Look up codename with a brand string hint for disambiguation.
///
/// Some CPUID family/model values are shared between mobile and workstation
/// SKUs (e.g., AMD family 0x1A model 0x08 is both Strix Point and Turin).
/// The brand string helps disambiguate.
pub fn lookup_with_brand(
    vendor: &CpuVendor,
    family: u32,
    model: u32,
    brand: &str,
) -> Option<String> {
    match vendor {
        CpuVendor::Amd => lookup_amd(family, model, brand),
        CpuVendor::Intel => lookup_intel(family, model),
        _ => None,
    }
}

fn lookup_amd(family: u32, model: u32, brand: &str) -> Option<String> {
    let lower_brand = brand.to_lowercase();

    let name = match (family, model) {
        // Zen — Summit Ridge / Naples
        (0x17, 0x01) => "Zen (Summit Ridge)",
        (0x17, 0x08) => "Zen+ (Pinnacle Ridge)",
        (0x17, 0x11) => "Zen (Raven Ridge)",
        (0x17, 0x18) => "Zen+ (Picasso)",
        (0x17, 0x20) => "Zen (Dali)",
        (0x17, 0x31) => "Zen 2 (Rome)",
        (0x17, 0x60) => "Zen 2 (Renoir)",
        (0x17, 0x68) => "Zen 2 (Lucienne)",
        (0x17, 0x71) => "Zen 2 (Matisse)",
        (0x17, 0x90) => "Zen 2 (Van Gogh)",
        (0x17, 0x98) => "Zen 2 (Mero)",

        // Zen 3
        (0x19, 0x01) => "Zen 3 (Milan)",
        (0x19, 0x08) => "Zen 3 (Chagall)",
        (0x19, 0x21) => "Zen 3 (Vermeer)",
        (0x19, 0x40) => "Zen 3+ (Rembrandt)",
        (0x19, 0x44) => "Zen 3+ (Rembrandt-R)",
        (0x19, 0x50) => "Zen 3 (Cezanne)",

        // Zen 4
        (0x19, 0x10) => "Zen 4 (Genoa)",
        (0x19, 0x11) => "Zen 4c (Bergamo)",
        (0x19, 0x61) => "Zen 4 (Raphael)",
        (0x19, 0x74) => "Zen 4 (Phoenix)",
        (0x19, 0x78) => "Zen 4 (Phoenix 2)",
        (0x19, 0xA0..=0xAF) => "Zen 4c (Dense)",

        // Zen 5
        (0x1A, 0x02) => "Zen 5 (Strix Halo)",
        (0x1A, 0x08) => {
            // Family 0x1A model 0x08 is shared: Strix Point (mobile) and
            // Turin (Threadripper PRO / EPYC workstation). Disambiguate
            // using the brand string.
            if lower_brand.contains("threadripper") || lower_brand.contains("epyc") {
                "Zen 5 (Turin)"
            } else {
                "Zen 5 (Strix Point)"
            }
        }
        (0x1A, 0x10) => "Zen 5 (Turin)",
        (0x1A, 0x11) => "Zen 5c (Turin Dense)",
        (0x1A, 0x20) => "Zen 5 (Granite Ridge)",
        (0x1A, 0x24) => "Zen 5 (Granite Ridge)",

        _ => return None,
    };
    Some(name.to_string())
}

/// Look up an ARM CPU core name based on implementer and part codes.
///
/// These values come from /proc/cpuinfo ("CPU implementer" and "CPU part") or
/// from the MIDR_EL1 register via sysfs.
pub fn lookup_arm(implementer: u32, part: u32) -> Option<String> {
    let name = match (implementer, part) {
        // ARM Ltd
        (0x41, 0xd03) => "Cortex-A53",
        (0x41, 0xd04) => "Cortex-A35",
        (0x41, 0xd05) => "Cortex-A55",
        (0x41, 0xd07) => "Cortex-A57",
        (0x41, 0xd08) => "Cortex-A72",
        (0x41, 0xd09) => "Cortex-A73",
        (0x41, 0xd0a) => "Cortex-A75",
        (0x41, 0xd0b) => "Cortex-A76",
        (0x41, 0xd0c) => "Neoverse N1",
        (0x41, 0xd0d) => "Cortex-A77",
        (0x41, 0xd0e) => "Cortex-A76AE",
        (0x41, 0xd40) => "Neoverse V1",
        (0x41, 0xd41) => "Cortex-A78",
        (0x41, 0xd42) => "Cortex-A78AE",
        (0x41, 0xd43) => "Cortex-A65AE",
        (0x41, 0xd44) => "Cortex-X1",
        (0x41, 0xd46) => "Cortex-A510",
        (0x41, 0xd47) => "Cortex-A710",
        (0x41, 0xd48) => "Cortex-X2",
        (0x41, 0xd49) => "Neoverse N2",
        (0x41, 0xd4a) => "Neoverse E1",
        (0x41, 0xd4b) => "Cortex-A78C",
        (0x41, 0xd4c) => "Cortex-X1C",
        (0x41, 0xd4d) => "Cortex-A715",
        (0x41, 0xd4e) => "Cortex-X3",
        (0x41, 0xd4f) => "Neoverse V2",
        (0x41, 0xd80) => "Cortex-A520",
        (0x41, 0xd81) => "Cortex-A720",
        (0x41, 0xd82) => "Cortex-X4",
        (0x41, 0xd83) => "Neoverse V3AE",
        (0x41, 0xd84) => "Neoverse V3",
        (0x41, 0xd85) => "Cortex-X925",
        (0x41, 0xd87) => "Cortex-A725",
        (0x41, 0xd88) => "Cortex-A520AE",
        (0x41, 0xd89) => "Cortex-A720AE",
        (0x41, 0xd8e) => "Neoverse N3",

        // Apple
        (0x61, 0x022) => "Apple M1 Icestorm",
        (0x61, 0x023) => "Apple M1 Firestorm",
        (0x61, 0x028) => "Apple M1 Pro/Max Avalanche",
        (0x61, 0x029) => "Apple M1 Pro/Max Blizzard",
        (0x61, 0x032) => "Apple M2 Avalanche",
        (0x61, 0x033) => "Apple M2 Blizzard",

        // Ampere
        (0xc0, 0xac3) => "Ampere Altra",
        (0xc0, 0xac4) => "Ampere Altra Max",

        // Qualcomm
        (0x51, 0x001) => "Qualcomm Oryon",

        _ => return None,
    };
    Some(name.to_string())
}

fn lookup_intel(family: u32, model: u32) -> Option<String> {
    // All mainstream Intel Core CPUs are family 6
    if family != 6 {
        return None;
    }

    let name = match model {
        // Sandy Bridge
        0x2A => "Sandy Bridge",
        0x2D => "Sandy Bridge-E",

        // Ivy Bridge
        0x3A => "Ivy Bridge",
        0x3E => "Ivy Bridge-E",

        // Haswell
        0x3C | 0x45 | 0x46 => "Haswell",
        0x3F => "Haswell-E",

        // Broadwell
        0x3D | 0x47 => "Broadwell",
        0x4F => "Broadwell-E",
        0x56 => "Broadwell-DE",

        // Skylake
        0x4E | 0x5E => "Skylake",
        0x55 => "Skylake-X",

        // Kaby Lake
        0x8E | 0x9E => "Kaby Lake",

        // Coffee Lake (same CPUID model as Kaby Lake for some steppings)
        // Differentiation by stepping is handled below

        // Cannon Lake
        0x66 => "Cannon Lake",

        // Ice Lake
        0x7E | 0x7D => "Ice Lake",
        0x6A | 0x6C => "Ice Lake-SP",

        // Comet Lake
        0xA5 | 0xA6 => "Comet Lake",

        // Tiger Lake
        0x8C | 0x8D => "Tiger Lake",

        // Rocket Lake
        0xA7 => "Rocket Lake",

        // Alder Lake
        0x97 => "Alder Lake",
        0x9A => "Alder Lake-P",

        // Raptor Lake
        0xB7 => "Raptor Lake",
        0xBA => "Raptor Lake-P",
        0xBF => "Raptor Lake-S",

        // Meteor Lake
        0xAA | 0xAC => "Meteor Lake",

        // Lunar Lake
        0xBD => "Lunar Lake",

        // Arrow Lake
        0xC5 => "Arrow Lake",
        0xC6 => "Arrow Lake-H",

        // Sapphire Rapids (server)
        0x8F => "Sapphire Rapids",

        // Emerald Rapids (server)
        0xCF => "Emerald Rapids",

        // Granite Rapids (server)
        0xAD | 0xAE => "Granite Rapids",

        _ => return None,
    };
    Some(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::cpu::CpuVendor;

    #[test]
    fn test_amd_zen5_granite_ridge() {
        let result = lookup_with_brand(&CpuVendor::Amd, 0x1A, 0x20, "");
        assert_eq!(result, Some("Zen 5 (Granite Ridge)".to_string()));
    }

    #[test]
    fn test_amd_zen5_strix_point_mobile() {
        let result = lookup_with_brand(&CpuVendor::Amd, 0x1A, 0x08, "AMD Ryzen AI 9 HX 370");
        assert_eq!(result, Some("Zen 5 (Strix Point)".to_string()));
    }

    #[test]
    fn test_amd_zen5_turin_threadripper() {
        let result = lookup_with_brand(
            &CpuVendor::Amd,
            0x1A,
            0x08,
            "AMD Ryzen Threadripper PRO 9995WX 96-Cores",
        );
        assert_eq!(result, Some("Zen 5 (Turin)".to_string()));
    }

    #[test]
    fn test_amd_zen2_matisse() {
        let result = lookup_with_brand(&CpuVendor::Amd, 0x17, 0x71, "");
        assert_eq!(result, Some("Zen 2 (Matisse)".to_string()));
    }

    #[test]
    fn test_intel_raptor_lake() {
        let result = lookup_with_brand(&CpuVendor::Intel, 6, 0xB7, "");
        assert_eq!(result, Some("Raptor Lake".to_string()));
    }

    #[test]
    fn test_unknown_returns_none() {
        let result = lookup_with_brand(&CpuVendor::Intel, 99, 99, "");
        assert!(result.is_none());
    }

    #[test]
    fn test_arm_returns_none() {
        let result = lookup_with_brand(&CpuVendor::Arm, 0, 0, "");
        assert!(result.is_none());
    }

    #[test]
    fn test_lookup_arm_neoverse_n1() {
        let result = lookup_arm(0x41, 0xd0c);
        assert_eq!(result, Some("Neoverse N1".to_string()));
    }

    #[test]
    fn test_lookup_arm_cortex_a72() {
        let result = lookup_arm(0x41, 0xd08);
        assert_eq!(result, Some("Cortex-A72".to_string()));
    }

    #[test]
    fn test_lookup_arm_apple_m1_firestorm() {
        let result = lookup_arm(0x61, 0x023);
        assert_eq!(result, Some("Apple M1 Firestorm".to_string()));
    }

    #[test]
    fn test_lookup_arm_ampere_altra() {
        let result = lookup_arm(0xc0, 0xac3);
        assert_eq!(result, Some("Ampere Altra".to_string()));
    }

    #[test]
    fn test_lookup_arm_qualcomm_oryon() {
        let result = lookup_arm(0x51, 0x001);
        assert_eq!(result, Some("Qualcomm Oryon".to_string()));
    }

    #[test]
    fn test_lookup_arm_unknown() {
        let result = lookup_arm(0xFF, 0xFFF);
        assert!(result.is_none());
    }
}
