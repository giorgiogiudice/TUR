// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Matching a device's `modalias` against the kernel's own module alias table.
//!
//! TUR does not keep a driver database of its own (specification §10.2). To
//! say whether the running kernel has a driver for a device, the device's
//! `modalias` string is matched against `modules.alias`, the table `depmod`
//! generates from the modules of that kernel, the same table `modprobe` uses.
//!
//! The table lists loadable modules only. A driver built into the kernel is
//! not in it, so "no module matches" means "no loadable module claims this
//! device", not "no driver exists".

use std::collections::BTreeSet;

use crate::text::kernel_name;

/// The most alias lines kept from one table.
pub const MAX_ALIASES: usize = 400_000;

/// The longest alias pattern kept, in bytes.
pub const MAX_PATTERN_BYTES: usize = 512;

/// The most modules reported for one device.
pub const MAX_MATCHES: usize = 16;

/// The alias table of one kernel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleAliases {
    entries: Vec<(Vec<u8>, String)>,
}

impl ModuleAliases {
    /// Parse `modules.alias`: lines of the form `alias <pattern> <module>`.
    /// Other lines, and lines that are not understood, are skipped.
    pub fn parse(raw: &[u8]) -> Self {
        let mut entries = Vec::new();
        for line in raw.split(|byte| *byte == b'\n') {
            if entries.len() >= MAX_ALIASES {
                break;
            }
            let Ok(line) = std::str::from_utf8(line) else {
                continue;
            };
            let mut words = line.split_ascii_whitespace();
            let (Some("alias"), Some(pattern), Some(module), None) =
                (words.next(), words.next(), words.next(), words.next())
            else {
                continue;
            };
            if pattern.len() > MAX_PATTERN_BYTES {
                continue;
            }
            let Some(module) = kernel_name(module) else {
                continue;
            };
            entries.push((pattern.as_bytes().to_vec(), module.replace('-', "_")));
        }
        Self { entries }
    }

    /// The number of aliases.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The modules whose aliases match a modalias string, sorted, without
    /// repeats, at most [`MAX_MATCHES`] of them.
    pub fn matching(&self, modalias: &str) -> Vec<String> {
        let text = modalias.as_bytes();
        let mut modules: BTreeSet<&str> = BTreeSet::new();
        for (pattern, module) in &self.entries {
            if !modules.contains(module.as_str()) && glob_match(pattern, text) {
                modules.insert(module);
            }
        }
        modules
            .into_iter()
            .take(MAX_MATCHES)
            .map(str::to_owned)
            .collect()
    }
}

enum Class {
    /// The class matched; the pattern continues at this index.
    Matched(usize),
    /// The class is well formed and did not match.
    NotMatched,
    /// There is no closing bracket: the `[` is an ordinary character.
    Literal,
}

/// Match one byte against the bracket expression that starts at
/// `pattern[start]`.
fn class_match(pattern: &[u8], start: usize, byte: u8) -> Class {
    let mut index = start + 1;
    let negate = matches!(pattern.get(index), Some(b'!' | b'^'));
    if negate {
        index += 1;
    }
    let mut matched = false;
    let mut first = true;
    loop {
        let Some(&low) = pattern.get(index) else {
            return Class::Literal;
        };
        if low == b']' && !first {
            break;
        }
        first = false;
        let high = match (pattern.get(index + 1), pattern.get(index + 2)) {
            (Some(b'-'), Some(&high)) if high != b']' => Some(high),
            _ => None,
        };
        if let Some(high) = high {
            matched |= (low..=high).contains(&byte);
            index += 3;
        } else {
            matched |= low == byte;
            index += 1;
        }
    }
    if matched == negate {
        Class::NotMatched
    } else {
        Class::Matched(index + 1)
    }
}

/// Shell-style pattern matching as `fnmatch(3)` does it without flags: `*`
/// matches any run of bytes, `?` any one byte, `[...]` a set with ranges and
/// `!` or `^` negation. Everything else matches itself.
///
/// The match runs in time proportional to the product of the two lengths at
/// worst and uses no recursion, so a hostile modalias cannot make it run away.
pub fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
    let mut p = 0_usize;
    let mut t = 0_usize;
    // Where to resume after the most recent `*`: (pattern index after the
    // star, text index it is currently assumed to cover up to).
    let mut resume: Option<(usize, usize)> = None;
    while let Some(&byte) = text.get(t) {
        let step = match pattern.get(p) {
            Some(b'*') => {
                resume = Some((p + 1, t));
                p += 1;
                continue;
            }
            Some(b'?') => Some(p + 1),
            Some(b'[') => match class_match(pattern, p, byte) {
                Class::Matched(next) => Some(next),
                Class::NotMatched => None,
                Class::Literal => (byte == b'[').then_some(p + 1),
            },
            Some(&literal) => (literal == byte).then_some(p + 1),
            None => None,
        };
        match (step, resume) {
            (Some(next), _) => {
                p = next;
                t += 1;
            }
            (None, Some((after_star, covered))) => {
                p = after_star;
                t = covered + 1;
                resume = Some((after_star, covered + 1));
            }
            (None, None) => return false,
        }
    }
    pattern
        .get(p..)
        .is_some_and(|rest| rest.iter().all(|byte| *byte == b'*'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glob(pattern: &str, text: &str) -> bool {
        glob_match(pattern.as_bytes(), text.as_bytes())
    }

    #[test]
    fn glob_matches_literals_and_wildcards() {
        assert!(glob("abc", "abc"));
        assert!(!glob("abc", "abd"));
        assert!(!glob("abc", "ab"));
        assert!(!glob("ab", "abc"));
        assert!(glob("", ""));
        assert!(!glob("", "a"));
        assert!(glob("*", ""));
        assert!(glob("*", "anything"));
        assert!(glob("a*c", "abbbc"));
        assert!(glob("a*c", "ac"));
        assert!(!glob("a*c", "abbbd"));
        assert!(glob("a?c", "abc"));
        assert!(!glob("a?c", "ac"));
        assert!(glob("*a*b*c*", "xxaxxbxxcxx"));
        assert!(!glob("*a*b*c*", "xxaxxcxxbxx"));
        assert!(glob("a**", "a"));
        assert!(glob("a*b*", "aXbXb"));
        assert!(glob("*ab", "aab"));
    }

    #[test]
    fn glob_matches_bracket_expressions() {
        assert!(glob("v[0-9A-F]x", "vBx"));
        assert!(!glob("v[0-9A-F]x", "vGx"));
        assert!(glob("[abc]", "b"));
        assert!(!glob("[abc]", "d"));
        assert!(glob("[!abc]", "d"));
        assert!(!glob("[!abc]", "a"));
        assert!(glob("[^abc]", "d"));
        assert!(glob("[]]", "]"));
        assert!(glob("[a-]", "-"));
        assert!(glob("*[0-9]", "abc7"));
        assert!(!glob("*[0-9]", "abc"));
        // An unclosed bracket is an ordinary character.
        assert!(glob("a[b", "a[b"));
        assert!(!glob("a[b", "ab"));
    }

    #[test]
    fn glob_matches_real_aliases() {
        let e1000e = "pci:v00008086d000015FCsv*sd*bc*sc*i*";
        assert!(glob(
            e1000e,
            "pci:v00008086d000015FCsv000017AAsd000022D5bc02sc00i00"
        ));
        assert!(!glob(
            e1000e,
            "pci:v00008086d000015FBsv000017AAsd000022D5bc02sc00i00"
        ));
        let hid = "usb:v*p*d*dc*dsc*dp*ic03isc*ip*in*";
        assert!(glob(
            hid,
            "usb:v046DpC52Bd1211dc00dsc00dp00ic03isc01ip01in00"
        ));
        assert!(!glob(
            hid,
            "usb:v046DpC52Bd1211dc00dsc00dp00ic08isc06ip50in00"
        ));
    }

    #[test]
    fn glob_is_bounded_on_hostile_input() {
        // The classic exponential case for a naive matcher.
        let pattern = "*a".repeat(200);
        let text = "a".repeat(4000);
        assert!(glob(&pattern, &text));
        let text = format!("{}b", "a".repeat(4000));
        assert!(!glob(&pattern, &text));
    }

    const TABLE: &str = "\
# Aliases extracted from modules themselves.
alias pci:v00008086d000015FCsv*sd*bc*sc*i* e1000e
alias pci:v000010DEd*sv*sd*bc03sc*i* nouveau
alias pci:v000010DEd*sv*sd*bc03sc00i00* nvidiafb
alias usb:v*p*d*dc*dsc*dp*ic03isc*ip*in* usbhid
alias usb:v0BDAp8153d*dc*dsc*dp*ic*isc*ip*in* r8152
alias usb:v0BDAp8153d*dc*dsc*dp*icFFiscFFip00in* r8152
alias pci:v000010ECd0000522Asv*sd*bc*sc*i* rtsx-pci
alias toofew
alias too many words here
alias pci:* bad/module
softdep nouveau pre: drm
";

    #[test]
    fn parses_the_alias_table() {
        let table = ModuleAliases::parse(TABLE.as_bytes());
        assert_eq!(table.len(), 7);
        assert!(ModuleAliases::parse(b"").is_empty());
        assert!(ModuleAliases::parse(b"\xff\xfe alias a b\n").is_empty());
    }

    #[test]
    fn finds_matching_modules() {
        let table = ModuleAliases::parse(TABLE.as_bytes());
        assert_eq!(
            table.matching("pci:v00008086d000015FCsv000017AAsd000022D5bc02sc00i00"),
            ["e1000e"]
        );
        // Sorted, and a module that matches twice is listed once.
        assert_eq!(
            table.matching("pci:v000010DEd000025A2sv000017AAsd000022D5bc03sc00i00"),
            ["nouveau", "nvidiafb"]
        );
        assert_eq!(
            table.matching("usb:v0BDAp8153d3100dc00dsc00dp00icFFiscFFip00in00"),
            ["r8152"]
        );
        // Module names use `_`, as in /sys/module.
        assert_eq!(
            table.matching("pci:v000010ECd0000522Asv000017AAsd000022D5bcFFsc00i00"),
            ["rtsx_pci"]
        );
        assert!(
            table
                .matching("pci:v00001234d00005678sv0sd0bc07sc80i00")
                .is_empty()
        );
        assert!(table.matching("").is_empty());
    }

    #[test]
    fn bounds_the_number_of_matches() {
        let mut table = String::new();
        for index in 0..100 {
            table.push_str("alias pci:* module");
            table.push_str(&index.to_string());
            table.push('\n');
        }
        let table = ModuleAliases::parse(table.as_bytes());
        assert_eq!(table.matching("pci:anything").len(), MAX_MATCHES);
    }
}
