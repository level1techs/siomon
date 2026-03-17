use std::collections::HashMap;
#[cfg(unix)]
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::db::cpu_codenames;
use crate::error::Result;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use crate::model::cpu::CacheLevel;
#[cfg(unix)]
use crate::model::cpu::NumaNode;
use crate::model::cpu::{CpuCache, CpuFeatures, CpuInfo, CpuTopology, CpuVendor, CpuVulnerability};
use crate::platform::{procfs, sysfs};

/// Collect CPU information, returning one `CpuInfo` per physical package.
pub fn collect() -> Result<Vec<CpuInfo>> {
    let cpuid_data = gather_cpuid();
    let cpuinfo_entries = procfs::parse_cpuinfo();
    let topology = gather_topology();
    let freq = gather_frequency();

    // Extract procfs fields from the first processor entry.
    let first_proc = cpuinfo_entries.first();
    let microcode = first_proc.and_then(|p| p.get("microcode").cloned()).or({
        #[cfg(not(unix))]
        {
            read_windows_microcode()
        }
        #[cfg(unix)]
        {
            None
        }
    });
    let vulnerabilities = gather_vulnerabilities();
    let (phys_addr_bits, virt_addr_bits) = parse_address_sizes(first_proc);

    // On ARM, gather architecture-specific information from /proc/cpuinfo and sysfs.
    let arm_info = gather_arm_info(first_proc);

    let vendor = cpuid_data
        .as_ref()
        .map(|c| c.vendor.clone())
        .or_else(|| arm_info.as_ref().map(|a| a.vendor.clone()))
        .unwrap_or_else(|| vendor_from_procfs(first_proc));

    let family = cpuid_data.as_ref().map(|c| c.family).unwrap_or(0);
    let model = cpuid_data.as_ref().map(|c| c.model).unwrap_or(0);
    let stepping = cpuid_data.as_ref().map(|c| c.stepping).unwrap_or(0);

    let brand = cpuid_data
        .as_ref()
        .and_then(|c| c.brand.clone())
        .or_else(|| arm_info.as_ref().and_then(|a| a.brand.clone()))
        .or_else(|| first_proc.and_then(|p| p.get("model name").cloned()))
        .unwrap_or_else(|| "Unknown CPU".to_string());

    let codename = cpu_codenames::lookup_with_brand(&vendor, family, model, &brand)
        .or_else(|| arm_info.as_ref().and_then(|a| a.codename.clone()));

    let features = cpuid_data
        .as_ref()
        .map(|c| c.features.clone())
        .or_else(|| arm_info.as_ref().map(|a| a.features.clone()))
        .unwrap_or_default();

    let cache = cpuid_data
        .as_ref()
        .map(|c| c.cache.clone())
        .unwrap_or_else(|| CpuCache {
            l1d: None,
            l1i: None,
            l2: None,
            l3: None,
        });

    let num_packages = topology.packages.max(1);
    let mut packages = Vec::with_capacity(num_packages as usize);

    for pkg_id in 0..num_packages {
        packages.push(CpuInfo {
            package_id: pkg_id,
            brand: brand.clone(),
            vendor: vendor.clone(),
            family,
            model,
            stepping,
            microcode: microcode.clone(),
            codename: codename.clone(),
            socket: None,
            tdp_watts: None,
            tj_max_celsius: None,
            base_clock_mhz: freq.base_clock_mhz,
            boost_clock_mhz: freq.boost_clock_mhz,
            scaling_driver: freq.scaling_driver.clone(),
            topology: topology.clone(),
            cache: cache.clone(),
            features: features.clone(),
            vulnerabilities: vulnerabilities.clone(),
            physical_address_bits: phys_addr_bits,
            virtual_address_bits: virt_addr_bits,
        });
    }

    Ok(packages)
}

// ---------------------------------------------------------------------------
// CPUID data gathering (x86/x86_64 only)
// ---------------------------------------------------------------------------

struct CpuidData {
    vendor: CpuVendor,
    brand: Option<String>,
    family: u32,
    model: u32,
    stepping: u32,
    features: CpuFeatures,
    cache: CpuCache,
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
fn gather_cpuid() -> Option<CpuidData> {
    let cpuid = raw_cpuid::CpuId::new();

    let vendor = match cpuid.get_vendor_info() {
        Some(v) => match v.as_str() {
            "GenuineIntel" => CpuVendor::Intel,
            "AuthenticAMD" => CpuVendor::Amd,
            other => CpuVendor::Unknown(other.to_string()),
        },
        None => return None,
    };

    let brand = cpuid
        .get_processor_brand_string()
        .map(|b| b.as_str().trim().to_string());

    let (family, model, stepping) = match cpuid.get_feature_info() {
        Some(fi) => {
            let base_family = fi.family_id() as u32;
            let base_model = fi.model_id() as u32;
            let ext_family = fi.extended_family_id() as u32;
            let ext_model = fi.extended_model_id() as u32;

            let display_family = if base_family == 0x0F {
                base_family + ext_family
            } else {
                base_family
            };

            let display_model = if base_family == 0x0F || base_family == 0x06 {
                (ext_model << 4) | base_model
            } else {
                base_model
            };

            (display_family, display_model, fi.stepping_id() as u32)
        }
        None => (0, 0, 0),
    };

    let features = gather_features(&cpuid);
    let cache = gather_cache(&cpuid);

    Some(CpuidData {
        vendor,
        brand,
        family,
        model,
        stepping,
        features,
        cache,
    })
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
fn gather_cpuid() -> Option<CpuidData> {
    None
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
fn gather_features(cpuid: &raw_cpuid::CpuId<raw_cpuid::CpuIdReaderNative>) -> CpuFeatures {
    let mut f = CpuFeatures::default();

    if let Some(fi) = cpuid.get_feature_info() {
        f.sse = fi.has_sse();
        f.sse2 = fi.has_sse2();
        f.sse3 = fi.has_sse3();
        f.ssse3 = fi.has_ssse3();
        f.sse4_1 = fi.has_sse41();
        f.sse4_2 = fi.has_sse42();
        f.avx = fi.has_avx();
        f.fma = fi.has_fma();
        f.aes_ni = fi.has_aesni();
        f.pclmulqdq = fi.has_pclmulqdq();
        f.popcnt = fi.has_popcnt();
        f.f16c = fi.has_f16c();
        f.rdrand = fi.has_rdrand();
        f.vmx = fi.has_vmx();
        f.hypervisor = fi.has_hypervisor();
    }

    if let Some(ef) = cpuid.get_extended_feature_info() {
        f.avx2 = ef.has_avx2();
        f.avx512f = ef.has_avx512f();
        f.avx512dq = ef.has_avx512dq();
        f.avx512bw = ef.has_avx512bw();
        f.avx512vl = ef.has_avx512vl();
        f.avx512cd = ef.has_avx512cd();
        f.avx512ifma = ef.has_avx512_ifma();
        f.avx512vbmi = ef.has_avx512vbmi();
        f.avx512vnni = ef.has_avx512vnni();
        f.avx512bf16 = ef.has_avx512_bf16();
        f.avx_vnni = ef.has_avx_vnni();
        f.bmi1 = ef.has_bmi1();
        f.bmi2 = ef.has_bmi2();
        f.adx = ef.has_adx();
        f.sha = ef.has_sha();
        f.rdseed = ef.has_rdseed();
        f.vaes = ef.has_vaes();
        f.amx_bf16 = ef.has_amx_bf16();
        f.amx_tile = ef.has_amx_tile();
        f.amx_int8 = ef.has_amx_int8();
        f.cet_ss = ef.has_cet_ss();
    }

    // Extended CPUID leaf for AMD-specific features (sse4a, svm, lzcnt)
    if let Some(ext) = cpuid.get_extended_processor_and_feature_identifiers() {
        f.sse4a = ext.has_sse4a();
        f.svm = ext.has_svm();
        f.lzcnt = ext.has_lzcnt();
    }

    f
}

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
fn gather_cache(cpuid: &raw_cpuid::CpuId<raw_cpuid::CpuIdReaderNative>) -> CpuCache {
    let mut l1d: Option<CacheLevel> = None;
    let mut l1i: Option<CacheLevel> = None;
    let mut l2: Option<CacheLevel> = None;
    let mut l3: Option<CacheLevel> = None;

    if let Some(cparams) = cpuid.get_cache_parameters() {
        for cache in cparams {
            let level = cache.level();
            let sets = cache.sets() as u32;
            let ways = cache.associativity() as u32;
            let line_size = cache.coherency_line_size() as u32;
            let partitions = cache.physical_line_partitions() as u32;
            let size = (ways as u64) * (partitions as u64) * (line_size as u64) * (sets as u64);
            let shared_by = cache.max_cores_for_cache() as u32;

            let is_data = cache.cache_type() == raw_cpuid::CacheType::Data
                || cache.cache_type() == raw_cpuid::CacheType::Unified;
            let is_instruction = cache.cache_type() == raw_cpuid::CacheType::Instruction;
            let is_unified = cache.cache_type() == raw_cpuid::CacheType::Unified;

            let cache_type_str = match cache.cache_type() {
                raw_cpuid::CacheType::Data => "Data",
                raw_cpuid::CacheType::Instruction => "Instruction",
                raw_cpuid::CacheType::Unified => "Unified",
                _ => "Unknown",
            };

            let entry = CacheLevel {
                level,
                cache_type: cache_type_str.to_string(),
                size_bytes: size,
                ways,
                line_size_bytes: line_size,
                sets: Some(sets),
                shared_by_cores: Some(shared_by),
                instances: None,
            };

            match level {
                1 if is_data && !is_instruction => {
                    l1d = Some(entry);
                }
                1 if is_instruction => {
                    l1i = Some(entry);
                }
                2 if is_data || is_unified => {
                    l2 = Some(entry);
                }
                3 if is_unified => {
                    l3 = Some(entry);
                }
                _ => {}
            }
        }
    }

    CpuCache { l1d, l1i, l2, l3 }
}

// ---------------------------------------------------------------------------
// ARM information gathering
// ---------------------------------------------------------------------------

/// Parsed ARM CPU information from /proc/cpuinfo and sysfs.
struct ArmInfo {
    vendor: CpuVendor,
    brand: Option<String>,
    codename: Option<String>,
    features: CpuFeatures,
}

/// Gather ARM-specific CPU information from /proc/cpuinfo and sysfs.
///
/// On aarch64, /proc/cpuinfo contains fields like "CPU implementer", "CPU part",
/// "CPU architecture", and "Features". On x86, these fields are absent so this
/// function returns `None`.
fn gather_arm_info(first_proc: Option<&HashMap<String, String>>) -> Option<ArmInfo> {
    let proc_entry = first_proc?;

    // Try MIDR_EL1 from sysfs first, fall back to /proc/cpuinfo fields.
    let (implementer, part, variant, revision) =
        read_midr_el1().or_else(|| parse_arm_cpuinfo_ids(proc_entry))?;

    let codename = cpu_codenames::lookup_arm(implementer, part);

    let implementer_name = arm_implementer_name(implementer);
    let brand = codename
        .as_ref()
        .map(|cn| format!("{} {}", implementer_name, cn))
        .or_else(|| {
            Some(format!(
                "{} (impl 0x{:02x} part 0x{:03x} r{}p{})",
                implementer_name, implementer, part, variant, revision,
            ))
        });

    // Parse the ARM "Features" line from /proc/cpuinfo.
    let features_str = proc_entry.get("Features").cloned();
    let features = parse_arm_features(features_str.as_deref());

    Some(ArmInfo {
        vendor: CpuVendor::Arm,
        brand,
        codename,
        features,
    })
}

/// Read MIDR_EL1 from sysfs and extract implementer, part, variant, revision.
///
/// The MIDR_EL1 register layout (64-bit value, upper 32 bits reserved):
///   [31:24] Implementer
///   [23:20] Variant
///   [19:16] Architecture
///   [15:4]  Primary part number
///   [3:0]   Revision
fn read_midr_el1() -> Option<(u32, u32, u32, u32)> {
    let midr_path = Path::new("/sys/devices/system/cpu/cpu0/regs/identification/midr_el1");
    let val = sysfs::read_u64_optional(midr_path)?;

    let implementer = ((val >> 24) & 0xFF) as u32;
    let variant = ((val >> 20) & 0xF) as u32;
    let part = ((val >> 4) & 0xFFF) as u32;
    let revision = (val & 0xF) as u32;

    Some((implementer, part, variant, revision))
}

/// Parse ARM CPU identification fields from a /proc/cpuinfo entry.
fn parse_arm_cpuinfo_ids(entry: &HashMap<String, String>) -> Option<(u32, u32, u32, u32)> {
    let implementer_str = entry.get("CPU implementer")?;
    let part_str = entry.get("CPU part")?;

    let implementer = parse_hex_or_dec(implementer_str)?;
    let part = parse_hex_or_dec(part_str)?;
    let variant = entry
        .get("CPU variant")
        .and_then(|s| parse_hex_or_dec(s))
        .unwrap_or(0);
    let revision = entry
        .get("CPU revision")
        .and_then(|s| parse_hex_or_dec(s))
        .unwrap_or(0);

    Some((implementer, part, variant, revision))
}

/// Map an ARM implementer code to a human-readable name.
fn arm_implementer_name(implementer: u32) -> &'static str {
    match implementer {
        0x41 => "ARM",
        0x42 => "Broadcom",
        0x43 => "Cavium",
        0x44 => "DEC",
        0x46 => "Fujitsu",
        0x48 => "HiSilicon",
        0x4e => "NVIDIA",
        0x50 => "APM",
        0x51 => "Qualcomm",
        0x53 => "Samsung",
        0x56 => "Marvell",
        0x61 => "Apple",
        0x69 => "Intel",
        0xc0 => "Ampere",
        _ => "Unknown",
    }
}

/// Parse ARM features string into `CpuFeatures`, mapping to x86 equivalents where
/// a direct correspondence exists, and storing the full raw string.
fn parse_arm_features(features_str: Option<&str>) -> CpuFeatures {
    let mut f = CpuFeatures::default();

    let Some(features_str) = features_str else {
        return f;
    };

    f.raw_features = Some(features_str.to_string());

    let flags: Vec<&str> = features_str.split_whitespace().collect();

    for flag in &flags {
        match *flag {
            // AES instructions -> maps to aes_ni conceptually
            "aes" => f.aes_ni = true,
            // SHA instructions
            "sha1" | "sha2" | "sha3" | "sha512" => f.sha = true,
            // CRC32 -> maps to sse4_2 conceptually (x86 CRC32 is part of SSE4.2)
            "crc32" => f.sse4_2 = true,
            // PMULL (polynomial multiply) -> maps to pclmulqdq conceptually
            "pmull" => f.pclmulqdq = true,
            // FP and advanced SIMD are baseline on aarch64
            "fp" | "asimd" => {}
            _ => {}
        }
    }

    f
}

/// Parse a string as hexadecimal (0x prefix) or decimal.
fn parse_hex_or_dec(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else {
        s.parse::<u32>().ok()
    }
}

// ---------------------------------------------------------------------------
// Topology from sysfs
// ---------------------------------------------------------------------------

#[cfg(not(unix))]
fn gather_topology() -> CpuTopology {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_cpu_all();
    let logical = sys.cpus().len() as u32;
    let physical = sys.physical_core_count().unwrap_or(logical as usize) as u32;
    let physical = physical.max(1);
    let logical = logical.max(physical);
    let smt_enabled = logical > physical;
    let threads_per_core = if smt_enabled && physical > 0 {
        logical / physical
    } else {
        1
    };
    CpuTopology {
        packages: 1,
        dies_per_package: 1,
        physical_cores: physical,
        logical_processors: logical,
        smt_enabled,
        threads_per_core,
        cores_per_die: Some(physical),
        numa_nodes: vec![],
        online_cpus: format!("0-{}", logical.saturating_sub(1)),
    }
}

#[cfg(unix)]
fn gather_topology() -> CpuTopology {
    let cpu_dirs = sysfs::glob_paths("/sys/devices/system/cpu/cpu[0-9]*");
    let logical_processors = cpu_dirs.len() as u32;

    let mut package_ids: BTreeSet<u32> = BTreeSet::new();
    let mut core_ids: BTreeSet<(u32, u32)> = BTreeSet::new();
    let mut die_ids_per_package: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    let mut max_threads_per_core: u32 = 1;

    for dir in &cpu_dirs {
        let topo = dir.join("topology");

        if let Some(pkg) = sysfs::read_u32_optional(&topo.join("physical_package_id")) {
            package_ids.insert(pkg);

            if let Some(core) = sysfs::read_u32_optional(&topo.join("core_id")) {
                core_ids.insert((pkg, core));
            }

            let die = sysfs::read_u32_optional(&topo.join("die_id")).unwrap_or(0);
            die_ids_per_package.entry(pkg).or_default().insert(die);
        }

        if let Some(siblings) = sysfs::read_string_optional(&topo.join("thread_siblings_list")) {
            let thread_count = count_cpulist_entries(&siblings);
            if thread_count > max_threads_per_core {
                max_threads_per_core = thread_count;
            }
        }
    }

    let packages = package_ids.len().max(1) as u32;
    let physical_cores = core_ids.len().max(1) as u32;

    let dies_per_package = die_ids_per_package
        .values()
        .map(|s| s.len() as u32)
        .max()
        .unwrap_or(1)
        .max(1);

    let smt_enabled = max_threads_per_core > 1;
    let threads_per_core = if smt_enabled { max_threads_per_core } else { 1 };

    let cores_per_die = if dies_per_package > 0 {
        Some(physical_cores / (packages * dies_per_package))
    } else {
        None
    };

    let online_cpus = sysfs::read_string_optional(Path::new("/sys/devices/system/cpu/online"))
        .unwrap_or_else(|| format!("0-{}", logical_processors.saturating_sub(1)));

    let numa_nodes = gather_numa_nodes();

    CpuTopology {
        packages,
        dies_per_package,
        physical_cores,
        logical_processors,
        smt_enabled,
        threads_per_core,
        cores_per_die,
        numa_nodes,
        online_cpus,
    }
}

#[cfg(unix)]
fn gather_numa_nodes() -> Vec<NumaNode> {
    let mut nodes = Vec::new();
    let node_dirs = sysfs::glob_paths("/sys/devices/system/node/node[0-9]*");

    for dir in &node_dirs {
        let node_name = match dir.file_name().and_then(|n| n.to_str()) {
            Some(name) => name,
            None => continue,
        };
        let node_id: u32 = match node_name.strip_prefix("node").and_then(|s| s.parse().ok()) {
            Some(id) => id,
            None => continue,
        };

        let cpu_list = sysfs::read_string_optional(&dir.join("cpulist")).unwrap_or_default();

        // Read NUMA node memory from its meminfo file.
        let memory_bytes = parse_numa_meminfo(&dir.join("meminfo"));

        nodes.push(NumaNode {
            node_id,
            cpu_list,
            memory_bytes,
        });
    }

    nodes.sort_by_key(|n| n.node_id);
    nodes
}

#[cfg(unix)]
fn parse_numa_meminfo(path: &Path) -> Option<u64> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        if line.contains("MemTotal") {
            // Format: "Node X MemTotal:    12345 kB"
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                if let Ok(kb) = parts[3].parse::<u64>() {
                    return Some(kb * 1024);
                }
            }
        }
    }
    None
}

/// Count entries in a CPU list string like "0-3,5,7" or "0,2".
#[cfg(unix)]
fn count_cpulist_entries(list: &str) -> u32 {
    let mut count = 0u32;
    for part in list.split(',') {
        let part = part.trim();
        if let Some((start, end)) = part.split_once('-') {
            if let (Ok(s), Ok(e)) = (start.parse::<u32>(), end.parse::<u32>()) {
                count += e - s + 1;
            }
        } else if part.parse::<u32>().is_ok() {
            count += 1;
        }
    }
    count
}

// ---------------------------------------------------------------------------
// Frequency from cpufreq sysfs
// ---------------------------------------------------------------------------

struct FrequencyInfo {
    base_clock_mhz: Option<f64>,
    boost_clock_mhz: Option<f64>,
    scaling_driver: Option<String>,
}

#[cfg(not(unix))]
fn gather_frequency() -> FrequencyInfo {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_cpu_all();
    // sysinfo reports the current frequency; use it as the boost/max indicator
    let freq_mhz = sys.cpus().first().map(|c| c.frequency() as f64);
    FrequencyInfo {
        base_clock_mhz: None,
        boost_clock_mhz: freq_mhz,
        scaling_driver: None,
    }
}

#[cfg(unix)]
fn gather_frequency() -> FrequencyInfo {
    let cpufreq = Path::new("/sys/devices/system/cpu/cpu0/cpufreq");

    let base_clock_mhz =
        sysfs::read_u64_optional(&cpufreq.join("cpuinfo_min_freq")).map(|khz| khz as f64 / 1000.0);

    let boost_clock_mhz =
        sysfs::read_u64_optional(&cpufreq.join("cpuinfo_max_freq")).map(|khz| khz as f64 / 1000.0);

    let scaling_driver = sysfs::read_string_optional(&cpufreq.join("scaling_driver"));

    FrequencyInfo {
        base_clock_mhz,
        boost_clock_mhz,
        scaling_driver,
    }
}

// ---------------------------------------------------------------------------
// /proc/cpuinfo helpers
// ---------------------------------------------------------------------------

fn vendor_from_procfs(entry: Option<&HashMap<String, String>>) -> CpuVendor {
    let proc = match entry {
        Some(p) => p,
        None => return CpuVendor::Unknown("Unknown".to_string()),
    };

    // x86: "vendor_id" field
    if let Some(vendor_id) = proc.get("vendor_id") {
        return match vendor_id.as_str() {
            "GenuineIntel" => CpuVendor::Intel,
            "AuthenticAMD" => CpuVendor::Amd,
            other => CpuVendor::Unknown(other.to_string()),
        };
    }

    // ARM: presence of "CPU implementer" field indicates ARM
    if proc.contains_key("CPU implementer") {
        return CpuVendor::Arm;
    }

    CpuVendor::Unknown("Unknown".to_string())
}

fn gather_vulnerabilities() -> Vec<CpuVulnerability> {
    let mut vulns = Vec::new();
    for entry in sysfs::glob_paths("/sys/devices/system/cpu/vulnerabilities/*") {
        let name = match entry.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let status = sysfs::read_string_optional(&entry).unwrap_or_else(|| "Unknown".into());
        vulns.push(CpuVulnerability { name, status });
    }
    vulns.sort_by(|a, b| a.name.cmp(&b.name));
    vulns
}

fn parse_address_sizes(entry: Option<&HashMap<String, String>>) -> (Option<u8>, Option<u8>) {
    let Some(sizes) = entry.and_then(|p| p.get("address sizes")) else {
        return (None, None);
    };
    // Format: "48 bits physical, 48 bits virtual"
    let mut physical = None;
    let mut virtual_ = None;

    for part in sizes.split(',') {
        let part = part.trim();
        if part.ends_with("physical") {
            physical = part
                .split_whitespace()
                .next()
                .and_then(|n| n.parse::<u8>().ok());
        } else if part.ends_with("virtual") {
            virtual_ = part
                .split_whitespace()
                .next()
                .and_then(|n| n.parse::<u8>().ok());
        }
    }

    (physical, virtual_)
}

#[cfg(not(unix))]
fn read_windows_microcode() -> Option<String> {
    let output = std::process::Command::new("reg")
        .args([
            "query",
            r"HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0",
            "/v",
            "Update Revision",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    // Line looks like: "    Update Revision    REG_BINARY    00000000XXXXXXXX"
    // The hex string after REG_BINARY contains the revision
    for line in text.lines() {
        if line.contains("Update Revision") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(hex) = parts.last() {
                // The revision is in the upper 4 bytes (bytes 4-7 of the 8-byte value)
                if hex.len() >= 16 {
                    let rev = &hex[8..16]; // upper 4 bytes
                    let trimmed = rev.trim_start_matches('0');
                    if !trimmed.is_empty() {
                        return Some(format!("0x{}", trimmed));
                    }
                }
                // Fallback: return full hex
                let trimmed = hex.trim_start_matches('0');
                if !trimmed.is_empty() {
                    return Some(format!("0x{}", trimmed));
                }
            }
        }
    }
    None
}

pub struct CpuCollector;

impl crate::collectors::Collector for CpuCollector {
    fn name(&self) -> &str {
        "cpu"
    }

    fn collect_into(&self, info: &mut crate::model::system::SystemInfo) {
        info.cpus = collect().unwrap_or_else(|e| {
            log::warn!("CPU collection failed: {e}");
            Vec::new()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_or_dec_hex() {
        assert_eq!(parse_hex_or_dec("0x41"), Some(0x41));
        assert_eq!(parse_hex_or_dec("0xd0c"), Some(0xd0c));
        assert_eq!(parse_hex_or_dec("0X1A"), Some(0x1A));
    }

    #[test]
    fn test_parse_hex_or_dec_decimal() {
        assert_eq!(parse_hex_or_dec("65"), Some(65));
        assert_eq!(parse_hex_or_dec("0"), Some(0));
    }

    #[test]
    fn test_parse_hex_or_dec_invalid() {
        assert_eq!(parse_hex_or_dec("xyz"), None);
        assert_eq!(parse_hex_or_dec(""), None);
    }

    #[test]
    fn test_arm_implementer_name() {
        assert_eq!(arm_implementer_name(0x41), "ARM");
        assert_eq!(arm_implementer_name(0x61), "Apple");
        assert_eq!(arm_implementer_name(0xc0), "Ampere");
        assert_eq!(arm_implementer_name(0x51), "Qualcomm");
        assert_eq!(arm_implementer_name(0xFF), "Unknown");
    }

    #[test]
    fn test_parse_arm_cpuinfo_ids() {
        let mut entry = HashMap::new();
        entry.insert("CPU implementer".to_string(), "0x41".to_string());
        entry.insert("CPU part".to_string(), "0xd0c".to_string());
        entry.insert("CPU variant".to_string(), "0x1".to_string());
        entry.insert("CPU revision".to_string(), "2".to_string());

        let result = parse_arm_cpuinfo_ids(&entry);
        assert_eq!(result, Some((0x41, 0xd0c, 0x1, 2)));
    }

    #[test]
    fn test_parse_arm_cpuinfo_ids_missing_fields() {
        let entry = HashMap::new();
        assert_eq!(parse_arm_cpuinfo_ids(&entry), None);

        let mut entry = HashMap::new();
        entry.insert("CPU implementer".to_string(), "0x41".to_string());
        // Missing "CPU part"
        assert_eq!(parse_arm_cpuinfo_ids(&entry), None);
    }

    #[test]
    fn test_parse_arm_features() {
        let features_str = "fp asimd evtstrm aes pmull sha1 sha2 crc32 atomics";
        let f = parse_arm_features(Some(features_str));

        assert!(f.aes_ni); // aes -> aes_ni
        assert!(f.sha); // sha1/sha2 -> sha
        assert!(f.sse4_2); // crc32 -> sse4_2
        assert!(f.pclmulqdq); // pmull -> pclmulqdq
        assert_eq!(f.raw_features, Some(features_str.to_string()));
    }

    #[test]
    fn test_parse_arm_features_none() {
        let f = parse_arm_features(None);
        assert!(!f.aes_ni);
        assert!(!f.sha);
        assert!(f.raw_features.is_none());
    }

    #[test]
    fn test_gather_arm_info_x86_entry() {
        // An x86 /proc/cpuinfo entry should not produce ArmInfo.
        let mut entry = HashMap::new();
        entry.insert("vendor_id".to_string(), "GenuineIntel".to_string());
        entry.insert("model name".to_string(), "Intel Core i7".to_string());

        let result = gather_arm_info(Some(&entry));
        assert!(result.is_none());
    }

    #[test]
    fn test_gather_arm_info_arm_entry() {
        let mut entry = HashMap::new();
        entry.insert("processor".to_string(), "0".to_string());
        entry.insert("BogoMIPS".to_string(), "48.00".to_string());
        entry.insert(
            "Features".to_string(),
            "fp asimd evtstrm aes pmull sha1 sha2 crc32".to_string(),
        );
        entry.insert("CPU implementer".to_string(), "0x41".to_string());
        entry.insert("CPU architecture".to_string(), "8".to_string());
        entry.insert("CPU variant".to_string(), "0x1".to_string());
        entry.insert("CPU part".to_string(), "0xd0c".to_string());
        entry.insert("CPU revision".to_string(), "2".to_string());

        let result = gather_arm_info(Some(&entry));
        assert!(result.is_some());

        let info = result.unwrap();
        assert_eq!(info.vendor, CpuVendor::Arm);
        assert_eq!(info.codename, Some("Neoverse N1".to_string()));
        assert!(info.brand.as_ref().unwrap().contains("ARM"));
        assert!(info.brand.as_ref().unwrap().contains("Neoverse N1"));
        assert!(info.features.aes_ni);
        assert!(info.features.sha);
    }

    #[test]
    fn test_vendor_from_procfs_arm() {
        let mut entry = HashMap::new();
        entry.insert("CPU implementer".to_string(), "0x41".to_string());

        let vendor = vendor_from_procfs(Some(&entry));
        assert_eq!(vendor, CpuVendor::Arm);
    }
}
