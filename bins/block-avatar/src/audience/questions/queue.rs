//! The queue's workings: taking questions in, approving and airing them, bans, saving and
//! loading (a tab-separated index next to the recordings).

#[allow(unused_imports)]
use super::*;

pub(super) fn esc(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "")
}

pub(super) fn unesc(s: &str) -> String {
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
pub(super) fn words(s: &str) -> Vec<String> {
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
    pub(super) fn get_mut(&mut self, id: u32) -> Option<&mut Question> {
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

    pub(super) fn drop_account(&mut self, account: &str) {
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
