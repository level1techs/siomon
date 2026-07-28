use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuInfo {
    pub package_id: u32,
    pub brand: String,
    pub vendor: CpuVendor,
    pub family: u32,
    pub model: u32,
    pub stepping: u32,
    pub microcode: Option<String>,
    pub codename: Option<String>,
    pub socket: Option<String>,
    pub tdp_watts: Option<f64>,
    pub tj_max_celsius: Option<f64>,
    pub base_clock_mhz: Option<f64>,
    pub boost_clock_mhz: Option<f64>,
    pub scaling_driver: Option<String>,
    pub topology: CpuTopology,
    pub cache: CpuCache,
    pub features: CpuFeatures,
    pub vulnerabilities: Vec<CpuVulnerability>,
    pub physical_address_bits: Option<u8>,
    pub virtual_address_bits: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuVulnerability {
    pub name: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CpuVendor {
    Intel,
    Amd,
    Arm,
    Unknown(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuTopology {
    pub packages: u32,
    pub dies_per_package: u32,
    pub physical_cores: u32,
    pub logical_processors: u32,
    pub smt_enabled: bool,
    pub threads_per_core: u32,
    pub cores_per_die: Option<u32>,
    pub numa_nodes: Vec<NumaNode>,
    pub online_cpus: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NumaNode {
    pub node_id: u32,
    pub cpu_list: String,
    pub memory_bytes: Option<u64>,
    pub distances: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuCache {
    pub l1d: Option<CacheLevel>,
    pub l1i: Option<CacheLevel>,
    pub l2: Option<CacheLevel>,
    pub l3: Option<CacheLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheLevel {
    pub level: u8,
    pub cache_type: String,
    pub size_bytes: u64,
    pub ways: u32,
    pub line_size_bytes: u32,
    pub sets: Option<u32>,
    pub shared_by_cores: Option<u32>,
    pub instances: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CpuFeatures {
    pub sse: bool,
    pub sse2: bool,
    pub sse3: bool,
    pub ssse3: bool,
    pub sse4_1: bool,
    pub sse4_2: bool,
    pub sse4a: bool,
    pub avx: bool,
    pub avx2: bool,
    pub avx512f: bool,
    pub avx512dq: bool,
    pub avx512bw: bool,
    pub avx512vl: bool,
    pub avx512cd: bool,
    pub avx512ifma: bool,
    pub avx512vbmi: bool,
    pub avx512vnni: bool,
    pub avx512bf16: bool,
    pub avx512fp16: bool,
    pub avx_vnni: bool,
    pub amx_bf16: bool,
    pub amx_tile: bool,
    pub amx_int8: bool,
    pub aes_ni: bool,
    pub vaes: bool,
    pub sha: bool,
    pub pclmulqdq: bool,
    pub bmi1: bool,
    pub bmi2: bool,
    pub popcnt: bool,
    pub lzcnt: bool,
    pub adx: bool,
    pub fma: bool,
    pub f16c: bool,
    pub rdrand: bool,
    pub rdseed: bool,
    pub vmx: bool,
    pub svm: bool,
    pub hypervisor: bool,
    pub cet_ss: bool,
    pub cet_ibt: bool,
    /// Raw feature flags string from /proc/cpuinfo (e.g., ARM "Features" line).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_features: Option<String>,
}
