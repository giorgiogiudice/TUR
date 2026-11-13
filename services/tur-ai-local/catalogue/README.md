# The TUR local model catalogue

One file per model that TUR offers to run on the person's own computer. Each
file says where the publisher's file is, its exact size and SHA-256 digest,
what the model can do, the memory it needs, its licence in plain words, and
what TUR has measured with it. `tur-ai-local` reads these files
(`--catalogue`, installed at `/usr/share/tur/ai-local/models/`); a model is
downloaded only from an entry, and used only after its size and digest match.

The format is [`schemas/local-model.schema.json`](../../../schemas/local-model.schema.json).
The loader (`services/tur-ai-local/src/catalogue.rs`) is authoritative and
refuses a field the format does not define.

**Why here and not in `catalogue/models/`.** The application catalogue's
loader (`services/tur-catalogue`) requires `catalogue/` to be one flat
directory of application entries and refuses a subdirectory, and the image
installs `catalogue/*.toml` as application entries. Putting model entries in
`catalogue/models/` would break the application catalogue's tests. Moving
them there needs that loader to skip the subdirectory first; see
[local-ai.md](../../../docs/architecture/local-ai.md), section 8.

## Rules

- The file is named `<id>.toml`; the id is lower case and never changes.
- **Official source only.** `repository` is the publisher's own repository.
  Every URL is `https` and pinned to one `revision` (a 40-digit commit), never
  to a branch.
- `size_bytes` and `sha256` are the publisher's, read from the repository's
  file list (Hugging Face gives the SHA-256 of every LFS file). The local-ai
  workflow downloads the file and checks both.
- **Licence in plain words.** `summary` says what the licence lets a person
  do. `restrictions` is exactly `none`, or the use restrictions in sentences;
  a model whose licence restricts use is listed only with its restrictions
  shown, and `tur-ai-local pull` asks for `--accept-licence`. `evidence`
  says what was read to establish the licence and what could not be found
  (for example, a repository with no licence file).
- `memory_bytes` is the memory needed at `memory_context` tokens: the files,
  the 16-bit context cache (`kv_bytes_per_token` × tokens) and the engine's
  working memory. It is an estimate until a measurement replaces it; the
  loader refuses a value below the files plus the cache.
- Languages are BCP 47 tags as the publisher states them; `mul` means "many
  languages" and `languages_note` quotes the publisher's claim.
- `[[tested]]` records come only from a real run, with the report in
  `docs/reports/` and the CI run's URL. No entry may claim a result that was
  not measured.

## The two lists

The first list (2026-11-12): Qwen3 0.6B, Granite 4.0 1B, Qwen3-VL 2B
Instruct, Qwen3 4B, Qwen3 Embedding 0.6B. The second list (2026-11-13,
issue #84): Gemma 4 E2B, Gemma 4 E4B, Ministral 3 3B Instruct, and for 32 GB
of memory or a graphics card Gemma 4 26B A4B and Qwen3 30B A3B. Each entry
may give `active_weight_bytes`, the bytes read for each generated token
when that is not the model file (a mixture of experts, Gemma's per-layer
embeddings), from which `tur-ai-local` estimates speed.

How the second list's licences were established is in each entry and in
[local-ai.md](../../../docs/architecture/local-ai.md), section 10. In short:
Gemma 4 is under the Apache License 2.0 alone (its license link redirects to
the Apache text, and Google's Gemma Terms of Use, with their Prohibited Use
Policy, say they cover the older Gemma models and point Gemma 4 to its own
licence); Ministral 3 3B is Apache-2.0 with one sentence its card adds,
shown as a restriction; Qwen3 30B A3B ships the Apache-2.0 text.

## Models considered and left out (2026-11-13)

| Model | Why it is not listed |
|---|---|
| LiquidAI LFM2.5 / LFM3 (GGUF from the publisher) | Licence `lfm1.0` (LFM Open License): not a permissive licence; its terms were not reviewed. It may be listed later with its restrictions shown. |
| Google Gemma 4 12B and 31B (`google/gemma-4-12B-it-qat-q4_0-gguf`, `…-31B-…`) | Apache-2.0 like the rest of Gemma 4. Left out to keep the list short: the 26B A4B mixture of experts answers faster than either on the same memory. Candidates for graphics cards with 12 to 24 GB once a card is measured. |
| Qwen3 8B, 14B, 32B (`Qwen/Qwen3-*-GGUF`) | Apache-2.0. Left out to keep the list short; 30B A3B covers the large machines and 4B the small ones. |
| Ministral 3 8B and 14B, and the Reasoning variants | Apache-2.0 per their cards. Left out to keep the list short; the 3B Instruct is the one asked for. |
| Microsoft Phi (MIT) | No small official GGUF from the publisher was found (`microsoft/phi-4-gguf` is a 14B model). |
| SmolLM2 1.7B Instruct (`HuggingFaceTB/SmolLM2-1.7B-Instruct-GGUF`, Apache-2.0) | Published in 2024 and superseded; no tool-calling claim by the publisher. |
