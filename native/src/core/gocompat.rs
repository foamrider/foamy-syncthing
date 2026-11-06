//! The few Go standard-library behaviours the Syncshell core relies on and
//! that its snapshots, REST calls and error messages show: `path/filepath`
//! (`Clean`, `Join`, `Base`, `Dir`, `Abs`, `EvalSymlinks`), `net/url`
//! escaping, `strings.TrimSpace`, byte truncation, errno texts, and
//! `encoding/json`'s HTML-safe string escaping.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// `filepath.Clean` for Unix paths.
pub fn clean(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let rooted = path.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|last| *last != "..") {
                    parts.pop();
                } else if !rooted {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    let joined = parts.join("/");
    match (rooted, joined.is_empty()) {
        (true, _) => format!("/{joined}"),
        (false, true) => ".".into(),
        (false, false) => joined,
    }
}

/// `filepath.Join`: the non-empty elements joined with `/`, then cleaned.
pub fn join(parts: &[&str]) -> String {
    let kept: Vec<&str> = parts.iter().copied().filter(|p| !p.is_empty()).collect();
    if kept.is_empty() {
        return String::new();
    }
    clean(&kept.join("/"))
}

/// `filepath.Base`.
pub fn base(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".into();
    }
    trimmed.rsplit('/').next().unwrap_or(trimmed).into()
}

/// `filepath.Dir`.
pub fn dir(path: &str) -> String {
    let index = path.rfind('/').map_or(0, |i| i + 1);
    clean(&path[..index])
}

pub fn is_abs(path: &str) -> bool {
    path.starts_with('/')
}

/// `filepath.Abs`.
pub fn abs(path: &str) -> io::Result<String> {
    if is_abs(path) {
        return Ok(clean(path));
    }
    let cwd = std::env::current_dir()?;
    Ok(join(&[&cwd.to_string_lossy(), path]))
}

/// `strings.TrimSpace` (Unicode white space).
pub fn trim_space(value: &str) -> &str {
    value.trim()
}

/// `value[:max]` on bytes, as Go cuts strings. A cut inside a UTF-8
/// sequence leaves bytes that Go's JSON encoder writes as one U+FFFD each,
/// so the result is that text.
pub fn truncate_bytes(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.into();
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    let mut result = String::with_capacity(max + 3);
    result.push_str(&value[..end]);
    for _ in end..max {
        result.push('\u{FFFD}');
    }
    result
}

/// Go's `syscall.Errno.Error()` text for the errors a path check can meet.
pub fn errno_text(error: &io::Error) -> String {
    use rustix::io::Errno;
    let Some(code) = error.raw_os_error() else {
        return error.to_string();
    };
    let known = [
        (Errno::PERM, "operation not permitted"),
        (Errno::NOENT, "no such file or directory"),
        (Errno::IO, "input/output error"),
        (Errno::NOMEM, "cannot allocate memory"),
        (Errno::ACCESS, "permission denied"),
        (Errno::EXIST, "file exists"),
        (Errno::NOTDIR, "not a directory"),
        (Errno::ISDIR, "is a directory"),
        (Errno::INVAL, "invalid argument"),
        (Errno::NFILE, "too many open files in system"),
        (Errno::MFILE, "too many open files"),
        (Errno::NOSPC, "no space left on device"),
        (Errno::ROFS, "read-only file system"),
        (Errno::NAMETOOLONG, "file name too long"),
        (Errno::LOOP, "too many levels of symbolic links"),
        (Errno::STALE, "stale NFS file handle"),
        (Errno::DQUOT, "disk quota exceeded"),
        (Errno::NODEV, "no such device"),
        (Errno::NXIO, "no such device or address"),
    ];
    known
        .iter()
        .find(|(errno, _)| errno.raw_os_error() == code)
        .map_or_else(|| format!("errno {code}"), |(_, text)| (*text).into())
}

/// Go's `*fs.PathError` text: `"<op> <path>: <errno>"`.
pub fn path_error(op: &str, path: &str, error: &io::Error) -> String {
    format!("{op} {path}: {}", errno_text(error))
}

pub fn lstat(path: &str) -> Result<fs::Metadata, String> {
    fs::symlink_metadata(Path::new(OsStr::from_bytes(path.as_bytes())))
        .map_err(|error| path_error("lstat", path, &error))
}

pub fn stat(path: &str) -> Result<fs::Metadata, String> {
    fs::metadata(path).map_err(|error| path_error("stat", path, &error))
}

/// `filepath.EvalSymlinks` (Go's `walkSymlinks`), with Go's error texts.
pub fn eval_symlinks(input: &str) -> Result<String, String> {
    let mut path = input.to_string();
    let mut vol_len = usize::from(path.starts_with('/'));
    let mut vol = path[..vol_len].to_string();
    let mut dest = vol.clone();
    let mut links_walked = 0;
    let mut start = vol_len;
    let mut end = vol_len;
    while start < path.len() {
        let bytes = path.as_bytes();
        while start < bytes.len() && bytes[start] == b'/' {
            start += 1;
        }
        end = start;
        while end < bytes.len() && bytes[end] != b'/' {
            end += 1;
        }
        if end == start {
            break;
        }
        let component = &path[start..end];
        if component == "." {
            start = end;
            continue;
        }
        if component == ".." {
            let last = dest[vol_len.min(dest.len())..]
                .rfind('/')
                .map(|i| i + vol_len);
            match last {
                Some(r) if &dest[r + 1..] != ".." => dest.truncate(r),
                _ => {
                    if dest.len() > vol_len {
                        dest.push('/');
                    }
                    dest.push_str("..");
                }
            }
            start = end;
            continue;
        }
        if !dest.is_empty() && !dest.ends_with('/') {
            dest.push('/');
        }
        dest.push_str(component);
        let metadata = lstat(&dest)?;
        if !metadata.file_type().is_symlink() {
            if !metadata.is_dir() && end < path.len() {
                return Err("not a directory".into());
            }
            start = end;
            continue;
        }
        links_walked += 1;
        if links_walked > 255 {
            return Err("EvalSymlinks: too many links".into());
        }
        let link = fs::read_link(&dest).map_err(|error| path_error("readlink", &dest, &error))?;
        let link = String::from_utf8_lossy(link.as_os_str().as_bytes()).into_owned();
        path = format!("{link}{}", &path[end..]);
        if link.starts_with('/') {
            dest = "/".into();
            end = 1;
            vol = "/".into();
            vol_len = 1;
        } else {
            match dest[vol_len.min(dest.len())..]
                .rfind('/')
                .map(|i| i + vol_len)
            {
                Some(r) => dest.truncate(r),
                None => dest = vol.clone(),
            }
            end = 0;
        }
        start = end;
    }
    let _ = end;
    Ok(clean(&dest))
}

/// True when one clean absolute path is the other or lies inside it
/// (Go's `filepath.Rel` without a leading `..` in either direction).
pub fn paths_overlap(first: &str, second: &str) -> bool {
    fn parts(path: &str) -> Vec<&str> {
        path.split('/').filter(|p| !p.is_empty()).collect()
    }
    let (a, b) = (parts(first), parts(second));
    a.iter().zip(&b).all(|(x, y)| x == y)
}

/// A regular file after following links (`os.Stat` + `IsRegular`).
pub fn regular_file(path: &str) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file())
}

/// `exec.LookPath(name)` for a bare name: the first absolute `PATH` entry
/// holding an executable non-directory (relative entries are refused, as
/// Go does since 1.19).
pub fn look_path(name: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for entry in path.as_bytes().split(|&b| b == b':') {
        let entry = String::from_utf8_lossy(entry);
        if !entry.starts_with('/') {
            continue;
        }
        let candidate = format!("{}/{name}", entry.trim_end_matches('/'));
        if let Ok(metadata) = fs::metadata(&candidate)
            && !metadata.is_dir()
            && metadata.mode() & 0o111 != 0
        {
            return Some(candidate);
        }
    }
    None
}

fn escape(value: &str, keep: impl Fn(u8) -> bool, space_plus: bool) -> String {
    let mut out = String::with_capacity(value.len());
    for &byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') || keep(byte) {
            out.push(byte as char);
        } else if byte == b' ' && space_plus {
            out.push('+');
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `url.QueryEscape`.
pub fn query_escape(value: &str) -> String {
    escape(value, |_| false, true)
}

/// `url.PathEscape` (one path segment).
pub fn path_escape(value: &str) -> String {
    escape(
        value,
        |b| matches!(b, b'$' | b'&' | b'+' | b':' | b'=' | b'@'),
        false,
    )
}

/// `encoding/json` output: compact, map keys sorted, and strings escaped
/// like Go (`<`, `>`, `&`, U+2028 and U+2029 as `\u…`), so frames match the
/// Go core's byte for byte.
pub fn to_json<T: serde::Serialize + ?Sized>(value: &T) -> serde_json::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(256);
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, GoFormatter);
    value.serialize(&mut serializer)?;
    Ok(out)
}

struct GoFormatter;

impl serde_json::ser::Formatter for GoFormatter {
    fn write_string_fragment<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        fragment: &str,
    ) -> io::Result<()> {
        let mut start = 0;
        for (index, ch) in fragment.char_indices() {
            let escaped = match ch {
                '<' => "\\u003c",
                '>' => "\\u003e",
                '&' => "\\u0026",
                '\u{2028}' => "\\u2028",
                '\u{2029}' => "\\u2029",
                _ => continue,
            };
            writer.write_all(&fragment.as_bytes()[start..index])?;
            writer.write_all(escaped.as_bytes())?;
            start = index + ch.len_utf8();
        }
        writer.write_all(&fragment.as_bytes()[start..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_join_base_dir_match_go() {
        for (input, want) in [
            ("", "."),
            ("/", "/"),
            ("//a//b/./c/..", "/a/b"),
            ("a/../..", ".."),
            ("/../a", "/a"),
            ("a/b/", "a/b"),
            ("./", "."),
            ("../../x", "../../x"),
        ] {
            assert_eq!(clean(input), want, "{input}");
        }
        assert_eq!(join(&["/a", "", "b/../c"]), "/a/c");
        assert_eq!(join(&["", "config.xml"]), "config.xml");
        assert_eq!(join(&["", ""]), "");
        assert_eq!(base("/a/b/"), "b");
        assert_eq!(base("///"), "/");
        assert_eq!(base(""), ".");
        assert_eq!(base("plan.md"), "plan.md");
        assert_eq!(dir("/a/b/c.xml"), "/a/b");
        assert_eq!(dir("c.xml"), ".");
        assert_eq!(dir("/c.xml"), "/");
    }

    #[test]
    fn truncation_cuts_bytes_like_go() {
        assert_eq!(truncate_bytes("abc", 5), "abc");
        assert_eq!(truncate_bytes("abcdef", 3), "abc");
        // "æ" is two bytes: a cut after the first leaves one invalid byte.
        assert_eq!(truncate_bytes("aæ", 2), "a\u{FFFD}");
        assert_eq!(truncate_bytes("a€", 3), "a\u{FFFD}\u{FFFD}");
    }

    #[test]
    fn escaping_matches_net_url() {
        assert_eq!(query_escape("a b/c,d&é"), "a+b%2Fc%2Cd%26%C3%A9");
        assert_eq!(path_escape("a b/c;d$&+:=@?!"), "a%20b%2Fc%3Bd$&+:=@%3F%21");
    }

    #[test]
    fn json_is_html_safe_like_go() {
        let value = serde_json::json!({"b": "<a & b>\u{2028}", "a": [1, "x\n\u{1}"]});
        assert_eq!(
            String::from_utf8(to_json(&value).unwrap()).unwrap(),
            r#"{"a":[1,"x\n\u0001"],"b":"\u003ca \u0026 b\u003e\u2028"}"#
        );
    }

    #[test]
    fn overlap_is_component_wise() {
        assert!(paths_overlap("/a/b", "/a/b"));
        assert!(paths_overlap("/a", "/a/b"));
        assert!(paths_overlap("/a/b", "/a"));
        assert!(paths_overlap("/", "/x"));
        assert!(!paths_overlap("/a/b", "/a/bc"));
        assert!(!paths_overlap("/a/b", "/a/c"));
    }

    #[test]
    fn eval_symlinks_resolves_like_go() {
        let base = std::env::temp_dir().join(format!("foamy-eval-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("real/inner")).unwrap();
        std::os::unix::fs::symlink("real", base.join("rel")).unwrap();
        std::os::unix::fs::symlink(base.join("real/inner"), base.join("abs")).unwrap();
        std::os::unix::fs::symlink("missing", base.join("dangling")).unwrap();
        fs::write(base.join("file"), b"").unwrap();
        let root = fs::canonicalize(&base)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            eval_symlinks(&format!("{root}/rel/inner")).unwrap(),
            format!("{root}/real/inner")
        );
        assert_eq!(
            eval_symlinks(&format!("{root}/abs/..")).unwrap(),
            format!("{root}/real")
        );
        assert_eq!(
            eval_symlinks(&format!("{root}/./real//inner/")).unwrap(),
            format!("{root}/real/inner")
        );
        assert_eq!(
            eval_symlinks(&format!("{root}/dangling")).unwrap_err(),
            format!("lstat {root}/missing: no such file or directory")
        );
        assert_eq!(
            eval_symlinks(&format!("{root}/file/x")).unwrap_err(),
            "not a directory"
        );
        fs::remove_dir_all(&base).unwrap();
    }
}
