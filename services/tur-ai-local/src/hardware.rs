// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! What this computer has for running a model: memory, processor features
//! and graphics devices.
//!
//! Everything is read, nothing is changed: `/proc/meminfo`, `/proc/cpuinfo`,
//! the DRM devices under `/sys/class/drm`, and the summary `vulkaninfo`
//! prints when it is installed. Tests and recorded machines give another
//! root directory instead of `/`, and a recorded `vulkaninfo --summary`
//! instead of running it.
//!
//! "A usable open backend" means a Vulkan device that is a real graphics
//! processor (not the software rasteriser) is present. Vulkan is an open
//! interface; whether the driver behind it is open source is reported
//! separately, from the driver identifier Vulkan gives. Vendor-proprietary
//! compute libraries (CUDA) are neither looked for nor used.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

/// The program that describes the Vulkan devices. Never looked up along a
/// search path.
pub const VULKANINFO: &str = "/usr/bin/vulkaninfo";

/// How long `vulkaninfo --summary` may take.
pub const VULKANINFO_TIMEOUT: Duration = Duration::from_secs(15);

/// Processor features that decide how fast the engine's processor backend
/// runs, by architecture. Only these are reported.
pub const RELEVANT_X86_FEATURES: &[&str] = &[
    "sse4_2",
    "avx",
    "avx2",
    "fma",
    "f16c",
    "avx_vnni",
    "avx512f",
    "avx512bw",
    "avx512vl",
    "avx512_vnni",
    "avx512_bf16",
    "amx_tile",
    "amx_int8",
    "amx_bf16",
];

/// As [`RELEVANT_X86_FEATURES`], for 64-bit Arm.
pub const RELEVANT_ARM_FEATURES: &[&str] = &[
    "asimd", "asimdhp", "asimddp", "fphp", "sve", "sve2", "i8mm", "bf16",
];

/// Memory, as the kernel reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Memory {
    /// MemTotal.
    pub total_bytes: u64,
    /// MemAvailable: what can be given to a new program without swapping.
    pub available_bytes: u64,
    /// SwapTotal, if the line is there.
    pub swap_total_bytes: Option<u64>,
}

/// The processor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Cpu {
    /// The model name, if `/proc/cpuinfo` gives one.
    pub model: Option<String>,
    /// `x86_64`, `aarch64` or the architecture of this program.
    pub architecture: String,
    /// Logical processors.
    pub logical_cpus: u32,
    /// Physical cores, when `/proc/cpuinfo` says which core each logical
    /// processor belongs to.
    pub physical_cores: Option<u32>,
    /// The relevant features present, in the order of the lists above.
    pub features: Vec<String>,
    /// AVX2 and FMA on x86, or the dot-product instructions on Arm: the
    /// features llama.cpp's fast processor kernels use.
    pub fast_matrix_instructions: bool,
}

string_enum! {
    /// Who made a graphics device, from its PCI vendor identifier.
    pub enum GpuVendor {
        /// 0x1002.
        Amd => "amd",
        /// 0x8086.
        Intel => "intel",
        /// 0x10de.
        Nvidia => "nvidia",
        /// Anything else, a virtual device included.
        Other => "other",
    }
}

/// A DRM graphics device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Gpu {
    /// Its name under /sys/class/drm, for example `card0`.
    pub card: String,
    /// The PCI vendor identifier, for example `0x1002`.
    pub vendor_id: Option<String>,
    /// The vendor.
    pub vendor: GpuVendor,
    /// The PCI device identifier.
    pub device_id: Option<String>,
    /// The kernel driver bound to it, for example `amdgpu`.
    pub driver: Option<String>,
    /// Its own memory, where the driver publishes it (amdgpu does).
    pub vram_bytes: Option<u64>,
    /// It is the device the firmware used for the boot display.
    pub boot_vga: Option<bool>,
}

string_enum! {
    /// What the Vulkan check found.
    pub enum VulkanStatus {
        /// vulkaninfo ran and listed devices.
        Listed => "listed",
        /// vulkaninfo is not installed.
        NotInstalled => "not-installed",
        /// vulkaninfo ran but failed or printed no device.
        Failed => "failed",
        /// Not checked: a recorded machine without a recorded summary.
        NotChecked => "not-checked",
    }
}

string_enum! {
    /// The kind of a Vulkan device.
    pub enum VulkanDeviceType {
        /// A graphics processor built into the processor package.
        IntegratedGpu => "integrated-gpu",
        /// A graphics card.
        DiscreteGpu => "discrete-gpu",
        /// A graphics processor of a virtual machine.
        VirtualGpu => "virtual-gpu",
        /// Vulkan implemented on the processor (llvmpipe).
        Cpu => "cpu",
        /// Anything else.
        Other => "other",
    }
}

/// A Vulkan device from `vulkaninfo --summary`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VulkanDevice {
    /// Its position in the summary: 0 for GPU0.
    pub index: u32,
    /// deviceName.
    pub name: Option<String>,
    /// deviceType.
    pub device_type: VulkanDeviceType,
    /// driverName, for example `radv`.
    pub driver_name: Option<String>,
    /// driverID as Vulkan names it, for example `DRIVER_ID_MESA_RADV`.
    pub driver_id: Option<String>,
    /// apiVersion.
    pub api_version: Option<String>,
    /// Whether the driver is open source, from its driver identifier; `None`
    /// for an identifier not known here.
    pub open_source_driver: Option<bool>,
    /// A real graphics processor that the engine's Vulkan backend can use.
    pub usable: bool,
}

/// The Vulkan check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Vulkan {
    /// What happened.
    pub status: VulkanStatus,
    /// The devices listed.
    pub devices: Vec<VulkanDevice>,
}

/// Everything read about the computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hardware {
    /// Memory.
    pub memory: Memory,
    /// The processor.
    pub cpu: Cpu,
    /// DRM graphics devices.
    pub gpus: Vec<Gpu>,
    /// Vulkan devices.
    pub vulkan: Vulkan,
    /// A usable Vulkan device is present: a real graphics processor, not
    /// the software rasteriser. `None` when Vulkan was not checked.
    pub usable_open_backend: Option<bool>,
}

/// Where the hardware is read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// The directory that holds `proc/` and `sys/`: `/` for this computer.
    pub root: PathBuf,
    /// A file holding what `vulkaninfo --summary` printed. Without it,
    /// vulkaninfo is run when the root is `/` and not checked otherwise.
    pub vulkaninfo: Option<PathBuf>,
}

impl Source {
    /// This computer.
    pub fn this_computer() -> Source {
        Source {
            root: PathBuf::from("/"),
            vulkaninfo: None,
        }
    }

    fn is_this_computer(&self) -> bool {
        self.root == Path::new("/")
    }
}

/// Why the hardware could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardwareError(
    /// What could not be read, and why.
    pub String,
);

impl std::fmt::Display for HardwareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Read everything. Only the memory is required; a processor or graphics
/// description that cannot be read is reported as unknown.
pub fn read(source: &Source) -> Result<Hardware, HardwareError> {
    let meminfo_path = source.root.join("proc/meminfo");
    let meminfo = fs::read_to_string(&meminfo_path)
        .map_err(|error| HardwareError(format!("{}: {error}", meminfo_path.display())))?;
    let memory = parse_meminfo(&meminfo).ok_or_else(|| {
        HardwareError(format!(
            "{}: no MemTotal or MemAvailable line",
            meminfo_path.display()
        ))
    })?;
    let cpuinfo = fs::read_to_string(source.root.join("proc/cpuinfo")).unwrap_or_default();
    let cpu = parse_cpuinfo(&cpuinfo);
    let gpus = read_gpus(&source.root.join("sys/class/drm"));
    let vulkan = match &source.vulkaninfo {
        Some(path) => match fs::read_to_string(path) {
            Ok(text) => vulkan_from_summary(&text),
            Err(_) => Vulkan {
                status: VulkanStatus::Failed,
                devices: Vec::new(),
            },
        },
        None if source.is_this_computer() => run_vulkaninfo(),
        None => Vulkan {
            status: VulkanStatus::NotChecked,
            devices: Vec::new(),
        },
    };
    let usable_open_backend = match vulkan.status {
        VulkanStatus::Listed | VulkanStatus::NotInstalled | VulkanStatus::Failed => {
            Some(vulkan.devices.iter().any(|device| device.usable))
        }
        VulkanStatus::NotChecked => None,
    };
    Ok(Hardware {
        memory,
        cpu,
        gpus,
        vulkan,
        usable_open_backend,
    })
}

/// MemTotal, MemAvailable and SwapTotal from `/proc/meminfo`.
pub fn parse_meminfo(text: &str) -> Option<Memory> {
    let field = |name: &str| -> Option<u64> {
        text.lines().find_map(|line| {
            let rest = line.strip_prefix(name)?.strip_prefix(':')?;
            let mut words = rest.split_whitespace();
            let value: u64 = words.next()?.parse().ok()?;
            match words.next() {
                Some("kB") => value.checked_mul(1024),
                None => Some(value),
                Some(_) => None,
            }
        })
    };
    Some(Memory {
        total_bytes: field("MemTotal")?,
        available_bytes: field("MemAvailable")?,
        swap_total_bytes: field("SwapTotal"),
    })
}

/// The processor from `/proc/cpuinfo`.
pub fn parse_cpuinfo(text: &str) -> Cpu {
    const NO_FEATURES: &[&str] = &[];
    let mut logical: u32 = 0;
    let mut model: Option<String> = None;
    let mut flags: Option<String> = None;
    let mut arm_features: Option<String> = None;
    let mut cores: std::collections::BTreeSet<(String, String)> = std::collections::BTreeSet::new();
    let mut core_known = true;

    // One block of lines per logical processor, separated by blank lines.
    // Arm kernels print the features once per processor too.
    for block in text.split("\n\n") {
        let mut is_processor = false;
        let mut physical_id: Option<&str> = None;
        let mut core_id: Option<&str> = None;
        for line in block.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                // Older Arm kernels also print a "Processor" line holding
                // the model; only "processor" with a number counts here.
                "processor" => is_processor = value.parse::<u32>().is_ok(),
                "model name" | "Model" | "Processor"
                    if model.is_none() && !value.is_empty() && value.parse::<u32>().is_err() =>
                {
                    model = Some(value.to_owned());
                }
                "flags" if flags.is_none() => flags = Some(value.to_owned()),
                "Features" if arm_features.is_none() => arm_features = Some(value.to_owned()),
                "physical id" => physical_id = Some(value),
                "core id" => core_id = Some(value),
                _ => {}
            }
        }
        if !is_processor {
            continue;
        }
        logical += 1;
        match (physical_id, core_id) {
            (Some(physical), Some(core)) => {
                cores.insert((physical.to_owned(), core.to_owned()));
            }
            _ => core_known = false,
        }
    }

    let (architecture, present, relevant): (String, Vec<&str>, &[&str]) =
        match (&flags, &arm_features) {
            (Some(flags), _) => (
                "x86_64".to_owned(),
                flags.split_whitespace().collect(),
                RELEVANT_X86_FEATURES,
            ),
            (None, Some(features)) => (
                "aarch64".to_owned(),
                features.split_whitespace().collect(),
                RELEVANT_ARM_FEATURES,
            ),
            (None, None) => (std::env::consts::ARCH.to_owned(), Vec::new(), NO_FEATURES),
        };
    let features: Vec<String> = relevant
        .iter()
        .filter(|feature| present.contains(feature))
        .map(|feature| (*feature).to_owned())
        .collect();
    let has = |name: &str| features.iter().any(|feature| feature == name);
    let fast_matrix_instructions = match architecture.as_str() {
        "x86_64" => has("avx2") && has("fma"),
        "aarch64" => has("asimddp"),
        _ => false,
    };
    let physical_cores = if core_known && !cores.is_empty() {
        u32::try_from(cores.len()).ok()
    } else {
        None
    };
    Cpu {
        model,
        architecture,
        logical_cpus: logical.max(1),
        physical_cores,
        features,
        fast_matrix_instructions,
    }
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

/// The DRM cards under `directory` (`/sys/class/drm`). Connectors such as
/// `card0-HDMI-A-1` and render nodes are not cards.
pub fn read_gpus(directory: &Path) -> Vec<Gpu> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            name.strip_prefix("card")
                .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
        })
        .collect();
    names.sort_by_key(|name| name[4..].parse::<u32>().unwrap_or(u32::MAX));
    names
        .into_iter()
        .map(|card| {
            let device = directory.join(&card).join("device");
            let vendor_id = read_trimmed(&device.join("vendor"));
            let vendor = match vendor_id.as_deref() {
                Some("0x1002") => GpuVendor::Amd,
                Some("0x8086") => GpuVendor::Intel,
                Some("0x10de") => GpuVendor::Nvidia,
                _ => GpuVendor::Other,
            };
            let driver = fs::read_link(device.join("driver"))
                .ok()
                .and_then(|link| link.file_name().map(|n| n.to_string_lossy().into_owned()))
                .or_else(|| {
                    read_trimmed(&device.join("uevent")).and_then(|uevent| {
                        uevent
                            .lines()
                            .find_map(|line| line.strip_prefix("DRIVER="))
                            .map(str::to_owned)
                    })
                });
            Gpu {
                card,
                vendor_id,
                vendor,
                device_id: read_trimmed(&device.join("device")),
                driver,
                vram_bytes: read_trimmed(&device.join("mem_info_vram_total"))
                    .and_then(|text| text.parse().ok()),
                boot_vga: read_trimmed(&device.join("boot_vga")).map(|text| text == "1"),
            }
        })
        .collect()
}

/// Whether a Vulkan driver identifier names an open-source driver.
pub fn driver_is_open_source(driver_id: &str) -> Option<bool> {
    const OPEN: &[&str] = &[
        "DRIVER_ID_MESA_RADV",
        "DRIVER_ID_INTEL_OPEN_SOURCE_MESA",
        "DRIVER_ID_MESA_NVK",
        "DRIVER_ID_MESA_TURNIP",
        "DRIVER_ID_MESA_V3DV",
        "DRIVER_ID_MESA_PANVK",
        "DRIVER_ID_MESA_VENUS",
        "DRIVER_ID_MESA_DOZEN",
        "DRIVER_ID_MESA_HONEYKRISP",
        "DRIVER_ID_MESA_LLVMPIPE",
        "DRIVER_ID_AMD_OPEN_SOURCE",
    ];
    const PROPRIETARY: &[&str] = &[
        "DRIVER_ID_NVIDIA_PROPRIETARY",
        "DRIVER_ID_AMD_PROPRIETARY",
        "DRIVER_ID_INTEL_PROPRIETARY_WINDOWS",
        "DRIVER_ID_QUALCOMM_PROPRIETARY",
        "DRIVER_ID_ARM_PROPRIETARY",
        "DRIVER_ID_IMAGINATION_PROPRIETARY",
        "DRIVER_ID_BROADCOM_PROPRIETARY",
    ];
    if OPEN.contains(&driver_id) {
        Some(true)
    } else if PROPRIETARY.contains(&driver_id) {
        Some(false)
    } else {
        None
    }
}

/// The devices of a `vulkaninfo --summary` output.
pub fn vulkan_from_summary(text: &str) -> Vulkan {
    let mut devices: Vec<VulkanDevice> = Vec::new();
    let mut current: Option<VulkanDevice> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(index) = trimmed
            .strip_prefix("GPU")
            .and_then(|rest| rest.strip_suffix(':'))
            .and_then(|number| number.parse::<u32>().ok())
        {
            if let Some(device) = current.take() {
                devices.push(device);
            }
            current = Some(VulkanDevice {
                index,
                name: None,
                device_type: VulkanDeviceType::Other,
                driver_name: None,
                driver_id: None,
                api_version: None,
                open_source_driver: None,
                usable: false,
            });
            continue;
        }
        let Some(device) = current.as_mut() else {
            continue;
        };
        let Some((key, value)) = trimmed.split_once('=') else {
            // A line that is neither a key nor a new device ends the list.
            if !trimmed.is_empty()
                && !line.starts_with(char::is_whitespace)
                && let Some(finished) = current.take()
            {
                devices.push(finished);
            }
            continue;
        };
        let value = value.trim().to_owned();
        match key.trim() {
            "deviceName" => device.name = Some(value),
            "deviceType" => {
                device.device_type = match value.as_str() {
                    "PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU" => VulkanDeviceType::IntegratedGpu,
                    "PHYSICAL_DEVICE_TYPE_DISCRETE_GPU" => VulkanDeviceType::DiscreteGpu,
                    "PHYSICAL_DEVICE_TYPE_VIRTUAL_GPU" => VulkanDeviceType::VirtualGpu,
                    "PHYSICAL_DEVICE_TYPE_CPU" => VulkanDeviceType::Cpu,
                    _ => VulkanDeviceType::Other,
                }
            }
            "driverName" => device.driver_name = Some(value),
            "driverID" => {
                device.open_source_driver = driver_is_open_source(&value);
                device.driver_id = Some(value);
            }
            "apiVersion" => device.api_version = Some(value),
            _ => {}
        }
    }
    if let Some(device) = current.take() {
        devices.push(device);
    }
    for device in &mut devices {
        device.usable = matches!(
            device.device_type,
            VulkanDeviceType::IntegratedGpu | VulkanDeviceType::DiscreteGpu
        );
    }
    let status = if devices.is_empty() {
        VulkanStatus::Failed
    } else {
        VulkanStatus::Listed
    };
    Vulkan { status, devices }
}

/// Run `vulkaninfo --summary` with a time limit and read what it prints.
fn run_vulkaninfo() -> Vulkan {
    if !Path::new(VULKANINFO).is_file() {
        return Vulkan {
            status: VulkanStatus::NotInstalled,
            devices: Vec::new(),
        };
    }
    let failed = Vulkan {
        status: VulkanStatus::Failed,
        devices: Vec::new(),
    };
    let Ok(mut child) = Command::new(VULKANINFO)
        .arg("--summary")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return failed;
    };
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return failed;
    };
    // Read on a thread of its own, so that a full pipe cannot stall the
    // program while this one waits for it.
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.by_ref().take(1 << 20).read_to_string(&mut text);
        text
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < VULKANINFO_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return failed;
            }
        }
    }
    match reader.join() {
        Ok(text) => vulkan_from_summary(&text),
        Err(_) => failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_is_read_in_bytes() {
        let memory = parse_meminfo(
            "MemTotal:        4009860 kB\nMemFree:  1 kB\nMemAvailable:     946728 kB\nSwapTotal: 0 kB\n",
        )
        .expect("memory");
        assert_eq!(memory.total_bytes, 4_009_860 * 1024);
        assert_eq!(memory.available_bytes, 946_728 * 1024);
        assert_eq!(memory.swap_total_bytes, Some(0));
        assert_eq!(parse_meminfo("MemTotal: 10 kB\n"), None);
        assert_eq!(parse_meminfo("MemTotal: x kB\nMemAvailable: 1 kB\n"), None);
    }

    #[test]
    fn processor_features_are_the_relevant_ones() {
        let cpu = parse_cpuinfo(
            "processor\t: 0\nmodel name\t: Test CPU\nphysical id\t: 0\ncore id\t: 0\nflags\t: fpu sse4_2 avx avx2 fma f16c sha_ni\n\n\
             processor\t: 1\nmodel name\t: Test CPU\nphysical id\t: 0\ncore id\t: 0\nflags\t: fpu sse4_2 avx avx2 fma f16c sha_ni\n",
        );
        assert_eq!(cpu.logical_cpus, 2);
        assert_eq!(cpu.physical_cores, Some(1));
        assert_eq!(cpu.model.as_deref(), Some("Test CPU"));
        assert_eq!(cpu.architecture, "x86_64");
        assert_eq!(cpu.features, ["sse4_2", "avx", "avx2", "fma", "f16c"]);
        assert!(cpu.fast_matrix_instructions);

        let old = parse_cpuinfo("processor : 0\nflags : fpu sse2 sse4_2\n");
        assert!(!old.fast_matrix_instructions);
        assert_eq!(old.physical_cores, None);

        let arm = parse_cpuinfo(
            "processor\t: 0\nBogoMIPS\t: 48.00\nFeatures\t: fp asimd evtstrm aes asimdhp asimddp i8mm\n",
        );
        assert_eq!(arm.architecture, "aarch64");
        assert_eq!(arm.features, ["asimd", "asimdhp", "asimddp", "i8mm"]);
        assert!(arm.fast_matrix_instructions);
    }

    #[test]
    fn a_vulkan_summary_lists_real_and_software_devices() {
        let text = "Devices:\n========\nGPU0:\n\tapiVersion = 1.4.318\n\tdeviceType = PHYSICAL_DEVICE_TYPE_DISCRETE_GPU\n\
                    \tdeviceName = AMD Radeon (RADV NAVI31)\n\tdriverID = DRIVER_ID_MESA_RADV\n\tdriverName = radv\n\
                    GPU1:\n\tdeviceType = PHYSICAL_DEVICE_TYPE_CPU\n\tdeviceName = llvmpipe (LLVM 20.1.8, 256 bits)\n\
                    \tdriverID = DRIVER_ID_MESA_LLVMPIPE\n\tdriverName = llvmpipe\n";
        let vulkan = vulkan_from_summary(text);
        assert_eq!(vulkan.status, VulkanStatus::Listed);
        assert_eq!(vulkan.devices.len(), 2);
        assert_eq!(vulkan.devices[0].device_type, VulkanDeviceType::DiscreteGpu);
        assert!(vulkan.devices[0].usable);
        assert_eq!(vulkan.devices[0].open_source_driver, Some(true));
        assert_eq!(vulkan.devices[1].device_type, VulkanDeviceType::Cpu);
        assert!(!vulkan.devices[1].usable);

        let only_software = vulkan_from_summary(
            "GPU0:\n\tdeviceType = PHYSICAL_DEVICE_TYPE_CPU\n\tdriverID = DRIVER_ID_MESA_LLVMPIPE\n",
        );
        assert!(only_software.devices.iter().all(|device| !device.usable));
        assert_eq!(vulkan_from_summary("").status, VulkanStatus::Failed);
        assert_eq!(
            driver_is_open_source("DRIVER_ID_NVIDIA_PROPRIETARY"),
            Some(false)
        );
        assert_eq!(driver_is_open_source("DRIVER_ID_SOMETHING_NEW"), None);
    }
}
