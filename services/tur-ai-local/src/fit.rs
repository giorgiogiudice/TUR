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
//! answers there depends mostly on the size of its file and on the
//! processor. Until measurements from real computers replace it, the rule
//! is: with the matrix instructions llama.cpp's fast kernels use (AVX2 and
//! FMA, or Arm's dot-product instructions), a model file of up to 3 GB on
//! four to seven logical processors and up to 5 GB on eight or more; without
//! them, up to 1.5 GB, or 0.8 GB on fewer than four. A graphics processor
//! is reported but not used for the recommendation: the engine TUR installs
//! today has no Vulkan backend (see `docs/architecture/local-ai.md`).
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

/// The largest model file the processor is expected to run at a usable
/// speed. See the module documentation.
pub fn cpu_budget_bytes(hardware: &Hardware) -> u64 {
    let cpus = hardware.cpu.logical_cpus;
    if hardware.cpu.fast_matrix_instructions {
        if cpus >= 8 {
            5_000_000_000
        } else if cpus >= 4 {
            3_000_000_000
        } else {
            1_500_000_000
        }
    } else if cpus >= 4 {
        1_500_000_000
    } else {
        800_000_000
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
    /// A model with image input that fits, if there is one.
    pub vision_model: Option<String>,
    /// An embeddings model that fits, if there is one.
    pub embeddings_model: Option<String>,
    /// The judgement on every model.
    pub fits: Vec<Fit>,
}

/// Recommend a model for this computer.
pub fn recommend(catalogue: &Catalogue, hardware: &Hardware, context: u32) -> Recommendation {
    let budget = cpu_budget_bytes(hardware);
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
        "Graphics: not used yet; the engine TUR installs runs models on the processor."
            .to_owned(),
    ];
    let (code, message) = match (&chosen, slow) {
        (Some(id), false) => {
            let name = catalogue.get(id).map_or(id.as_str(), |model| model.name.as_str());
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
            let name = catalogue.get(id).map_or(id.as_str(), |model| model.name.as_str());
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
        vision_model,
        embeddings_model,
        fits,
    }
}
