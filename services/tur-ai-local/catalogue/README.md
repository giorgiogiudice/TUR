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

## Models considered and left out (2026-11-12)

| Model | Why it is not listed |
|---|---|
| LiquidAI LFM2.5 / LFM3 (GGUF from the publisher) | Licence `lfm1.0` (LFM Open License): not a permissive licence; its terms were not reviewed for this slice. It may be listed later with its restrictions shown. |
| Google Gemma 4 E2B (`google/gemma-4-E2B-it-qat-q4_0-gguf`) | Card declares Apache-2.0, but the model file is 3.3 GB plus a 1.0 GB projector: too large for the processor-only first list. Candidate for a second list. |
| Mistral Ministral 3 3B Instruct (`mistralai/Ministral-3-3B-Instruct-2512-GGUF`) | Card declares Apache-2.0; image input and tool calling. Left out only to keep the first CI run short; a candidate for the next list. |
| Microsoft Phi (MIT) | No small official GGUF from the publisher was found (`microsoft/phi-4-gguf` is a 14B model). |
| SmolLM2 1.7B Instruct (`HuggingFaceTB/SmolLM2-1.7B-Instruct-GGUF`, Apache-2.0) | Published in 2024 and superseded; no tool-calling claim by the publisher. |
