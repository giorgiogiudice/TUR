// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Whether a model fits this computer, and which model to recommend.
//!
//! **Memory.** A model needs the memory its catalogue entry states for the
//! context it is started with: its files, the cache of its context and the
//! engine's working memory. A reserve is kept for the rest of the system:
//! the larger of 1 GiB and a tenth of the total memory. A model
//! * *fits* when its need plus the reserve is no more than the memory
//!   available now (`TUR-AI-LOCAL-004`);
//! * fits *not now* when that is more than what is available but no more
//!   than the total (`TUR-AI-LOCAL-006`): close programs first;
//! * is *too large* when even the total is not enough (`TUR-AI-LOCAL-005`).
//!
//! A model that does not fit now is never started.
//!
//! **Speed.** This slice runs models on the processor. How fast a model
//! answers there depends mostly on the size of its file, on the processor
//! and on the engine's build. The rule, set from the measurements of report
//! local-ai-1 on a 4-processor runner (`docs/reports/local-ai-1.md`): when
//! the processor has the matrix instructions llama.cpp's fast kernels use
//! (AVX2 and FMA, or Arm's dot-product instructions) **and** the engine has
//! processor variants that use them, a model file of up to 3 GB on four to
//! seven logical processors and up to 5 GB on eight or more (1.5 GB on
//! fewer); otherwise up to 1.5 GB on eight or more, 0.8 GB on four to
//! seven, 0.5 GB on fewer. An engine with one processor library and no
//! variants, like Fedora 45's `llama-cpp`, counts as without the fast
//! kernels: it answered 6 to 9 times more slowly on the same runner. A
//! graphics processor is reported but not used for the recommendation: the
//! engine TUR installs today has no Vulkan backend (see
//! `docs/architecture/local-ai.md`).
//!
//! **Recommendation.** Among the models that fit now, can converse and can
//! call tools (so that P@x can act), and whose file is within the speed
//! budget: the one with the largest file. A model with image input, and an
//! embeddings model, are named beside it when one fits.

use serde::Serialize;

use crate::GIB;
use crate::catalogue::{Capability, Catalogue, Model};
use crate::codes::AiLocalCode;
use crate::hardware::{Hardware, VulkanDeviceType};

/// The context size a model is started with unless another is asked for.
pub const DEFAULT_CONTEXT: u32 = 8192;

string_enum! {
    /// Whether a model fits.
    pub enum Verdict {
        /// It fits in the memory available now.
        Fits => "fits",
        /// It fits in the total memory, not in what is available now.
        NotNow => "not-now",
        /// It needs more than the total memory.
        TooLarge => "too-large",
    }
}

impl Verdict {
    /// The code that reports this verdict.
    pub fn code(self) -> AiLocalCode {
        match self {
            Verdict::Fits => AiLocalCode::Fits,
            Verdict::NotNow => AiLocalCode::NotNow,
            Verdict::TooLarge => AiLocalCode::TooLarge,
        }
    }
}

/// The judgement on one model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fit {
    /// The model.
    pub model: String,
    /// The context size judged.
    pub context: u32,
    /// The memory the model needs at that context.
    pub need_bytes: u64,
    /// The reserve kept for the rest of the system.
    pub reserve_bytes: u64,
    /// MemAvailable.
    pub available_bytes: u64,
    /// MemTotal.
    pub total_bytes: u64,
    /// The verdict.
    pub verdict: Verdict,
    /// Its code.
    pub code: AiLocalCode,
    /// In plain words.
    pub message: String,
}

/// The reserve kept free for the rest of the system.
pub fn reserve(total_bytes: u64) -> u64 {
    GIB.max(total_bytes / 10)
}

/// Judge one model at one context size.
pub fn judge(model: &Model, hardware: &Hardware, context: u32) -> Fit {
    let need = model.memory_for_context(context);
    let reserve = reserve(hardware.memory.total_bytes);
    let wanted = need.saturating_add(reserve);
    let verdict = if wanted <= hardware.memory.available_bytes {
        Verdict::Fits
    } else if wanted <= hardware.memory.total_bytes {
        Verdict::NotNow
    } else {
        Verdict::TooLarge
    };
    let message = match verdict {
        Verdict::Fits => format!(
            "{} needs about {} of memory and fits: {} is free.",
            model.name,
            crate::human_bytes(need),
            crate::human_bytes(hardware.memory.available_bytes)
        ),
        Verdict::NotNow => format!(
            "{} needs about {} of memory plus {} kept free for the rest of the system; only {} \
             is free now.",
            model.name,
            crate::human_bytes(need),
            crate::human_bytes(reserve),
            crate::human_bytes(hardware.memory.available_bytes)
        ),
        Verdict::TooLarge => format!(
            "{} needs about {} of memory plus {} kept free for the rest of the system; this \
             computer has {} in all.",
            model.name,
            crate::human_bytes(need),
            crate::human_bytes(reserve),
            crate::human_bytes(hardware.memory.total_bytes)
        ),
    };
    Fit {
        model: model.id.clone(),
        context,
        need_bytes: need,
        reserve_bytes: reserve,
        available_bytes: hardware.memory.available_bytes,
        total_bytes: hardware.memory.total_bytes,
        verdict,
        code: verdict.code(),
        message,
    }
}

string_enum! {
    /// What the engine's processor backend is built for, from its libraries.
    pub enum EngineCpu {
        /// Processor variants chosen at run time (`libggml-cpu-haswell.so`
        /// and the like): the fast kernels are used where the processor
        /// has the instructions.
        Variants => "variants",
        /// One processor library and no variants. Fedora 45's build is of
        /// this kind and has no AVX2 kernels.
        Single => "single",
        /// Not known: the engine was not found.
        Unknown => "unknown",
    }
}

impl EngineCpu {
    /// The kind of build, from the backends `engine::backends` found.
    pub fn from_backends(backends: &[String]) -> EngineCpu {
        if backends.iter().any(|backend| backend.starts_with("cpu-")) {
            EngineCpu::Variants
        } else if backends.iter().any(|backend| backend == "cpu") {
            EngineCpu::Single
        } else {
            EngineCpu::Unknown
        }
    }
}

/// The largest model file expected to answer at a usable speed. See the
/// module documentation.
pub fn cpu_budget_bytes(hardware: &Hardware, engine: EngineCpu) -> u64 {
    let cpus = hardware.cpu.logical_cpus;
    let fast = hardware.cpu.fast_matrix_instructions && engine != EngineCpu::Single;
    if fast {
        if cpus >= 8 {
            5_000_000_000
        } else if cpus >= 4 {
            3_000_000_000
        } else {
            1_500_000_000
        }
    } else if cpus >= 8 {
        1_500_000_000
    } else if cpus >= 4 {
        800_000_000
    } else {
        500_000_000
    }
}

/// What the engine was built with: its processor kind, and whether it has
/// the Vulkan backend that runs a model on a graphics processor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct EngineBuild {
    /// The processor build.
    pub cpu: EngineCpu,
    /// It has a Vulkan backend.
    pub vulkan: bool,
}

impl EngineBuild {
    /// The build, from the backends `engine::backends` found.
    pub fn from_backends(backends: &[String]) -> EngineBuild {
        EngineBuild {
            cpu: EngineCpu::from_backends(backends),
            vulkan: backends.iter().any(|backend| backend == "vulkan"),
        }
    }
}

string_enum! {
    /// How well a model suits this computer.
    pub enum Tier {
        /// It fits in the memory and answers at reading speed or faster.
        RunsWell => "runs-well",
        /// It fits in the memory and answers more slowly than reading speed.
        Slow => "slow",
        /// It needs more memory than this computer has.
        DoesNotFit => "does-not-fit",
    }
}

string_enum! {
    /// Where the engine would run a model.
    pub enum Device {
        /// The processor.
        Processor => "processor",
        /// A graphics card with memory of its own.
        GraphicsCard => "graphics-card",
        /// A graphics processor built into the processor, sharing its memory.
        IntegratedGraphics => "integrated-graphics",
    }
}

string_enum! {
    /// Where a speed figure comes from.
    pub enum SpeedBasis {
        /// A measurement of the catalogue's, scaled to this computer.
        Measured => "measured",
        /// The rule of the module documentation.
        Estimated => "estimated",
    }
}

/// The generation speed taken as running well: a little faster than a
/// person reads.
pub const RUNS_WELL_TOKENS_PER_SECOND: f64 = 8.0;

/// The bytes of weights a processor with the fast kernels reads per second
/// for each logical processor, when the engine has the variants that use
/// them. Set from report local-ai-1: on the 4-processor CI runner llama.cpp's
/// own build generated 14.85 tokens a second from Qwen3 4B's 2.50 GB and
/// 32.87 from Qwen3-VL 2B's 1.11 GB, about 36.5 GB a second, or 9 for each
/// logical processor.
pub const FAST_BYTES_PER_CPU: f64 = 9.0e9;

/// The most a processor's memory gives, with the fast kernels: about what
/// two channels of current desktop memory sustain.
pub const FAST_BYTES_CAP: f64 = 60.0e9;

/// As [`FAST_BYTES_PER_CPU`], without the fast kernels: Fedora 45's build
/// generated 1.61 and 3.72 tokens a second from the same two models on the
/// same runner, about 4 GB a second.
pub const SLOW_BYTES_PER_CPU: f64 = 1.0e9;

/// The most a processor gives without the fast kernels.
pub const SLOW_BYTES_CAP: f64 = 16.0e9;

/// What a graphics card with its own memory reads per second, assumed:
/// half of what a mid-range card's memory gives. Not measured: no runner of
/// the project has a graphics processor.
pub const GRAPHICS_CARD_BYTES: f64 = 150.0e9;

/// What a graphics processor built into the processor reads per second,
/// assumed. Not measured.
pub const INTEGRATED_GRAPHICS_BYTES: f64 = 40.0e9;

/// The machine the catalogue's measurements were made on: GitHub's
/// 4-processor runner, with AVX2 and FMA.
const MEASURED_CPUS: u32 = 4;

/// The bytes a second the processor gives, by the rule above.
pub fn processor_bytes_per_second(cpus: u32, fast: bool) -> f64 {
    let cpus = f64::from(cpus.max(1));
    if fast {
        (FAST_BYTES_PER_CPU * cpus).min(FAST_BYTES_CAP)
    } else {
        (SLOW_BYTES_PER_CPU * cpus).min(SLOW_BYTES_CAP)
    }
}

/// The judgement of how one model suits this computer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelTier {
    /// The model.
    pub model: String,
    /// The tier.
    pub tier: Tier,
    /// Where it would run; none when it does not fit.
    pub device: Option<Device>,
    /// The generation speed expected, in tokens a second; none when it does
    /// not fit.
    pub tokens_per_second: Option<f64>,
    /// Where the speed comes from.
    pub basis: Option<SpeedBasis>,
    /// It fits in the memory free now, not only in the total.
    pub fits_now: bool,
    /// In plain words: the tier and the main reason.
    pub message: String,
    /// Why, line by line.
    pub reasons: Vec<String>,
}

/// The speed measured for a model with this kind of engine build, in
/// tokens a second on the runner, if the catalogue has one.
fn measured(model: &Model, cpu: EngineCpu) -> Option<f64> {
    model
        .tested
        .iter()
        .filter(|tested| {
            let fedora = tested.engine.contains("Fedora");
            match cpu {
                EngineCpu::Single => fedora,
                EngineCpu::Variants | EngineCpu::Unknown => !fedora,
            }
        })
        .filter_map(|tested| tested.generation_tokens_per_second)
        .reduce(f64::max)
}

/// The graphics processor the engine would run on, its memory, and the
/// bytes a second it is assumed to give; or why there is none.
fn graphics(hardware: &Hardware, engine: EngineBuild) -> Result<(Device, Option<u64>, f64), String> {
    let usable: Vec<_> = hardware
        .vulkan
        .devices
        .iter()
        .filter(|device| device.usable)
        .collect();
    if usable.is_empty() {
        return Err(
            "Graphics: no graphics processor the engine could use was found; it runs models on \
             the processor."
                .to_owned(),
        );
    }
    if !engine.vulkan {
        return Err(
            "Graphics: a graphics processor is there, but the engine installed has no Vulkan \
             backend, so models run on the processor."
                .to_owned(),
        );
    }
    let discrete = usable
        .iter()
        .any(|device| device.device_type == VulkanDeviceType::DiscreteGpu);
    if discrete {
        let memory = hardware.gpus.iter().filter_map(|gpu| gpu.vram_bytes).max();
        Ok((Device::GraphicsCard, memory, GRAPHICS_CARD_BYTES))
    } else {
        Ok((Device::IntegratedGraphics, None, INTEGRATED_GRAPHICS_BYTES))
    }
}

/// How each model of the catalogue suits this computer.
///
/// **Does not fit**: it needs more than the total memory, with the reserve
/// ([`judge`]). Otherwise the speed decides: the bytes of weights read for
/// each generated token ([`Model::active_bytes`]) against what the device
/// reads in a second. **Runs well** at [`RUNS_WELL_TOKENS_PER_SECOND`] or
/// more, **slow** below. A measurement of the catalogue's, for the same kind
/// of engine build, is scaled to this processor and used instead of the
/// rule. A graphics card is counted only with an engine that has the Vulkan
/// backend and when the model fits in the card's own memory, which only
/// `amdgpu` reports; otherwise the processor is.
pub fn tiers(
    catalogue: &Catalogue,
    hardware: &Hardware,
    context: u32,
    engine: EngineBuild,
) -> Vec<ModelTier> {
    let fast = hardware.cpu.fast_matrix_instructions && engine.cpu != EngineCpu::Single;
    let processor = processor_bytes_per_second(hardware.cpu.logical_cpus, fast);
    let runner = processor_bytes_per_second(MEASURED_CPUS, engine.cpu != EngineCpu::Single);
    let graphics = graphics(hardware, engine);
    catalogue
        .models
        .iter()
        .filter(|model| !model.can(Capability::Embeddings))
        .map(|model| {
            let fit = judge(model, hardware, context);
            let mut reasons = vec![fit.message.clone()];
            if fit.verdict == Verdict::TooLarge {
                return ModelTier {
                    model: model.id.clone(),
                    tier: Tier::DoesNotFit,
                    device: None,
                    tokens_per_second: None,
                    basis: None,
                    fits_now: false,
                    message: format!(
                        "{} does not fit: it needs more memory than this computer has.",
                        model.name
                    ),
                    reasons,
                };
            }
            let active = model.active_bytes().max(1) as f64;
            // The processor, from a measurement when there is one.
            let (mut device, mut speed, mut basis) = match measured(model, engine.cpu) {
                Some(on_runner) => (
                    Device::Processor,
                    on_runner * processor / runner,
                    SpeedBasis::Measured,
                ),
                None => (Device::Processor, processor / active, SpeedBasis::Estimated),
            };
            match &graphics {
                Ok((kind, own_memory, bytes)) => {
                    let need = model
                        .download_bytes()
                        .saturating_add(model.kv_bytes_per_token.saturating_mul(u64::from(context)))
                        .saturating_add(crate::GIB / 2);
                    let fits_card = match (kind, own_memory) {
                        (Device::GraphicsCard, Some(memory)) => need <= *memory,
                        (Device::GraphicsCard, None) => false,
                        _ => true,
                    };
                    let on_graphics = bytes / active;
                    if fits_card && on_graphics > speed {
                        device = *kind;
                        speed = on_graphics;
                        basis = SpeedBasis::Estimated;
                    } else if !fits_card {
                        reasons.push(match own_memory {
                            Some(memory) => format!(
                                "It needs about {} on the graphics card, which has {} of its own; \
                                 it runs on the processor.",
                                crate::human_bytes(need),
                                crate::human_bytes(*memory)
                            ),
                            None => "The graphics card does not report its own memory, so the \
                                     model is counted on the processor."
                                .to_owned(),
                        });
                    }
                }
                Err(why) => reasons.push(why.clone()),
            }
            let speed = (speed * 10.0).round() / 10.0;
            let place = match device {
                Device::Processor => "on the processor",
                Device::GraphicsCard => "on the graphics card",
                Device::IntegratedGraphics => "on the built-in graphics processor",
            };
            let how = match basis {
                SpeedBasis::Measured => "measured in TUR's tests and scaled to this processor",
                SpeedBasis::Estimated => "estimated from the model's size and this hardware",
            };
            reasons.push(format!(
                "About {speed} tokens a second {place} ({how})."
            ));
            let tier = if speed >= RUNS_WELL_TOKENS_PER_SECOND {
                Tier::RunsWell
            } else {
                Tier::Slow
            };
            let fits_now = fit.verdict == Verdict::Fits;
            let message = match tier {
                Tier::RunsWell => format!("{} fits and runs well.", model.name),
                _ => format!("{} fits but will be slow.", model.name),
            };
            if !fits_now {
                reasons.push(
                    "Close some programs first: it does not fit in the memory free now."
                        .to_owned(),
                );
            }
            ModelTier {
                model: model.id.clone(),
                tier,
                device: Some(device),
                tokens_per_second: Some(speed),
                basis: Some(basis),
                fits_now,
                message,
                reasons,
            }
        })
        .collect()
}

/// The recommendation for this computer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Recommendation {
    /// The model recommended for conversation with P@x, if one fits.
    pub model: Option<String>,
    /// `TUR-AI-LOCAL-004` when one was found, `TUR-AI-LOCAL-007` when not.
    pub code: AiLocalCode,
    /// In plain words.
    pub message: String,
    /// Why, line by line.
    pub reasons: Vec<String>,
    /// The largest model file considered usable on this processor by the
    /// first slice's rule of thumb ([`cpu_budget_bytes`]). Reported; the
    /// choice now follows [`tiers`].
    pub cpu_budget_bytes: u64,
    /// The kind of the engine's processor build.
    pub engine_cpu: EngineCpu,
    /// The engine has a Vulkan backend.
    pub engine_vulkan: bool,
    /// A model with image input that fits, if there is one.
    pub vision_model: Option<String>,
    /// An embeddings model that fits, if there is one.
    pub embeddings_model: Option<String>,
    /// The judgement on every model.
    pub fits: Vec<Fit>,
    /// How every model that converses suits this computer.
    pub tiers: Vec<ModelTier>,
}

/// Recommend a model for this computer: among the models that converse and
/// call tools, fit in the memory free now and run well, the one with the
/// largest download; without any that runs well, the smallest that fits,
/// marked slow.
pub fn recommend(
    catalogue: &Catalogue,
    hardware: &Hardware,
    context: u32,
    engine: EngineBuild,
) -> Recommendation {
    let budget = cpu_budget_bytes(hardware, engine.cpu);
    let fits: Vec<Fit> = catalogue
        .models
        .iter()
        .map(|model| judge(model, hardware, context))
        .collect();
    let tiers = tiers(catalogue, hardware, context, engine);
    let tier_of = |model: &Model| tiers.iter().find(|tier| tier.model == model.id);
    let fitting = |model: &&Model| {
        fits.iter()
            .any(|fit| fit.model == model.id && fit.verdict == Verdict::Fits)
    };
    let runs_well = |model: &&Model| {
        tier_of(*model).is_some_and(|tier| tier.tier == Tier::RunsWell && tier.fits_now)
    };
    let largest = |models: Vec<&Model>| -> Option<String> {
        models
            .into_iter()
            .max_by_key(|model| (model.download_bytes(), std::cmp::Reverse(model.id.clone())))
            .map(|model| model.id.clone())
    };

    let conversational: Vec<&Model> = catalogue
        .models
        .iter()
        .filter(|model| model.can(Capability::Chat) && model.can(Capability::Tools))
        .collect();
    let chosen = largest(
        conversational
            .iter()
            .copied()
            .filter(fitting)
            .filter(runs_well)
            .collect(),
    );
    // Without anything that runs well, the smallest model that fits is
    // still better than none: it is slow, and the message says so.
    let (chosen, slow) = match chosen {
        Some(id) => (Some(id), false),
        None => {
            let smallest = conversational
                .iter()
                .copied()
                .filter(fitting)
                .min_by_key(|model| (model.download_bytes(), model.id.clone()))
                .map(|model| model.id.clone());
            let slow = smallest.is_some();
            (smallest, slow)
        }
    };
    let vision_model = largest(
        catalogue
            .models
            .iter()
            .filter(|model| model.can(Capability::Vision))
            .filter(fitting)
            .filter(runs_well)
            .collect(),
    );
    let embeddings_model = largest(
        catalogue
            .models
            .iter()
            .filter(|model| model.can(Capability::Embeddings))
            .filter(fitting)
            .collect(),
    );

    let fast = hardware.cpu.fast_matrix_instructions && engine.cpu != EngineCpu::Single;
    let mut reasons = vec![
        format!(
            "Memory: {} in all, {} free now; {} is kept free for the rest of the system.",
            crate::human_bytes(hardware.memory.total_bytes),
            crate::human_bytes(hardware.memory.available_bytes),
            crate::human_bytes(reserve(hardware.memory.total_bytes))
        ),
        format!(
            "Processor: {} logical processors, {} the fast matrix instructions; about {} of \
             model weights read a second.",
            hardware.cpu.logical_cpus,
            if hardware.cpu.fast_matrix_instructions {
                "with"
            } else {
                "without"
            },
            crate::human_bytes(
                processor_bytes_per_second(hardware.cpu.logical_cpus, fast) as u64
            )
        ),
        match engine.cpu {
            EngineCpu::Variants => "Engine: processor variants that use these instructions.",
            EngineCpu::Single => {
                "Engine: one processor build without the fast kernels, which answers several \
                 times more slowly; speeds are counted as for a processor without them."
            }
            EngineCpu::Unknown => "Engine: not found; speeds assume a build with the fast kernels.",
        }
        .to_owned(),
        match graphics(hardware, engine) {
            Ok((Device::GraphicsCard, memory, _)) => format!(
                "Graphics: a graphics card the engine can use{}.",
                memory
                    .map(|bytes| format!(", with {} of its own memory", crate::human_bytes(bytes)))
                    .unwrap_or_default()
            ),
            Ok(_) => "Graphics: a built-in graphics processor the engine can use.".to_owned(),
            Err(why) => why,
        },
    ];
    let (code, message) = match (&chosen, slow) {
        (Some(id), false) => {
            let name = catalogue
                .get(id)
                .map_or(id.as_str(), |model| model.name.as_str());
            reasons.push(format!(
                "{name} is the largest model that fits now, can call tools and runs well."
            ));
            (
                AiLocalCode::Fits,
                format!("{name} is the best fit for this computer."),
            )
        }
        (Some(id), true) => {
            let name = catalogue
                .get(id)
                .map_or(id.as_str(), |model| model.name.as_str());
            reasons.push(format!(
                "No model that calls tools runs well here; {name} is the smallest that fits."
            ));
            (
                AiLocalCode::Fits,
                format!("{name} fits this computer, but will answer slowly."),
            )
        }
        (None, _) => (
            AiLocalCode::NoModelFits,
            AiLocalCode::NoModelFits.entry().user_message.to_owned(),
        ),
    };
    Recommendation {
        model: chosen,
        code,
        message,
        reasons,
        cpu_budget_bytes: budget,
        engine_cpu: engine.cpu,
        engine_vulkan: engine.vulkan,
        vision_model,
        embeddings_model,
        fits,
        tiers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{Vulkan, VulkanStatus, parse_cpuinfo, parse_meminfo};

    fn machine(cpus: usize, flags: &str) -> Hardware {
        let cpuinfo: Vec<String> = (0..cpus)
            .map(|index| format!("processor : {index}\nflags : {flags}\n"))
            .collect();
        Hardware {
            memory: parse_meminfo("MemTotal: 16000000 kB\nMemAvailable: 15000000 kB\n")
                .expect("memory"),
            cpu: parse_cpuinfo(&cpuinfo.join("\n")),
            gpus: Vec::new(),
            vulkan: Vulkan {
                status: VulkanStatus::NotChecked,
                devices: Vec::new(),
            },
            usable_open_backend: None,
        }
    }

    #[test]
    fn the_engine_build_is_read_from_its_libraries() {
        let names = |list: &[&str]| {
            list.iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            EngineCpu::from_backends(&names(&["cpu-haswell", "cpu-x64", "vulkan"])),
            EngineCpu::Variants
        );
        assert_eq!(
            EngineCpu::from_backends(&names(&["cpu", "hip"])),
            EngineCpu::Single
        );
        assert_eq!(EngineCpu::from_backends(&[]), EngineCpu::Unknown);
    }

    #[test]
    fn a_single_baseline_build_gets_the_budget_of_a_processor_without_the_kernels() {
        let fast = machine(4, "avx avx2 fma f16c");
        assert_eq!(cpu_budget_bytes(&fast, EngineCpu::Variants), 3_000_000_000);
        assert_eq!(cpu_budget_bytes(&fast, EngineCpu::Unknown), 3_000_000_000);
        assert_eq!(cpu_budget_bytes(&fast, EngineCpu::Single), 800_000_000);
        let old = machine(4, "sse4_2");
        assert_eq!(cpu_budget_bytes(&old, EngineCpu::Variants), 800_000_000);
        assert_eq!(
            cpu_budget_bytes(&machine(16, "avx2 fma"), EngineCpu::Variants),
            5_000_000_000
        );
        assert_eq!(
            cpu_budget_bytes(&machine(2, "sse2"), EngineCpu::Variants),
            500_000_000
        );
    }

    fn seed() -> Catalogue {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("catalogue");
        Catalogue::load(&directory).expect("the seed catalogue")
    }

    fn tier<'a>(tiers: &'a [ModelTier], id: &str) -> &'a ModelTier {
        tiers
            .iter()
            .find(|tier| tier.model == id)
            .unwrap_or_else(|| panic!("{id} has no tier"))
    }

    const VARIANTS: EngineBuild = EngineBuild {
        cpu: EngineCpu::Variants,
        vulkan: false,
    };

    #[test]
    fn three_tiers_with_a_reason_each_on_a_16_gb_computer() {
        let catalogue = seed();
        let tiers = tiers(&catalogue, &machine(16, "avx avx2 fma f16c"), 8192, VARIANTS);
        // Embeddings models have no tier: they do not converse.
        assert!(tiers.iter().all(|tier| tier.model != "qwen3-embedding-0.6b"));
        for id in ["qwen3-30b-a3b", "gemma-4-26b-a4b"] {
            let judged = tier(&tiers, id);
            assert_eq!(judged.tier, Tier::DoesNotFit, "{id}");
            assert_eq!(judged.tokens_per_second, None);
            assert!(judged.message.ends_with("does not fit: it needs more memory than this computer has."));
        }
        // 16 logical processors with the fast kernels: 60 GB a second,
        // 2.9 GB read for each token of Gemma 4 E4B.
        let e4b = tier(&tiers, "gemma-4-e4b");
        assert_eq!(e4b.tier, Tier::RunsWell);
        assert_eq!(e4b.device, Some(Device::Processor));
        assert_eq!(e4b.basis, Some(SpeedBasis::Estimated));
        assert_eq!(e4b.tokens_per_second, Some(20.7));
        assert!(e4b.fits_now);
        assert_eq!(e4b.message, "Gemma 4 E4B fits and runs well.");
        assert!(e4b.reasons.iter().any(|reason| reason.contains("no graphics processor")));
        // Qwen3 4B was measured: 14.85 tokens a second on the 4-processor
        // runner, scaled by 60/36.
        let measured = tier(&tiers, "qwen3-4b");
        assert_eq!(measured.basis, Some(SpeedBasis::Measured));
        let speed = measured.tokens_per_second.expect("a speed");
        assert!((24.6..=24.9).contains(&speed), "{speed}");
        assert!(
            tiers
                .iter()
                .all(|tier| tier.reasons.iter().all(|reason| reason.ends_with('.')))
        );
    }

    #[test]
    fn without_the_fast_kernels_models_are_slow() {
        let catalogue = seed();
        let build = EngineBuild {
            cpu: EngineCpu::Single,
            vulkan: false,
        };
        // Two logical processors: half of the 4-processor runner.
        let tiers = tiers(&catalogue, &machine(2, "sse4_2"), 8192, build);
        // Fedora's build measured 1.61 tokens a second on the runner.
        let measured = tier(&tiers, "qwen3-4b");
        assert_eq!(measured.tier, Tier::Slow);
        assert_eq!(measured.basis, Some(SpeedBasis::Measured));
        assert_eq!(measured.tokens_per_second, Some(0.8));
        assert_eq!(measured.message, "Qwen3 4B fits but will be slow.");
        let e2b = tier(&tiers, "gemma-4-e2b");
        assert_eq!(e2b.tier, Tier::Slow);
        assert_eq!(e2b.basis, Some(SpeedBasis::Estimated));
        let recommendation = recommend(&catalogue, &machine(2, "sse4_2"), 8192, build);
        assert!(recommendation.message.ends_with("but will answer slowly."));
    }

    #[test]
    fn a_graphics_card_counts_only_with_vulkan_and_room_in_its_own_memory() {
        use crate::hardware::{Gpu, GpuVendor, VulkanDevice};
        let catalogue = seed();
        let mut hardware = machine(8, "avx avx2 fma");
        hardware.gpus = vec![Gpu {
            card: "card0".to_owned(),
            vendor_id: Some("0x1002".to_owned()),
            vendor: GpuVendor::Amd,
            device_id: None,
            driver: Some("amdgpu".to_owned()),
            vram_bytes: Some(8_573_157_376),
            boot_vga: Some(true),
        }];
        hardware.vulkan = Vulkan {
            status: VulkanStatus::Listed,
            devices: vec![VulkanDevice {
                index: 0,
                name: Some("AMD Radeon".to_owned()),
                device_type: VulkanDeviceType::DiscreteGpu,
                driver_name: Some("radv".to_owned()),
                driver_id: Some("DRIVER_ID_MESA_RADV".to_owned()),
                api_version: None,
                open_source_driver: Some(true),
                usable: true,
            }],
        };
        let with_vulkan = EngineBuild {
            cpu: EngineCpu::Variants,
            vulkan: true,
        };
        let on_card = tiers(&catalogue, &hardware, 8192, with_vulkan);
        let e2b = tier(&on_card, "gemma-4-e2b");
        assert_eq!(e2b.device, Some(Device::GraphicsCard));
        assert_eq!(e2b.tier, Tier::RunsWell);
        assert_eq!(e2b.basis, Some(SpeedBasis::Estimated));
        // The engine without Vulkan cannot use the card.
        let without = tiers(&catalogue, &hardware, 8192, VARIANTS);
        let e2b = tier(&without, "gemma-4-e2b");
        assert_eq!(e2b.device, Some(Device::Processor));
        assert!(e2b.reasons.iter().any(|reason| reason.contains("no Vulkan backend")));
        // A card that does not report its memory is not counted on.
        hardware.gpus[0].vram_bytes = None;
        let unknown = tiers(&catalogue, &hardware, 8192, with_vulkan);
        assert_eq!(tier(&unknown, "gemma-4-e2b").device, Some(Device::Processor));
    }

    #[test]
    fn the_recommendation_is_the_largest_model_that_runs_well_now() {
        let catalogue = seed();
        let recommendation = recommend(&catalogue, &machine(16, "avx2 fma"), 8192, VARIANTS);
        assert_eq!(recommendation.code, AiLocalCode::Fits);
        assert_eq!(recommendation.model.as_deref(), Some("gemma-4-e4b"));
        assert_eq!(recommendation.vision_model.as_deref(), Some("gemma-4-e4b"));
        assert_eq!(
            recommendation.embeddings_model.as_deref(),
            Some("qwen3-embedding-0.6b")
        );
        assert_eq!(recommendation.tiers.len(), catalogue.models.len() - 1);
        assert!(!recommendation.engine_vulkan);
    }
}
