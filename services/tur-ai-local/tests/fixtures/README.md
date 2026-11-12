# Test fixtures of tur-ai-local

| Fixture | What it is | Where it comes from |
|---|---|---|
| `hosts/small-vm/` | `proc/meminfo`, `proc/cpuinfo` and the DRM card's `vendor`, `device`, `uevent` and `boot_vga` of a 2-vCPU, 4 GB cloud virtual machine (AMD EPYC 7601, a bochs-drm virtual display) | Recorded on 2026-11-12 from the development server the change was written on, by copying the files. Nothing was edited. |
| `hosts/desktop-amd/` | A desktop with 32 GB, 16 logical processors with AVX2 and AVX-512, and an AMD graphics card with 8 GB of its own memory | **Constructed by hand** in the formats of the kernel's files, to test what the recorded machine cannot show (much memory, a usable graphics card). It is not a recording of a real machine. |
| `hosts/desktop-amd/vulkaninfo-summary.txt` | A `vulkaninfo --summary` listing a RADV discrete card and llvmpipe | **Written by hand** in the format of `vulkaninfo --summary` (Vulkan-Tools). The local-ai workflow captures the real output of the runner, which has only llvmpipe, and checks that `tur-ai-local hardware` reads it (artifact `local-ai-fedora-evidence`, `vulkaninfo-summary.txt`). |
| `fake-llama-server` | A Python stand-in for `llama-server` that answers `/health`, `/metrics` and a trivial chat completion | Written for these tests. It proves the supervisor, never the engine. |
