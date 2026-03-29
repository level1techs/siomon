//! Board feature requirement checking and diagnostic hints.
//!
//! Boards declare prerequisites via [`FeatureRequirements`] on their
//! [`BoardTemplate`]. This module checks those requirements at runtime
//! against system state (BIOS version, etc.) and produces actionable
//! warnings when features may not work.

use super::Requirement;

/// Result of checking requirements against the running system.
#[derive(Default)]
pub struct DiagReport {
    /// Actionable warning messages.
    pub warnings: Vec<String>,
    /// True if any requirement definitively failed (not just advisory).
    pub has_hard_failure: bool,
}

/// Read the BIOS version string from DMI sysfs.
pub fn read_bios_version() -> Option<String> {
    crate::platform::sysfs::read_string_optional(std::path::Path::new(
        "/sys/class/dmi/id/bios_version",
    ))
}

/// Check a slice of requirements against the running system's BIOS info.
pub fn check_requirements(
    requirements: &[Requirement],
    bios_version_str: Option<&str>,
) -> DiagReport {
    let mut report = DiagReport::default();

    for req in requirements {
        match req {
            Requirement::MinBiosVersion { version, hint } => {
                match bios_version_str.and_then(|s| s.parse::<u32>().ok()) {
                    Some(actual) if actual >= *version => {}
                    Some(actual) => {
                        report.has_hard_failure = true;
                        report.warnings.push(format!(
                            "BIOS version {actual} is below minimum {version}. {hint}"
                        ));
                    }
                    None => {
                        let got = bios_version_str.unwrap_or("unknown");
                        report.warnings.push(format!(
                            "Cannot verify BIOS version (got \"{got}\"); may require >= {version}. {hint}"
                        ));
                    }
                }
            }
            Requirement::BiosSetting { .. } => {
                // Advisory only — surfaced by probe_failure_hints when probing
                // actually fails, not during the proactive startup check.
            }
        }
    }

    report
}

/// Generate diagnostic hints when a feature probe found zero results.
///
/// Includes any unmet requirement warnings plus generic troubleshooting steps.
pub fn probe_failure_hints(
    feature_name: &str,
    requirements: &[Requirement],
    bios_version_str: Option<&str>,
) -> Vec<String> {
    let mut hints = Vec::new();

    // Include verifiable requirement failures (e.g., BIOS version too old).
    let report = check_requirements(requirements, bios_version_str);
    hints.extend(report.warnings);

    // Include advisory BiosSetting hints (skipped by check_requirements).
    for req in requirements {
        if let Requirement::BiosSetting { description } = req {
            hints.push(format!("May require BIOS setting: {description}"));
        }
    }

    if hints.is_empty() {
        hints.push(format!(
            "{feature_name}: no known requirements — verify I2C buses are accessible"
        ));
    }

    hints
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_bios_version_pass() {
        let reqs = [Requirement::MinBiosVersion {
            version: 1317,
            hint: "Update BIOS.",
        }];
        let report = check_requirements(&reqs, Some("1317"));
        assert!(report.warnings.is_empty());
        assert!(!report.has_hard_failure);
    }

    #[test]
    fn min_bios_version_newer_pass() {
        let reqs = [Requirement::MinBiosVersion {
            version: 1317,
            hint: "Update BIOS.",
        }];
        let report = check_requirements(&reqs, Some("1400"));
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn min_bios_version_fail() {
        let reqs = [Requirement::MinBiosVersion {
            version: 1317,
            hint: "Update BIOS.",
        }];
        let report = check_requirements(&reqs, Some("1316"));
        assert_eq!(report.warnings.len(), 1);
        assert!(report.has_hard_failure);
        assert!(report.warnings[0].contains("1316"));
        assert!(report.warnings[0].contains("1317"));
        assert!(report.warnings[0].contains("Update BIOS."));
    }

    #[test]
    fn min_bios_version_unparseable() {
        let reqs = [Requirement::MinBiosVersion {
            version: 1317,
            hint: "Update BIOS.",
        }];
        let report = check_requirements(&reqs, Some("A.B.C"));
        assert_eq!(report.warnings.len(), 1);
        assert!(!report.has_hard_failure);
        assert!(report.warnings[0].contains("Cannot verify"));
    }

    #[test]
    fn min_bios_version_none() {
        let reqs = [Requirement::MinBiosVersion {
            version: 1317,
            hint: "Update BIOS.",
        }];
        let report = check_requirements(&reqs, None);
        assert_eq!(report.warnings.len(), 1);
        assert!(!report.has_hard_failure);
        assert!(report.warnings[0].contains("Cannot verify"));
    }

    #[test]
    fn bios_setting_skipped_by_check() {
        // BiosSetting is advisory — not emitted by check_requirements
        // (only surfaced by probe_failure_hints on actual failure).
        let reqs = [Requirement::BiosSetting {
            description: "Enable SPD passthrough",
        }];
        let report = check_requirements(&reqs, Some("1317"));
        assert!(report.warnings.is_empty());
        assert!(!report.has_hard_failure);
    }

    #[test]
    fn bios_setting_in_probe_failure_hints() {
        let reqs = [Requirement::BiosSetting {
            description: "Enable SPD passthrough",
        }];
        let hints = probe_failure_hints("DDR5", &reqs, Some("1317"));
        assert_eq!(hints.len(), 1);
        assert!(hints[0].contains("Enable SPD passthrough"));
    }

    #[test]
    fn no_requirements_empty_report() {
        let report = check_requirements(&[], Some("1317"));
        assert!(report.warnings.is_empty());
        assert!(!report.has_hard_failure);
    }

    #[test]
    fn probe_failure_hints_with_requirements() {
        let reqs = [Requirement::MinBiosVersion {
            version: 1317,
            hint: "Update BIOS.",
        }];
        let hints = probe_failure_hints("DDR5 temp", &reqs, Some("1316"));
        assert!(!hints.is_empty());
        assert!(hints[0].contains("1316"));
    }

    #[test]
    fn probe_failure_hints_no_requirements() {
        let hints = probe_failure_hints("DDR5 temp", &[], Some("1317"));
        assert_eq!(hints.len(), 1);
        assert!(hints[0].contains("no known requirements"));
    }
}
