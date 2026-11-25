//! Tiny local control server so the avatar can be driven while another window has focus:
//! open http://127.0.0.1:<port>/ in a browser, or add it to OBS as a Custom Browser Dock
//! (Docks > Custom Browser Docks). Anything that can send an HTTP GET (Stream Deck plugins,
//! Bitfocus Companion, curl) can also hit /cmd/<command>, e.g. /cmd/face:2 or /cmd/play:Victory.
//! GET /state returns the current settings and the character's clip list as JSON.
//! GET /questions lists viewer questions; GET /q/<id>.wav plays one (the private preview).
//! Listens on localhost only, and refuses commands sent by other websites (a page you visit
//! can't reach in and press buttons here).

use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver},
    },
};

use bevy::prelude::Resource;

#[derive(Resource)]
pub struct Commands(pub Mutex<Receiver<String>>, pub String);

/// JSON snapshot the app refreshes a few times a second; served at GET /state.
#[derive(Resource, Clone, Default)]
pub struct State(pub Arc<Mutex<String>>);

const PAGE: &str = r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Avatar control</title><style>
:root{color-scheme:dark;--bg:#17181d;--panel:#22242c;--ink:#eef0f6;--mute:#9aa0b4;--acc:#d8282f;--on:#2f7d4a}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--ink);font:14px/1.4 system-ui,sans-serif;padding:10px}
h2{font-size:11px;letter-spacing:.08em;text-transform:uppercase;color:var(--mute);margin:14px 0 6px}
.g{display:grid;grid-template-columns:repeat(auto-fill,minmax(96px,1fr));gap:6px}
button{font:inherit;color:var(--ink);background:var(--panel);border:1px solid #33364a;border-radius:6px;padding:8px 6px;cursor:pointer}
button:hover{border-color:var(--acc)}button:active{background:var(--acc)}
button.on{background:var(--on);border-color:var(--on)}
.top{position:sticky;top:0;z-index:5;background:var(--bg);padding:4px 0 6px;border-bottom:1px solid #2a2c36}
#now{color:var(--mute);font-size:12px;padding-bottom:4px}
.strip{display:flex;flex-wrap:wrap;gap:6px;font-size:12px}.chip{background:var(--panel);border:1px solid #33364a;border-radius:10px;padding:2px 8px;color:var(--mute);display:inline-flex;align-items:center;gap:6px}
.chip.ok{color:#9be3ae;border-color:#2f7d4a}.chip.warn{color:#ffb36b;border-color:#7a5a2b}
.lv{display:inline-block;width:46px;height:6px;background:#111;border-radius:3px;overflow:hidden}.lv i{display:block;height:100%;width:0;background:#4cc56f}
.tabs{display:flex;gap:4px;margin-top:6px;overflow-x:auto}.tabs button{padding:6px 12px;white-space:nowrap}.tabs button[aria-selected=true]{background:var(--acc);border-color:var(--acc)}
.tab[hidden]{display:none}
.pv{margin-top:6px;font-size:12px;color:var(--mute)}.pv img{display:block;max-height:280px;max-width:100%;margin-top:6px;border-radius:6px;background:#000}
.hk{display:flex;gap:6px;align-items:center;padding:4px 0;border-bottom:1px solid #22242c}.hk kbd{min-width:110px;font:12px ui-monospace,monospace;background:var(--panel);border:1px solid #33364a;border-radius:4px;padding:2px 6px}.hk span{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis}.hk button{padding:3px 8px;font-size:12px}
#now b{color:var(--ink);font-weight:600}
input{font:inherit;width:100%;padding:8px;border-radius:6px;border:1px solid #33364a;background:var(--panel);color:var(--ink);margin-bottom:6px}
.clips{max-height:340px;overflow:auto;border:1px solid #2a2c36;border-radius:6px}
.row{display:flex;align-items:center;gap:6px;padding:4px 6px;border-bottom:1px solid #22242c}
.row span{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.row button{padding:4px 8px;font-size:12px}
#s{color:var(--mute);font-size:12px;min-height:1.4em;margin-top:10px}
.row2{display:flex;gap:6px;margin-top:6px}.row2 input{margin:0}.row2 button{white-space:nowrap}
.st{color:var(--mute);font-size:12px;margin-top:4px}
button.bad{border-color:#7a2b31}button.bad:hover{background:#7a2b31}
button:disabled{opacity:.4;cursor:not-allowed}
.q{border:1px solid #2a2c36;border-radius:8px;padding:8px;margin-top:6px}.q.on_air{border-color:var(--on)}.q.flag{border-color:#c0392b}
.q.done,.q.skipped{opacity:.5}.q .hd{display:flex;gap:6px;align-items:baseline;flex-wrap:wrap}.q .hd b{font-size:15px}
.tag{font-size:11px;padding:1px 6px;border-radius:9px;background:#33364a;color:var(--ink)}.tag.ok{background:var(--on)}.tag.warn{background:#c0392b}
.q audio{width:100%;height:32px;margin:6px 0}.q .acts{display:flex;gap:6px;flex-wrap:wrap;margin-top:6px}.q .acts button{padding:5px 10px;font-size:12px}
.q .row2 input{font-size:13px;padding:6px}
.setup{background:#1f2a1f;border:1px solid #3c6b45;border-radius:10px;padding:12px 14px;margin:8px 0 14px}
.setup h2{margin-top:0;color:#9be3ae}.setup ol{padding-left:18px;margin:6px 0}.setup li{margin:10px 0}
.setup .row2{margin-top:6px}.meter{height:8px;background:#111;border-radius:4px;overflow:hidden;margin-top:6px}.meter div{height:100%;width:0;background:#4cc56f;transition:width .1s}
.chk{list-style:none;padding:0;margin:4px 0}.chk li{margin:3px 0}.ok{color:#7ddc9b}.no{color:#ffb36b}
.probs{background:#2a1c1c;border:1px solid #7a2b31;border-radius:8px;padding:8px 10px;margin:8px 0;font-size:12px}
</style></head><body>
<section id="setup" class="setup" hidden>
<h2>Welcome! Five quick steps</h2>
<ol>
<li><b>Microphone.</b> Pick the one you talk into, then say something: the bar should move.
 <div class="row2"><select id="s-mic" aria-label="Microphone"></select><button id="s-micgo">Use this mic</button></div>
 <div class="meter" aria-hidden="true"><div id="s-lvl"></div></div></li>
<li><b>Your character.</b> <div class="row2"><select id="s-model" aria-label="Character"></select><button id="s-modelgo">Switch</button></div></li>
<li><b>Tune the mouth to your voice</b> (10 seconds): press, then say "ah", "ee" and "oo" when the avatar window asks.
 <div class="row2"><button data-c="vowels">Start voice tuning</button></div></li>
<li><b>Twitch</b> (optional). Your channel, for chat throws and dress-up:
 <div class="row2"><input id="s-tw" placeholder="your_channel" aria-label="Twitch channel"><button id="s-twgo">Connect</button></div>
 For viewer questions with Twitch sign-in, a client ID from dev.twitch.tv (README: Viewer questions):
 <div class="row2"><input id="s-cid" placeholder="Twitch client ID" aria-label="Twitch client ID"><button id="s-cidgo">Save</button></div></li>
<li><b>Checks.</b><ul class="chk" id="s-checks"><li>Checking...</li></ul></li>
</ol>
<div class="g"><button id="s-done">Done, show me the controls</button></div>
</section>
<div id="probs" class="probs" hidden></div>
<header class="top"><div id="now">Connecting...</div>
<div class="strip" aria-live="off"><span class="chip" id="c-mic">Mic <i class="lv"><i id="c-lvl"></i></i></span><span class="chip" id="c-fps">-- fps</span><span class="chip" id="c-tw">Twitch</span><span class="chip" id="c-cam">Webcam</span><span class="chip" id="c-rec">Recording</span><span class="chip" id="c-q">Questions</span></div>
<nav class="tabs" role="tablist"><button data-tab="stream">Stream</button><button data-tab="chat">Chat</button><button data-tab="questions">Questions</button><button data-tab="show">Show</button><button data-tab="clips">Clips</button><button data-tab="keys">Hotkeys</button></nav></header>
<details id="golive" class="pv"><summary>Go-live check</summary><button id="glgo">Run the check</button> <span class="st">Talk for a few seconds while it runs.</span><ul id="glres"></ul></details>
<details id="pvbox" class="pv"><summary>Live preview</summary><img id="pv" alt="Live preview from the shorts camera"><div id="pvst"></div></details>
<div class="tab" data-tab="stream">
<h2>Face</h2><div class="g" id="faces"></div>
<h2>Lip sync style</h2><div class="g" id="lips"></div>
<h2>Emotes (layer on top of anything)</h2><div class="g" id="emotes"></div>
<h2>Behaviour</h2><div class="g">
<button data-c="react" id="b-react">Voice reactions</button><button data-c="bop" id="b-bop">Bop to beat</button>
<button data-c="autotalk">Talk gestures</button><button data-c="mute">Mute mic</button><button data-c="webcam" id="b-webcam">Webcam output</button>
<button data-c="facecap:on" id="b-facecap">Face capture</button><button data-c="facecap:mirror" id="b-fcmirror">Mirror head turn</button><button data-c="body" id="b-body">Body language</button><button data-c="formants" id="b-formants">Vowels from formants</button><button data-c="vowels">Calibrate vowels</button></div>
</div><div class="tab" data-tab="chat">
<h2>Audience</h2><div class="g">
<button data-c="throw:tomato">Throw tomato</button><button data-c="throw:egg">Throw egg</button><button data-c="throw:confetti">Confetti</button>
<button data-c="chat" id="b-chat">Chat throws</button></div>
<div class="row2"><input id="tw" placeholder="Twitch channel (no #)" aria-label="Twitch channel"><button id="twgo">Read chat</button></div>
<details class="st"><summary>YouTube and Kick chat</summary>
<div class="row2"><input id="ytkey" type="password" placeholder="YouTube Data API key (Google Cloud console)" aria-label="YouTube API key"><button id="ytkeygo">Save key</button></div>
<div class="row2"><input id="ytv" placeholder="YouTube live link or video id (empty: off)" aria-label="YouTube live video"><button id="ytgo">Read YouTube chat</button></div>
<div class="row2"><input id="kick" placeholder="Kick channel or chatroom number (empty: off)" aria-label="Kick channel"><button id="kickgo">Read Kick chat</button></div>
<div>YouTube checks for new messages every 8 seconds to stay inside the free daily quota. Kick is read the way its own chat page does; Kick can change that at any time.</div></details>
<div id="chatst" class="st"></div>
<h2>Twitch moments</h2><div class="g">
<button data-c="event:raid">Test raid</button><button data-c="event:sub">Test sub</button><button data-c="event:gift">Test gift subs</button><button data-c="event:cheer">Test 600 bits</button>
<button data-c="event:crowd" id="b-crowd">Raid crowd</button><button data-c="event:subs" id="b-subs">Sub celebrations</button></div>
<h2>Viewer crowd</h2><div class="g"><button data-c="crowd:on" id="b-vcrowd">Chatters join the crowd</button><button data-c="crowd:test">Test chatters</button></div>
<div id="crst" class="st"></div>
<div class="st">Bits throw tomatoes: one per 100 bits, a giant one from 500.</div>
<h2>Dress-up</h2><div class="g">
<button data-c="wear:chat" id="b-wear">Chat dress-up</button><button data-c="wear:who:everyone" id="w-everyone">Everyone</button><button data-c="wear:who:subs" id="w-subs">Subs + VIPs</button><button data-c="wear:who:mods" id="w-mods">Mods</button></div>
<div class="row2"><select id="witem" aria-label="Wardrobe item"></select><button id="wgo">Wear</button></div>
<div class="g"><button data-c="wear:off:hat">Hat off</button><button data-c="wear:off:face">Face off</button><button data-c="wear:off:hand">Hand empty</button><button data-c="wear:clear">Take everything off</button></div>
<div id="wst" class="st"></div>
<h2>Poll (tug-of-war)</h2>
<div class="row2"><input id="ptitle" placeholder="Question" aria-label="Poll question"></div>
<div class="row2"><input id="pa" placeholder="Option 1 (!1)" aria-label="Option 1"><input id="pb" placeholder="Option 2 (!2)" aria-label="Option 2"><input id="psec" value="60" size="3" aria-label="Seconds" style="flex:0 0 56px"></div>
<div class="g"><button id="pgo">Start poll</button><button data-c="poll:end">End now</button><button data-c="poll:cancel">Cancel</button></div>
<div id="pst" class="st"></div>
</div><div class="tab" data-tab="clips">
<h2>Clips and comics</h2><div class="g"><button data-c="clip">Clip that (last 20 s)</button><button data-c="clip:rec" id="b-crec">Recording</button>
<button data-c="clip:auto" id="b-cauto">Auto shorts</button><button data-c="clip:comics" id="b-ccomic">Comic strips</button><button data-c="clip:cards" id="b-ccards">Title cards + captions</button><button data-c="clip:reel">Highlight reel (this stream's shorts)</button></div>
<div id="clst" class="st"></div>
<div class="g"><button data-c="folder:clips">Open clips folder</button><button data-c="folder">Open your files</button></div>
</div><div class="tab" data-tab="show">
<h2>Show</h2><div class="g"><button data-c="director" id="b-director">Auto-director</button><button data-c="press" id="b-press">Press conference</button></div>
<div class="row2"><select id="setsel" aria-label="Stage set"></select><button id="setgo">Use set</button></div>
<div id="shst" class="st"></div>
<h2>Scenes</h2><div class="row2"><select id="psel" aria-label="Saved scene"></select><button id="pload">Load</button><button id="pdel" class="bad">Delete</button></div>
<div class="row2"><input id="pname" placeholder="Name this scene (character, set, framing, lighting, outfit)" aria-label="Scene name"><button id="psave">Save current</button></div>
<div class="row2"><button data-c="sfx:on" id="b-sfx">Sound effects</button><label class="st">Volume <input type="range" id="sfxvol" min="0" max="100" step="5" aria-label="Sound effects volume"></label></div>
<h2>Effects</h2><div class="g"><button data-c="fx:on" id="b-fx">Effects</button>
<button data-c="fx:fireworks">Fireworks</button><button data-c="fx:confetti">Confetti</button><button data-c="fx:sparks">Sparks</button><button data-c="fx:hearts">Hearts</button><button data-c="fx:stars">Stars</button></div>
<div class="g"><button data-c="fx:auto" id="wx-auto">Set's weather</button><button data-c="fx:rain" id="wx-rain">Rain</button><button data-c="fx:snow" id="wx-snow">Snow</button><button data-c="fx:dust" id="wx-dust">Dust</button><button data-c="fx:drift" id="wx-drift">Confetti drift</button><button data-c="fx:off" id="wx-off">No weather</button></div>
<div id="fxst" class="st"></div>
<h2>Your voice</h2><div class="g">
<button data-c="voice:laugh" id="b-laugh">Laugh detection</button><button data-c="voice:sing" id="b-sing">Sing detection</button>
<button data-c="voice:gags" id="b-gags">Catchphrase gags</button><button data-c="voice:reload">Reload gags.txt</button></div>
<div id="vst" class="st"></div>
<h2>Co-host</h2><div class="g"><button data-c="cohost" id="b-cohost">Co-host on stage</button><button data-c="cohost:next">Next character</button></div>
<div class="row2"><input id="cmic" placeholder="Co-host mic (part of its name)" aria-label="Co-host microphone"><button id="cmicgo">Use mic</button></div>
<div id="cst" class="st"></div>
<h2>Go live</h2><div class="g"><button data-c="net:host" id="b-net">Stream to the relay</button><button data-c="net:stop">End</button><button data-c="net:guest:on" id="b-gon">Let guest on</button><button data-c="net:guest:off">Guest off</button></div>
<div class="row2"><input id="relay" placeholder="Relay address, e.g. https://relay.example.com" aria-label="Relay address"><button id="relaygo">Use relay</button></div>
<div id="nst" class="st"></div>
</div><div class="tab" data-tab="questions">
<h2>Viewer questions</h2><div class="g">
<button data-c="ask:toggle" id="b-ask">Questions open</button><button data-c="ask:next" id="b-next">Air next</button>
<button data-c="ask:done" id="b-done">Done (thanks)</button><button data-c="ask:cut" class="bad">Cut</button>
<button data-c="ask:names" id="b-names">Allow typed names</button><button data-c="ask:clear">Clear finished</button><button data-c="ask:words">Reload blocked words</button></div>
<div id="askst" class="st"></div>
<div id="qs"></div>
<details class="st"><summary>Banned</summary><div id="bans"></div></details>
</div><div class="tab" data-tab="stream">
<h2>Speech bubbles and look</h2><div class="g">
<button data-c="bubbles" id="b-bubbles">Speech bubbles</button><button data-c="retro" id="b-retro">Retro sprite mode</button>
<button data-c="retro:3">Retro pixels 3</button><button data-c="retro:4">Retro pixels 4</button><button data-c="retro:6">Retro pixels 6</button></div>
<div class="row2"><input id="say" placeholder="Type a line for the speech bubble" aria-label="Speech bubble text"><button id="saygo">Say</button></div>
<div id="capst" class="st"></div>
<h2>Gestures</h2><div class="g" id="gest"></div>
<h2>Character</h2><div class="g" id="char">
<button data-c="model:prev">Previous</button><button data-c="model:next">Next</button></div>
<h2>Every clip</h2>
<input id="q" type="search" placeholder="Search clips" aria-label="Search clips">
<div class="clips" id="clips"></div>
<h2>Setup</h2><div class="g">
<button data-c="calibrate">Calibrate</button><button data-c="gate:-2">Gate -2 dB</button><button data-c="gate:2">Gate +2 dB</button>
<button data-c="bg">Background</button><button data-c="frame">Framing</button><button data-c="hud">Hide HUD</button>
<button data-c="light:on" id="b-light">Studio lighting</button><button data-c="light:shadows" id="b-shadows">Shadows</button><button data-c="light:glow" id="b-glow">Glow</button><button data-c="light:grade" id="b-grade">Colour grading</button><button data-c="light:blur" id="b-blur">Background blur</button><button data-c="bevel:on" id="b-bevel">Block bevels</button><button data-c="bevel:blend" id="b-blend">Blend blocks</button>
<button data-c="idle" id="b-idle">Idle life</button>
<button id="b-setup">Setup steps again</button><button data-c="folder">Open your files</button></div>
<div class="row2"><label class="st">Quality <select id="qsel" aria-label="Quality"><option value="auto">Auto</option><option value="high">High</option><option value="medium">Medium</option><option value="low">Low</option></select></label><label class="st">Background blur <input type="range" id="blurv" min="0" max="100" step="5" aria-label="Background blur on sets"></label><span class="st" id="lat"></span></div>
<div class="row2"><label class="st">Bevel <input type="range" id="bv-bevel" min="0" max="100" step="5" aria-label="Bevel strength"></label><label class="st">Seams <input type="range" id="bv-grid" min="0" max="100" step="5" aria-label="Seams between blocks"></label><label class="st">Wobble <input type="range" id="bv-wobble" min="0" max="100" step="5" aria-label="Per-block wobble"></label><label class="st">Beard, hair, cloth <input type="range" id="bv-detail" min="0" max="100" step="5" aria-label="Beard, hair and cloth detail"></label></div>
</div><div class="tab" data-tab="keys">
<h2>Global hotkeys</h2>
<div id="hkst" class="st"></div>
<div id="hks"></div>
<div class="row2"><input id="hkkey" placeholder="Click here, press the keys" readonly aria-label="Key combination"><input id="hkcmd" placeholder="Command, e.g. throw:tomato" aria-label="Command" list="hkcmds"><button id="hkgo">Add</button></div>
<datalist id="hkcmds"><option value="throw:tomato"><option value="throw:confetti"><option value="emote:hop"><option value="emote:spin"><option value="mute"><option value="clip"><option value="ask:next"><option value="ask:done"><option value="model:next"><option value="face:0"><option value="react"><option value="bop"><option value="director"><option value="poll:end"></datalist>
<div class="g"><button data-c="hotkey:reload">Reload hotkeys.txt</button><button data-c="hotkey:defaults">Back to defaults</button></div>
<div class="st">Any command from the README works (the same ones as the buttons). Hold at least one of Ctrl, Alt, Shift or Win, or use an F key on its own.</div>
</div>
<div id="s" role="status"></div>
<script>
const F=["Neutral","Happy","Angry","Sad","Surprised","Smug","Suspicious","Nervous","Rage","KO"];
const G=["Emotes","Victory","Rally chant","Rally dance","Hit reaction (heavy)","Knockdown and get up","Jab","Cover up"];
const $=id=>document.getElementById(id);
const mk=(host,label,cmd,id)=>{const b=document.createElement('button');b.textContent=label;b.dataset.c=cmd;if(id)b.id=id;host.appendChild(b);return b;};
F.forEach((n,i)=>mk($('faces'),`${(i+1)%10} ${n}`,'face:'+i,'f-'+i));
G.forEach(n=>mk($('gest'),n,'play:'+encodeURIComponent(n)));
let st=null,model='',ownBtns=[];
function clipList(){
  const q=$('q').value.toLowerCase(),host=$('clips');host.textContent='';
  (st?st.clips:[]).filter(c=>c.toLowerCase().includes(q)).forEach(c=>{
    const r=document.createElement('div');r.className='row';
    const n=document.createElement('span');n.textContent=c;r.appendChild(n);
    mk(r,'Play','play:'+encodeURIComponent(c));
    const l=mk(r,c===st.loop?'Looping':'Loop',c===st.loop?'loop:':'loop:'+encodeURIComponent(c));
    if(c===st.loop)l.className='on';
    host.appendChild(r);});
}
$('q').addEventListener('input',clipList);
// ---- tabs
function showTab(t){document.querySelectorAll('.tab').forEach(d=>d.hidden=d.dataset.tab!==t);
  document.querySelectorAll('.tabs button').forEach(b=>b.setAttribute('aria-selected',b.dataset.tab===t));
  try{localStorage.setItem('tab',t);}catch(e){}}
document.querySelectorAll('.tabs button').forEach(b=>b.onclick=()=>showTab(b.dataset.tab));
let startTab='stream';try{startTab=localStorage.getItem('tab')||'stream';}catch(e){}
showTab(startTab);
// ---- live preview (the shorts camera's newest frame)
setInterval(()=>{if(!$('pvbox').open)return;const img=new Image();
  img.onload=()=>{$('pv').src=img.src;$('pv').hidden=false;$('pvst').textContent='';};
  img.onerror=()=>{$('pv').hidden=true;$('pvst').textContent='Turn on Recording (Clips tab) to see the preview.';};
  img.src='/preview.jpg?'+Date.now();},500);
// ---- hotkeys
const KN={ArrowLeft:'Left',ArrowRight:'Right',ArrowUp:'Up',ArrowDown:'Down',' ':'Space',Enter:'Enter',Tab:'Tab',Home:'Home',End:'End',PageUp:'PageUp',PageDown:'PageDown',Insert:'Insert',Delete:'Delete',Pause:'Pause'};
$('hkkey').addEventListener('keydown',e=>{e.preventDefault();if(['Control','Alt','Shift','Meta','OS'].includes(e.key))return;
  let k=KN[e.key]||(/^F\d+$/.test(e.key)?e.key:/^Numpad\d$/.test(e.code)?'Num'+e.code.slice(6):/^Digit\d$/.test(e.code)?e.code.slice(5):/^Key[A-Z]$/.test(e.code)?e.code.slice(3):e.key.length===1?e.key.toUpperCase():'');
  if(!k)return;$('hkkey').value=(e.ctrlKey?'Ctrl+':'')+(e.altKey?'Alt+':'')+(e.shiftKey?'Shift+':'')+(e.metaKey?'Win+':'')+k;});
$('hkgo').onclick=()=>{const k=$('hkkey').value,c=$('hkcmd').value.trim();if(!k||!c){$('s').textContent='Press a key combination and type a command';return;}
  send('hotkey:set:'+encodeURIComponent(k+'='+c));$('hkkey').value='';$('hkcmd').value='';};
let hkSeen='';
function hotkeys(h){const j=JSON.stringify(h);if(j===hkSeen)return;hkSeen=j;
  $('hkst').textContent=h.supported?'These work while any program has focus.':'Global hotkeys work on Windows only. They are saved here and will work there.';
  if(h.problems&&h.problems.length)$('hkst').textContent+=' hotkeys.txt: '+h.problems.join('; ');
  const host=$('hks');host.textContent='';
  h.list.forEach(b=>{const r=document.createElement('div');r.className='hk';const k=document.createElement('kbd');k.textContent=b.keys;r.appendChild(k);
    const c=document.createElement('span');c.textContent=b.cmd+(b.error&&h.supported?' ('+b.error+')':'');if(b.error&&h.supported)c.className='no';r.appendChild(c);
    mk(r,'Remove','hotkey:del:'+encodeURIComponent(b.keys));host.appendChild(r);});}
// ---- first-run setup
let setupShown=false;
async function setupInit(){
  $('setup').hidden=false;setupShown=true;
  try{const m=await (await fetch('/mics',{cache:'no-store'})).json();const sel=$('s-mic');sel.textContent='';
    m.devices.forEach(d=>{const o=document.createElement('option');o.value=d;o.textContent=d+(d===m.default?' (default)':'');sel.appendChild(o);});
    if(!m.devices.length){const o=document.createElement('option');o.value='';o.textContent='No microphone found: plug one in, then reload this page';sel.appendChild(o);}
    if(m.default)sel.value=m.default;}catch(e){}
  try{const c=await (await fetch('/checks',{cache:'no-store'})).json();const ul=$('s-checks');ul.textContent='';
    c.items.forEach(it=>{const li=document.createElement('li');const s=document.createElement('span');s.className=it.ok?'ok':it.optional?'':'no';s.textContent=(it.ok?'OK ':it.optional?'Optional: ':'Needs attention: ')+it.name;
      li.appendChild(s);if(it.note){li.append(' - '+it.note);}ul.appendChild(li);});}catch(e){}
}
$('s-micgo').onclick=()=>send('mic:'+encodeURIComponent($('s-mic').value));
$('s-modelgo').onclick=()=>send('model:'+encodeURIComponent($('s-model').value));
$('s-twgo').onclick=()=>send('twitch:'+encodeURIComponent($('s-tw').value.trim()));
$('s-cidgo').onclick=()=>send('askcfg:client:'+encodeURIComponent($('s-cid').value.trim()));
$('bv-bevel').onchange=()=>send('bevel:'+$('bv-bevel').value);
$('bv-grid').onchange=()=>send('bevel:grid:'+$('bv-grid').value);
$('bv-wobble').onchange=()=>send('bevel:wobble:'+$('bv-wobble').value);
$('bv-detail').onchange=()=>send('bevel:detail:'+$('bv-detail').value);
$('qsel').onchange=()=>send('quality:'+$('qsel').value);
$('blurv').onchange=()=>send('light:blur:'+$('blurv').value);
$('psave').onclick=()=>{const n=$('pname').value.trim();if(n){send('preset:save:'+encodeURIComponent(n));$('pname').value='';}};
$('pload').onclick=()=>{if($('psel').value)send('preset:load:'+encodeURIComponent($('psel').value));};
$('pdel').onclick=()=>{if($('psel').value)send('preset:del:'+encodeURIComponent($('psel').value));};
// go-live: the setup checks plus a few seconds of live readings
$('glgo').onclick=async()=>{const ul=$('glres');ul.textContent='Checking (5 seconds)...';const res=[];
  let c={items:[]};try{c=await (await fetch('/checks',{cache:'no-store'})).json();}catch(e){}
  let fps=[],lvl=0,s=null;for(let i=0;i<10;i++){try{s=await (await fetch('/state',{cache:'no-store'})).json();fps.push(s.fps);lvl=Math.max(lvl,s.mic_level||0);}catch(e){}await new Promise(r=>setTimeout(r,500));}
  c.items.filter(it=>!it.optional||it.ok).forEach(it=>res.push([it.ok,it.name,it.note]));
  if(s){const f=Math.min(...fps);res.push([f>=50,'Frame rate',f+' fps at worst'+(f<50?': try a lower quality':'')]);
    res.push([lvl>0.15,'Mic picks you up',lvl>0.15?'':'no voice heard: check the mic or the gate']);
    if(s.twitch)res.push([/reading/i.test(s.chat_status),'Twitch chat',s.chat_status]);
    if(s.webcam)res.push([/sending/i.test(s.webcam_status),'Webcam output',s.webcam_status]);
    res.push([s.rec,'Clip recording',s.rec?'':'off: no shorts or reel this stream']);
    res.push([true,'OBS mic sync','set your mic\'s Sync Offset to about '+s.lip_latency_ms+' ms']);}
  ul.textContent='';res.forEach(([ok,n,note])=>{const li=document.createElement('li');const b=document.createElement('span');b.className=ok?'ok':'no';b.textContent=(ok?'OK ':'Check ')+n;li.appendChild(b);if(note)li.append(' - '+note);ul.appendChild(li);});
  const bad=res.filter(r=>!r[0]).length;const li=document.createElement('li');li.textContent=bad?bad+' to look at.':'All good. Have a great stream.';ul.appendChild(li);};
$('sfxvol').onchange=()=>send('sfx:vol:'+$('sfxvol').value);
$('s-done').onclick=()=>{send('setup');$('setup').hidden=true;setupShown=false;history.replaceState(null,'','/');};
if(location.search.includes('setup'))setupInit();
$('b-setup').onclick=()=>{setupInit();window.scrollTo(0,0);};
async function poll(){
  try{
    st=await (await fetch('/state',{cache:'no-store'})).json();
    if(!setupShown&&st.setup_done===false)setupInit();
    if(setupShown){$('s-lvl').style.width=Math.round((st.mic_level||0)*100)+'%';
      if(!$('s-model').options.length)(st.models||[]).forEach(n=>{const o=document.createElement('option');o.value=n;o.textContent=n;$('s-model').appendChild(o);});
      if(document.activeElement!==$('s-model'))$('s-model').value=st.model;}
    const pr=st.problems||[];$('probs').hidden=!pr.length;
    if(pr.length){$('probs').textContent='';const h=document.createElement('b');h.textContent='Problems ';$('probs').appendChild(h);
      const c=document.createElement('button');c.textContent='Clear';c.dataset.c='problems';$('probs').appendChild(c);
      pr.slice().reverse().forEach(p=>{const d=document.createElement('div');d.textContent=p;$('probs').appendChild(d);});}
    $('c-lvl').style.width=Math.round((st.mic_level||0)*100)+'%';
    $('c-fps').textContent=st.fps+' fps';$('c-fps').className='chip'+(st.fps>=50?' ok':st.fps<25?' warn':'');
    $('c-tw').textContent=st.twitch?'Twitch #'+st.twitch:'Twitch off';$('c-tw').className='chip'+(st.twitch?(/reading/i.test(st.chat_status)?' ok':' warn'):'');$('c-tw').title=st.chat_status;
    $('c-cam').textContent=st.webcam?'Webcam on':'Webcam off';$('c-cam').className='chip'+(st.webcam?(/sending/i.test(st.webcam_status)?' ok':' warn'):'');$('c-cam').title=st.webcam_status;
    $('c-rec').textContent=st.rec?'Recording':'Not recording';$('c-rec').className='chip'+(st.rec?' ok':'');
    $('c-q').textContent=qv&&qv.open?'Questions open':'Questions closed';$('c-q').className='chip'+(qv&&qv.open?' ok':'');
    if(st.hotkeys)hotkeys(st.hotkeys);
    $('now').innerHTML='';
    const parts=[['Character',st.model],['Face',F[st.face]],['Lip',st.lip],['Body',st.body+(st.asleep?' (asleep)':'')]];
    parts.forEach(([k,v],i)=>{if(i)$('now').append(' | ');$('now').append(k+' ');const b=document.createElement('b');b.textContent=v;$('now').append(b);});
    F.forEach((_,i)=>$('f-'+i).className=i===st.face?'on':'');
    if(!$('lips').children.length){st.lips.forEach(m=>mk($('lips'),m,'lip:'+m,'l-'+m));st.emotes.forEach(e=>mk($('emotes'),e[0].toUpperCase()+e.slice(1),'emote:'+e));}
    st.lips.forEach(m=>$('l-'+m).className=m===st.lip?'on':'');
    $('b-react').className=st.reactions?'on':'';$('b-webcam').className=st.webcam?'on':'';$('b-chat').className=st.chat?'on':'';$('b-bubbles').className=st.bubbles?'on':'';$('b-retro').className=st.retro?'on':'';
    $('b-ask').className=qv&&qv.open?'on':'';$('b-names').className=st.ask_names?'on':'';
    $('askst').textContent='Viewer page: '+st.ask_url+' (share it through your tunnel)'+(st.ask_login==='twitch'?'':st.ask_login==='names'?' | typed names, unverified':' | no sign-in set up: add twitch_client_id')+(st.guest?' | On stage: #'+st.guest.id+' '+st.guest.user+', '+st.guest.phase:'')+(st.guest_status?' | '+st.guest_status:'');
    $('chatst').textContent=st.chat_status;
    if(st.wardrobe){const w=st.wardrobe;$('b-wear').className=w.chat?'on':'';['everyone','subs','mods'].forEach(x=>$('w-'+x).className=w.who===x?'on':'');
      if(!$('witem').options.length)w.items.forEach(i=>{const o=document.createElement('option');o.value=i.name;o.textContent=i.slot+': '+i.name;$('witem').appendChild(o);});
      $('wst').textContent='Wearing: '+(w.worn.filter(x=>x.item).map(x=>x.item+(x.by?' (from '+x.by+')':'')).join(', ')||'nothing')+'. Chat: !hat cowboy, !glasses shades, !hold coffee, !hat off';}
    if(st.light){$('b-light').className=st.light.on?'on':'';$('b-shadows').className=st.light.on&&st.light.shadows?'on':'';$('b-glow').className=st.light.on&&st.light.glow?'on':'';$('b-grade').className=st.light.on&&st.light.grade?'on':'';$('b-blur').className=st.light.blur>0?'on':'';
      if(document.activeElement!==$('blurv'))$('blurv').value=st.light.blur;
      if(document.activeElement!==$('qsel'))$('qsel').value=st.light.quality;}
    $('b-idle').className=st.idle_life?'on':'';
    if(st.facecap){$('b-facecap').className=st.facecap.on?'on':'';$('b-fcmirror').className=st.facecap.mirror?'on':'';$('b-facecap').title=st.facecap.status;$('b-facecap').textContent=st.facecap.tracking?'Face capture ('+st.facecap.tracking+')':'Face capture';}
    if(st.effects){$('b-fx').className=st.effects.on?'on':'';['auto','rain','snow','dust','drift','off'].forEach(k=>$('wx-'+k).className=st.effects.weather===k?'on':'');
      $('fxst').textContent='Weather now: '+(st.effects.now||'none')+' | '+st.effects.live+' particles | chat: <3 hearts, GG confetti, fire emoji sparks';}
    $('lat').textContent='Quality now: '+(st.light?st.light.level:'?')+' | mouth lags the mic by about '+st.lip_latency_ms+' ms: set that as the mic\'s Sync Offset in OBS';
    if(st.crowd){$('b-vcrowd').className=st.crowd.on?'on':'';$('crst').textContent=st.crowd.count+' chatters on stage (up to 8; they leave after 3 quiet minutes)';}
    if(st.presets){const ps=$('psel'),want=st.presets.join('\n');if(ps.dataset.k!==want){ps.dataset.k=want;const keep=ps.value;ps.textContent='';
      if(!st.presets.length){const o=document.createElement('option');o.value='';o.textContent='No saved scenes';ps.appendChild(o);}
      st.presets.forEach(n=>{const o=document.createElement('option');o.value=n;o.textContent=n;ps.appendChild(o);});if(st.presets.includes(keep))ps.value=keep;}}
    if(st.bevel){$('b-bevel').className=st.bevel.on?'on':'';$('b-blend').className=st.bevel.blend?'on':'';['bevel','grid','wobble','detail'].forEach(k=>{const el=$('bv-'+k);if(document.activeElement!==el)el.value=st.bevel[k];});}
    if(st.sfx){$('b-sfx').className=st.sfx.on?'on':'';if(document.activeElement!==$('sfxvol'))$('sfxvol').value=st.sfx.volume;}
    if(st.voice){$('b-laugh').className=st.voice.laugh?'on':'';$('b-sing').className=st.voice.sing?'on':'';$('b-gags').className=st.voice.gags?'on':'';
      $('vst').textContent=st.voice.gag_count+' gags in gags.txt'+(st.voice.last?' | last: '+st.voice.last:'')+(st.voice.singing?' | singing':'');}
    if(st.show){$('b-director').className=st.show.director?'on':'';$('b-press').className=st.show.press?'on':'';
      if(!$('setsel').options.length)['none','auto'].concat(st.show.sets).forEach(n=>{const o=document.createElement('option');o.value=n;o.textContent=n==='none'?'none (green screen)':n==='auto'?'auto (match the character)':n;$('setsel').appendChild(o);});
      if(document.activeElement!==$('setsel'))$('setsel').value=st.show.set;
      $('shst').textContent='Set: '+st.show.set+(st.show.press?' | press conference: approved questions wait in the front row; A calls the next one':'');}
    if(st.shorts){const c=st.shorts;$('b-crec').className=c.on?'on':'';$('b-cauto').className=c.auto?'on':'';$('b-ccomic').className=c.comics?'on':'';$('b-ccards').className=c.cards?'on':'';
      $('clst').textContent=(c.status||'')+(c.saved.length?' | latest: '+c.saved.slice(-3).join(', '):'')+' | folder: clips/';}
    $('b-cohost').className=st.cohost_on?'on':'';$('cst').textContent=st.cohost_status||'';
    if(st.net){$('b-net').className=st.net.role==='host'?'on':'';$('b-gon').className=st.net.guest_on?'on':'';
      $('nst').textContent=st.net.role==='host'&&st.net.code?('Live · code '+st.net.code+' · '+st.net.viewers+' watching'+(st.net.guest?' · guest: '+st.net.guest:'')+' · watch link: '+st.net.relay+'/'+st.net.session):(st.net.status||'Relay: '+st.net.relay);}
    $('b-crowd').className=st.raid_crowd?'on':'';$('b-subs').className=st.sub_party?'on':'';
    $('pst').textContent=st.poll?st.poll.line.replace('\n',' | ')+' ('+st.poll.counts[0]+' vs '+st.poll.counts[1]+')':'No poll running. Chat votes !1 or !2.';$('capst').textContent='Words from: '+st.caption_source;if(!$('tw').matches(':focus')&&!$('tw').value)$('tw').value=st.twitch;$('b-body').className=st.body_language?'on':'';$('b-formants').className=st.formants?'on':'';$('b-bop').className=st.bop?'on':'';
    if(st.model!==model||st.clips.length!==(window.nclips||0)){
      model=st.model;window.nclips=st.clips.length;
      ownBtns.forEach(b=>b.remove());ownBtns=st.own.map(c=>mk($('char'),c,'play:'+encodeURIComponent(c)));
    }
    clipList();
  }catch(e){$('now').textContent='Avatar app not reachable';}
}
document.addEventListener('click',async e=>{const c=e.target.dataset&&e.target.dataset.c;if(c===undefined||c.startsWith('ask:text:'))return;
 try{const r=await fetch('/cmd/'+c);$('s').textContent=r.ok?'Sent: '+decodeURIComponent(c):'Error '+r.status;}catch(err){$('s').textContent='Avatar app not reachable';}
 setTimeout(()=>{poll();qver=-1;qpoll();},120);});
const send=async c=>{try{await fetch('/cmd/'+c);$('s').textContent='Sent: '+decodeURIComponent(c);}catch(e){$('s').textContent='Avatar app not reachable';}setTimeout(poll,150);};
$('saygo').onclick=()=>{const v=$('say').value.trim();if(v){send('say:'+encodeURIComponent(v));$('say').value='';}};
$('say').addEventListener('keydown',e=>{if(e.key==='Enter')$('saygo').click();});
$('twgo').onclick=()=>send('twitch:'+encodeURIComponent($('tw').value.trim()));
$('ytkeygo').onclick=()=>{const k=$('ytkey').value.trim();if(k){send('youtube:key:'+encodeURIComponent(k));$('ytkey').value='';}};
$('ytgo').onclick=()=>send('youtube:'+encodeURIComponent($('ytv').value.trim()));
$('kickgo').onclick=()=>send('kick:'+encodeURIComponent($('kick').value.trim()));
$('setgo').onclick=()=>send('set:'+$('setsel').value);
$('cmicgo').onclick=()=>send('cohost:mic:'+encodeURIComponent($('cmic').value.trim()));
$('relaygo').onclick=()=>send('net:relay:'+encodeURIComponent($('relay').value.trim()));
$('wgo').onclick=()=>send('wear:'+encodeURIComponent($('witem').value));
$('pgo').onclick=()=>{const t=$('ptitle').value.trim()||'Poll',a=$('pa').value.trim(),b=$('pb').value.trim();if(!a||!b){$('s').textContent='Poll needs two options';return;}
  send('poll:start:'+(parseInt($('psec').value)||60)+':'+encodeURIComponent(t.replace(/\|/g,'/')+'|'+a.replace(/\|/g,'/')+'|'+b.replace(/\|/g,'/')));};
// viewer questions: rows keep their audio player and text box across refreshes
let qv=null,qver=-1;const audios=new Map(),texts=new Map();
async function qpoll(){
  try{qv=await (await fetch('/questions',{cache:'no-store'})).json();}catch(e){return}
  if(qv.version===qver)return;qver=qv.version;
  const host=$('qs');host.textContent='';
  if(!qv.items.length){const p=document.createElement('div');p.className='st';p.textContent=qv.open?'No questions yet.':'Questions are closed.';host.appendChild(p);}
  qv.items.forEach(x=>{
    const box=document.createElement('div');box.className='q '+x.status+(x.flagged?' flag':'');
    const hd=document.createElement('div');hd.className='hd';
    const b=document.createElement('b');b.textContent='#'+x.id+' '+x.user;hd.appendChild(b);
    const tag=(t,c)=>{const s=document.createElement('span');s.className='tag '+(c||'');s.textContent=t;hd.appendChild(s);};
    tag(x.verified?'Twitch':'unverified',x.verified?'':'warn');tag(x.character);tag(x.secs+' s');if(x.votes)tag(x.votes+' votes');
    if(x.flagged)tag('blocked word','warn');if(x.shown_as!==x.user)tag('shown as "'+x.shown_as+'"');
    tag({new:x.previewed?'heard':'not heard yet',approved:'approved',on_air:'ON AIR',done:'done',skipped:'skipped'}[x.status],x.status==='approved'||x.status==='on_air'?'ok':'');
    box.appendChild(hd);
    if(x.status==='new'||x.status==='approved'||x.status==='on_air'){
      let a=audios.get(x.id);if(!a){a=document.createElement('audio');a.controls=true;a.preload='none';a.src='/q/'+x.id+'.wav';a.addEventListener('play',()=>setTimeout(()=>{qver=-1;qpoll()},400));audios.set(x.id,a);}
      box.appendChild(a);
      let t=texts.get(x.id);if(!t){t=document.createElement('input');t.placeholder='Bubble text (what they ask)';t.setAttribute('aria-label','Bubble text for question '+x.id);texts.set(x.id,t);}
      if(document.activeElement!==t)t.value=x.text;
      const r=document.createElement('div');r.className='row2';r.appendChild(t);mk(r,'Save text','ask:text:'+x.id+':').onclick=e=>{e.stopPropagation();send('ask:text:'+x.id+':'+encodeURIComponent(t.value));};box.appendChild(r);
    }
    const acts=document.createElement('div');acts.className='acts';
    if(x.status==='new'){const ap=mk(acts,'Approve','ask:approve:'+x.id);if(!x.previewed){ap.disabled=true;ap.title='Listen first';}}
    if(x.status==='approved'){const on=mk(acts,'On air','ask:air:'+x.id);on.disabled=!!qv.on_air;}
    if(x.status==='on_air'){mk(acts,'Done','ask:done');mk(acts,'Cut','ask:cut').className='bad';}
    if(x.status==='new'||x.status==='approved'){mk(acts,'Skip','ask:skip:'+x.id);}
    if(!qv.banned.includes(x.account))mk(acts,'Ban '+x.user,'ask:ban:'+x.id).className='bad';
    box.appendChild(acts);host.appendChild(box);
  });
  for(const id of [...audios.keys()])if(!qv.items.some(x=>x.id===id&&x.status!=='done'&&x.status!=='skipped')){audios.delete(id);texts.delete(id);}
  const bans=$('bans');bans.textContent='';
  if(!qv.banned.length)bans.textContent='Nobody banned.';
  qv.banned.forEach(a=>{const r=document.createElement('div');r.className='row';const n=document.createElement('span');n.textContent=a;r.appendChild(n);mk(r,'Unban','ask:unban:'+encodeURIComponent(a));bans.appendChild(r);});
}
poll();qpoll();setInterval(()=>{if(!$('q').matches(':focus'))poll();if(!document.activeElement||document.activeElement.tagName!=='INPUT')qpoll();},1500);
</script></body></html>"#;

pub fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Starts the server thread. Returns the command receiver and a display URL.
pub fn start(
    port: u16,
    state: State,
    queue: Arc<Mutex<crate::questions::Queue>>,
) -> (Receiver<String>, String) {
    let (tx, rx) = mpsc::channel();
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            crate::problem!("[control] can't listen on port {port}: {e}");
            return (rx, format!("off (port {port} busy)"));
        }
    };
    let url = format!("http://127.0.0.1:{port}/");
    crate::note!("[control] {url}");
    std::thread::Builder::new()
        .name("control".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(&stream);
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                // headers: only the ones that tell us who's asking
                let (mut host, mut fetch_site) = (String::new(), String::new());
                let mut h = String::new();
                while reader.read_line(&mut h).map(|n| n > 2).unwrap_or(false) {
                    if let Some((k, v)) = h.split_once(':') {
                        match k.trim().to_ascii_lowercase().as_str() {
                            "host" => host = v.trim().to_ascii_lowercase(),
                            "sec-fetch-site" => fetch_site = v.trim().to_ascii_lowercase(),
                            _ => {}
                        }
                    }
                    h.clear();
                }
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                let mut out = &stream;
                if !allowed(&host, &fetch_site, port) {
                    let _ = out.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    continue;
                }
                if let Some(id) = path.strip_prefix("/q/").and_then(|p| p.strip_suffix(".wav")).and_then(|i| i.parse::<u32>().ok()) {
                    // the private preview: only from this machine, and it counts as "heard"
                    let file = {
                        let q = queue.lock().unwrap();
                        std::fs::read(q.wav_path(id)).ok()
                    };
                    match file {
                        Some(b) => {
                            queue.lock().unwrap().mark_previewed(id);
                            let head = format!("HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", b.len());
                            let _ = out.write_all(head.as_bytes());
                            let _ = out.write_all(&b);
                        }
                        None => {
                            let _ = out.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        }
                    }
                    continue;
                }
                if path.starts_with("/preview.jpg") {
                    // the newest frame of the shorts camera (only while clip recording is on)
                    match crate::clips::latest_jpeg() {
                        Some(j) => {
                            let head = format!("HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", j.len());
                            let _ = out.write_all(head.as_bytes());
                            let _ = out.write_all(&j);
                        }
                        None => {
                            let _ = out.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        }
                    }
                    continue;
                }
                let resp = if let Some(cmd) = path.strip_prefix("/cmd/") {
                    let _ = tx.send(decode(cmd));
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nCache-Control: no-store\r\nConnection: close\r\n\r\nok".to_string()
                } else if path == "/mics" || path == "/checks" {
                    let body = if path == "/mics" { mics_json() } else { checks_json() };
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                } else if path == "/questions" {
                    let body = queue.lock().unwrap().to_json();
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                } else if path == "/state" {
                    let body = state.0.lock().unwrap().clone();
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                } else if path == "/" || path.starts_with("/?") {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        PAGE.len(),
                        PAGE
                    )
                } else {
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                };
                let _ = out.write_all(resp.as_bytes());
            }
        })
        .expect("spawn control thread");
    (rx, url)
}

/// The viewer question page's port, for the checks.
pub static ASK_PORT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(7778);

/// Free bytes on the disk holding `dir`.
pub fn free_space(dir: &std::path::Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
            return None;
        }
        Some(st.f_bavail as u64 * st.f_frsize as u64)
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let w: Vec<u16> = dir
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut free: u64 = 0;
        let ok = unsafe {
            windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
                w.as_ptr(),
                &mut free,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        (ok != 0).then_some(free)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = dir;
        None
    }
}

/// Input devices, for the setup page's microphone list.
fn mics_json() -> String {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    #[allow(deprecated)]
    let name = |d: &cpal::Device| d.name().unwrap_or_default();
    let default = host.default_input_device().map(|d| name(&d));
    let devices: Vec<String> = host
        .input_devices()
        .map(|it| {
            it.map(|d| name(&d))
                .filter(|n| !n.trim().is_empty())
                .collect()
        })
        .unwrap_or_default();
    serde_json::json!({"devices": devices, "default": default}).to_string()
}

/// What the setup page checks: OBS's virtual camera, ffmpeg, Whisper, and where your files go.
fn checks_json() -> String {
    let mut items = vec![];
    if cfg!(windows) {
        let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
        let dll = std::path::Path::new(&pf)
            .join("obs-studio\\data\\obs-plugins\\win-dshow\\obs-virtualcam-module64.dll");
        let ok = dll.exists();
        items.push(serde_json::json!({"name": "OBS virtual camera (for the W webcam mode)", "ok": ok,
            "note": if ok { "installed" } else { "install OBS Studio and start its Virtual Camera once" }}));
    }
    let ff = std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    items.push(serde_json::json!({"name": "ffmpeg (for saving shorts as videos)", "ok": ff, "note": if ff { "found" } else { "winget install Gyan.FFmpeg, then restart the app" }}));
    let model = crate::whisper_model(None);
    let whisper = cfg!(feature = "whisper") && model.exists();
    items.push(serde_json::json!({"name": "Speech-to-text (bubbles, gags, question text)", "ok": whisper, "optional": !cfg!(feature = "whisper"),
        "note": if whisper { "Whisper ready".to_string() } else if cfg!(feature = "whisper") { format!("download ggml-base.en.bin (huggingface.co/ggerganov/whisper.cpp) into {}", model.parent().map(|p| p.display().to_string()).unwrap_or_default()) } else { "optional: this build has no Whisper; a caption file or the Say box works instead".to_string() }}));
    let dir = crate::applog::user_dir();
    items.push(serde_json::json!({"name": "Your files", "ok": true, "note": format!("settings, clips and logs are in {}", dir.display())}));
    // room for shorts (each is 5-15 MB)
    match free_space(&dir) {
        Some(b) => {
            let gb = b as f64 / 1e9;
            items.push(serde_json::json!({"name": "Disk space for clips", "ok": gb >= 2.0, "note": format!("{gb:.1} GB free")}));
        }
        None => items.push(serde_json::json!({"name": "Disk space for clips", "ok": true, "optional": true, "note": "couldn't tell"})),
    }
    // the viewer question page answers
    let ask = std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from((
            [127, 0, 0, 1],
            ASK_PORT.load(std::sync::atomic::Ordering::Relaxed),
        )),
        std::time::Duration::from_millis(300),
    )
    .is_ok();
    items.push(serde_json::json!({"name": "Viewer question page", "ok": ask, "optional": !ask, "note": if ask { "running" } else { "not running (optional)" }}));
    serde_json::json!({"items": items}).to_string()
}

/// Only this machine's own pages and tools. A browser tab on some website can make requests to
/// 127.0.0.1 too; browsers mark those `Sec-Fetch-Site: cross-site`, and a renamed host
/// (DNS rebinding) shows up in `Host`. Tools like curl or a Stream Deck send neither.
pub fn allowed(host: &str, fetch_site: &str, port: u16) -> bool {
    let host_ok = host.is_empty()
        || host == format!("127.0.0.1:{port}")
        || host == format!("localhost:{port}")
        || host == format!("[::1]:{port}");
    let site_ok = fetch_site.is_empty() || fetch_site == "same-origin" || fetch_site == "none";
    host_ok && site_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn other_websites_cant_press_buttons() {
        assert!(
            allowed("127.0.0.1:7777", "same-origin", 7777),
            "the control page itself"
        );
        assert!(
            allowed("localhost:7777", "none", 7777),
            "typed into the address bar"
        );
        assert!(allowed("", "", 7777), "curl, Stream Deck");
        assert!(
            allowed("127.0.0.1:7777", "", 7777),
            "OBS dock / older browsers"
        );
        assert!(
            !allowed("127.0.0.1:7777", "cross-site", 7777),
            "a page on another site"
        );
        assert!(!allowed("127.0.0.1:7777", "same-site", 7777));
        assert!(
            !allowed("evil.example:7777", "same-origin", 7777),
            "DNS rebinding"
        );
    }
}
