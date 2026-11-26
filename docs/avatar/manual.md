# Block Avatar: the manual

The streaming avatar app in `bins/block-avatar`. For the repository as a whole see the
[README](../../README.md); for Vox's worlds see [docs/vox](../vox/README.md).

Your block-style characters as a live, mic-driven avatar for streams and videos, built on Bevy.

- **Lip sync from your microphone, five styles.** Visemes, Detailed, Flap, Volume and Chatter (see
  below). Press **L** to switch. Vowels are read from your voice's formants, so "ah", "ee" and "oo"
  get the right mouth; press **P** once to calibrate them to your voice.
- **Body language from your voice.** Stressed syllables get a nod and a small hand beat, a rising ending
  followed by a pause (how most yes/no questions end) tilts the head, and the brows lift when your
  pitch goes up.
- **Alive on its own.** Random blinks, a small nod with your voice, and the body switches between
  Idle and the Talk gestures while you speak.
- **Voice reactions.** Shout and the face flashes to Rage with a jolt; go quiet for 30 s and the
  character dozes off, then wakes up startled when you speak; the odd idle fidget in between.
- **Emotes that layer on top of anything.** Nod, shake, tilt, hop, spin, bow, shiver and lean in play
  over whatever the body is doing, and **Bop** makes the head and body bounce to every beat or syllable.
- **Every clip in the model.** The control page lists all of the character's clips (100+) with a
  search box: play any of them once, or loop one as the resting pose.
- **10 expressions, 8 gestures, any character.** Every expression keeps lip sync working; it just
  changes which mouths are used (an Angry face talks through gritted teeth).
- **A real webcam (Windows).** Press **W** and Zoom, Teams, Meet or Discord can pick the avatar as
  a camera, at 1280x720 and 60 fps, with no OBS scene needed.
- **Live speech bubbles.** What you say pops up in a pixel speech bubble by the head, typed out
  like an old RPG text box; quiet speech gets the dashed whisper bubble. Words come from built-in
  Whisper speech-to-text, a captioning tool's text file, or the control page.
- **Chat throws things.** Viewers type `!tomato`, `!egg` or `!confetti` in Twitch chat (no login
  needed) and it flies at the avatar, splats on its face and makes it flinch. Any bot can trigger
  the same over HTTP, so YouTube and Kick work through Streamer.bot or similar.
- **Viewer questions.** Viewers sign in with Twitch, pick a character and record a question on a
  web page; you listen privately, approve, and their character pops in beside yours and asks it in
  their own voice, lip-synced. Votes from chat, bans that follow your Twitch mods, a panic Cut.
- **Chat dresses you up.** `!hat cowboy`, `!glasses shades`, `!hold coffee`: 15 hats, 4 face
  pieces and 12 things to hold, fitted to every character's head.
- **Twitch moments.** A raid brings a crowd of little characters running on stage; a sub sets
  off confetti and a victory dance; bits throw tomatoes (a giant one from 500).
- **Tug-of-war polls.** Chat votes `!1` or `!2` and two characters pull a rope across the bottom
  of the picture.
- **Your voice does more.** Laugh and your character cracks up; sing and it grabs a microphone
  while music notes float up; say a catchphrase of yours and a gag fires.
- **Live co-host.** A second mic (or a friend on Discord) drives a second character beside you.
- **A produced look.** Six voxel sets instead of the green screen, an auto-director that cuts
  between shots, and a press-conference mode where viewers' questions wait as a row of reporters.
- **Shorts and comics, made for you.** The best moments save themselves as vertical 9:16 videos
  with sound, plus a 4-panel comic strip of each.
- **Retro sprite mode.** Press **Y** and the 3D avatar is drawn as chunky pixel art in the
  character's own palette with a dark outline, like the original 2D sprites. Live, any character,
  any animation.
- **Made for OBS.** Green, magenta or studio background, or a truly transparent window, plus a
  local control page you can dock inside OBS so you never have to click the avatar window.

## What's in this folder

| Path | What |
|---|---|
| `bins/block-avatar/src/` | The avatar app (Bevy 0.19.1): lip sync, reactions, emotes, control page, webcam output |
| `assets/models/` | The 38 playable characters (Blue, the lead, 25 cast, 4 animals, peanut, 6 roasted peanuts); the app loads these |
| `assets/models/variants/` | Lead without props, and the stepped-timing twins |
| `assets/models/early/` | The first three voxel versions of the lead, before the rig |
| `assets/props/` | 124 props, set pieces and effects as `.glb` (lead 91, cast 13, roasted peanut 20) |
| `source/vox/` | MagicaVoxel sources for every character, prop and early version |
| `data/` | Clip manifests: timing, props used, dialogue lines for the speech bubbles |
| `tools/assets/` | `build_animals.py`, `build_roasted.py` (rebuild the new characters), `make_stepped.py`, `repack.py` and the other asset builders |
| `docs/avatar/` | This manual, line-ups, turnarounds and the props catalogue (images) |
| `assets/props/wardrobe/`, `assets/sets/` | Dress-up hats and face pieces, and the six stage sets (built by `tools/assets/build_wardrobe.py`, `tools/assets/build_sets.py`) |
| `assets/sounds/`, `assets/portraits/` | Sound effects (real CC0 recordings, credits inside) and a head-and-shoulders picture of every character (`tools/assets/make_portraits.py`), used on the viewer question page and the shorts' title cards |
| `bins/block-avatar/installer/`, `.github/workflows/` | The Windows installer script (NSIS) and the build that makes it |
| `assets/ui/`, `assets/fonts/`, `assets/shaders/` | Speech-bubble art, the Press Start 2P pixel font (SIL Open Font License, licence file alongside), the retro shader |
| `demo/` | Real speech recordings (LibriSpeech) for driving the mouth without a mic; `demo/questions/` has real recorded questions (CMU ARCTIC) for trying the question queue |
| `testdata/real/` | Real-voice test recordings used by `cargo test`, including real laughs and a cappella singing for the laugh and sing detectors |

Everything is listed in [Models](#models) below.

## Install (Windows)

Run `Block-Avatar-Setup-<version>.exe`. It installs to Program Files with a Start-menu shortcut
(and, if you tick it, a desktop shortcut and a firewall rule for the viewer question page). The
first time the app starts, the control page opens on a five-step setup: pick your microphone
(the bar moves when you talk), pick your character, tune the mouth to your voice, add your Twitch
channel, and see what's missing (OBS virtual camera, ffmpeg, Whisper). It never asks again; open
it any time at `http://127.0.0.1:7777/?setup`.

The installer is built by `.github/workflows/windows.yml` on every `v*` tag (it also attaches a
portable zip), or by hand on Windows: `cargo build --release`, then
`makensis /DVERSION=0.1.0 installer\block-avatar.nsi`.

### Your files

Settings, gags.txt, saved shorts and comics, viewer questions, your gag sounds and the log are in
**%APPDATA%\Block Avatar** on an installed copy (the Start menu has a "Your files" shortcut;
uninstalling leaves it alone). Running from a source folder keeps them in that folder, as
before. Set `BLOCK_AVATAR_HOME` to put them anywhere else.

`avatar.log` there records what happened this run (the last run is kept as `avatar.prev.log`);
anything that went wrong also shows in red at the top of the control page until you clear it.
Attach the log when you report a problem.

## Run it from source

You need Rust (rustup.rs). Then, from this folder:

```
cargo run --release
```

The first build takes a while (it compiles Bevy); after that it starts in seconds. On Linux,
install the audio and input headers first: `sudo apt install libasound2-dev libudev-dev`.

All 38 characters are in `assets/models/`. Press **Tab** to cycle them, or start with one:
`cargo run --release -- --model president.glb`.

## Controls

| Key | Does |
|---|---|
| 1 2 3 4 5 6 7 8 9 0 | Neutral, Happy, Angry, Sad, Surprised, Smug, Suspicious, Nervous, Rage, KO |
| F1–F8 | Emotes (thumbs up, point, shrug), Victory, Rally chant, Rally dance, Heavy hit, Knockdown, Jab, Cover up |
| F9–F12 | The character's own clips (Senator: Floor speech, Handshake, No comment; and so on) |
| L | Lip-sync style: Visemes, Detailed, Flap, Volume, Chatter |
| Z X C J K U I O | Emotes: nod, shake, tilt, hop, spin, bow, shiver, lean in |
| D | Bop to the beat on/off |
| R | Voice reactions on/off |
| T | Body language from your voice (nods, hand beats, question tilt, brows) on/off |
| P | Vowel calibration: hold "ah", then "ee", then "oo" for about 2 s each |
| Tab / Shift+Tab | Next / previous character |
| B | Background: green, magenta, studio (and transparent if started with `--transparent`) |
| V | Framing: head and shoulders, waist up, full body |
| G | Talk gestures while speaking on/off |
| M | Mute the mic (mouth rests) |
| N | Calibrate: stay quiet for 1.5 s and the noise gate sets itself |
| [ and ] | Gate down / up 2 dB, if room noise opens the mouth or quiet speech doesn't |
| W | Webcam output on/off |
| S | Speech bubbles on/off |
| Q | Test throw (tomato, egg or confetti) |
| Y | Retro sprite mode on/off |
| A | Viewer questions: air the next approved one; while one is on stage, end it (cut if it's still talking) |
| E | Clip that: save the last 20 s as a vertical short |
| H | Hide the on-screen info (do this before going live) |

Settings (character, face, lip style, background, framing, gate, reactions, bop) are saved to
`avatar_settings.txt`. `sleep_after=30` in that file sets how many quiet seconds before it dozes off; `vowels=` holds your
calibrated vowels and `formants=false` turns formant reading off.

## Calibrate your vowels (once)

Press **P** (or the control page's **Calibrate vowels**) and follow the prompts on screen: hold a
long **"aah"** as in *father*, then **"eee"** as in *see*, then **"ooo"** as in *food*, about two
seconds each. The info line then shows `Vowels set: ah 850/1330 ee 290/2250 oo 350/1550 Hz` and the
values are saved. Without it the app uses averages for adult voices, which work for most deep and
mid voices; higher voices benefit most from calibrating. If it says the vowels came out too
similar, get closer to the mic and exaggerate the mouth shapes a little.

## Lip-sync styles

| Style | Mouth | Good for |
|---|---|---|
| Visemes (default) | 5 shapes: rest, mid, open, round "oo", spread "ee" | Most streams: readable and steady |
| Detailed | 7 shapes: adds teeth for s/sh/f and pressed lips for m/b/p | Close-up talking-head videos; busiest |
| Flap | Open/shut, one flap per syllable | Classic PNGTuber look |
| Volume | Closed / half / open by loudness only | Noisy rooms, music, or heavy mic processing |
| Chatter | A random shape every syllable | Comedy bits, retro-RPG gibberish feel |

All five ignore what language you speak; they listen to loudness and tone, not words.

## Control it while you're in a game

The avatar window only hears the keyboard when it has focus, so the app also serves a control page
at **http://127.0.0.1:7777/** with buttons for every face, lip style, emote, gesture and setting,
plus a searchable list of every clip the current character has (Play once, or Loop as its resting
pose). The buttons light up to show what's on.

- **Inside OBS:** Docks > Custom Browser Docks, name it "Avatar", URL `http://127.0.0.1:7777/`.
- **Stream Deck / Companion / scripts:** send an HTTP GET to `/cmd/<command>`:
  `face:0`–`face:9`, `play:Victory` (any clip name), `loop:Rally dance` (`loop:` back to Idle),
  `own:0`–`own:3`, `emote:nod` (nod shake tilt hop spin bow shiver lean), `lip` (next style) or
  `lip:Flap`, `react`, `bop`, `body` (body language), `vowels` (calibrate), `formants`, `model:next`, `model:prev`, `mute`, `calibrate`, `gate:2`,
  `gate:-2`, `bg`, `frame`, `hud`, `autotalk`, `throw:tomato` / `throw:egg` / `throw:confetti`
  (add `:ViewerName` to credit them and apply the per-viewer cooldown), `chat` (chat throws on/off),
  `twitch:channelname`, `say:Any text` (speech bubble), `bubbles`, `caption_file:C:\path\captions.txt`,
  `retro`, `retro:4` (pixel size 2-12). URL-encode spaces as `%20`.
  Example: `curl http://127.0.0.1:7777/cmd/emote:spin`
- `GET /state` returns the current settings and the clip list as JSON.

The page is split into tabs (Stream, Chat, Questions, Show, Clips, Hotkeys). The strip along the
top shows the mic level, frame rate, Twitch, webcam, recording and questions at a glance, and
**Live preview** shows what the shorts camera sees. Anything that goes wrong shows in red above
it (and in `avatar.log`).

**Global hotkeys (Windows):** key combinations that work while your game has focus. The Hotkeys
tab lists them: click the box, press the keys, type the command (any command from the list
above), Add. They live in `hotkeys.txt` in your files folder, one per line
(`Ctrl+Alt+T = throw:tomato`). Defaults: Ctrl+Alt+1-4 faces, T tomato, C confetti, H hop,
S spin, M mute, E clip, N next question, D done, Right next character. A combination another
program already uses shows as taken.

It listens on your own machine only (127.0.0.1). Change the port with `--port 8800`.

## Use it as a webcam (Windows)

The app sends frames straight to the **OBS Virtual Camera** driver. OBS doesn't need to be open.

1. Install OBS Studio (obsproject.com). That installs the "OBS Virtual Camera" device.
   If it doesn't show up in your video app later, open OBS once, click **Start Virtual Camera**,
   then **Stop** and close OBS.
2. Start the avatar and press **W**, or run `cargo run --release -- --webcam`. The info line says
   `Webcam: OBS Virtual Camera 1280x720 @ 60 fps, sending 60.0`.
3. In Zoom / Discord / Meet / Teams, choose **OBS Virtual Camera** as your camera.

Good to know:
- **One user at a time.** If OBS's own virtual camera is running, the avatar can't take the
  driver; the info line says so. Stop it in OBS and press **W** again.
- **The background shows.** A webcam can't send transparency, so the call sees the green (or
  studio) background. Press **B** and pick Studio for a plain dark backdrop.
- **The HUD isn't in the webcam picture**, and the picture is 16:9, so it shows a bit more on
  each side than the square window. **V** changes the framing for both.
- **60 fps needs the window to draw at 60 fps.** The webcam copies frames off the same render loop
  as the window, so on a 60 Hz or faster monitor it sends 60; on 144 Hz it picks every 2nd or 3rd
  frame to average exactly 60. Leave the avatar window open (it can sit behind other windows); if
  rendering pauses, the last frame keeps going so the call never flashes to OBS's placeholder.
- Other sizes or rates: `--webcam-size 1920x1080 --webcam-fps 30`. These and on/off are saved.
- Some apps that only use Windows' newest camera system may not list DirectShow cameras like this
  one; Zoom, Discord, Chrome-based Meet and Teams do.

For testing without the driver, `--webcam-dump frames.nv12` writes the frames to a file instead
(play it with `ffplay -f rawvideo -pix_fmt nv12 -video_size 1280x720 -framerate 60 frames.nv12`).

## Test on Windows

About 20 minutes, most of it the first build. The webcam code was written and unit-tested on
Linux against a port of OBS's reader, but hasn't run on Windows yet, so this is the check.

**1. Install the tools** (PowerShell):
```
winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install Rustlang.Rustup
winget install OBSProject.OBSStudio
winget install Gyan.FFmpeg
```
The first line installs the Microsoft C++ linker and Windows SDK that Rust needs on Windows
(several GB; installing Rust through winget does not add them for you). Open a new terminal
afterwards so the new commands are on your PATH. If you see `linker link.exe not found`, the C++
tools are missing: run the first line again, or in the Visual Studio Installer choose Modify >
"Desktop development with C++".

**2. Register the camera.** Open OBS once, click **Start Virtual Camera**, then **Stop**, and close
OBS.

**3. Run the tests.** In this folder:
```
cargo test --release
```
The first build takes several minutes. On Windows the webcam test runs against Windows' real
shared memory (on Linux it uses a stand-in), so a pass here means the driver queue is right. If it
fails to compile or a test fails, keep the error text.

**Visual tests** (optional): `python tools/test/visual_test.py` renders eight fixed scenes with
`--snapshot` and compares them with `testdata/visual/ref/`; `report.html` in
`testdata/visual/out/` shows reference, now and difference side by side. The references were made
on Linux with software rendering, so on a Windows GPU run it once with `--update` first and keep
those as your own. The `Visual tests` GitHub workflow runs them on every push.

**4. Start the avatar with the webcam on:**
```
cargo run --release -- --webcam
```
The info line should read `Webcam: OBS Virtual Camera 1280x720 @ 60 fps, sending 60.0`. If Windows
asks for microphone access, allow it (Settings > Privacy & security > Microphone > Let desktop apps
access your microphone).

**5. Check that the camera is listed and measure its frame rate** (second terminal):
```
ffmpeg -list_devices true -f dshow -i dummy
ffmpeg -f dshow -video_size 1280x720 -framerate 60 -i video="OBS Virtual Camera" -t 10 -f null -
```
The first should list "OBS Virtual Camera". On the second, the `fps=` value on the progress line
should settle near 60.

**6. Look at the picture:**
```
ffplay -f dshow -i video="OBS Virtual Camera"
```
You should see the avatar on green, no on-screen info, mouth moving with your voice. Then try a
real app: Zoom (Settings > Video), Discord (Settings > Voice & Video > Test Video), or Google Meet.

**7. Check these behaviours:**

| Try | Expected |
|---|---|
| Press **W** off, then on | OBS's placeholder picture, then the avatar again |
| Press **Tab** to switch character | No flash to the placeholder while the next one loads |
| Put another window over the avatar | Webcam keeps running at 60 |
| Minimise the avatar | Probably freezes on the last frame; if so, keep it behind other windows instead |
| Start Virtual Camera in OBS, then press **W** | Info line says the camera is already in use |
| Press **B** for the Studio background | The call shows a dark backdrop instead of green |

**If something's off**
- **Not listed in an app:** repeat step 2 and restart the app. Apps built only on Windows' newest
  camera system (such as the built-in Camera app) may never list it; use Zoom, Discord or ffplay.
- **Below 60 fps:** compare the info line with ffmpeg. If "sending" is also below 60, the window
  isn't drawing at 60 (monitor refresh rate or GPU). If the info line says 60 but ffmpeg measures
  less, the problem is on the camera side.
- **Wrong colours or a garbled picture:** take a screenshot of the ffplay window.

## Stream extras

### Speech bubbles (**S**)

The bubble sits above the head, flips to the other side near the edge of the picture, types out
40 letters a second, scrolls old words off after about 80 letters, and disappears 3 s after the
last words. It shows in the window and in the virtual camera. Three ways to feed it words:

1. **Built-in Whisper (best).** Speech-to-text runs inside the app on your own machine.
   - Build with it: `cargo run --release --features whisper`. That compiles whisper.cpp, so it
     needs CMake and, on Windows, LLVM as well: `winget install Kitware.CMake`, then
     `winget install LLVM.LLVM`.
   - Download a model from huggingface.co/ggerganov/whisper.cpp (the `ggml-*.bin` files):
     `ggml-base.en.bin` (142 MB) is a good start, `ggml-small.en.bin` is more accurate but needs a
     faster CPU, `ggml-tiny.en.bin` is the fastest. Put it in the `whisper` folder inside your files folder (see "Your files" below), or in `assets/models/whisper/`, or point
     `whisper_model=` in avatar_settings.txt at it.
   - The info line then says `Bubbles: on (Whisper (ggml-base.en.bin))`. Words appear about a
     second after you say them and are corrected as the sentence goes on.
2. **A caption file.** Any captioning tool that writes its latest line to a .txt or .srt file works,
   for example the OBS plugin LocalVocal (which also runs Whisper) with file output switched on. Set
   `caption_file:` on the control page (or `caption_file=` in avatar_settings.txt); the app checks
   the file 5 times a second and shows its last line.
3. **Typed.** The control page's "Say" box, or `GET /cmd/say:Hello%20chat` from a Stream Deck
   button. Handy for canned lines.

### Chat throws (**Q** to test)

- **Twitch:** type your channel name on the control page and press "Read chat". The app reads
  chat anonymously (read-only, no account or token), so there's nothing to authorise.
- Viewers type `!tomato`, `!egg` or `!confetti`. Each viewer can throw once every 12 s, there's at
  least 0.6 s between throws, and at most 3 things fly at once, so a spam wave stays watchable.
  The thrower's name shows at the bottom ("PeanutFan42 threw a tomato").
- Tomatoes and eggs arc in from the side, splat on the face for about 3.5 s and make the avatar
  flinch; confetti rains down and the avatar cheers and hops. All of it is 3D, so it shows in the
  virtual camera too.
- **YouTube, Kick, channel points, bits:** have your chat bot (Streamer.bot, Mix It Up, Nightbot's
  `$(urlfetch)`) call `http://127.0.0.1:7777/cmd/throw:tomato:ViewerName`.
- Turn viewer throws off with the "Chat throws" button; your own buttons keep working.

### Retro sprite mode (**Y**)

The avatar renders at a fraction of the size and is shown with big square pixels: every pixel
snaps to the character's own palette (each colour in three shades, so the lighting still reads),
and the silhouette gets a one-pixel dark outline. Pixel size 4 by default; `retro:3` is finer,
`retro:6` chunkier. It applies to the window and the virtual camera; speech bubbles stay crisp on
top. The palette is read from each character's model, so the dog stays golden and the riot officer
stays black.

### Viewer questions

Viewers record a short question on a web page, pick a character (each shown with its portrait), and send it. You listen to it
privately, approve it, and when you air it their character pops in next to yours, the camera
widens to a two-shot, and it asks the question **in their own recorded voice** (not changed or
synthesised), mouth and brows moving with it, the words typing out in its speech bubble. Your
character turns to listen. Afterwards the guest waits with a curious head tilt until you've
answered and press **Done**.

**Set up (once)**

1. **Twitch sign-in for viewers.** Go to dev.twitch.tv/console, *Register Your Application*:
   any name, OAuth Redirect URL `http://localhost`, category *Other*, **Client Type: Public**.
   Copy the Client ID into avatar_settings.txt as `twitch_client_id=...`. No secret is needed:
   viewers sign in with a code at twitch.tv/activate, the app reads their username once and throws
   the token away. (No Twitch? `ask_names=true`, or the **Allow typed names** button, lets
   viewers type a name instead; those show as *unverified*.)
2. **Let viewers reach the page.** The ask page runs on `http://127.0.0.1:7778/` and only answers
   your own machine. Put a tunnel in front of it, for example Cloudflare's free quick tunnel:
   `winget install Cloudflare.cloudflared`, then `cloudflared tunnel --url http://localhost:7778`.
   It prints an `https://....trycloudflare.com` address: share that in chat or a panel. Only the
   ask page goes through the tunnel; the control page (7777) stays private.
3. **Sound.** The question plays on your default speakers. To send it only to OBS, set
   `guest_output=` to part of a device name (a virtual audio cable, for example) and add that
   device to OBS. While a question plays the app ignores your mic so speaker spill doesn't move
   your character's mouth (`guest_mutes_host=false` turns that off, if you wear headphones).

**During the stream**, on the control page's *Viewer questions* section (or the OBS dock):

- **Questions open** switches the page on and off. Closed, it just says so.
- Each question shows the viewer, Twitch or *unverified*, character, length and chat votes, with a
  player. **Listen first**: Approve stays greyed out until you've played it. Only approved
  questions can air.
- The text box is what the bubble shows. Whisper builds fill it in automatically; otherwise type
  it, or leave it empty and the bubble just says "*name* asks:".
- **On air** (or **Air next**, or **A**) brings it on. **Done** (or **A** again) thanks them and
  they pop off. **Cut** stops the sound instantly and removes them.
- Chat can push questions up the line with `!up 12` (one vote per viewer, not their own).

**Moderation**

| | |
|---|---|
| Limits | 30 s per question (`ask_max_secs=`), 1 waiting per viewer, 3 per stream (`ask_per_viewer=`), 60 s between sends, 200 in the queue. Uploads are size-checked and rate-limited per address. |
| Nothing airs unheard | New → you listen → Approve → On air. The page never plays anyone's question to other viewers. |
| Ban | Removes all of that viewer's waiting questions and blocks them (typed names: their address too). **Banned** lists them, with Unban. Stored in `questions/banned.txt`. |
| Twitch bans carry over | When you or your mods ban someone in chat, they're banned here; a timeout blocks them for its length. Needs your channel set under *Audience* (chat is read anonymously). |
| Blocked words | `questions/blocked_words.txt`, one per line, then **Reload blocked words**. Matching transcripts get a red flag; a matching username shows as "a viewer" on stream. |
| Panic | **Cut**, or **A** while it's talking. |
| Clean-up | A question's recording is deleted once it's done, skipped or banned; everything is dropped after 24 h. Twitch tokens are never stored. |
| Who can press buttons | The control page only takes commands from your own machine; other websites can't send it any. |

Other settings: `ask_title=` (the page heading), `ask_characters=Dog,Cat,Peanut` (limit the choice;
default all), `ask_port=` / `--ask-port` (default 7778). Questions survive a restart: they're in
`questions/` next to avatar_settings.txt.

## The show

Everything below is on the control page too, and every button is a `GET /cmd/...` a Stream
Deck, a chat bot or OBS's Advanced Scene Switcher can call.

### Chat dresses you up

| Chat types | Does |
|---|---|
| `!hat cowboy` | cowboy, crown, party, tophat, propeller, chef, viking, pirate, halo, cone, wizard, hardhat, santa, grad, beanie |
| `!glasses shades` | shades, nerd, moustache, clown (nose) |
| `!hold coffee` | coffee, burger, soda, donut, phone, mic, book, sign, bell, broom, gavel, flashlight |
| `!wear crown` | any of the above, whatever slot it goes in |
| `!hat off` | takes it off (`!glasses off`, `!hold off`) |

One of each at a time; whoever changed it gets a credit line. Each viewer can change something
every 30 s; choose who's allowed on the control page (everyone, subs and VIPs, or mods). Items
come off by themselves after 10 minutes (`dressup_minutes=`, 0 = never). Hats sit on top of each
character's head, cap included (measured by `tools/assets/build_wardrobe.py` into `data/head_fit.json`);
held items stay upright whatever the arm does.

### Twitch moments

Read from the same anonymous chat connection as throws, so nothing to authorise.
- **Raid:** one little character per 10 raiders (3 to 16) runs on from both sides, cheers in front
  of you and runs off; the camera goes wide for it; confetti and a victory pose.
- **Sub, resub, gift subs:** confetti, a victory pose, a hop and a thank-you line.
- **Bits:** a tomato per 100 bits in a quick volley (up to 8), plus a giant one from 500.
- Test them with **Test raid / Test sub / Test gift subs / Test 600 bits**. Turn off with `raid_crowd=false`,
  `sub_party=false`.

### Block bevels (hand-made normal mapping)

There's no high-poly model to bake detail from, so the app paints it by hand: a small normal-map
atlas, one tile per voxel face, with a rounded bevel on every real outside edge (the corners
catch the light like toy blocks), a faint seam where blocks sit side by side, and a tiny
per-block wobble so flat areas aren't dead flat. At load, each model's merged quads are cut back
into voxel faces so every face knows which of its edges are outside edges. It works on every
character, hat, prop and set, and costs no extra draw calls. Look section of the Stream tab:
**Block bevels** (on/off, to compare) and sliders for Bevel, Seams and Wobble
(`bevel:on`, `bevel:<0-100>`, `bevel:grid:<0-100>`, `bevel:wobble:<0-100>`).

Surfaces can carry their own detail too: **stubble** (a beard), **hair** (strands that run
down the head, also on its sides) and **cloth** (a diagonal twill for denim and caps). Each is a
64 x 64 texture per block face, drawn from one height field that drives both the colour (darker
gaps and whiskers, lighter strand tops) and the bump, so the two line up. The block's own
palette colour lives in the mesh and the detail shades it, so every character keeps its colours.
`data/surfaces.json` says which palette colours of which model are which (the lead and most of
the cast are set up; `tools/assets/guess_surfaces.py` makes first guesses for new characters):

    "models/dog.glb": { "c8843c": "hair" }

The **Beard, hair, cloth** slider sets their strength (`bevel:detail:<0-100>`).

**Blend blocks** (on by default, `bevel:blend`) makes blocks of one surface read as one surface:
no seam line between neighbours, much less per-block tilt, and one wrapping pattern per surface
so strands and threads run straight across block edges. Turn it off for the separate-blocks look.

**Caps come off for hats.** Ten characters have a cap or hat built into the head (the lead's red
cap, the officer's, the rally hat, the peanuts'...). Each carries a second, bare head (built by
`tools/assets/build_bare_heads.py`, with hair or shell where the cap was), and a dress-up hat swaps it
in, so the hat sits on the head instead of on top of the cap. Helmets stay on.

### Blocks and levels

Besides the modelled sets there are **block levels**: sets built from half-metre blocks with
realistic textures. Sixteen blocks come with the app: grass, dirt, stone, cobblestone, bricks,
planks, log, sand, gravel, snow, leaves, glass, concrete, metal, tile and lamp (it glows).
Every texture is original, drawn from noise by `tools/assets/build_blocks.py`, and tiles seamlessly.
Each has its own bumps (the mortar between bricks, the gaps between cobbles, bark), its own
shine (glazed tile, brushed metal, matte dirt), and corners where blocks meet are shaded.

- `set:yard`: a back yard with a brick house, a cobblestone path between lamp posts, trees, a
  snowy hill, crates and a sand pit. `set:blocks` shows every block on one wall.
- **Your own textures:** the tiles are plain PNGs in `assets/blocks/tiles/`: `<name>.png` for
  the colour and `<name>.height.png` for the bumps (white is high; without one, the colour's
  brightness is used). Repaint any of them (128 x 128, and keep it wrapping at the edges) and
  restart.
- **Your own blocks:** `data/blocks.json` lists each block's top, side and bottom tiles,
  roughness, metal, bump depth, and whether it's glass or glows.
- **Your own levels:** a level is a JSON file of boxes of blocks, in `levels/` in your files
  folder (or `data/levels/` next to the app); `set:<file name>` loads it. Each box gives a
  block and two corners in block units (`(0, 0, 0)` is where the character stands, the floor's
  top is `y = 0`, and `-z` is away from the camera), and can be `hollow` (just the walls) or
  `round` (a tree crown, a hill). Later boxes overwrite earlier ones, and `"air"` cuts holes
  (doors, windows). `data/levels/yard.json` is a worked example. An editor to do this by
  pointing and clicking is the next step.

### Studio lighting

On by default (**Studio lighting** on the Stream tab, `light:on`, to compare):
- **Three-point lighting:** a warm key light, a cool fill from the other side, and a rim light
  from behind and above that outlines the character (it helps after keying, too), with less
  flat ambient light.
- **Shadows** from the key light: the cap brim shades the face, arms shade the body, and on a
  set the character casts a shadow on the wall and floor. The plain backgrounds stay one flat
  colour, so green-screen keying is unaffected. **Shadows** turns just these off on a slow PC
  (`light:shadows`).
- **Corner shading** baked into the blocks where they meet (block-game ambient occlusion).
- **A slight plastic sheen** on characters and props, so the bevels catch highlights; sets stay
  matte. Eyes are glossier, with a small glint.
- **Each set has its own light:** warm diner, cool blue night, bright rally, flat-white
  newsroom and press room, green-tinged hospital.
- **Glow** (`light:glow`) on bright props (the diner's EAT sign, the night windows, the hospital
  cross, the newsroom screens). It's only on with a set or the studio backdrop, never over a green
  screen, where the haze would spoil the key.
- **Event lighting:** a raid sweeps a spotlight across the stage, a sub cycles party colours, a
  big hit flashes red, the press conference fires camera flashes, and singing brings up a
  follow spot.
- **Drop shadow** under the character in transparent-window mode, so it doesn't float over your
  game.

- **Colour grading** (`light:grade`) on the dark scenes: a filmic curve with a little lift and
  colour on the night set and the studio backdrop. The bright sets look better without it (the
  curve dulls bright colours), and it's never applied over a green screen.

- **Background blur** on sets (on by default; **Background blur** button and slider on the Stream
  tab, `light:blur` to toggle, `light:blur:<0-100>` for strength): a depth of field focused on
  the character's face, so the set behind softens and things right in front of the lens (a
  raised hand, the near floor) do too, like a real camera. It works out the lens for each camera
  (the window, shorts, webcam) from how far away the character stands, so the background is
  just as soft in a close-up as in a full shot. Never over a green screen or the studio
  backdrop, and off at Low quality.

**Quality** (Stream tab, `quality:low|medium|high|auto`): High has everything; Medium drops
glow and uses smaller shadow maps; Low also drops shadows and bevels. Auto (the default) starts
at High and steps down when the frame rate stays under 45.

### Motion

Hats and tails swing when you turn or hop and settle back. After a few quiet seconds the
character does small idle things (looks around, shifts weight, stretches, checks a watch);
turn that off with **Idle life** (`idle`). Emotes and gestures blend into each other instead
of snapping. The mouth now lags the mic by about 50 ms at 60 fps; the control page shows the
exact figure, and setting your mic's **Sync Offset** in OBS (Advanced Audio Properties) to it
lines the sound up with the mouth.

### Scenes

Save the current character, set, framing, background, lighting and outfit as a named scene
(Show tab, or `/cmd/preset:save:Late show`), and switch back in one step (`preset:load:Late show`,
handy on a Stream Deck; `preset:del:` removes one). They're kept in `presets.json` in your files
folder.

### Go-live check

**Go-live check** at the top of the control page runs the setup checks (disk space, the viewer
question page, ffmpeg and the rest), then watches for five seconds while you talk: frame rate,
mic level, Twitch chat, webcam output and clip recording, plus the OBS sync offset to use.

### Sound effects and feel

Real recordings (CC0, credits in `assets/sounds/README.md`): a splat when a tomato or egg lands,
a pop for confetti, a knock when a hat drops on, a thump when you land a hop, applause for a raid
and fireworks for a sub. They play on your default output and go into the shorts. Turn them off
or down on the Show tab (`sfx:on`, `sfx:vol:40`); put your own `splat.wav` (etc.) in your files
folder's `sounds/` to replace one.

The motion has more give, too: a hit squashes the head flat and it wobbles back, a giant tomato
shakes the camera and kicks up dust, hops crouch first, stretch on the way up and squash on
landing, and hats drop onto your head and bounce. The eyes glance toward a tomato on its way in,
look around now and then while you're idle, and blinks vary (the odd double or slow blink).

### Viewer crowd

With **Chatters join the crowd** on (Chat tab, `crowd:on`), people who talk in chat appear on
stage as small characters with their name above them: up to eight, filling in on both sides,
cheering when they talk and wandering off after three quiet minutes. **Test chatters**
(`crowd:test`) fills it with made-up names.
- The name sign's border shows where they chat from (Twitch purple, YouTube red, Kick green);
  subscribers, members, VIPs and mods get a star.
- `!me dog` (any character's name, or the start of it) picks who they are in the crowd.
- The crowd ducks when you're hit, dances for a raid or a sub, and during a tug-of-war poll
  everyone who voted walks over to their side.
- Your eyes glance at whoever just spoke.
- Someone banned or timed out on any platform is shown out, and their chat is ignored until the
  timeout ends. A YouTube or Kick ban never touches a Twitch viewer of the same name.

### Effects

Particle effects in the same block style (Show tab, `fx:` commands; how many at once follows the
quality setting):
- **Weather:** each set has its own (`fx:auto`): rain on the night set, dust motes in the diner's
  light, confetti drifting at the rally. Or pick one: `fx:rain`, `fx:snow`, `fx:dust`,
  `fx:drift`, `fx:off`.
- **Bursts:** fireworks for every sub, confetti cannons for every raid, sparks on a big hit, and
  on demand: `fx:fireworks`, `fx:confetti`, `fx:sparks`, `fx:hearts`, `fx:stars`.
- **On you:** Zs while you doze, sweat when you shout.
- **From chat:** `<3` sends up hearts, a fire emoji sparks, `GG` pops confetti (a burst every few
  seconds at most). **Effects** turns all of it off (`fx:on`).

### Live face capture

The face can copy yours from a face-tracking app (**Face capture** on the Stream tab, on by
default; it only listens on this computer and does nothing until a tracker sends):
- **iPhone:** iFacialMocap (or any app that speaks its format): set the PC address in the app to
  this computer's address; it sends to port 49983.
- **Webcam or other trackers:** VSeeFace, Warudo, VNyan and others send the VMC protocol: point
  their VMC sender at this computer, port 39539.

Blinks, wide or squinting eyes, looking around, brows (up, worried, angry) and mouth shapes
(smile, smirk, frown, pout, "o", tongue) pick the face's parts, and your head's turn, nod and
tilt move the character's head. Your voice still decides how open the mouth is, so lip sync stays
tight; with the mic quiet, opening your jaw opens the mouth. If the head turns the wrong way,
**Mirror head turn** (`facecap:mirror`) flips it. Other ports: `facecap_vmc_port`,
`facecap_ifm_port` in avatar_settings.txt.

### YouTube and Kick chat

Chat from YouTube and Kick works like Twitch chat (throws, dress-up, polls, votes, the crowd),
alongside it or instead of it. Set them up under **YouTube and Kick chat** on the Chat tab.
- **YouTube** needs your own YouTube Data API key (Google Cloud console: create a project, enable
  "YouTube Data API v3", create an API key) and the live stream's link. Every check for new
  messages uses some of the free daily quota, so the app checks every 8 seconds, which is roughly
  a 4-hour stream a day; set `youtube_poll_s` in avatar_settings.txt to change it. Super Chats
  count as bits (one dollar = 100 bits), new members as subs.
- Bans and timeouts from YouTube and Kick moderators work like Twitch ones (the crowd shows them
  out; their messages are ignored for the timeout).
- **Kick** needs the channel name. The app reads chat the way Kick's own chat page does, without
  signing in; that isn't an official Kick API and can change. If Kick blocks the channel lookup,
  enter the chatroom number instead.

### Tug-of-war polls

Type a question and two options, pick how long, **Start poll**. Chat votes `!1` / `!2` (or `!a`,
`!b`, or the option's first word); voting again changes your vote. Two characters pull a rope
along the bottom of the picture, the knot follows the vote share, and at the end the winner
celebrates and the loser falls over. From a Stream Deck: `/cmd/poll:start:60:Best snack?|Pizza|Tacos`.

### Your voice does more

- **Laugh:** your character throws its head back, eyes squeezed shut. It listens for quick, evenly
  spaced, breathy "ha"s. On real recordings it catches 9 of 16 laughs and never fired on any of 11
  speech recordings or the singing.
- **Sing:** a microphone appears in its hand, music notes float off its head and it sways, until
  you stop. It listens for held notes, which speech rarely has; it notices within about 2 s.
- **Catchphrase gags:** write `gags.txt` next to avatar_settings.txt (one is made for you):

  ```
  let's go = throw:confetti; emote:hop
  big if true = emote:spin; face:4
  mic drop = sound:boom.wav; emote:bow
  ```

  Any control-page command works as an action; `sound:` plays your own .wav from `sounds/`. The
  words come from the speech bubbles' source (built-in Whisper, a caption file, or the Say box),
  so gags need one of those. Each gag waits 8 s before it can fire again. **Reload gags.txt**
  after editing.

### Live co-host

A second character on your left, driven by a second microphone with the same lip sync, nods and
blinks as yours; you glance over when they talk. For someone on a call, send Discord's output to
a virtual audio cable (VB-Cable is free) and pick that as the co-host mic. Type part of the mic's
name on the control page (`--list-mics` shows them), **Co-host on stage**, **Next character** to
change who they play. Settings: `cohost_mic=`, `cohost_model=`. Test without a second person:
`--cohost-wav file.wav`.

### Shows over the network (relay, guests, viewers)

Stream your show to other copies of the app through a relay: viewers see exactly what your
character does (every joint, the face, the camera), and a guest on another computer stands
beside you with their own character, face and voice.

1. **Run the relay** somewhere both of you can reach: `cargo run --release -p vox-relay --
   --listen 0.0.0.0:7779`. For the internet, put it behind TLS (a reverse proxy, or Cloudflare)
   and give everyone the `https://` address. `http://<relay>:7779/` lists the live shows.
2. **Go live:** control page, **Go live**: type the relay address, **Use relay**, then **Stream
   to the relay**. The status line shows the join code and the watch link.
3. **Watch:** `block_avatar --watch http://<relay>:7779/<session id>` (add `--code ABC123` for a
   private show). Your own mic and reactions are switched off; the host's scene, character and
   camera take over, 100 ms behind.
4. **Be a guest:** `block_avatar --join http://<relay>:7779/<session id> --code ABC123 --name
   Sam`. You keep your own character, mic and face capture and stand on the host's left; the
   host clicks **Let guest on**. Viewers see you both.

Voices go as they are (no voice changing), 16 kHz, about 8 KB/s each, and only while someone is
talking. **Use headphones**: a voice from your speakers would reach your mic and echo back. To
send the other voices to OBS, set `net_output=` to part of an output device's name (a virtual
cable). Settings: `relay_url=`, `net_output=`. Testing a bad network: run the relay with
`--simulate-delay 200 --simulate-loss 0.05`. `BLOCK_AVATAR_NET_DEBUG=1` logs what arrives.

### Sets, auto-director, press conference

- **Sets** replace the green screen: rally, pressroom, diner, hospital, newsroom, night. **auto**
  picks one to suit the character (the medic gets the hospital, the cook the diner, senators the
  press room...). `set=` in the settings, or `/cmd/set:diner` from a scene switcher.
- **Auto-director:** cuts (not glides) between head-and-shoulders and waist-up every so often
  while you talk, goes wide after 8 s of quiet, cuts back in when you speak, and punches in close
  on a splat or a laugh. Raids, guests and the co-host still get their own shots.
- **Press conference:** a podium in front of you, and the approved viewer questions waiting as a
  row of little reporters (in the characters they picked) along the bottom; the next one up waves
  to be picked. **A** calls on them.

### Shorts and comic strips

A third camera records the show in portrait (9:16) with the same bubbles, credits, sets and retro
look, and keeps the last 45 s in memory with the sound (your mic, viewers' questions, gag sounds).
- **Clip that:** **E**, the control page, or a mod typing `!clip`: the last 20 s.
- **Auto shorts:** a splat, a laugh, a sub or raid, a gag, a poll result and a song save
  themselves (9 s before, 5 s after), and every viewer question you answered is saved from when
  they arrive until you press Done. A question you cut is never saved.
- **Title cards and captions** (on by default, **Title cards + captions** to turn off): every
  short opens on a 1.4 s card with your character's portrait and what happened, carries that line
  as a caption across the top, and ends on a 1.6 s card with your Twitch channel.
- **Comic strips:** each one also becomes a 4-panel strip (PNG) around the moment, with a caption
  bar under it (what happened, and your channel).
- **Highlight reel** (Clips tab, `clip:reel`): joins this stream's newest 10 shorts, oldest
  first, into one `reel-<time>.mp4` (no re-encoding, so it takes seconds).
- Files go to `clips/` in your files folder (**Open clips folder** on the Clips tab): 1080x1920 .mp4 with AAC sound, ready for TikTok, Shorts or Reels.
  This needs ffmpeg: `winget install Gyan.FFmpeg`. Without it you get the frames and sound in a
  folder with a MAKE_VIDEO.txt holding the one command that joins them.
- It costs one extra small render; turn **Recording** off if your PC is struggling.

### Stream Deck

Every control is a web address too: `docs/avatar/STREAMDECK.md` lists them, ready to paste into Stream
Deck's Website action (with "GET request in background").

## Put it in OBS

1. Add a **Window Capture** (macOS: **macOS Screen Capture** > Window) of "Block Avatar".
2. Right-click the source > Filters > **Chroma Key**, key colour Green (or Magenta if you wear green).
3. Size and place it over your scene.

Transparent instead of chroma key: start with `cargo run --release -- --transparent`. On Windows use
**Game Capture** with "Allow Transparency" ticked. Transparent windows depend on the OS and GPU
driver, so if you see black, use the green background.

## Microphone

It uses your system's default input. To pick another: `cargo run --release -- --list-mics`, then
`--mic "part of the name"`. Shure, Rode, Elgato, Focusrite and USB headsets all work through the
normal OS audio stack.

The gate calibrates for 1.5 s at startup, so stay quiet when it launches, or press **N** later.
Push-to-talk style: press **M**.

## Test without talking

To try the question queue without viewers: open `http://127.0.0.1:7778/` in your browser with
typed names allowed, and record or upload one of the real questions in `demo/questions/`.

`cargo test --release` runs the lip-sync, voice, webcam, chat and bubble tests, including checks on real recordings
(`testdata/real`: six speakers from the Free Spoken Digit Dataset and four LJ Speech sentences).
`cargo run --release -- --wav demo/libri-female-198-209-0000.wav` drives the mouth from a real
recording, looped; there are two male readers in `demo/` too (all LibriSpeech, see `demo/README.md`).
`cargo run --release -- --analyze in.wav out.csv Flap` writes the mouth shape for every 1/60 s in
any style, which is handy for tuning or for lip-syncing a pre-recorded video. The CSV also has pitch,
F1/F2 and the body-language events (`beat`, `question`), and uses your calibrated vowels if
`avatar_settings.txt` has them. Add `--no-formants` to compare against tone-only vowels.
The best test is your own voice: record yourself on your streaming mic (any WAV), then run it
through `--analyze` or `--wav`.

## How the lip sync works

`src/lipsync.rs` measures, every 10 ms: loudness, the share of energy below 500 Hz, above 900 Hz
and above 2.5 kHz, how often the waveform crosses zero, and syllable starts (sudden jumps in level).
Loudness above the noise gate decides closed, slightly open or wide open, and "wide" adapts to your
own speaking level, so mic gain doesn't matter much. Low-heavy sound means rounded ("oo", "oh"),
bright means spread ("ee"), bright plus lots of zero crossings means hiss ("s", "sh"), and a quick
dip on a low hum means pressed lips ("m"). Flap and Chatter use the syllable starts. Each shape
holds for 50–90 ms so the block mouth doesn't flicker.

`src/voice.rs` adds two measurements on a 11-16 kHz copy of the signal. **Pitch** comes from the YIN
algorithm on a 40 ms window (70-400 Hz), with a clarity score that says whether the sound is voiced.
**Formants** come from Burg LPC (order = kHz + 2) on a 25 ms window, read off the roots of the
predictor as Praat does: F1 is how open the jaw is and F2 how spread
the lips are, so on voiced frames the Visemes and Detailed styles place you in your own vowel space
(jaw open: ah; lips spread: ee; otherwise rounded: oo/oh) instead of guessing from tone. Consonants
and anything unclear fall back to the tone rules. **Prosody** tracks your pitch against your own
running average: a louder or higher syllable start is a beat (nod, hand beat on alternating arms), a
phrase that ends 4+ semitones above where it was and is followed by a 0.6 s pause gets a curious head
tilt for about a second (commas rise too in natural English, but rarely pause that long), and
pitch held 3.5+ semitones up raises the brows when the face's brows are neutral. All of this costs
well under a millisecond per 10 ms of audio (offline it analyses about 100x faster than real time).

Measured on real, unmodified recordings (`tools/test/eval_voice.py`, Praat as the reference):

| | Result |
|---|---|
| Pitch, 3 LibriSpeech readers (2 male, 1 female) | same voicing decision as Praat on ~93 % of frames; median error 5-8 cents; under 5 % of frames off by more than 20 % |
| F1 / F2 against Praat, same readers, at 16, 44.1 and 48 kHz | median error 5-8 % (F1) and 3-5 % (F2) |
| Vowel decision in the vowel's centre, 6 speakers, 312 words | 89 % right (91 % after calibrating) |
| Mouth shown across each vowel (incl. transitions), same words, uncalibrated | 62 % right with formants, 24 % with tone only |
| Rising-ending tilt on 11 recordings of read speech (LibriSpeech, LJ Speech) | 0 false tilts |

The real-voice sets are all English and mostly male; higher voices were only checked through one
LibriSpeech reader, so calibrate (**P**) if you have a higher voice. Real questions weren't in any
set available here, so the tilt's hit rate on questions is untested; it needs your voice. All of
it runs on the audio thread in well under a millisecond, so mouth latency is the mic buffer plus
one frame.

The face, emotes and bop are applied after Bevy's animation step every frame, so they always win
over the expressions baked into the body clips and stack on top of any gesture or loop. The bow
bends at the waist and counter-rotates the legs so the feet stay planted.

## Models

All characters share one rig and one face kit, so every animation, face and prop works on all of
them. Nothing in the cast is based on a real person. Line-ups are in `docs/avatar/`.

**Rig.** `Character > Rig > Hips > Head, UpperArm_L > LowerArm_L, UpperArm_R > LowerArm_R,
UpperLeg_L > LowerLeg_L, UpperLeg_R > LowerLeg_R`; each node's origin is its joint. 5 cm blocks,
about 1.85 m tall, facing +Z. Swappable parts are children toggled by scale (0 hidden, 1 shown):
8 eye shapes, 4 brow shapes, 11 mouth shapes, 3 hand shapes, plus face paint, horns and a hi-vis
vest. All files pass the Khronos glTF validator with no errors or warnings.

**Clips.** Every character has the lead's 100 body clips (0-99, listed below). The lead also has
24 face-only clips (100-123) that layer over any body clip. Cast members have their own clips from
100 up, with their props built in.

### Blue (`assets/models/blue.glb`)

Vox's main character, and who a new player or a first run starts as: an American pit bull
terrier with a blue nose, a blue-grey coat, a white blaze, chest and paws, rose ears and an orange
collar. Built on the shared rig by `tools/assets/build_animals.py` (like the other animals), with the
100 body clips plus Tail wag, Bark, Beg and Scratch. Source: `source/vox/characters/blue.vox`.

### Lead (`assets/models/scene_all.glb`)

The original lead, no longer the main character: red MAGA cap, blue overalls over a white undershirt, short brown beard. 124 clips, and each clip shows only
the props it uses, so it plays complete in any glTF viewer. Source: `source/vox/characters/lead.vox`.

- **0-15 Core moves:** Idle, Walk, Run, Jump, Pick up, Carry, Place item, Block, Push, Climb,
  Hammer, Throw, Hurt, Knockout, Victory, Emotes
- **16-31 Satire and extras:** Flag rally, Scale the scaffold, Lectern heist, Feet on the desk,
  Rotunda selfie, Horned costume, Tear gas, Livestream, Perp walk, Sentencing, Sharpie forecast,
  Boxes in the bathroom, Ballot audit, Fry cook, Crouch, Ledge grab
- **32-63 More satire and gameplay:** Foot wedge, Rally dance, 3 a.m. posting, Merch haul,
  Hamberders, Soda button, Eclipse, Windmill rant, Paper towel toss, Build the wall, Tariff receipt,
  Egg prices, Crowd size, Boat parade, The weave, Rigged tantrum, Meme coin, Mugshot, Rake the
  forest, Garbage duty, Wrong venue presser, Rally chant, Idle (long), Belly slide, Swim, Coin grab,
  Talk, Sleep, Respawn, Tomato hit, Lawn chair, Enter door
- **64-67 Movement basics:** Sprint, Sneak, Crawl, Belly crawl
- **68-83 Combat, attacks:** Jab, Cross, Hook, Uppercut, Haymaker, One-two combo, Front kick, High
  kick, Stomp, Shoulder charge, Shove, Headbutt, Flying kick, Elbow strike, Overhead smash, Belly bump
- **84-99 Combat, defence:** Guard stance, High block, Low block, Parry, Duck, Dodge back, Backstep,
  Brace, Cover up, Catch and counter, Hit reaction (light), Hit reaction (heavy), Knockdown and get
  up, Guard break, Leg check, Trash-lid shield
- **100-123 Face layer:** Blink, Talk, Shout, Angry, Grin, Shocked, Sad, Smug, Wince, KO,
  Surprised, Nervous, Suspicious, Eye roll, Thinking, Scheming, Laugh, Disgust, Sleepy, Crying,
  Rage, Look around, Proud, Worried

### Cast (`assets/models/*.glb`, 25 characters)

| Group | File | Character | Own clips (100+) |
|---|---|---|---|
| White House | `president.glb` | Fictional president: royal-blue suit, gold tie, flag pin, short blonde hair, clean-shaven | Address the nation, Sign the bill, Salute, Wave from the stairs |
| White House | `vice_president.glb` | Fictional vice president: black suit, gold lapel pin, silver side-swept hair | Tie-breaking vote, Oath, Rope line, Photo op |
| White House | `agent_buzz.glb` | Secret Service agent: black suit, sunglasses, coiled earpiece, buzz cut | Flank walk, Post stance, Earpiece, Clear the path, Shield the principal |
| White House | `agent_shaved.glb` | Secret Service agent, shaved head | same as above |
| White House | `agent_bun.glb` | Secret Service agent, hair in a tight bun | same as above |
| White House | `staffer.glb` | Overworked Hill staffer: rolled sleeves, loose tie, lanyard, cowlick | Folder sprint, Coffee run, Whisper, Phone call |
| Congress | `senator.glb` | Charcoal suit, blue tie, silver side-parted hair | Floor speech, Handshake, No comment |
| Congress | `senator_bob.glb` | Slate skirt suit, silver brooch, silver bob | same as Senator |
| Congress | `senator_tan.glb` | Tan suit, brown tie, black hair greying at the temples | same as Senator |
| Congress | `senator_glasses.glb` | Charcoal suit, green tie, wire glasses, thinning white hair | same as Senator |
| Congress | `senator_plum.glb` | Plum pantsuit, gold pin, short natural hair with grey flecks | same as Senator |
| Congress | `representative.glb` | Burgundy blazer, gold member pin, full curly hair | Floor remarks, Objection, Cast vote, Town hall |
| Congress | `rep_open_collar.glb` | Navy blazer, open-collar shirt, brown quiff | same as Representative |
| Congress | `rep_ginger.glb` | Green blazer, copper-red ponytail | same as Representative |
| Congress | `rep_beard.glb` | Grey suit, purple tie, full trimmed beard | same as Representative |
| Congress | `rep_cream.glb` | Cream skirt suit, long straight black hair | same as Representative |
| Police | `officer.glb` | Capitol-style officer: peaked cap, duty belt and radio, badge | Escort walk, Halt, Radio check, Arrest |
| Police | `plainclothes.glb` | Detective: leather jacket, jeans, badge on belt, mustache | Badge flash, Blend in, Tail a suspect, Call it in |
| Police | `riot_a.glb` | Riot officer: helmet with clear visor, armour plates, shield | Shield line, Shield advance, Shield bash, Hold the line |
| Police | `riot_b.glb` | Riot officer, second face under the same gear | same as Riot officer A |
| Supporting | `judge.glb` | Black robe, white collar, glasses, thinning grey hair | Gavel strike, Sentence, Deliberate |
| Supporting | `reporter.glb` | TV reporter: teal blazer, press badge, black bob, microphone | Live report, Shout a question, Take notes |
| Supporting | `rally_sign.glb` | Rally-goer: flag-print T-shirt, USA cap with ponytail, sign | Sign wave, Sign march |
| Supporting | `rally_hat.glb` | Rally-goer: stars-and-stripes shirt, white beard, bucket hat | Fist pump, Crowd selfie |
| Supporting | `executive.glb` | Tech executive: black MAGA cap, black T-shirt and jacket, jeans, glasses | Keynote, Walk and talk, Check watch, Close the deal |

Sources: `source/vox/characters/<name>.vox`. Clip timing and dialogue: `data/cast_manifest.json`.

### Animals and peanuts (`assets/models/`)

Same rig, so all 124 shared clips play on them; each has its own clips after index 123 (F9-F12 in
the app). Animals have a `Tail` joint that sways in every clip. The face kit sits on the snout, so
lip sync works the same. Sources in `source/vox/characters/`; poses in `docs/avatar/animals-poses.png`.

| File | Character | Own clips |
|---|---|---|
| `dog.glb` | Golden-furred dog: cream muzzle and paws, floppy ears, red bandana with a gold tag | Tail wag, Bark, Beg, Scratch |
| `cat.glb` | Grey tabby: striped back and crown, white chest and socks, pink nose, whiskers | Stretch, Hiss, Knock it over, Groom |
| `rhino.glb` | Rhino in a navy suit, white shirt, red tie and flag pin; horn on the snout tip | Charge, Horn toss, Stomp, Snort |
| `donkey.glb` | Donkey in a blue blazer, open-collar shirt and jeans; tall ears, dark mane | Hee-haw, Back kick, Stubborn, Munch |
| `peanut.glb` | Original peanut character: raw two-lobed shell, stick limbs, orange ball cap | Tip cap, Backflip, Wobble, Startled |

### Roasted Peanut series (`assets/models/roasted_*.glb`)

One roasted, salted peanut (golden-brown shell net, toasted spots, salt crystals) in six outfits.
Each has five poses with hand props that appear only in the poses that use them. Poses in
`docs/avatar/roasted-peanut-poses.png`; props also as separate files in `assets/props/roasted/`.

| File | Outfit | Poses | Props |
|---|---|---|---|
| `roasted_army.glb` | Olive helmet with chin strap, webbing, pouches, radio pack, rifle slung on the back, boots | Salute, March, Scout, Radio call, Plant the flag | binoculars, handset, flag |
| `roasted_medic.glb` | Teal scrubs, white vest with a green first-aid cross, stethoscope, headband, blue gloves | Rush in, CPR, Listen, Bandage, Chart | medkit, clipboard, bandage |
| `roasted_miami.glb` | 1980s Miami detective: pastel jacket with pushed-up sleeves, pink tee, white pants, loafers, aviators | Badge flash, Too cool, Radio in, Freeze!, Donut break | badge, walkie, donut |
| `roasted_cook.glb` | Fry cook: red uniform, white apron with name tag, paper fold hat | Burger flip, Basket shake, Order up!, Salt it, Wipe brow | spatula, patty, basket, salt, bell |
| `roasted_garbage.glb` | Lime hi-vis vest with reflective bands, navy work clothes, orange cap, yellow gloves | Bag toss, Haul the can, Sweep, Ride the truck, Thumbs up | trashbag, can, broom |
| `roasted_riot.glb` | Black helmet with clear visor and neck guard, chest and back plates, shoulder and knee pads | Shield up, Advance, Hold the line, Kneel and cover, Radio check | shield (clear), baton, radio |

The medic uses a green cross on purpose: the red cross on white is a protected emblem.

Rebuild any of these with `python3 tools/assets/build_animals.py` or `python3 tools/assets/build_roasted.py`
(add names, e.g. `roasted_cook`, to rebuild just those). Outfits, colours, props and poses are all
plain Python in those two files.

### Variants (`assets/models/variants/`)

| File | What |
|---|---|
| `character_rigged.glb` | The lead without props, same 124 clips. Use it when your game spawns props itself. |
| `scene_all_stepped.glb` | The lead with props, each pose held until the next sprite frame so motion snaps like the 2D sprites |
| `character_rigged_stepped.glb` | Stepped twin of the prop-free lead |

The default files are smooth: the engine interpolates poses at any frame rate, with eased combat
moves. Stepped and smooth share node names and clip indices, so you can mix them per clip.
`tools/assets/make_stepped.py in.glb out.glb data/lead_manifest.json` makes a stepped twin of any rigged
model, cast included.

### Early versions (`assets/models/early/`)

| File | What |
|---|---|
| `maga_voxel.glb` | First voxel lead, 5 test clips |
| `maga_voxel_hd.glb` | Denser-voxel lead, 8 test clips |
| `maga_block.glb` | First block-style lead, 7 test clips |

`source/vox/early/` holds their sources plus `lead-toy-blocks.vox`, the toy-block redesign that
was compared side by side and not chosen.

### Props and effects (`assets/props/`, sources in `source/vox/props/`)

Origin is the grip point for hand-held items and the base centre for set pieces. See
`docs/avatar/rig-props-catalog.png`.

- **Lead props and set pieces (`lead/`):** bag, ball, bankbox, bed, binoculars, bleachers, boat,
  book, brick, burger, can, capstack, card, club, coin, columns, crate, crate_big, cuffs, desk,
  door_frame, door_panel, easel (+ 3 loop frames), eclipse, egg, flag, flashlight, fries, fryer,
  garage, hammer, height_chart, ladder, lawnchair, lectern, ledge, lid, night_backdrop,
  office_chair, paper, phone, pine, placard, podium, rake, receipt_seg, roll, scaffold,
  selfie_stick, sharpie, sneaker, soda_button, table_burgers, toilet, tomato, trashcan,
  turbine_rotor, turbine_tower, uv_beam, wall_ladder, wall_row0-6, water
- **Lead effects (`lead/fx_*`):** camflash, chart_crash, chart_up, chart_up2, cough, dust,
  eggsplat, gas, impact, leaves, live, notes, spark, sparkle, speed, splat, stars, sweat, tears, zzz
- **Cast props (`cast/`):** badge_wallet, coffee, flag_stand, folder, gavel, judge_bench, mic,
  notepad, pen, pres_podium, riot_shield, sign, votecard
- **Roasted peanut props (`roasted/`, origin at the grip):** badge, bandage, basket, baton, bell,
  binoculars, broom, can, clipboard, donut, flag, handset, medkit, patty, radio, salt, shield,
  spatula, trashbag, walkie
- **Wardrobe (`wardrobe/`, hats: origin at the bottom centre; face pieces: on the face):** beanie,
  chef, clown_nose, cone, cowboy, crown, grad_cap, halo, hard_hat, moustache, nerd_glasses, party,
  pirate, propeller, santa, shades, top_hat, viking, wizard

### Stage sets (`assets/sets/`, sources in `source/vox/sets/`)

rally, pressroom, diner, hospital (green first-aid cross), newsroom, night. 14 m wide, 6 m tall,
10 cm voxels; the back wall stands 2 m behind the character. See `docs/avatar/stage-sets.png`.

`data/lead_manifest.json` lists, for every lead clip, its timing, which props it uses, its
dialogue lines (for the 2D speech bubbles) and whether it's a night scene.

## Versions

Bevy 0.19.1 and cpal 0.17 (the same audio backend Bevy's own audio uses). Tested on Linux. The
webcam writer (`src/webcam.rs`) follows OBS's own `shared-memory-queue.c` byte for byte and is
unit-tested against a port of OBS's reader, but hasn't been run on a Windows machine yet.
