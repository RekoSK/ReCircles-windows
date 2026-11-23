use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const RECORD_SEP: u8 = 0;

#[derive(Debug, Clone)]
pub struct History {
    dir: PathBuf,
    counts: HashMap<String, u32>,
    pending: HashMap<String, u32>,
    recency: HashMap<String, u64>,
    pending_recency: HashMap<String, u64>,
    last_seq: u64,
}

impl History {
    pub fn load_default() -> Self {
        Self::load_in(crate::paths::config_dir())
    }

    pub fn load_in(dir: PathBuf) -> Self {
        let mut h = Self {
            dir,
            counts: HashMap::new(),
            pending: HashMap::new(),
            recency: HashMap::new(),
            pending_recency: HashMap::new(),
            last_seq: 0,
        };
        h.reload();
        h
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn reload(&mut self) {
        self.counts.clear();
        self.recency.clear();
        let main_path = self.dir.join("usage");
        if let Ok(bytes) = fs::read(&main_path) {
            parse_usage_bytes(&bytes, &mut self.counts, &mut self.recency);
        }
        let pending_path = self.dir.join("usage.pending");
        if let Ok(bytes) = fs::read(&pending_path) {
            parse_usage_bytes(&bytes, &mut self.counts, &mut self.recency);
        }
        self.last_seq = self
            .recency
            .values()
            .chain(self.pending_recency.values())
            .copied()
            .max()
            .unwrap_or(0);
    }

    pub fn app_score(&self, id: &str) -> u32 {
        let key = format!("a{id}");
        self.counts.get(&key).copied().unwrap_or(0)
            + self.pending.get(&key).copied().unwrap_or(0)
    }

    pub fn path_score(&self, path: &str) -> u32 {
        let key = format!("p{path}");
        self.counts.get(&key).copied().unwrap_or(0)
            + self.pending.get(&key).copied().unwrap_or(0)
    }

    pub fn bump_app(&mut self, id: &str) {
        let key = format!("a{id}");
        *self.pending.entry(key).or_insert(0) += 1;
    }

    pub fn bump_path(&mut self, path: &str) {
        let key = format!("p{path}");
        *self.pending.entry(key).or_insert(0) += 1;

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let seq = now_ms.max(self.last_seq.saturating_add(1));
        self.last_seq = seq;

        let norm = normalize_recency_path(path);
        let rkey = format!("r{norm}");
        let entry = self.pending_recency.entry(rkey).or_insert(0);
        *entry = (*entry).max(seq);
    }

    pub fn recent_path_keys(&self) -> Vec<String> {
        let mut merged: HashMap<&str, u64> = HashMap::new();
        for (k, &v) in &self.recency {
            if let Some(rest) = k.strip_prefix('r') {
                let e = merged.entry(rest).or_insert(0);
                *e = (*e).max(v);
            }
        }
        for (k, &v) in &self.pending_recency {
            if let Some(rest) = k.strip_prefix('r') {
                let e = merged.entry(rest).or_insert(0);
                *e = (*e).max(v);
            }
        }
        let mut ranked: Vec<(&str, u64)> = merged.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));

        let mut seen: HashSet<String> = HashSet::new();
        let mut out = Vec::with_capacity(ranked.len());
        for (path, _) in ranked {
            let dedup = dedup_path_key(path);
            if seen.insert(dedup) {
                out.push(path.to_string());
            }
        }
        out
    }

    pub fn all_path_keys_ranked(&self) -> Vec<String> {
        let mut merged: Vec<(&str, u32)> = Vec::new();
        for (k, v) in &self.counts {
            if let Some(rest) = k.strip_prefix('p') {
                let extra = self.pending.get(k).copied().unwrap_or(0);
                merged.push((rest, v.saturating_add(extra)));
            }
        }
        for (k, v) in &self.pending {
            if !self.counts.contains_key(k)
                && let Some(rest) = k.strip_prefix('p')
            {
                merged.push((rest, *v));
            }
        }
        merged.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        merged.into_iter().map(|(k, _)| k.to_string()).collect()
    }

    pub fn save(&mut self) -> io::Result<()> {
        if self.pending.is_empty() && self.pending_recency.is_empty() {
            return Ok(());
        }
        fs::create_dir_all(&self.dir)?;

        let lock_path = self.dir.join("usage.lock");
        let _lock = match acquire_lock(&lock_path) {
            Ok(l) => Some(l),
            Err(_) => {
                // Contended: append directly to usage.pending cleanly
                let pending_path = self.dir.join("usage.pending");
                let mut file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&pending_path)?;
                let buf = encode_usage_maps(&self.pending, &self.pending_recency);
                file.write_all(&buf)?;
                for (k, v) in self.pending.drain() {
                    *self.counts.entry(k).or_insert(0) += v;
                }
                for (k, v) in self.pending_recency.drain() {
                    let e = self.recency.entry(k).or_insert(0);
                    *e = (*e).max(v);
                }
                return Ok(());
            }
        };

        let mut merged_counts = HashMap::new();
        let mut merged_recency = HashMap::new();
        let main_path = self.dir.join("usage");
        if let Ok(bytes) = fs::read(&main_path) {
            parse_usage_bytes(&bytes, &mut merged_counts, &mut merged_recency);
        }
        let pending_path = self.dir.join("usage.pending");
        if let Ok(bytes) = fs::read(&pending_path) {
            parse_usage_bytes(&bytes, &mut merged_counts, &mut merged_recency);
        }
        for (k, v) in self.pending.drain() {
            *merged_counts.entry(k).or_insert(0) += v;
        }
        for (k, v) in self.pending_recency.drain() {
            let e = merged_recency.entry(k).or_insert(0);
            *e = (*e).max(v);
        }

        let tmp_path = self.dir.join(format!("usage.tmp.{}", std::process::id()));
        {
            let mut f = File::create(&tmp_path)?;
            f.write_all(&encode_usage_maps(&merged_counts, &merged_recency))?;
            f.sync_all().ok();
        }
        let _ = fs::remove_file(&main_path);
        fs::rename(&tmp_path, &main_path)?;
        let _ = fs::remove_file(&pending_path);
        self.counts = merged_counts;
        self.recency = merged_recency;
        self.last_seq = self
            .recency
            .values()
            .copied()
            .max()
            .unwrap_or(self.last_seq);
        Ok(())
    }

    pub fn reset_in_place(&mut self) -> io::Result<()> {
        self.counts.clear();
        self.pending.clear();
        self.recency.clear();
        self.pending_recency.clear();
        self.last_seq = 0;
        reset_in(&self.dir)
    }

    pub fn reset() -> io::Result<()> {
        reset_in(&crate::paths::config_dir())
    }
}

pub fn reset_in(dir: &Path) -> io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for name in ["usage", "usage.pending", "usage.lock"] {
        let p = dir.join(name);
        if p.exists() {
            fs::remove_file(p)?;
        }
    }
    Ok(())
}

fn normalize_recency_path(path: &str) -> String {
    let trimmed = path.trim();
    let mut norm = if cfg!(windows) {
        trimmed.replace('/', "\\")
    } else {
        trimmed.replace('\\', "/")
    };
    while norm.len() > 1 && norm.ends_with(['\\', '/']) {
        if cfg!(windows) && norm.len() == 3 && norm.as_bytes()[1] == b':' {
            break;
        }
        norm.pop();
    }
    norm
}

fn dedup_path_key(path: &str) -> String {
    let norm = normalize_recency_path(path);
    if cfg!(windows) {
        norm.to_ascii_lowercase()
    } else {
        norm
    }
}

fn parse_usage_bytes(
    bytes: &[u8],
    counts: &mut HashMap<String, u32>,
    recency: &mut HashMap<String, u64>,
) {
    for record in bytes.split(|&b| b == RECORD_SEP) {
        if record.is_empty() {
            continue;
        }
        if let Ok(text) = std::str::from_utf8(record)
            && let Some((val_str, key)) = text.split_once('\t')
            && !key.is_empty()
        {
            if key.starts_with('r') {
                if let Ok(seq) = val_str.parse::<u64>() {
                    let e = recency.entry(key.to_string()).or_insert(0);
                    *e = (*e).max(seq);
                }
            } else if let Ok(count) = val_str.parse::<u32>() {
                *counts.entry(key.to_string()).or_insert(0) += count;
            }
        }
    }
}

fn encode_usage_maps(counts: &HashMap<String, u32>, recency: &HashMap<String, u64>) -> Vec<u8> {
    let mut count_keys: Vec<(&String, &u32)> = counts.iter().collect();
    count_keys.sort_by(|a, b| a.0.cmp(b.0));
    let mut out = Vec::new();
    for (key, count) in count_keys {
        out.extend_from_slice(format!("{count}\t{key}").as_bytes());
        out.push(RECORD_SEP);
    }
    let mut recency_keys: Vec<(&String, &u64)> = recency.iter().collect();
    recency_keys.sort_by(|a, b| a.0.cmp(b.0));
    for (key, seq) in recency_keys {
        out.extend_from_slice(format!("{seq}\t{key}").as_bytes());
        out.push(RECORD_SEP);
    }
    out
}

struct LockGuard(PathBuf);
impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn acquire_lock(path: &Path) -> io::Result<LockGuard> {
    if let Ok(meta) = fs::metadata(path)
        && let Ok(modified) = meta.modified()
        && let Ok(age) = SystemTime::now().duration_since(modified)
        && age > Duration::from_secs(10)
    {
        let _ = fs::remove_file(path);
    }
    let mut f = OpenOptions::new().write(true).create_new(true).open(path)?;
    writeln!(f, "{}", std::process::id()).ok();
    Ok(LockGuard(path.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let d = std::env::temp_dir().join(format!("recircles_hist_{tag}_{nanos}"));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn saves_merges_and_resets_in_place() {
        let d = temp_dir("save");
        let mut h = History::load_in(d.clone());
        h.bump_app("notepad.exe");
        h.bump_app("notepad.exe");
        h.bump_path("C:\\Users\\Andrej\\Documents");
        h.save().unwrap();

        let h2 = History::load_in(d.clone());
        assert_eq!(h2.app_score("notepad.exe"), 2);
        assert_eq!(h2.path_score("C:\\Users\\Andrej\\Documents"), 1);

        h.reset_in_place().unwrap();
        assert_eq!(h.app_score("notepad.exe"), 0);
        assert_eq!(h.path_score("C:\\Users\\Andrej\\Documents"), 0);
        assert!(h.recent_path_keys().is_empty());
        assert!(!d.join("usage").exists());

        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn recent_path_keys_orders_most_recently_opened_first_and_persists() {
        let d = temp_dir("recent");
        let mut h = History::load_in(d.clone());
        h.bump_path("C:\\Alpha\\");
        h.bump_path("C:\\Beta\\");
        h.bump_path("C:\\Gamma\\");
        h.bump_path("C:\\Alpha");
        assert_eq!(
            h.recent_path_keys(),
            vec!["C:\\Alpha", "C:\\Gamma", "C:\\Beta"]
        );

        h.save().unwrap();
        let h2 = History::load_in(d.clone());
        assert_eq!(
            h2.recent_path_keys(),
            vec!["C:\\Alpha", "C:\\Gamma", "C:\\Beta"]
        );

        let _ = fs::remove_dir_all(d);
    }
}
