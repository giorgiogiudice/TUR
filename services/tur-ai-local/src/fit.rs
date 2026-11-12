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
use crate::hardware::Hardware;

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

/// The recommendation for this computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Recommendation {
    /// The model recommended for conversation with P@x, if one fits.
    pub model: Option<String>,
    /// `TUR-AI-LOCAL-004` when one was found, `TUR-AI-LOCAL-007` when not.
    pub code: AiLocalCode,
    /// In plain words.
    pub message: String,
    /// Why, line by line.
    pub reasons: Vec<String>,
    /// The largest model file considered usable on this processor.
    pub cpu_budget_bytes: u64,
    /// The kind of the engine's processor build.
    pub engine_cpu: EngineCpu,
    /// A model with image input that fits, if there is one.
    pub vision_model: Option<String>,
    /// An embeddings model that fits, if there is one.
    pub embeddings_model: Option<String>,
    /// The judgement on every model.
    pub fits: Vec<Fit>,
}

/// Recommend a model for this computer.
pub fn recommend(
    catalogue: &Catalogue,
    hardware: &Hardware,
    context: u32,
    engine: EngineCpu,
) -> Recommendation {
    let budget = cpu_budget_bytes(hardware, engine);
    let fits: Vec<Fit> = catalogue
        .models
        .iter()
        .map(|model| judge(model, hardware, context))
        .collect();
    let fitting = |model: &&Model| {
        fits.iter()
            .any(|fit| fit.model == model.id && fit.verdict == Verdict::Fits)
    };
    let within_budget = |model: &&Model| model.download_bytes() <= budget;
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
            .filter(within_budget)
            .collect(),
    );
    // Without anything within the speed budget, the smallest model that
    // fits is still better than none: it is slow, and the message says so.
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
            .filter(within_budget)
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

    let mut reasons = vec![
        format!(
            "Memory: {} in all, {} free now; {} is kept free for the rest of the system.",
            crate::human_bytes(hardware.memory.total_bytes),
            crate::human_bytes(hardware.memory.available_bytes),
            crate::human_bytes(reserve(hardware.memory.total_bytes))
        ),
        format!(
            "Processor: {} logical processors, {} the fast matrix instructions; model files up \
             to {} are expected to answer at a usable speed.",
            hardware.cpu.logical_cpus,
            if hardware.cpu.fast_matrix_instructions {
                "with"
            } else {
                "without"
            },
            crate::human_bytes(budget)
        ),
        match engine {
            EngineCpu::Variants => "Engine: processor variants that use these instructions.",
            EngineCpu::Single => {
                "Engine: one processor build without the fast kernels, which answers several \
                 times more slowly; the budget is set as for a processor without them."
            }
            EngineCpu::Unknown => "Engine: not found; the budget assumes a build with the fast kernels.",
        }
        .to_owned(),
        "Graphics: not used yet; the engine TUR installs runs models on the processor.".to_owned(),
    ];
    let (code, message) = match (&chosen, slow) {
        (Some(id), false) => {
            let name = catalogue
                .get(id)
                .map_or(id.as_str(), |model| model.name.as_str());
            reasons.push(format!(
                "{name} is the largest model that fits, can call tools and is within the speed \
                 budget."
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
                "No model is within the speed budget; {name} is the smallest that fits."
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
        engine_cpu: engine,
        vision_model,
        embeddings_model,
        fits,
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
}
