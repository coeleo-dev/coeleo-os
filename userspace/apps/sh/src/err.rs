use libcoeleo::write;

/// Formatted error: `error: [cmd]: [msg]`
pub fn err(cmd: &str, msg: &str) {
    let _ = write(1, b"\x1b[1;31merror:\x1b[0m ");
    let _ = write(1, cmd.as_bytes());
    let _ = write(1, b": ");
    let _ = write(1, msg.as_bytes());
    let _ = write(1, b"\n");
}

/// Contextual error: `error: [cmd]: '[target]': [msg]`
pub fn err_target(cmd: &str, target: &str, msg: &str) {
    let _ = write(1, b"\x1b[1;31merror:\x1b[0m ");
    let _ = write(1, cmd.as_bytes());
    let _ = write(1, b": '");
    let _ = write(1, target.as_bytes());
    let _ = write(1, b"': ");
    let _ = write(1, msg.as_bytes());
    let _ = write(1, b"\n");
}

/// Usage hint: `usage: [cmd] [syntax]`
pub fn usage(cmd: &str, syntax: &str) {
    let _ = write(1, b"\x1b[90musage: ");
    let _ = write(1, cmd.as_bytes());
    let _ = write(1, b" ");
    let _ = write(1, syntax.as_bytes());
    let _ = write(1, b"\x1b[0m\n");
}

/// Warning: `warning: [msg]`
pub fn warn(msg: &str) {
    let _ = write(1, b"\x1b[1;33mwarning:\x1b[0m ");
    let _ = write(1, msg.as_bytes());
    let _ = write(1, b"\n");
}

/// Hint: `tip: [msg]`
pub fn hint(msg: &str) {
    let _ = write(1, b"\x1b[1;36mtip:\x1b[0m ");
    let _ = write(1, msg.as_bytes());
    let _ = write(1, b"\n");
}

/// Levenshtein distance on stack buffers for strings <= 32 chars.
pub fn levenshtein(a: &str, b: &str) -> usize {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    if ab.is_empty() {
        return bb.len();
    }
    if bb.is_empty() {
        return ab.len();
    }
    if ab.len() > 32 || bb.len() > 32 {
        return usize::MAX;
    }
    let mut d = [0usize; 33];
    for j in 0..=bb.len() {
        d[j] = j;
    }
    for i in 1..=ab.len() {
        let mut prev = d[0];
        d[0] = i;
        for j in 1..=bb.len() {
            let temp = d[j];
            let cost = if ab[i - 1] == bb[j - 1] { 0 } else { 1 };
            d[j] = (d[j] + 1).min(d[j - 1] + 1).min(prev + cost);
            prev = temp;
        }
    }
    d[bb.len()]
}

/// Suggests the closest matching known command if distance <= 2.
pub fn suggest_command(typed: &str, candidates: &[&'static str]) -> Option<&'static str> {
    let mut best_dist = 3usize;
    let mut best_match = None;
    for &cand in candidates {
        let dist = levenshtein(typed, cand);
        if dist < best_dist {
            best_dist = dist;
            best_match = Some(cand);
        }
    }
    best_match
}
