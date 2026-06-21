#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EdidInfo {
    pub manufacturer: String,
    pub product_code: u16,
    pub serial_number: Option<u32>,
    pub manufacture_week: Option<u8>,
    pub manufacture_year: Option<u16>,
    pub monitor_name: Option<String>,
    pub max_horizontal_cm: u8,
    pub max_vertical_cm: u8,
    pub preferred_width: Option<u32>,
    pub preferred_height: Option<u32>,
    pub preferred_refresh_hz: Option<f64>,
}

/// Parse an EDID blob from a DRM connector sysfs path.
/// Returns None if the file is missing, empty, or unparsable.
pub fn parse_from_drm(connector_path: &std::path::Path) -> Option<EdidInfo> {
    let edid_path = connector_path.join("edid");
    let data = std::fs::read(&edid_path).ok()?;
    parse_edid(&data)
}

/// Parse a raw 128+ byte EDID block.
pub fn parse_edid(data: &[u8]) -> Option<EdidInfo> {
    if data.len() < 128 {
        return None;
    }

    // Verify EDID header: 00 FF FF FF FF FF FF 00
    if data[0..8] != [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00] {
        return None;
    }

    // Manufacturer ID: 3 compressed ASCII chars in bytes 8-9
    let mfg_raw = ((data[8] as u16) << 8) | data[9] as u16;
    let c1 = ((mfg_raw >> 10) & 0x1F) as u8 + b'A' - 1;
    let c2 = ((mfg_raw >> 5) & 0x1F) as u8 + b'A' - 1;
    let c3 = (mfg_raw & 0x1F) as u8 + b'A' - 1;
    let manufacturer = format!("{}{}{}", c1 as char, c2 as char, c3 as char);

    // Product code: bytes 10-11 (little-endian)
    let product_code = u16::from_le_bytes([data[10], data[11]]);

    // Serial number: bytes 12-15 (little-endian, 0 = not used)
    let serial_raw = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let serial_number = if serial_raw != 0 {
        Some(serial_raw)
    } else {
        None
    };

    // Manufacture week and year
    let week = data[16];
    let year_offset = data[17];
    let manufacture_week = if week > 0 && week <= 54 {
        Some(week)
    } else {
        None
    };
    let manufacture_year = if year_offset > 0 {
        Some(1990 + year_offset as u16)
    } else {
        None
    };

    // Max display size: bytes 21-22 (cm)
    let max_horizontal_cm = data[21];
    let max_vertical_cm = data[22];

    // Parse descriptor blocks (4 x 18 bytes starting at offset 54)
    let mut monitor_name = None;
    let mut preferred_width = None;
    let mut preferred_height = None;
    let mut preferred_refresh_hz = None;

    for i in 0..4 {
        let base = 54 + i * 18;
        if base + 18 > data.len() {
            break;
        }
        let block = &data[base..base + 18];

        // Check if this is a detailed timing descriptor (first two bytes non-zero)
        if block[0] != 0 || block[1] != 0 {
            // Detailed Timing Descriptor — parse preferred resolution
            if preferred_width.is_none() {
                let pixel_clock_khz = u16::from_le_bytes([block[0], block[1]]) as u32 * 10;

                let h_active = ((block[4] as u32 & 0xF0) << 4) | block[2] as u32;
                let h_blank = ((block[4] as u32 & 0x0F) << 8) | block[3] as u32;
                let v_active = ((block[7] as u32 & 0xF0) << 4) | block[5] as u32;
                let v_blank = ((block[7] as u32 & 0x0F) << 8) | block[6] as u32;

                if h_active > 0 && v_active > 0 && pixel_clock_khz > 0 {
                    preferred_width = Some(h_active);
                    preferred_height = Some(v_active);
                    let h_total = h_active + h_blank;
                    let v_total = v_active + v_blank;
                    if h_total > 0 && v_total > 0 {
                        preferred_refresh_hz = Some(
                            pixel_clock_khz as f64 * 1000.0 / (h_total as f64 * v_total as f64),
                        );
                    }
                }
            }
        } else if block[3] == 0xFC {
            // Monitor Name Descriptor
            let name_bytes = &block[5..18];
            let name = name_bytes
                .iter()
                .take_while(|&&b| b != 0x0A && b != 0x00)
                .map(|&b| b as char)
                .collect::<String>()
                .trim()
                .to_string();
            if !name.is_empty() {
                monitor_name = Some(name);
            }
        }
    }

    Some(EdidInfo {
        manufacturer,
        product_code,
        serial_number,
        manufacture_week,
        manufacture_year,
        monitor_name,
        max_horizontal_cm,
        max_vertical_cm,
        preferred_width,
        preferred_height,
        preferred_refresh_hz,
    })
}
