// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The URL rules of the catalogue.
//!
//! Every URL in an entry is a link a person follows: a source that was
//! checked, a publisher's download page, a web application. The catalogue
//! accepts `https` only, and never a link that is itself a program or an
//! archive, because TUR must not fetch a binary the user did not ask for.

/// The longest URL an entry may hold, in bytes.
pub const MAX_URL_BYTES: usize = 2048;

/// File endings that mark a URL as a direct download rather than a page.
const DOWNLOAD_ENDINGS: [&str; 27] = [
    ".exe",
    ".msi",
    ".msix",
    ".msixbundle",
    ".appx",
    ".zip",
    ".7z",
    ".rar",
    ".dmg",
    ".pkg",
    ".apk",
    ".xapk",
    ".apks",
    ".aab",
    ".appimage",
    ".deb",
    ".rpm",
    ".flatpak",
    ".tar",
    ".gz",
    ".tgz",
    ".xz",
    ".bz2",
    ".zst",
    ".iso",
    ".bin",
    ".run",
];

/// Checks that `text` is a well-formed `https` URL.
///
/// The rules are deliberately narrower than the URL standard: the scheme is
/// `https` in lower case; the host is a lower-case DNS name with at least one
/// dot, optionally followed by a port; there is no user name or password; and
/// the rest is printable ASCII without spaces, quotes, angle brackets,
/// braces, backslashes, carets, vertical bars or backticks.
///
/// The error is a short reason suitable for the end of a sentence.
pub fn check_https_url(text: &str) -> Result<(), &'static str> {
    if text.len() > MAX_URL_BYTES {
        return Err("it is longer than a URL may be");
    }
    let Some(rest) = text.strip_prefix("https://") else {
        return Err("it does not start with https://");
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    if authority.contains('@') {
        return Err("it holds a user name or password");
    }
    let (host, port) = match authority.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    if let Some(port) = port {
        let valid = !port.is_empty() && port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit());
        if !valid {
            return Err("its port is not a number");
        }
    }
    if host.is_empty() || host.len() > 253 {
        return Err("it has no host name");
    }
    if !host.contains('.') {
        return Err("its host name has no dot");
    }
    for label in host.split('.') {
        let valid = !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !valid {
            return Err("its host name is not a lower-case DNS name");
        }
    }
    let tail_valid = tail.bytes().all(|b| {
        b.is_ascii_graphic()
            && !matches!(
                b,
                b'"' | b'<' | b'>' | b'\\' | b'`' | b'^' | b'{' | b'|' | b'}'
            )
    });
    if !tail_valid {
        return Err("it holds a space, a quote or a character outside printable ASCII");
    }
    Ok(())
}

/// Whether a URL points at a program, package or archive instead of a page.
///
/// The test looks at the path only, ignoring the query and the fragment, and
/// compares its ending with a list of download file endings, in any case.
pub fn is_direct_download(url: &str) -> bool {
    let without_scheme = url.strip_prefix("https://").unwrap_or(url);
    let path_start = without_scheme.find('/').unwrap_or(without_scheme.len());
    let path = &without_scheme[path_start..];
    let path_end = path.find(['?', '#']).unwrap_or(path.len());
    let path = path[..path_end].to_ascii_lowercase();
    DOWNLOAD_ENDINGS
        .iter()
        .any(|ending| path.ends_with(*ending))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_https_urls() {
        for url in [
            "https://www.protondb.com/app/1245620",
            "https://appdb.winehq.org/objectManager.php?sClass=application&iId=17",
            "https://example.org",
            "https://example.org/",
            "https://example.org:8443/a/b?c=d#e",
            "https://a-b.example.co.uk/path_(x)/~y",
        ] {
            assert_eq!(check_https_url(url), Ok(()), "{url}");
        }
    }

    #[test]
    fn rejects_other_schemes_and_malformed_urls() {
        for url in [
            "http://example.org/",
            "HTTPS://example.org/",
            "ftp://example.org/",
            "example.org",
            "https://",
            "https:///path",
            "https://localhost/",
            "https://user@example.org/",
            "https://user:secret@example.org/",
            "https://example.org:port/",
            "https://example.org:/",
            "https://Example.org/",
            "https://exa mple.org/",
            "https://example..org/",
            "https://-example.org/",
            "https://example.org/a b",
            "https://example.org/\"x\"",
            "https://example.org/<x>",
            "https://example.org/{x}",
            "https://example.org/a|b",
            "https://example.org/caf\u{e9}",
            "https://example.org/a\nb",
            " https://example.org/",
        ] {
            assert!(check_https_url(url).is_err(), "{url}");
        }
        let long = format!("https://example.org/{}", "a".repeat(MAX_URL_BYTES));
        assert!(check_https_url(&long).is_err());
    }

    #[test]
    fn recognises_direct_downloads_by_the_path_ending() {
        for url in [
            "https://example.org/setup.exe",
            "https://example.org/Setup.EXE",
            "https://example.org/app.msi?source=x",
            "https://example.org/app.dmg#top",
            "https://example.org/app.apk",
            "https://example.org/app.AppImage",
            "https://example.org/app.tar.gz",
            "https://example.org/app.deb",
        ] {
            assert!(is_direct_download(url), "{url}");
        }
        for url in [
            "https://example.org/download",
            "https://example.org/download/",
            "https://example.org/download.html",
            "https://example.org/download?file=setup.exe",
            "https://example.org",
            "https://downloads.example.org/windows",
        ] {
            assert!(!is_direct_download(url), "{url}");
        }
    }
}
