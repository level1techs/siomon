use crate::model::network::{IpAddress, NetworkAdapter, NetworkInterfaceType};
#[cfg(unix)]
use crate::platform::sysfs;
#[cfg(unix)]
use std::path::Path;

#[cfg(unix)]
pub fn collect(physical_only: bool) -> Vec<NetworkAdapter> {
    let mut adapters = Vec::new();

    for entry in sysfs::glob_paths("/sys/class/net/*") {
        let name = match entry.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };

        let is_physical = entry.join("device").exists();
        if physical_only && !is_physical {
            continue;
        }

        if let Some(adapter) = collect_adapter(&name, &entry, is_physical) {
            adapters.push(adapter);
        }
    }

    adapters.sort_by(|a, b| a.name.cmp(&b.name));
    adapters
}

#[cfg(not(unix))]
pub fn collect(physical_only: bool) -> Vec<NetworkAdapter> {
    win_collect_adapters(physical_only)
}

#[cfg(not(unix))]
pub fn collect_ip_addresses(adapter_name: &str) -> Vec<IpAddress> {
    let all = win_collect_adapters(false);
    all.into_iter()
        .find(|a| a.name == adapter_name)
        .map(|a| a.ip_addresses)
        .unwrap_or_default()
}

#[cfg(not(unix))]
fn win_collect_adapters(physical_only: bool) -> Vec<NetworkAdapter> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use winapi::shared::ifdef::IfOperStatusUp;
    use winapi::shared::ws2def::AF_UNSPEC;
    use winapi::um::iphlpapi::GetAdaptersAddresses;
    use winapi::um::iptypes::{GAA_FLAG_INCLUDE_PREFIX, IP_ADAPTER_ADDRESSES_LH};

    let mut adapters = Vec::new();

    unsafe {
        // First call to determine required buffer size
        let mut buf_len: u32 = 0;
        let ret = GetAdaptersAddresses(
            AF_UNSPEC as u32,
            GAA_FLAG_INCLUDE_PREFIX,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut buf_len,
        );
        if ret != winapi::shared::winerror::ERROR_BUFFER_OVERFLOW {
            log::warn!("GetAdaptersAddresses sizing call failed: {}", ret);
            return adapters;
        }

        // Allocate buffer and call again
        let mut buffer: Vec<u8> = vec![0u8; buf_len as usize];
        let adapter_ptr = buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
        let ret = GetAdaptersAddresses(
            AF_UNSPEC as u32,
            GAA_FLAG_INCLUDE_PREFIX,
            std::ptr::null_mut(),
            adapter_ptr,
            &mut buf_len,
        );
        if ret != 0 {
            log::warn!("GetAdaptersAddresses failed: {}", ret);
            return adapters;
        }

        // Walk the linked list
        let mut current = adapter_ptr;
        while !current.is_null() {
            let adapter = &*current;

            // FriendlyName is a wide string (PWCHAR)
            let friendly_name = {
                let mut len = 0;
                let ptr = adapter.FriendlyName;
                while *ptr.add(len) != 0 {
                    len += 1;
                }
                let slice = std::slice::from_raw_parts(ptr, len);
                OsString::from_wide(slice).to_string_lossy().to_string()
            };

            // IfType -> NetworkInterfaceType
            let if_type = adapter.IfType;
            let interface_type = match if_type {
                6 => NetworkInterfaceType::Ethernet,  // IF_TYPE_ETHERNET_CSMACD
                71 => NetworkInterfaceType::Wifi,     // IF_TYPE_IEEE80211
                24 => NetworkInterfaceType::Loopback, // IF_TYPE_SOFTWARE_LOOPBACK
                131 => NetworkInterfaceType::Tun,     // IF_TYPE_TUNNEL
                _ => NetworkInterfaceType::Unknown(if_type),
            };

            // Filter out non-physical adapters if requested
            let is_physical = matches!(
                interface_type,
                NetworkInterfaceType::Ethernet | NetworkInterfaceType::Wifi
            );
            if physical_only && !is_physical {
                current = adapter.Next;
                continue;
            }

            // MAC address: PhysicalAddress[0..PhysicalAddressLength]
            let mac_len = adapter.PhysicalAddressLength as usize;
            let mac_address = if mac_len > 0 {
                let bytes = &adapter.PhysicalAddress[..mac_len];
                let mac_str = bytes
                    .iter()
                    .map(|b| format!("{:02X}", b))
                    .collect::<Vec<_>>()
                    .join(":");
                // Filter out all-zeros MAC
                if bytes.iter().all(|&b| b == 0) {
                    None
                } else {
                    Some(mac_str)
                }
            } else {
                None
            };

            // OperStatus
            let operstate = if adapter.OperStatus == IfOperStatusUp {
                "up".to_string()
            } else {
                "down".to_string()
            };

            // Speed (bits/sec -> Mbps), TransmitLinkSpeed
            let speed_mbps = {
                let speed_bps = adapter.TransmitLinkSpeed;
                if speed_bps > 0 && speed_bps != u64::MAX {
                    Some((speed_bps / 1_000_000) as u32)
                } else {
                    None
                }
            };

            // MTU
            let mtu = adapter.Mtu;

            // Collect IP addresses from unicast address chain
            let ip_addresses = collect_unicast_addresses(adapter.FirstUnicastAddress);

            adapters.push(NetworkAdapter {
                name: friendly_name,
                driver: None,
                mac_address,
                permanent_mac: None,
                speed_mbps,
                operstate,
                duplex: None,
                mtu,
                interface_type,
                is_physical,
                pci_bus_address: None,
                pci_vendor_id: None,
                pci_device_id: None,
                ip_addresses,
                numa_node: None,
            });

            current = adapter.Next;
        }
    }

    // Populate driver names from WMI
    win_populate_driver_names(&mut adapters);

    adapters.sort_by(|a, b| a.name.cmp(&b.name));
    adapters
}

/// Query WMI for network adapter driver service names and match them to our
/// adapter list by friendly name.  If the query fails, adapters simply keep
/// `driver: None`.
#[cfg(not(unix))]
fn win_populate_driver_names(adapters: &mut [NetworkAdapter]) {
    let ps_script = r#"
Get-CimInstance Win32_NetworkAdapter | ForEach-Object {
    [PSCustomObject]@{
        Name        = $_.Name
        NetConnectionID = $_.NetConnectionID
        ServiceName = $_.ServiceName
    }
} | ConvertTo-Json -Compress
"#;

    let output = match std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", ps_script])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            log::debug!("WMI network driver query failed to launch: {e}");
            return;
        }
    };

    if !output.status.success() {
        log::debug!(
            "WMI network driver query failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stdout = stdout.trim();
    if stdout.is_empty() {
        return;
    }

    // PowerShell emits a bare object (not array) when there is exactly one result.
    let json_str = if stdout.starts_with('[') {
        stdout.to_string()
    } else {
        format!("[{stdout}]")
    };

    #[derive(serde::Deserialize)]
    #[allow(non_snake_case)]
    struct WmiNicRow {
        Name: Option<String>,
        NetConnectionID: Option<String>,
        ServiceName: Option<String>,
    }

    let rows: Vec<WmiNicRow> = match serde_json::from_str(&json_str) {
        Ok(r) => r,
        Err(e) => {
            log::debug!("Failed to parse WMI network adapter JSON: {e}");
            return;
        }
    };

    for adapter in adapters.iter_mut() {
        // Match by NetConnectionID (the friendly name Windows shows, e.g. "Wi-Fi",
        // "Ethernet") which is what GetAdaptersAddresses returns as FriendlyName.
        // Fall back to matching by Name (the full device description).
        if let Some(row) = rows.iter().find(|r| {
            r.NetConnectionID
                .as_deref()
                .is_some_and(|id| id == adapter.name)
                || r.Name.as_deref().is_some_and(|n| n == adapter.name)
        }) {
            if let Some(ref svc) = row.ServiceName {
                if !svc.is_empty() {
                    adapter.driver = Some(svc.clone());
                }
            }
        }
    }
}

#[cfg(not(unix))]
unsafe fn collect_unicast_addresses(
    first: *mut winapi::um::iptypes::IP_ADAPTER_UNICAST_ADDRESS_LH,
) -> Vec<IpAddress> {
    use winapi::shared::ws2def::{AF_INET, AF_INET6, SOCKADDR_IN};
    use winapi::shared::ws2ipdef::SOCKADDR_IN6;

    let mut addrs = Vec::new();
    let mut current = first;

    while !current.is_null() {
        let unicast = unsafe { &*current };
        let sa = unicast.Address.lpSockaddr;
        if !sa.is_null() {
            let family = unsafe { (*sa).sa_family } as i32;
            if family == AF_INET {
                let sockaddr_in = unsafe { &*(sa as *const SOCKADDR_IN) };
                let raw = sockaddr_in.sin_addr.S_un;
                let bytes = unsafe { raw.S_addr() }.to_ne_bytes();
                let ip = std::net::Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]);
                let prefix_len = unicast.OnLinkPrefixLength;
                addrs.push(IpAddress {
                    address: ip.to_string(),
                    prefix_len,
                    family: "inet".into(),
                    scope: None,
                });
            } else if family == AF_INET6 {
                let sockaddr_in6 = unsafe { &*(sa as *const SOCKADDR_IN6) };
                let bytes = unsafe { sockaddr_in6.sin6_addr.u.Byte() };
                let ip = std::net::Ipv6Addr::from(*bytes);
                let prefix_len = unicast.OnLinkPrefixLength;
                let scope_id = unsafe { *sockaddr_in6.u.sin6_scope_id() };
                let scope = match scope_id {
                    0 => Some("global".into()),
                    _ => Some("link".into()),
                };
                addrs.push(IpAddress {
                    address: ip.to_string(),
                    prefix_len,
                    family: "inet6".into(),
                    scope,
                });
            }
        }
        current = unicast.Next;
    }

    addrs
}

pub struct NetworkCollector {
    pub physical_only: bool,
}

impl crate::collectors::Collector for NetworkCollector {
    fn name(&self) -> &str {
        "network"
    }

    fn collect_into(&self, info: &mut crate::model::system::SystemInfo) {
        info.network = collect(self.physical_only);
    }
}

#[cfg(unix)]
fn collect_adapter(name: &str, path: &Path, is_physical: bool) -> Option<NetworkAdapter> {
    let operstate =
        sysfs::read_string_optional(&path.join("operstate")).unwrap_or_else(|| "unknown".into());
    let mac_address =
        sysfs::read_string_optional(&path.join("address")).filter(|m| m != "00:00:00:00:00:00");
    let mtu = sysfs::read_u32_optional(&path.join("mtu")).unwrap_or(1500);

    let speed_mbps = sysfs::read_string_optional(&path.join("speed"))
        .and_then(|s| s.parse::<i32>().ok())
        .filter(|&s| s > 0)
        .map(|s| s as u32);

    let duplex = sysfs::read_string_optional(&path.join("duplex"));

    let driver = sysfs::read_link_basename(&path.join("device/driver"));

    let type_code = sysfs::read_u64_optional(&path.join("type")).unwrap_or(0) as u32;
    let interface_type = classify_interface(name, type_code, is_physical);

    let pci_bus_address = path
        .join("device")
        .canonicalize()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()));

    let pci_vendor_id = sysfs::read_u64_optional(&path.join("device/vendor")).map(|v| v as u16);
    let pci_device_id = sysfs::read_u64_optional(&path.join("device/device")).map(|v| v as u16);

    let numa_node = sysfs::read_string_optional(&path.join("device/numa_node"))
        .and_then(|s| s.parse::<i32>().ok());

    let permanent_mac = sysfs::read_string_optional(&path.join("device/net_address"))
        .filter(|m| m != "00:00:00:00:00:00");

    let ip_addresses = collect_ip_addresses(name);

    Some(NetworkAdapter {
        name: name.to_string(),
        driver,
        mac_address,
        permanent_mac,
        speed_mbps,
        operstate,
        duplex,
        mtu,
        interface_type,
        is_physical,
        pci_bus_address,
        pci_vendor_id,
        pci_device_id,
        ip_addresses,
        numa_node,
    })
}

#[cfg(unix)]
fn classify_interface(name: &str, type_code: u32, is_physical: bool) -> NetworkInterfaceType {
    // ARPHRD_LOOPBACK = 772
    if type_code == 772 || name == "lo" {
        return NetworkInterfaceType::Loopback;
    }
    // ARPHRD_ETHER = 1
    if type_code == 1 {
        if name.starts_with("wl") {
            return NetworkInterfaceType::Wifi;
        }
        if name.starts_with("br") || name.starts_with("virbr") {
            return NetworkInterfaceType::Bridge;
        }
        if name.starts_with("bond") {
            return NetworkInterfaceType::Bond;
        }
        if name.contains('.') {
            return NetworkInterfaceType::Vlan;
        }
        if name.starts_with("veth") || name.starts_with("docker") || name.starts_with("cni") {
            return NetworkInterfaceType::Virtual;
        }
        if is_physical {
            return NetworkInterfaceType::Ethernet;
        }
        return NetworkInterfaceType::Virtual;
    }
    // ARPHRD_NONE or TUN = 65534
    if type_code == 65534 {
        return NetworkInterfaceType::Tun;
    }
    NetworkInterfaceType::Unknown(type_code)
}

#[cfg(unix)]
fn collect_ip_addresses(name: &str) -> Vec<IpAddress> {
    let mut addrs = Vec::new();

    // Use getifaddrs via libc
    unsafe {
        let mut ifaddrs: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut ifaddrs) != 0 {
            return addrs;
        }

        let mut current = ifaddrs;
        while !current.is_null() {
            let ifa = &*current;
            let ifa_name = std::ffi::CStr::from_ptr(ifa.ifa_name).to_string_lossy();
            if ifa_name == name && !ifa.ifa_addr.is_null() {
                let family = (*ifa.ifa_addr).sa_family as i32;
                if family == libc::AF_INET {
                    let addr = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                    let ip = std::net::Ipv4Addr::from(u32::from_be(addr.sin_addr.s_addr));
                    let prefix = if !ifa.ifa_netmask.is_null() {
                        let mask = &*(ifa.ifa_netmask as *const libc::sockaddr_in);
                        u32::from_be(mask.sin_addr.s_addr).count_ones() as u8
                    } else {
                        0
                    };
                    addrs.push(IpAddress {
                        address: ip.to_string(),
                        prefix_len: prefix,
                        family: "inet".into(),
                        scope: None,
                    });
                } else if family == libc::AF_INET6 {
                    let addr = &*(ifa.ifa_addr as *const libc::sockaddr_in6);
                    let ip = std::net::Ipv6Addr::from(addr.sin6_addr.s6_addr);
                    let prefix = if !ifa.ifa_netmask.is_null() {
                        let mask = &*(ifa.ifa_netmask as *const libc::sockaddr_in6);
                        mask.sin6_addr
                            .s6_addr
                            .iter()
                            .map(|b| b.count_ones() as u8)
                            .sum()
                    } else {
                        0
                    };
                    let scope = match addr.sin6_scope_id {
                        0 => Some("global".into()),
                        _ => Some("link".into()),
                    };
                    addrs.push(IpAddress {
                        address: ip.to_string(),
                        prefix_len: prefix,
                        family: "inet6".into(),
                        scope,
                    });
                }
            }
            current = ifa.ifa_next;
        }
        libc::freeifaddrs(ifaddrs);
    }

    addrs
}
