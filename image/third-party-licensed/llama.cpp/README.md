# llama.cpp: a licence text it needs (Apache-2.0, not the TUR licence)

`Apache-2.0.txt` is the Apache License, Version 2.0, as the Apache Software
Foundation publishes it (<https://www.apache.org/licenses/LICENSE-2.0.txt>,
SHA-256 `cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30`).
It is **not** under the TUR licence and is not TUR's work.

It is here because the engine of TUR's local AI, llama.cpp's `llama-server`,
which the image build compiles from llama.cpp's source
([`../../engine/`](../../engine/); `image/README.md`, "The local AI engine"),
contains a few lines that nlohmann/json, which llama.cpp bundles, took from
Google's Abseil under the Apache License 2.0 (`vendor/nlohmann/json.hpp`,
"START OF CODE FROM GOOGLE ABSEIL"). The licence asks that a copy of its text
go with the program. llama.cpp's source does not carry one, so the image build
installs this file beside llama.cpp's own licence files:

| Source | Installed as |
|---|---|
| `Apache-2.0.txt` | `/usr/share/licenses/llama.cpp/Apache-2.0.txt` |
