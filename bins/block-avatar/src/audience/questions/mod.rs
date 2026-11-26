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

mod queue;
mod audio;
#[allow(unused_imports)]
pub use queue::*;
#[allow(unused_imports)]
pub use audio::*;

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
            std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
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
