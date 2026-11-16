# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Census of docs/reports/exp-macfw-0.md, section 3.1: which architectures
# the downloads of Homebrew's casks cover. Reads metadata only; no
# application is downloaded.
#
#   curl -sS --compressed -o cask.json https://formulae.brew.sh/api/cask.json
#   jq -r -f cask-arch.jq cask.json > census.tsv
#   awk -F'\t' '$2 == "app"' census.tsv | cut -f3 | sort | uniq -c
#
# Output: token, app|other (does the cask install an .app), class,
# auto_updates. The API's default values are the arm64 ones; the Intel
# values are in `variations` under macOS names without an arm64_ prefix.
def has_app: [.artifacts[]? | keys[]] | any(. == "app");
def intel_keys: ["golden_gate","tahoe","sequoia","sonoma","ventura","monterey","big_sur"];
def intel_url:
  . as $c
  | [intel_keys[] as $k | ($c.variations[$k].url // empty)] | first // $c.url;
def arch_dep: [.depends_on.arch[]?.type] | first // "";
.[]
| select((.deprecated or .disabled) | not)
| . as $c
| (if arch_dep == "arm" then "arm64-only (depends_on arch)"
   elif arch_dep == "intel" then "x86_64-only (depends_on arch)"
   elif .caveats_rosetta == true then "x86_64-only (requires Rosetta)"
   elif (.supported_platforms // null) != null
        and ([.supported_platforms[] | select(startswith("arm64") or endswith("linux") | not)] | length) == 0
        then "arm64-only (supported_platforms)"
   elif ($c | intel_url) != $c.url then "separate downloads per architecture"
   else "one download for both (universal, inferred)" end) as $class
| [.token, (if has_app then "app" else "other" end), $class, (.auto_updates // false | tostring)]
| @tsv
