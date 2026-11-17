//! Read and write the STUN server entry of an frpc configuration file.
//!
//! The file is handled as plain text so that comments, ordering and every
//! proxy section the user cares about survive a change untouched. Two dialects
//! are supported:
//!
//! * TOML (`frpc.toml`) — the root-level key `natHoleStunServer`.
//! * Legacy INI (`frpc.ini`) — `nat_hole_stun_server` inside `[common]`.

use std::fs;
use std::path::Path;

pub const TOML_KEY: &str = "natHoleStunServer";
pub const INI_KEY: &str = "nat_hole_stun_server";
pub const INI_SECTION: &str = "common";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Toml,
    Ini,
}

fn format_of(path: &Path) -> Format {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase());
    match ext.as_deref() {
        Some("ini") => Format::Ini,
        _ => Format::Toml,
    }
}

/// The STUN server currently configured in `path`, if the key is active
/// (commented-out placeholders are ignored, they are not in effect).
pub fn read_stun_server(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let doc = Doc::new(&text, format_of(path));
    let idx = doc.find_key(KeyFilter::Active)?;
    let raw = doc.lines.get(idx)?.split_once('=')?.1.trim();
    Some(unquote(raw))
}

/// Write (or replace) the STUN server entry. Returns `true` when an existing
/// line was updated instead of a new one being inserted.
///
/// The line that is currently **in effect** always wins over a commented-out
/// placeholder: many configs keep a commented list of candidate servers, and
/// filling one of those in while another line is active would define the key
/// twice, which TOML (and frp) rejects. Any second active line left over from
/// an older edit is commented out, so the file ends up with exactly one.
pub fn set_stun_server(path: &Path, value: &str) -> Result<bool, String> {
    let text = read_text(path)?;
    let mut doc = Doc::new(&text, format_of(path));
    let replaced = doc.set(value);
    write_text(path, &doc.render())?;
    Ok(replaced)
}

/// Comment the STUN entry out, so frp falls back to its own default.
/// Returns `false` when there was nothing active to comment out.
pub fn clear_stun_server(path: &Path) -> Result<bool, String> {
    let text = read_text(path)?;
    let mut doc = Doc::new(&text, format_of(path));
    let changed = doc.clear();
    if changed {
        write_text(path, &doc.render())?;
    }
    Ok(changed)
}

/// Collapse a config that defines the STUN key more than once, which makes frp
/// fail with `key natHoleStunServer is already defined`. The first active line
/// is kept (frp's own error points at the later one), the rest are commented
/// out. Returns how many duplicate lines were disabled.
pub fn repair_stun_duplicates(path: &Path) -> Result<usize, String> {
    let text = read_text(path)?;
    let mut doc = Doc::new(&text, format_of(path));
    let active = doc.find_keys(KeyFilter::Active);
    if active.len() < 2 {
        return Ok(0);
    }
    for &index in &active[1..] {
        doc.comment_out(index);
    }
    write_text(path, &doc.render())?;
    Ok(active.len() - 1)
}

fn read_text(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("读取 {} 失败: {}", path.display(), e))
}

fn write_text(path: &Path, text: &str) -> Result<(), String> {
    fs::write(path, text).map_err(|e| format!("写入 {} 失败: {}", path.display(), e))
}

fn unquote(raw: &str) -> String {
    let trimmed = raw.trim();
    let trimmed = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
        })
        .unwrap_or(trimmed);
    trimmed.trim().to_string()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum KeyFilter {
    /// Only lines that are not commented out.
    Active,
    /// Active lines and commented-out placeholders.
    Any,
}

struct Doc {
    lines: Vec<String>,
    newline: &'static str,
    format: Format,
}

impl Doc {
    fn new(text: &str, format: Format) -> Self {
        let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let lines = text
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
            .collect();
        Self {
            lines,
            newline,
            format,
        }
    }

    fn render(&self) -> String {
        let mut out = self.lines.join(self.newline);
        if !out.is_empty() && !out.ends_with(self.newline) {
            out.push_str(self.newline);
        }
        out
    }

    fn key_name(&self) -> &'static str {
        match self.format {
            Format::Toml => TOML_KEY,
            Format::Ini => INI_KEY,
        }
    }

    fn key_line(&self, value: &str) -> String {
        match self.format {
            Format::Toml => format!("{} = \"{}\"", TOML_KEY, value),
            Format::Ini => format!("{} = {}", INI_KEY, value),
        }
    }

    /// Index of `[common]` in an INI file plus one past the end of that
    /// section.
    fn ini_common_range(&self) -> Option<(usize, usize)> {
        let header = format!("[{}]", INI_SECTION);
        let start = self
            .lines
            .iter()
            .position(|l| l.trim().eq_ignore_ascii_case(&header))?;
        let end = self
            .lines
            .iter()
            .enumerate()
            .skip(start + 1)
            .find(|(_, l)| l.trim_start().starts_with('['))
            .map(|(i, _)| i)
            .unwrap_or(self.lines.len());
        Some((start, end))
    }

    /// Every line that defines the key, in file order.
    fn find_keys(&self, filter: KeyFilter) -> Vec<usize> {
        let key = self.key_name();
        let mut found = Vec::new();
        match self.format {
            Format::Toml => {
                for (i, line) in self.lines.iter().enumerate() {
                    let trimmed = line.trim();
                    // Root keys end at the first table header; a same-named key
                    // inside a table is not the one frp reads.
                    if trimmed.starts_with('[') {
                        break;
                    }
                    if key_matches(trimmed, key, filter).is_some() {
                        found.push(i);
                    }
                }
            }
            Format::Ini => {
                if let Some((start, end)) = self.ini_common_range() {
                    for (i, line) in self.lines.iter().enumerate().take(end).skip(start + 1) {
                        if key_matches(line.trim(), key, filter).is_some() {
                            found.push(i);
                        }
                    }
                }
            }
        }
        found
    }

    fn find_key(&self, filter: KeyFilter) -> Option<usize> {
        self.find_keys(filter).into_iter().next()
    }

    /// Insert or replace the entry, returning `true` on replacement.
    ///
    /// Precedence matters: the line that is in effect is reused before any
    /// commented placeholder, otherwise a config with a commented candidate
    /// list would end up defining the key twice.
    fn set(&mut self, value: &str) -> bool {
        let active = self.find_keys(KeyFilter::Active);
        if let Some(&keep) = active.first() {
            self.lines[keep] = self.key_line(value);
            for &index in &active[1..] {
                self.comment_out(index);
            }
            return true;
        }
        // No line is in effect: fill in an existing placeholder if there is
        // one, so the value lands where the user already wrote the key.
        if let Some(i) = self.find_key(KeyFilter::Any) {
            self.lines[i] = self.key_line(value);
            return true;
        }
        let line = self.key_line(value);
        match self.format {
            Format::Toml => {
                let mut pos = self
                    .lines
                    .iter()
                    .position(|l| l.trim_start().starts_with('['))
                    .unwrap_or(self.lines.len());
                while pos > 0 && self.lines[pos - 1].trim().is_empty() {
                    pos -= 1;
                }
                self.lines.insert(pos, line);
                self.ensure_blank_gap(pos);
            }
            Format::Ini => match self.ini_common_range() {
                Some((_, end)) => {
                    let mut pos = end;
                    while pos > 0 && self.lines[pos - 1].trim().is_empty() {
                        pos -= 1;
                    }
                    self.lines.insert(pos, line);
                    self.ensure_blank_gap(pos);
                }
                None => {
                    self.lines.insert(0, format!("[{}]", INI_SECTION));
                    self.lines.insert(1, line);
                    self.ensure_blank_gap(1);
                }
            },
        }
        false
    }

    /// Keep a blank line between the inserted key and whatever follows it.
    fn ensure_blank_gap(&mut self, pos: usize) {
        if pos + 1 < self.lines.len() && !self.lines[pos + 1].trim().is_empty() {
            self.lines.insert(pos + 1, String::new());
        }
    }

    fn comment_out(&mut self, index: usize) {
        let trimmed = self.lines[index].trim().to_string();
        self.lines[index] = format!("# {}", trimmed);
    }

    fn clear(&mut self) -> bool {
        let active = self.find_keys(KeyFilter::Active);
        if active.is_empty() {
            return false;
        }
        for index in active {
            self.comment_out(index);
        }
        true
    }
}

/// `Some(is_commented)` when `line` defines the given key.
fn key_matches(line: &str, key: &str, filter: KeyFilter) -> Option<bool> {
    let (body, commented) = match line.trim_start().strip_prefix('#') {
        Some(rest) => (rest.trim_start(), true),
        None => (line.trim_start(), false),
    };
    if commented && filter == KeyFilter::Active {
        return None;
    }
    let (name, _value) = body.split_once('=')?;
    // TOML also allows quoted keys, e.g. "natHoleStunServer" = "...".
    let name = name.trim().trim_matches('"').trim_matches('\'');
    if name.eq_ignore_ascii_case(key) {
        Some(commented)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_file(name: &str, body: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("frpc-tray-conf-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn toml_reads_and_writes_root_key() {
        let path = temp_file(
            "frpc.toml",
            "serverAddr = \"1.2.3.4\"\nserverPort = 7000\n\n[[proxies]]\nname = \"ssh\"\n",
        );
        assert_eq!(read_stun_server(&path), None);

        assert!(!set_stun_server(&path, "stun.l.google.com:19302").unwrap());
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("natHoleStunServer = \"stun.l.google.com:19302\""));
        assert_eq!(
            read_stun_server(&path).as_deref(),
            Some("stun.l.google.com:19302")
        );

        assert!(set_stun_server(&path, "stun.nextcloud.com:443").unwrap());
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("natHoleStunServer").count(), 1);
        assert_eq!(
            read_stun_server(&path).as_deref(),
            Some("stun.nextcloud.com:443")
        );

        // The key must stay at root level, before the first table.
        let key_pos = text.find("natHoleStunServer").unwrap();
        let table_pos = text.find("[[proxies]]").unwrap();
        assert!(key_pos < table_pos);
    }

    #[test]
    fn toml_fills_in_commented_placeholder() {
        let path = temp_file(
            "frpc_placeholder.toml",
            "serverAddr = \"1.2.3.4\"\n# natHoleStunServer = \"stun.easyvoip.com:3478\"\n",
        );
        assert_eq!(read_stun_server(&path), None);

        assert!(set_stun_server(&path, "stun.miwifi.com:3478").unwrap());
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("natHoleStunServer").count(), 1);
        assert!(!text.contains("# natHoleStunServer"));
        assert_eq!(
            read_stun_server(&path).as_deref(),
            Some("stun.miwifi.com:3478")
        );
    }

    #[test]
    fn ini_writes_into_common_section() {
        let path = temp_file(
            "frpc.ini",
            "[common]\nserver_addr = 1.2.3.4\nserver_port = 7000\n\n[ssh]\ntype = tcp\n",
        );
        assert!(!set_stun_server(&path, "stun.hitv.com:3478").unwrap());

        let text = fs::read_to_string(&path).unwrap();
        let key_pos = text.find("nat_hole_stun_server").unwrap();
        let ssh_pos = text.find("[ssh]").unwrap();
        assert!(key_pos < ssh_pos);
        assert_eq!(
            read_stun_server(&path).as_deref(),
            Some("stun.hitv.com:3478")
        );

        assert!(clear_stun_server(&path).unwrap());
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("# nat_hole_stun_server = stun.hitv.com:3478"));
        assert_eq!(read_stun_server(&path), None);
        // Nothing left to comment out the second time.
        assert!(!clear_stun_server(&path).unwrap());
    }

    #[test]
    fn ini_without_common_section_gets_one() {
        let path = temp_file("frpc_bare.ini", "[ssh]\ntype = tcp\n");
        set_stun_server(&path, "stun.gmx.net:3478").unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("[common]\n"));
        assert_eq!(
            read_stun_server(&path).as_deref(),
            Some("stun.gmx.net:3478")
        );
    }

    #[test]
    fn crlf_line_endings_are_preserved() {
        let path = temp_file("frpc_crlf.toml", "serverAddr = \"1.2.3.4\"\r\n");
        set_stun_server(&path, "stun.l.google.com:19302").unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\r\n"));
        assert!(!text.contains("\n\n"));
        assert_eq!(
            read_stun_server(&path).as_deref(),
            Some("stun.l.google.com:19302")
        );
    }

    /// Regression: a config that keeps a commented candidate list *and* one
    /// active entry. Picking a server whose name only appears as a comment used
    /// to fill that comment in, leaving two active keys and breaking frp with
    /// `key natHoleStunServer is already defined`.
    const CANDIDATE_LIST_TOML: &str = "\
serverAddr = \"43.172.97.199\"
serverPort = 7100
auth.method = \"token\"
# natHoleStunServer = \"stun.hitv.com:3478\"
# natHoleStunServer = \"stun.cheapvoip.com:3478\"
natHoleStunServer = \"stun.miwifi.com:3478\"
# natHoleStunServer = \"stun.chat.bilibili.com:3478\"
# natHoleStunServer = \"stun.douyucdn.cn:18000\"

[[visitors]]
name = \"rdp-client\"
type = \"xtcp\"
";

    #[test]
    fn active_entry_wins_over_commented_candidate() {
        let path = temp_file("frpc_candidates.toml", CANDIDATE_LIST_TOML);
        assert!(set_stun_server(&path, "stun.hitv.com:3478").unwrap());

        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("\nnatHoleStunServer").count(), 1, "{text}");
        assert!(text.contains("natHoleStunServer = \"stun.hitv.com:3478\""));
        assert_eq!(
            read_stun_server(&path).as_deref(),
            Some("stun.hitv.com:3478")
        );
        // The untouched candidates stay commented and stay in place.
        assert!(text.contains("# natHoleStunServer = \"stun.cheapvoip.com:3478\""));
        assert!(text.contains("# natHoleStunServer = \"stun.douyucdn.cn:18000\""));
    }

    #[test]
    fn existing_duplicates_are_collapsed_on_write() {
        let broken = CANDIDATE_LIST_TOML.replace(
            "# natHoleStunServer = \"stun.hitv.com:3478\"",
            "natHoleStunServer = \"stun.hitv.com:3478\"",
        );
        let path = temp_file("frpc_broken.toml", &broken);

        // The repair path alone fixes a file produced by older builds.
        assert_eq!(repair_stun_duplicates(&path).unwrap(), 1);
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("\nnatHoleStunServer").count(), 1, "{text}");
        // frp reads the first one, so that is the value that survives.
        assert_eq!(
            read_stun_server(&path).as_deref(),
            Some("stun.hitv.com:3478")
        );

        // Writing again keeps exactly one active line.
        set_stun_server(&path, "stun.nextcloud.com:443").unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("\nnatHoleStunServer").count(), 1, "{text}");
        assert_eq!(
            read_stun_server(&path).as_deref(),
            Some("stun.nextcloud.com:443")
        );
        assert_eq!(repair_stun_duplicates(&path).unwrap(), 0);
    }

    #[test]
    fn ini_duplicates_are_collapsed() {
        let path = temp_file(
            "frpc_dup.ini",
            "[common]\nserver_addr = 1.2.3.4\nnat_hole_stun_server = a.example:3478\n\
             nat_hole_stun_server = b.example:3478\n\n[ssh]\ntype = tcp\n",
        );
        assert_eq!(repair_stun_duplicates(&path).unwrap(), 1);
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("\nnat_hole_stun_server").count(), 1, "{text}");
        assert_eq!(read_stun_server(&path).as_deref(), Some("a.example:3478"));

        // 恢复默认 removes every active copy.
        assert!(clear_stun_server(&path).unwrap());
        assert_eq!(read_stun_server(&path), None);
    }
}
