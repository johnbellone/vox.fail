//! Viewer questions: the queue behind the "ask" page.
//!
//! Viewers record a short question on the ask page (see ask.rs). It lands here as *New*. The
//! streamer previews it privately on the control page, approves it, and only then can it go on
//! air, where the viewer's chosen character says it with their own (unmodified) voice.
//!
//! Moderation lives here too: per-viewer limits, bans (also from Twitch chat bans), a blocked
//! words list, and clean-up (audio is deleted once a question is finished; everything older
//! than a day is dropped).
//!
//! Files, in `questions/` next to avatar_settings.txt:
//!   index.tsv          the queue (one question per line), so a restart keeps it
//!   <id>.wav           the levelled recording (16-bit mono), deleted when finished
//!   banned.txt         banned accounts, one per line ("tw:login", or "name:x<TAB>address" for
//!                      typed names, whose address is banned with them)
//!   blocked_words.txt  words that flag a question and hide a username on stream

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const MAX_SECS_HARD: f32 = 60.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// waiting, not heard yet (or heard, not approved)
    New,
    /// approved: may go on air
    Approved,
    OnAir,
    Done,
    Skipped,
}

impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Status::New => "new",
            Status::Approved => "approved",
            Status::OnAir => "on_air",
            Status::Done => "done",
            Status::Skipped => "skipped",
        }
    }
    fn parse(s: &str) -> Option<Status> {
        [
            Status::New,
            Status::Approved,
            Status::OnAir,
            Status::Done,
            Status::Skipped,
        ]
        .into_iter()
        .find(|x| x.name() == s)
    }
    pub fn waiting(self) -> bool {
        matches!(self, Status::New | Status::Approved)
    }
}

#[derive(Clone, Debug)]
pub struct Question {
    pub id: u32,
    /// Twitch login, or the typed name in open mode
    pub user: String,
    pub verified: bool,
    /// account key used for limits and bans: "tw:<login>" or "name:<name>"
    pub account: String,
    /// viewer's address as seen by the ask server (for open mode limits and bans)
    pub ip: String,
    /// character label and its model path under assets/
    pub character: String,
    pub model: String,
    pub secs: f32,
    pub submitted: u64,
    pub status: Status,
    /// the streamer has listened to it on the control page
    pub previewed: bool,
    pub votes: HashSet<String>,
    /// what the speech bubble shows (Whisper's transcript, edited by the streamer)
    pub text: String,
    /// transcript or name matched the blocked words list
    pub flagged: bool,
}

#[derive(Clone, Debug)]
pub struct Limits {
    pub max_secs: f32,
    pub min_secs: f32,
    /// questions one viewer may have waiting at once
    pub per_user_waiting: usize,
    /// questions one viewer may send per stream (counted over the last 12 hours)
    pub per_user_total: usize,
    pub cooldown_s: u64,
    pub max_waiting: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_secs: 30.0,
            min_secs: 1.0,
            per_user_waiting: 1,
            per_user_total: 3,
            cooldown_s: 60,
            max_waiting: 200,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum SubmitError {
    Closed,
    Banned,
    QueueFull,
    AlreadyWaiting,
    TooMany,
    Cooldown(u64),
    BadAudio(String),
    TooShort,
    TooLong(f32),
    TooQuiet,
}

impl SubmitError {
    pub fn message(&self) -> String {
        match self {
            SubmitError::Closed => "Questions are closed right now.".into(),
            SubmitError::Banned => "You can't send questions to this stream.".into(),
            SubmitError::QueueFull => "The queue is full. Try again later.".into(),
            SubmitError::AlreadyWaiting => "You already have a question waiting.".into(),
            SubmitError::TooMany => {
                "You've sent the most questions allowed for this stream.".into()
            }
            SubmitError::Cooldown(s) => {
                format!("Please wait {s} s before sending another question.")
            }
            SubmitError::BadAudio(e) => format!("That recording couldn't be read ({e})."),
            SubmitError::TooShort => "That recording is too short.".into(),
            SubmitError::TooLong(m) => format!("Questions can be at most {m:.0} seconds."),
            SubmitError::TooQuiet => {
                "We couldn't hear anything in that recording. Check your microphone.".into()
            }
        }
    }
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub struct Queue {
    dir: PathBuf,
    pub open: bool,
    pub items: Vec<Question>,
    next_id: u32,
    /// banned account -> the address banned with it (typed names only)
    banned: HashMap<String, String>,
    /// temporary blocks from Twitch chat timeouts: account -> until (unix s)
    timeouts: HashMap<String, u64>,
    pub blocked_words: Vec<String>,
    pub limits: Limits,
    /// bumped on every change, so the control page knows when to redraw
    pub version: u64,
    /// recordings to transcribe (Whisper builds)
    pub transcribe: Option<std::sync::mpsc::Sender<(u32, PathBuf)>>,
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "")
}

fn unesc(s: &str) -> String {
    let mut o = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('t') => o.push('\t'),
                Some('n') => o.push('\n'),
                Some(x) => o.push(x),
                None => {}
            }
        } else {
            o.push(c);
        }
    }
    o
}

/// Lowercase words of a text, for the blocked words check.
fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

impl Queue {
    /// Opens (or creates) the queue folder and reloads what was there.
    pub fn load(dir: PathBuf) -> Queue {
        let _ = std::fs::create_dir_all(&dir);
        let mut q = Queue {
            dir,
            open: false,
            items: vec![],
            next_id: 1,
            banned: HashMap::new(),
            timeouts: HashMap::new(),
            blocked_words: vec![],
            limits: Limits::default(),
            version: 1,
            transcribe: None,
        };
        if let Ok(s) = std::fs::read_to_string(q.dir.join("banned.txt")) {
            q.banned = s
                .lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(|l| {
                    let (a, ip) = l.split_once('\t').unwrap_or((l, ""));
                    (a.trim().to_string(), ip.trim().to_string())
                })
                .collect();
        }
        q.reload_words();
        let cutoff = unix_now().saturating_sub(24 * 3600);
        if let Ok(s) = std::fs::read_to_string(q.dir.join("index.tsv")) {
            for line in s.lines() {
                let f: Vec<&str> = line.split('\t').collect();
                if f.len() < 14 {
                    continue;
                }
                let Ok(id) = f[0].parse::<u32>() else {
                    continue;
                };
                let Some(mut status) = Status::parse(f[9]) else {
                    continue;
                };
                let submitted = f[8].parse().unwrap_or(0);
                q.next_id = q.next_id.max(id + 1);
                if submitted < cutoff {
                    let _ = std::fs::remove_file(q.wav_path(id));
                    continue;
                }
                // whoever was on air when the app stopped goes back to approved
                if status == Status::OnAir {
                    status = Status::Approved;
                }
                q.items.push(Question {
                    id,
                    user: unesc(f[1]),
                    verified: f[2] == "1",
                    account: unesc(f[3]),
                    ip: unesc(f[4]),
                    character: unesc(f[5]),
                    model: unesc(f[6]),
                    secs: f[7].parse().unwrap_or(0.0),
                    submitted,
                    status,
                    previewed: f[10] == "1",
                    votes: f[11]
                        .split(',')
                        .filter(|v| !v.is_empty())
                        .map(|v| v.to_string())
                        .collect(),
                    flagged: f[12] == "1",
                    text: unesc(f[13]),
                });
            }
        }
        // a recording that's gone (deleted by hand) can't be aired
        let missing: Vec<u32> = q
            .items
            .iter()
            .filter(|x| x.status.waiting() && !q.wav_path(x.id).exists())
            .map(|x| x.id)
            .collect();
        for id in missing {
            q.finish(id, Status::Skipped);
        }
        q.save();
        q
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn reload_words(&mut self) {
        let path = self.dir.join("blocked_words.txt");
        if !path.exists() {
            let _ = std::fs::write(
                &path,
                "# One word or name per line. Questions whose transcript or username contains one are\n# flagged in the queue, and a matching username is hidden on stream.\n",
            );
        }
        self.blocked_words = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .lines()
            .map(|l| l.trim().to_lowercase())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        let flags: Vec<(u32, bool)> = self
            .items
            .iter()
            .map(|q| (q.id, self.is_blocked(&q.text) || self.is_blocked(&q.user)))
            .collect();
        for (id, f) in flags {
            if let Some(q) = self.get_mut(id) {
                q.flagged = f;
            }
        }
        self.version += 1;
    }

    /// True if any blocked word appears in the text (as a word, or inside a run-together name).
    pub fn is_blocked(&self, text: &str) -> bool {
        if self.blocked_words.is_empty() {
            return false;
        }
        let ws = words(text);
        let joined: String = ws.concat();
        self.blocked_words.iter().any(|b| {
            ws.iter().any(|w| w == b) || (b.chars().count() >= 4 && joined.contains(b.as_str()))
        })
    }

    /// The name shown on stream: hidden if it matches the blocked words list.
    pub fn display_name(&self, q: &Question) -> String {
        if self.is_blocked(&q.user) {
            "a viewer".into()
        } else {
            q.user.clone()
        }
    }

    pub fn wav_path(&self, id: u32) -> PathBuf {
        self.dir.join(format!("{id}.wav"))
    }

    pub fn save(&mut self) {
        self.version += 1;
        let mut s = String::new();
        for q in &self.items {
            let votes: Vec<&str> = q.votes.iter().map(|v| v.as_str()).collect();
            s += &format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.2}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                q.id,
                esc(&q.user),
                q.verified as u8,
                esc(&q.account),
                esc(&q.ip),
                esc(&q.character),
                esc(&q.model),
                q.secs,
                q.submitted,
                q.status.name(),
                q.previewed as u8,
                esc(&votes.join(",").replace(['\t', '\n'], "")),
                q.flagged as u8,
                esc(&q.text)
            );
        }
        let tmp = self.dir.join("index.tsv.tmp");
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(&tmp, self.dir.join("index.tsv"));
        }
        let mut b: Vec<String> = self
            .banned
            .iter()
            .map(|(a, ip)| {
                if ip.is_empty() {
                    format!("{a}\n")
                } else {
                    format!("{a}\t{ip}\n")
                }
            })
            .collect();
        b.sort();
        let _ = std::fs::write(self.dir.join("banned.txt"), b.concat());
    }

    pub fn get(&self, id: u32) -> Option<&Question> {
        self.items.iter().find(|q| q.id == id)
    }
    fn get_mut(&mut self, id: u32) -> Option<&mut Question> {
        self.items.iter_mut().find(|q| q.id == id)
    }

    pub fn is_banned(&self, account: &str, ip: &str, now: u64) -> bool {
        self.banned.contains_key(account)
            || (!ip.is_empty() && self.banned.values().any(|b| b == ip))
            || self.timeouts.get(account).is_some_and(|until| *until > now)
    }

    /// Checks everything that doesn't need the recording, so the page can say "you can't ask"
    /// before someone records.
    pub fn can_submit(&self, account: &str, ip: &str, now: u64) -> Result<(), SubmitError> {
        if !self.open {
            return Err(SubmitError::Closed);
        }
        if self.is_banned(account, ip, now) {
            return Err(SubmitError::Banned);
        }
        if self.items.iter().filter(|q| q.status.waiting()).count() >= self.limits.max_waiting {
            return Err(SubmitError::QueueFull);
        }
        let mine: Vec<&Question> = self.items.iter().filter(|q| q.account == account).collect();
        if mine
            .iter()
            .filter(|q| q.status.waiting() || q.status == Status::OnAir)
            .count()
            >= self.limits.per_user_waiting
        {
            return Err(SubmitError::AlreadyWaiting);
        }
        if mine
            .iter()
            .filter(|q| now.saturating_sub(q.submitted) < 12 * 3600)
            .count()
            >= self.limits.per_user_total
        {
            return Err(SubmitError::TooMany);
        }
        if let Some(last) = mine.iter().map(|q| q.submitted).max() {
            let since = now.saturating_sub(last);
            if since < self.limits.cooldown_s {
                return Err(SubmitError::Cooldown(self.limits.cooldown_s - since));
            }
        }
        Ok(())
    }

    /// A new question from the ask page. `wav` is the uploaded file as sent.
    #[allow(clippy::too_many_arguments)]
    pub fn submit(
        &mut self,
        user: &str,
        verified: bool,
        account: &str,
        ip: &str,
        character: (&str, &str),
        wav: &[u8],
        now: u64,
    ) -> Result<u32, SubmitError> {
        self.can_submit(account, ip, now)?;
        let (rate, samples) = parse_wav_mono16(wav).map_err(SubmitError::BadAudio)?;
        let secs = samples.len() as f32 / rate as f32;
        if secs < self.limits.min_secs {
            return Err(SubmitError::TooShort);
        }
        if secs > self.limits.max_secs + 0.5 {
            return Err(SubmitError::TooLong(self.limits.max_secs));
        }
        let levelled = level_voice(&samples).ok_or(SubmitError::TooQuiet)?;
        let id = self.next_id;
        self.next_id += 1;
        let path = self.wav_path(id);
        write_wav16(&path, rate, &levelled)
            .map_err(|e| SubmitError::BadAudio(format!("can't save: {e}")))?;
        let flagged = self.is_blocked(user);
        self.items.push(Question {
            id,
            user: user.to_string(),
            verified,
            account: account.to_string(),
            ip: ip.to_string(),
            character: character.0.to_string(),
            model: character.1.to_string(),
            secs,
            submitted: now,
            status: Status::New,
            previewed: false,
            votes: HashSet::new(),
            text: String::new(),
            flagged,
        });
        if let Some(tx) = &self.transcribe {
            let _ = tx.send((id, path));
        }
        self.save();
        Ok(id)
    }

    /// Waiting questions in airing order: most votes first, then oldest.
    pub fn waiting(&self) -> Vec<&Question> {
        let mut w: Vec<&Question> = self.items.iter().filter(|q| q.status.waiting()).collect();
        w.sort_by(|a, b| b.votes.len().cmp(&a.votes.len()).then(a.id.cmp(&b.id)));
        w
    }

    /// (place, of how many) for a waiting question, 1-based.
    pub fn position(&self, id: u32) -> Option<(usize, usize)> {
        let w = self.waiting();
        w.iter().position(|q| q.id == id).map(|i| (i + 1, w.len()))
    }

    /// `!up <id>` from chat. One vote per viewer per question; your own doesn't count.
    pub fn vote(&mut self, id: u32, voter: &str) -> bool {
        let voter = voter.to_ascii_lowercase();
        let Some(q) = self.get_mut(id) else {
            return false;
        };
        if !q.status.waiting() || q.account == format!("tw:{voter}") {
            return false;
        }
        let added = q.votes.insert(voter);
        if added {
            self.save();
        }
        added
    }

    /// The control page fetched the recording.
    pub fn mark_previewed(&mut self, id: u32) {
        if let Some(q) = self.get_mut(id) {
            if !q.previewed {
                q.previewed = true;
                self.save();
            }
        }
    }

    pub fn approve(&mut self, id: u32) -> Result<(), String> {
        let q = self.get_mut(id).ok_or("no such question")?;
        if q.status != Status::New {
            return Err(format!("question {id} is {}", q.status.name()));
        }
        if !q.previewed {
            return Err(format!("listen to question {id} first"));
        }
        q.status = Status::Approved;
        self.save();
        Ok(())
    }

    /// Takes an approved question for the stage. Only approved ones can air.
    pub fn take_for_air(&mut self, id: u32) -> Result<Question, String> {
        let path = self.wav_path(id);
        let q = self.get_mut(id).ok_or("no such question")?;
        if q.status != Status::Approved {
            return Err(format!("question {id} isn't approved"));
        }
        if !path.exists() {
            return Err(format!("question {id} has no recording"));
        }
        q.status = Status::OnAir;
        let q = q.clone();
        self.save();
        Ok(q)
    }

    /// Top approved question, in airing order.
    pub fn next_approved(&self) -> Option<u32> {
        self.waiting()
            .into_iter()
            .find(|q| q.status == Status::Approved)
            .map(|q| q.id)
    }

    /// Done or skipped: the recording is deleted.
    pub fn finish(&mut self, id: u32, status: Status) {
        let path = self.wav_path(id);
        if let Some(q) = self.get_mut(id) {
            q.status = status;
            let _ = std::fs::remove_file(path);
            self.save();
        }
    }

    pub fn set_text(&mut self, id: u32, text: &str) {
        let flagged = self.is_blocked(text);
        if let Some(q) = self.get_mut(id) {
            q.text = text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(400)
                .collect();
            q.flagged = flagged || q.flagged;
            self.save();
        }
    }

    /// Bans the account behind a question (and, in open mode, its address) and drops all of its
    /// waiting questions. Returns the account.
    pub fn ban_question(&mut self, id: u32) -> Option<String> {
        let q = self.get(id)?.clone();
        let ip = if !q.verified && q.ip != "127.0.0.1" && q.ip != "::1" {
            q.ip.clone()
        } else {
            String::new()
        };
        self.banned.insert(q.account.clone(), ip);
        self.drop_account(&q.account);
        Some(q.account)
    }

    /// A Twitch chat ban (`secs` None) or timeout from the channel's moderators.
    pub fn chat_ban(&mut self, login: &str, secs: Option<u64>, now: u64) {
        let account = format!("tw:{}", login.to_ascii_lowercase());
        match secs {
            None => {
                self.banned.insert(account.clone(), String::new());
            }
            Some(s) => {
                self.timeouts.insert(account.clone(), now + s);
            }
        }
        self.drop_account(&account);
    }

    fn drop_account(&mut self, account: &str) {
        let ids: Vec<u32> = self
            .items
            .iter()
            .filter(|q| q.account == account && q.status.waiting())
            .map(|q| q.id)
            .collect();
        for id in ids {
            self.finish(id, Status::Skipped);
        }
        self.save();
    }

    pub fn unban(&mut self, account: &str) -> bool {
        // a typed name's address ban goes with it
        let gone = self.banned.remove(account).is_some();
        let timed_out = self.timeouts.remove(account).is_some();
        self.save();
        gone || timed_out
    }

    pub fn banned_list(&self) -> Vec<String> {
        let mut b: Vec<String> = self.banned.keys().cloned().collect();
        b.sort();
        b
    }

    /// Drops finished questions from the list.
    pub fn clear_finished(&mut self) {
        self.items
            .retain(|q| !matches!(q.status, Status::Done | Status::Skipped));
        self.save();
    }

    /// For the control page: everything except finished questions older than an hour.
    pub fn to_json(&self) -> String {
        let now = unix_now();
        let on_air = self
            .items
            .iter()
            .find(|q| q.status == Status::OnAir)
            .map(|q| q.id);
        let order: HashMap<u32, usize> = self
            .waiting()
            .iter()
            .enumerate()
            .map(|(i, q)| (q.id, i))
            .collect();
        let mut list: Vec<&Question> = self
            .items
            .iter()
            .filter(|q| {
                q.status.waiting()
                    || q.status == Status::OnAir
                    || now.saturating_sub(q.submitted) < 3600
            })
            .collect();
        list.sort_by_key(|q| {
            (
                order.get(&q.id).copied().unwrap_or(usize::MAX),
                std::cmp::Reverse(q.id),
            )
        });
        let items: Vec<serde_json::Value> = list
            .iter()
            .map(|q| {
                serde_json::json!({
                    "id": q.id, "user": q.user, "shown_as": self.display_name(q), "verified": q.verified,
                    "account": q.account, "character": q.character, "secs": ((q.secs as f64) * 10.0).round() / 10.0,
                    "age_s": now.saturating_sub(q.submitted), "status": q.status.name(), "previewed": q.previewed,
                    "votes": q.votes.len(), "text": q.text, "flagged": q.flagged,
                })
            })
            .collect();
        serde_json::json!({
            "version": self.version, "open": self.open, "on_air": on_air, "items": items,
            "waiting": self.waiting().len(), "banned": self.banned_list(),
            "limits": {"max_secs": self.limits.max_secs, "per_user_total": self.limits.per_user_total, "cooldown_s": self.limits.cooldown_s},
        })
        .to_string()
    }
}

// ------------------------------------------------------------------------------------------
// Audio
// ------------------------------------------------------------------------------------------

/// Reads an uploaded WAV: 16-bit PCM, mono, 8-48 kHz, nothing else. Never panics on bad input.
pub fn parse_wav_mono16(b: &[u8]) -> Result<(u32, Vec<f32>), String> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err("not a WAV file".into());
    }
    let u16_at =
        |i: usize| -> Option<u16> { Some(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?])) };
    let u32_at = |i: usize| -> Option<u32> {
        Some(u32::from_le_bytes([
            *b.get(i)?,
            *b.get(i + 1)?,
            *b.get(i + 2)?,
            *b.get(i + 3)?,
        ]))
    };
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    let mut i = 12usize;
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let len = u32_at(i + 4).ok_or("truncated chunk")? as usize;
        let start = i + 8;
        let end = start.checked_add(len).ok_or("bad chunk size")?.min(b.len());
        if id == b"fmt " {
            if end - start < 16 {
                return Err("short format chunk".into());
            }
            fmt = Some((
                u16_at(start).unwrap_or(0),
                u16_at(start + 2).unwrap_or(0),
                u32_at(start + 4).unwrap_or(0),
                u16_at(start + 14).unwrap_or(0),
            ));
        } else if id == b"data" {
            let (tag, ch, rate, bits) = fmt.ok_or("data before format")?;
            if tag != 1 || bits != 16 {
                return Err("expected 16-bit PCM".into());
            }
            if ch != 1 {
                return Err("expected mono".into());
            }
            if !(8000..=48000).contains(&rate) {
                return Err(format!("unsupported sample rate {rate}"));
            }
            let s: Vec<f32> = b[start..end]
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
                .collect();
            return Ok((rate, s));
        }
        i = start.checked_add(len + (len & 1)).ok_or("bad chunk size")?;
    }
    Err("no audio data".into())
}

/// Levels a recording so every viewer comes out about as loud as the others: one gain for the
/// whole clip (no compression, no effects), set from the speech parts, capped so peaks stay
/// below full scale. None if there's no speech to hear.
pub fn level_voice(s: &[f32]) -> Option<Vec<f32>> {
    let peak = s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    if peak < 0.003 {
        return None;
    }
    // speech level: RMS of the 20 ms blocks within 30 dB of the loudest block
    let block = 320.max(s.len() / 2000);
    let db: Vec<f32> = s
        .chunks(block)
        .map(|c| {
            10.0 * ((c.iter().map(|x| x * x).sum::<f32>() / c.len() as f32).max(1e-12)).log10()
        })
        .collect();
    let top = db.iter().cloned().fold(-200.0f32, f32::max);
    if top < -55.0 {
        return None;
    }
    let active: Vec<f32> = db.iter().cloned().filter(|d| *d > top - 30.0).collect();
    let mean_pow = active.iter().map(|d| 10f32.powf(d / 10.0)).sum::<f32>() / active.len() as f32;
    let speech_db = 10.0 * mean_pow.log10();
    let target_db = -20.0;
    let gain_db = (target_db - speech_db).min(24.0);
    let gain = 10f32.powf(gain_db / 20.0).min(0.89 / peak);
    Some(s.iter().map(|x| x * gain).collect())
}

pub fn write_wav16(path: &Path, rate: u32, s: &[f32]) -> std::io::Result<()> {
    std::fs::write(path, wav16_bytes(rate, s))
}

pub fn wav16_bytes(rate: u32, s: &[f32]) -> Vec<u8> {
    let data_len = (s.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + s.len() * 2);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for x in s {
        b.extend_from_slice(&((x.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
    }
    b
}

/// Built-in transcription of questions (Whisper builds): fills in each question's bubble text.
#[cfg(feature = "whisper")]
pub fn start_transcriber(
    model: String,
    queue: std::sync::Arc<std::sync::Mutex<Queue>>,
) -> Result<(), String> {
    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
    let ctx = WhisperContext::new_with_params(&model, WhisperContextParameters::default())
        .map_err(|e| format!("can't load Whisper model {model}: {e}"))?;
    let (tx, rx) = std::sync::mpsc::channel::<(u32, PathBuf)>();
    // anything waiting without text (from before a restart) gets a pass too
    {
        let mut q = queue.lock().unwrap();
        for x in q
            .items
            .iter()
            .filter(|x| x.status.waiting() && x.text.is_empty())
        {
            let _ = tx.send((x.id, q.wav_path(x.id)));
        }
        q.transcribe = Some(tx);
    }
    std::thread::Builder::new()
        .name("ask-whisper".into())
        .spawn(move || {
            let Ok(mut state) = ctx.create_state() else {
                return;
            };
            for (id, path) in rx {
                let Ok(bytes) = std::fs::read(&path) else {
                    continue;
                };
                let Ok((rate, s)) = parse_wav_mono16(&bytes) else {
                    continue;
                };
                let s16 = resample_linear(&s, rate, 16000);
                let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
                p.set_language(Some("en"));
                p.set_n_threads(
                    std::thread::available_parallelism()
                        .map(|n| n.get().min(4) as i32)
                        .unwrap_or(2),
                );
                p.set_no_context(true);
                p.set_suppress_blank(true);
                p.set_print_progress(false);
                p.set_print_realtime(false);
                p.set_print_special(false);
                p.set_print_timestamps(false);
                if state.full(p, &s16).is_err() {
                    continue;
                }
                let mut text = String::new();
                for seg in state.as_iter() {
                    if let Ok(t) = seg.to_str_lossy() {
                        text.push_str(&t);
                    }
                }
                let text = crate::bubbles::clean_transcript(&text);
                let mut q = queue.lock().unwrap();
                // the streamer may have typed something already: keep theirs
                if q.get(id).is_some_and(|x| x.text.is_empty()) {
                    q.set_text(id, &text);
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg_attr(not(feature = "whisper"), allow(dead_code))]
pub fn resample_linear(s: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || s.is_empty() {
        return s.to_vec();
    }
    let step = from as f64 / to as f64;
    let n = ((s.len() as f64) / step) as usize;
    (0..n)
        .map(|i| {
            let p = i as f64 * step;
            let j = p as usize;
            let f = (p - j as f64) as f32;
            let a = s[j.min(s.len() - 1)];
            let b = s[(j + 1).min(s.len() - 1)];
            a + (b - a) * f
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("avatar-q-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    /// A real recorded question (CMU ARCTIC, see demo/questions/README.md).
    pub fn real_question(name: &str) -> Vec<u8> {
        std::fs::read(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("demo/questions")
                .join(name),
        )
        .unwrap()
    }

    fn open_queue(name: &str) -> Queue {
        let mut q = Queue::load(tmpdir(name));
        q.open = true;
        q
    }

    const CH: (&str, &str) = ("Dog", "models/dog.glb");

    #[test]
    fn rejects_broken_and_wrong_wavs_without_panicking() {
        assert!(parse_wav_mono16(b"").is_err());
        assert!(parse_wav_mono16(b"RIFF\0\0\0\0WAVE").is_err());
        // fmt chunk claiming a huge size
        let mut b = b"RIFF\0\0\0\0WAVEfmt \xff\xff\xff\xff".to_vec();
        b.extend_from_slice(&[0; 8]);
        assert!(parse_wav_mono16(&b).is_err());
        // stereo and 8-bit are refused
        let mut st = wav16_bytes(16000, &[0.1; 100]);
        st[22] = 2;
        assert!(parse_wav_mono16(&st).unwrap_err().contains("mono"));
        let mut b8 = wav16_bytes(16000, &[0.1; 100]);
        b8[34] = 8;
        assert!(parse_wav_mono16(&b8).is_err());
        // every truncation of a good file is either fine or an error, never a panic
        let good = real_question("slt-a0005.wav");
        for n in (0..good.len().min(400)).chain([good.len() / 2, good.len() - 1]) {
            let _ = parse_wav_mono16(&good[..n]);
        }
        let (rate, s) = parse_wav_mono16(&good).unwrap();
        assert_eq!(rate, 16000);
        assert!(s.len() > 16000);
    }

    #[test]
    fn levelling_is_one_gain_and_keeps_peaks_safe() {
        let (_, s) = parse_wav_mono16(&real_question("bdl-a0042.wav")).unwrap();
        for scale in [0.05f32, 1.0] {
            let quiet: Vec<f32> = s.iter().map(|x| x * scale).collect();
            let out = level_voice(&quiet).unwrap();
            let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            assert!(peak <= 0.9, "{peak}");
            // a single gain: the ratio is the same everywhere
            let i = quiet.iter().position(|x| x.abs() > 0.001).unwrap();
            let g = out[i] / quiet[i];
            assert!(
                quiet
                    .iter()
                    .zip(&out)
                    .all(|(a, b)| (a * g - b).abs() < 1e-4)
            );
        }
        assert!(level_voice(&vec![0.0; 32000]).is_none());
        assert!(level_voice(&vec![0.0005; 32000]).is_none());
    }

    #[test]
    fn viewer_limits_and_airing_order() {
        let mut q = open_queue("limits");
        let wav = real_question("slt-a0005.wav");
        let now = 1_000_000;
        let a = q.submit("ann", true, "tw:ann", "", CH, &wav, now).unwrap();
        assert_eq!(
            q.submit("ann", true, "tw:ann", "", CH, &wav, now + 5),
            Err(SubmitError::AlreadyWaiting)
        );
        let b = q
            .submit("bob", true, "tw:bob", "", CH, &wav, now + 1)
            .unwrap();
        let c = q
            .submit("cat", true, "tw:cat", "", CH, &wav, now + 2)
            .unwrap();
        // order: oldest first, until chat votes move one up
        assert_eq!(
            q.waiting().iter().map(|x| x.id).collect::<Vec<_>>(),
            vec![a, b, c]
        );
        assert!(q.vote(c, "dave"));
        assert!(!q.vote(c, "Dave"), "one vote per viewer");
        assert!(!q.vote(c, "cat"), "no voting for yourself");
        assert_eq!(q.position(c), Some((1, 3)));
        // finish ann's: she may ask again only after the cooldown, and only 3 per stream
        q.mark_previewed(a);
        q.approve(a).unwrap();
        q.take_for_air(a).unwrap();
        q.finish(a, Status::Done);
        assert!(!q.wav_path(a).exists(), "audio is deleted when done");
        assert!(matches!(
            q.submit("ann", true, "tw:ann", "", CH, &wav, now + 30),
            Err(SubmitError::Cooldown(_))
        ));
        let a2 = q
            .submit("ann", true, "tw:ann", "", CH, &wav, now + 70)
            .unwrap();
        q.finish(a2, Status::Skipped);
        let a3 = q
            .submit("ann", true, "tw:ann", "", CH, &wav, now + 140)
            .unwrap();
        q.finish(a3, Status::Skipped);
        assert_eq!(
            q.submit("ann", true, "tw:ann", "", CH, &wav, now + 210),
            Err(SubmitError::TooMany)
        );
        // too long, too short, silent
        let long = wav16_bytes(16000, &vec![0.2; 16000 * 40]);
        assert_eq!(
            q.submit("eve", true, "tw:eve", "", CH, &long, now),
            Err(SubmitError::TooLong(30.0))
        );
        assert_eq!(
            q.submit(
                "eve",
                true,
                "tw:eve",
                "",
                CH,
                &wav16_bytes(16000, &[0.2; 4000]),
                now
            ),
            Err(SubmitError::TooShort)
        );
        assert_eq!(
            q.submit(
                "eve",
                true,
                "tw:eve",
                "",
                CH,
                &wav16_bytes(16000, &vec![0.0; 32000]),
                now
            ),
            Err(SubmitError::TooQuiet)
        );
        q.open = false;
        assert_eq!(
            q.submit("eve", true, "tw:eve", "", CH, &wav, now),
            Err(SubmitError::Closed)
        );
    }

    #[test]
    fn nothing_airs_unheard_or_unapproved() {
        let mut q = open_queue("approve");
        let id = q
            .submit(
                "ann",
                true,
                "tw:ann",
                "",
                CH,
                &real_question("slt-a0005.wav"),
                10,
            )
            .unwrap();
        assert!(q.take_for_air(id).is_err(), "new questions can't air");
        assert!(q.approve(id).is_err(), "must be heard first");
        assert_eq!(q.next_approved(), None);
        q.mark_previewed(id);
        q.approve(id).unwrap();
        assert_eq!(q.next_approved(), Some(id));
        assert_eq!(q.take_for_air(id).unwrap().status, Status::OnAir);
        assert!(q.take_for_air(id).is_err(), "can't air twice");
    }

    #[test]
    fn bans_drop_questions_and_stick_across_restarts() {
        let dir = tmpdir("ban");
        let wav = real_question("slt-a0005.wav");
        {
            let mut q = Queue::load(dir.clone());
            q.open = true;
            let id = q
                .submit("troll", false, "name:troll", "203.0.113.9", CH, &wav, 10)
                .unwrap();
            q.submit("ann", true, "tw:ann", "", CH, &wav, 11).unwrap();
            assert_eq!(q.ban_question(id).as_deref(), Some("name:troll"));
            assert_eq!(q.get(id).unwrap().status, Status::Skipped);
            // a new name from the same address is still banned in open mode
            assert_eq!(
                q.submit("troll2", false, "name:troll2", "203.0.113.9", CH, &wav, 500),
                Err(SubmitError::Banned)
            );
            // Twitch chat moderation carries over
            q.chat_ban("Ann", None, 20);
            assert_eq!(q.waiting().len(), 0);
            q.chat_ban("bob", Some(600), 20);
            assert_eq!(q.can_submit("tw:bob", "", 100), Err(SubmitError::Banned));
            assert!(q.can_submit("tw:bob", "", 700).is_ok(), "timeouts expire");
        }
        let mut q = Queue::load(dir);
        q.open = true;
        assert_eq!(
            q.can_submit("tw:ann", "", unix_now()),
            Err(SubmitError::Banned)
        );
        assert!(q.unban("tw:ann"));
        assert!(q.can_submit("tw:ann", "", unix_now()).is_ok());
        assert_eq!(
            q.can_submit("name:troll3", "203.0.113.9", unix_now()),
            Err(SubmitError::Banned),
            "address ban survives a restart"
        );
        assert!(q.unban("name:troll"));
        assert!(q.banned_list().is_empty(), "{:?}", q.banned_list());
        assert!(
            q.can_submit("name:troll3", "203.0.113.9", unix_now())
                .is_ok(),
            "the address ban went with the name"
        );
    }

    #[test]
    fn queue_survives_a_restart() {
        let dir = tmpdir("restart");
        let now = unix_now();
        let id;
        {
            let mut q = Queue::load(dir.clone());
            q.open = true;
            id = q
                .submit(
                    "ann\tx",
                    true,
                    "tw:ann",
                    "",
                    CH,
                    &real_question("slt-a0005.wav"),
                    now,
                )
                .unwrap();
            q.set_text(id, "Will we ever\nforget it?");
            q.mark_previewed(id);
            q.approve(id).unwrap();
            q.take_for_air(id).unwrap();
            q.vote(id, "zed");
        }
        let q = Queue::load(dir);
        let x = q.get(id).unwrap();
        assert_eq!(x.user, "ann\tx");
        assert_eq!(x.text, "Will we ever forget it?");
        assert_eq!(
            x.status,
            Status::Approved,
            "on-air goes back to approved after a restart"
        );
        assert!(x.previewed);
        assert!(!q.open, "questions start closed");
    }

    #[test]
    fn blocked_words_flag_text_and_hide_names() {
        let mut q = open_queue("words");
        std::fs::write(
            q.dir().join("blocked_words.txt"),
            "# comment\nbadword\nzzz\n",
        )
        .unwrap();
        q.reload_words();
        assert!(q.is_blocked("this has a BadWord in it"));
        assert!(q.is_blocked("xX_badword_Xx"), "run-together names");
        assert!(
            !q.is_blocked("zzzebra"),
            "short entries match whole words only"
        );
        assert!(q.is_blocked("zzz"));
        let id = q
            .submit(
                "xxbadwordxx",
                true,
                "tw:xxbadwordxx",
                "",
                CH,
                &real_question("slt-a0005.wav"),
                10,
            )
            .unwrap();
        let x = q.get(id).unwrap().clone();
        assert!(x.flagged);
        assert_eq!(q.display_name(&x), "a viewer");
        let id2 = q
            .submit(
                "ann",
                true,
                "tw:ann",
                "",
                CH,
                &real_question("slt-a0005.wav"),
                10,
            )
            .unwrap();
        q.set_text(id2, "what about the badword");
        assert!(q.get(id2).unwrap().flagged);
    }
}
