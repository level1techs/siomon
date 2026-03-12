//! AMD HSMP (Host System Management Port) sensor source.
//!
//! Reads CPU telemetry directly from the SMU via /dev/hsmp ioctl.
//! Provides socket power, SVI rail power, FCLK/MCLK, DDR bandwidth,
//! frequency limits, and C0 residency. AMD Zen 3+ only.

#[cfg(target_arch = "x86_64")]
mod inner {
    use std::fs::{File, OpenOptions};
    use std::os::unix::io::AsRawFd;

    use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};

    // HSMP ioctl: _IOWR(0xF8, 0, hsmp_message)
    // The ioctl number encodes size=42 (the kernel's unpacked field sum).
    // Our #[repr(C, packed(4))] struct is 44 bytes due to trailing padding
    // on sock_ind (u16 → 4-byte boundary). The kernel reads 42 bytes via
    // copy_from_user, so the 2-byte tail padding is never accessed.
    const HSMP_IOCTL: libc::c_ulong = 0xC02AF800;

    // Message IDs
    const HSMP_TEST: u32 = 0x01;
    const HSMP_GET_PROTO_VER: u32 = 0x03;
    const HSMP_GET_SOCKET_POWER: u32 = 0x04;
    const HSMP_GET_SOCKET_POWER_LIMIT: u32 = 0x06;
    const HSMP_GET_FCLK_MCLK: u32 = 0x0F;
    const HSMP_GET_CCLK_THROTTLE_LIMIT: u32 = 0x10;
    const HSMP_GET_C0_PERCENT: u32 = 0x11;
    const HSMP_GET_DDR_BANDWIDTH: u32 = 0x14;
    const HSMP_GET_RAILS_SVI: u32 = 0x1B;
    const HSMP_GET_SOCKET_FMAX_FMIN: u32 = 0x1C;

    #[repr(C, packed(4))]
    struct HsmpMessage {
        msg_id: u32,
        num_args: u16,
        response_sz: u16,
        args: [u32; 8],
        sock_ind: u16,
    }

    pub struct HsmpSource {
        fd: Option<File>,
    }

    impl HsmpSource {
        pub fn discover() -> Self {
            if !std::path::Path::new("/dev/hsmp").exists() {
                log::debug!("HSMP: /dev/hsmp not found");
                return Self { fd: None };
            }

            let file = match OpenOptions::new().read(true).write(true).open("/dev/hsmp") {
                Ok(f) => f,
                Err(e) => {
                    log::debug!("HSMP: failed to open /dev/hsmp: {}", e);
                    return Self { fd: None };
                }
            };

            // Verify with HSMP_TEST
            match hsmp_call_fd(&file, HSMP_TEST, 1, 1, &[42], 0) {
                Ok(args) if args[0] == 43 => {}
                _ => {
                    log::debug!("HSMP: test message failed");
                    return Self { fd: None };
                }
            }

            let proto_ver = hsmp_call_fd(&file, HSMP_GET_PROTO_VER, 0, 1, &[], 0)
                .map(|a| a[0])
                .unwrap_or(0);

            log::info!("HSMP sensor source available (proto v{})", proto_ver);
            Self { fd: Some(file) }
        }

        pub fn poll(&self) -> Vec<(SensorId, SensorReading)> {
            let Some(fd) = &self.fd else {
                return Vec::new();
            };

            let mut readings = Vec::new();

            // Socket power (mW)
            if let Ok(args) = hsmp_call_fd(fd, HSMP_GET_SOCKET_POWER, 0, 1, &[], 0).map_err(|e| {
                log::debug!("HSMP GET_SOCKET_POWER failed: {}", e);
                e
            }) {
                let watts = args[0] as f64 / 1000.0;
                readings.push(sensor(
                    "socket_power",
                    "Socket Power",
                    watts,
                    SensorUnit::Watts,
                    SensorCategory::Power,
                ));
            }

            // Socket power limit (mW)
            if let Ok(args) = hsmp_call_fd(fd, HSMP_GET_SOCKET_POWER_LIMIT, 0, 1, &[], 0) {
                let watts = args[0] as f64 / 1000.0;
                readings.push(sensor(
                    "socket_power_limit",
                    "Socket Power Limit",
                    watts,
                    SensorUnit::Watts,
                    SensorCategory::Power,
                ));
            }

            // SVI rails power (mW)
            if let Ok(args) = hsmp_call_fd(fd, HSMP_GET_RAILS_SVI, 0, 1, &[], 0) {
                let watts = args[0] as f64 / 1000.0;
                readings.push(sensor(
                    "svi_power",
                    "SVI Rails Power",
                    watts,
                    SensorUnit::Watts,
                    SensorCategory::Power,
                ));
            }

            // FCLK / MCLK
            if let Ok(args) = hsmp_call_fd(fd, HSMP_GET_FCLK_MCLK, 0, 2, &[], 0) {
                readings.push(sensor(
                    "fclk",
                    "Fabric Clock",
                    args[0] as f64,
                    SensorUnit::Mhz,
                    SensorCategory::Frequency,
                ));
                readings.push(sensor(
                    "mclk",
                    "Memory Clock",
                    args[1] as f64,
                    SensorUnit::Mhz,
                    SensorCategory::Frequency,
                ));
            }

            // CCLK throttle limit
            if let Ok(args) = hsmp_call_fd(fd, HSMP_GET_CCLK_THROTTLE_LIMIT, 0, 1, &[], 0) {
                readings.push(sensor(
                    "cclk_limit",
                    "CCLK Throttle Limit",
                    args[0] as f64,
                    SensorUnit::Mhz,
                    SensorCategory::Frequency,
                ));
            }

            // C0 residency
            if let Ok(args) = hsmp_call_fd(fd, HSMP_GET_C0_PERCENT, 0, 1, &[], 0) {
                readings.push(sensor(
                    "c0_residency",
                    "C0 Residency",
                    args[0] as f64,
                    SensorUnit::Percent,
                    SensorCategory::Utilization,
                ));
            }

            // DDR bandwidth: max[31:20] used[19:8] pct[7:0]
            if let Ok(args) = hsmp_call_fd(fd, HSMP_GET_DDR_BANDWIDTH, 0, 1, &[], 0) {
                let max_gbps = ((args[0] >> 20) & 0xFFF) as f64;
                let used_gbps = ((args[0] >> 8) & 0xFFF) as f64;
                let pct = (args[0] & 0xFF) as f64;
                readings.push(sensor(
                    "ddr_bw_max",
                    "DDR BW Max",
                    max_gbps,
                    SensorUnit::Unitless,
                    SensorCategory::Throughput,
                ));
                readings.push(sensor(
                    "ddr_bw_used",
                    "DDR BW Used",
                    used_gbps,
                    SensorUnit::Unitless,
                    SensorCategory::Throughput,
                ));
                readings.push(sensor(
                    "ddr_bw_util",
                    "DDR BW Utilization",
                    pct,
                    SensorUnit::Percent,
                    SensorCategory::Utilization,
                ));
            }

            // Fmax / Fmin: fmax[31:16] fmin[15:0]
            if let Ok(args) = hsmp_call_fd(fd, HSMP_GET_SOCKET_FMAX_FMIN, 0, 1, &[], 0) {
                let fmax = ((args[0] >> 16) & 0xFFFF) as f64;
                let fmin = (args[0] & 0xFFFF) as f64;
                readings.push(sensor(
                    "fmax",
                    "Socket Fmax",
                    fmax,
                    SensorUnit::Mhz,
                    SensorCategory::Frequency,
                ));
                readings.push(sensor(
                    "fmin",
                    "Socket Fmin",
                    fmin,
                    SensorUnit::Mhz,
                    SensorCategory::Frequency,
                ));
            }

            readings
        }

        pub fn is_available(&self) -> bool {
            self.fd.is_some()
        }
    }

    impl crate::sensors::SensorSource for HsmpSource {
        fn name(&self) -> &str {
            "hsmp"
        }

        fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
            HsmpSource::poll(self)
        }
    }

    fn sensor(
        name: &str,
        label: &str,
        value: f64,
        unit: SensorUnit,
        category: SensorCategory,
    ) -> (SensorId, SensorReading) {
        let id = SensorId {
            source: "hsmp".into(),
            chip: "smu".into(),
            sensor: name.into(),
        };
        (
            id,
            SensorReading::new(label.to_string(), value, unit, category),
        )
    }

    fn hsmp_call_fd(
        file: &File,
        msg_id: u32,
        num_args: u16,
        response_sz: u16,
        args_in: &[u32],
        sock: u16,
    ) -> std::io::Result<[u32; 8]> {
        let mut msg = HsmpMessage {
            msg_id,
            num_args,
            response_sz,
            args: [0u32; 8],
            sock_ind: sock,
        };

        for (i, &v) in args_in.iter().enumerate().take(8) {
            msg.args[i] = v;
        }

        let ret = unsafe { libc::ioctl(file.as_raw_fd(), HSMP_IOCTL, &mut msg as *mut _) };
        if ret < 0 {
            return Err(std::io::Error::last_os_error());
        }

        Ok(msg.args)
    }
}

// Stub for non-x86_64 platforms
#[cfg(not(target_arch = "x86_64"))]
mod inner {
    use crate::model::sensor::{SensorId, SensorReading};

    pub struct HsmpSource;

    impl HsmpSource {
        pub fn discover() -> Self {
            Self
        }
        pub fn poll(&self) -> Vec<(SensorId, SensorReading)> {
            Vec::new()
        }
        pub fn is_available(&self) -> bool {
            false
        }
    }

    impl crate::sensors::SensorSource for HsmpSource {
        fn name(&self) -> &str {
            "hsmp"
        }

        fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
            HsmpSource::poll(self)
        }
    }
}

pub use inner::HsmpSource;

#[cfg(test)]
mod tests {
    #[test]
    fn discover_without_hardware() {
        let src = super::HsmpSource::discover();
        // May or may not be available depending on test environment
        let _ = src.is_available();
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn hsmp_message_size() {
        // msg_id(4) + num_args(2) + response_sz(2) + args(32) + sock_ind(2) = 42
        #[repr(C, packed(4))]
        struct HsmpMessageTest {
            msg_id: u32,
            num_args: u16,
            response_sz: u16,
            args: [u32; 8],
            sock_ind: u16,
        }
        // Field sum = 42; packed(4) pads sock_ind to 4-byte boundary → 44.
        // The ioctl number encodes 42; kernel reads 42 via copy_from_user.
        assert_eq!(std::mem::size_of::<HsmpMessageTest>(), 44);
    }
}
