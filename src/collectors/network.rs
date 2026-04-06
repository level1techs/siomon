use crate::model::network::{IpAddress, NetworkAdapter, NetworkInterfaceType};
use crate::platform::sysfs;
use std::path::Path;

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

fn ipv6_scope(ip: &std::net::Ipv6Addr) -> &'static str {
    let octets = ip.octets();
    if octets[0] == 0xfe && (octets[1] & 0xc0) == 0x80 {
        "link"
    } else if ip.is_loopback() {
        "host"
    } else {
        "global"
    }
}

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
            // codeql[rust/access-invalid-pointer] - getifaddrs contract guarantees validity until freeifaddrs
            let ifa = &*current;

            if ifa.ifa_name.is_null() || ifa.ifa_addr.is_null() {
                current = ifa.ifa_next;
                continue;
            }
            let ifa_name = std::ffi::CStr::from_ptr(ifa.ifa_name).to_string_lossy();
            if ifa_name != name {
                current = ifa.ifa_next;
                continue;
            }

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
                let scope = Some(ipv6_scope(&ip).into());
                addrs.push(IpAddress {
                    address: ip.to_string(),
                    prefix_len: prefix,
                    family: "inet6".into(),
                    scope,
                });
            }
            current = ifa.ifa_next;
        }
        libc::freeifaddrs(ifaddrs);
    }

    addrs
}
