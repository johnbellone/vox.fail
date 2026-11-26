//! The page itself: its HTML, CSS and JavaScript.

#[allow(unused_imports)]
use super::*;

// ------------------------------------------------------------------------------------------
// The page
// ------------------------------------------------------------------------------------------

pub(super) const PAGE: &str = r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Ask a question</title><link rel="stylesheet" href="/app.css"></head><body>
<main>
<header><h1 id="title">Ask a question</h1><p class="sub">Record a question. If the streamer picks it, your character asks it live, in your voice.</p></header>
<section id="closed" class="card" hidden><p class="big">Questions are closed right now.</p><p class="mute">Check back when the stream opens them.</p></section>
<section id="login" class="card" hidden>
 <h2>1. Who's asking?</h2>
 <div id="tw" hidden><button id="twgo" class="primary">Log in with Twitch</button>
  <div id="code" hidden><p>Approve this code on Twitch:</p><p class="code" id="ucode"></p><p><a id="twlink" target="_blank" rel="noopener noreferrer">Open Twitch to approve</a></p><p class="mute" id="twwait">Waiting for Twitch...</p></div></div>
 <div id="nm" hidden><label for="name">Your name</label><div class="row"><input id="name" maxlength="25" autocomplete="nickname" placeholder="letters, numbers, _"><button id="namego" class="primary">Continue</button></div></div>
 <p id="nologin" class="mute" hidden>This stream hasn't set up sign-in for questions yet.</p>
</section>
<section id="ask" class="card" hidden>
 <p class="me">Asking as <b id="who"></b> <button id="out" class="link">not you?</button></p>
 <h2>2. Pick your character</h2><div id="chars" class="chars" role="radiogroup" aria-label="Character"></div>
 <h2>3. Record your question</h2>
 <p class="mute">Up to <span id="max">30</span> seconds. Keep it friendly: no hate, harassment or personal details.</p>
 <div class="rec"><button id="rec" class="primary rec-btn">Record</button><div class="bar"><div id="fill"></div></div><span id="t">0.0 s</span></div>
 <div id="review" hidden><audio id="play" controls></audio><div class="row"><button id="again">Record again</button><button id="send" class="primary">Send question</button></div></div>
 <p id="cant" class="warn" role="alert" hidden></p>
</section>
<section id="mine" class="card" hidden><h2>Your questions</h2><ul id="list"></ul></section>
<p id="msg" class="warn" role="status"></p>
<footer>The streamer listens to every question privately before it can air. Recordings are deleted once your question is answered or skipped.</footer>
</main><script src="/app.js"></script></body></html>"#;

pub(super) const APP_CSS: &str = r#"@font-face{font-family:Pixel;src:url(/font.ttf)}
:root{color-scheme:dark;--bg:#14151b;--card:#1f212b;--ink:#f1f2f7;--mute:#a3a8bb;--acc:#ffc93c;--acc-ink:#1b1a1f;--line:#343748;--warn:#ff8a7a;--ok:#7ddc9b}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--ink);font:16px/1.5 system-ui,-apple-system,Segoe UI,sans-serif}
main{max-width:560px;margin:0 auto;padding:24px 16px 48px}
h1{font-family:Pixel,system-ui;font-size:20px;line-height:1.5;margin:0 0 8px;color:var(--acc)}h2{font-size:15px;margin:18px 0 8px}
.sub,.mute{color:var(--mute)}.mute{font-size:14px}
.card{background:var(--card);border:1px solid var(--line);border-radius:12px;padding:16px;margin:16px 0}
button{font:inherit;color:var(--ink);background:#2a2d3a;border:1px solid var(--line);border-radius:8px;padding:10px 14px;cursor:pointer;min-height:44px}
button:hover{border-color:var(--acc)}button:disabled{opacity:.45;cursor:not-allowed}
button.primary{background:var(--acc);color:var(--acc-ink);border-color:var(--acc);font-weight:600}
button.link{background:none;border:none;color:var(--mute);text-decoration:underline;padding:0;min-height:0}
button:focus-visible,input:focus-visible,a:focus-visible{outline:3px solid var(--acc);outline-offset:2px}
input{font:inherit;color:var(--ink);background:#14151b;border:1px solid var(--line);border-radius:8px;padding:10px;width:100%;min-height:44px}
label{display:block;margin-bottom:6px}.row{display:flex;gap:8px;margin-top:10px}.row input{flex:1}
a{color:var(--acc)}.code{font-family:Pixel,monospace;font-size:22px;letter-spacing:.12em;margin:6px 0}
.chars{display:grid;grid-template-columns:repeat(auto-fill,minmax(120px,1fr));gap:6px;max-height:260px;overflow:auto;padding:2px}
.chars button{display:flex;flex-direction:column;align-items:center;gap:4px;text-align:center;font-size:14px;min-height:40px;padding:8px 6px}.chars img{image-rendering:pixelated;width:56px;height:56px}.chars button[aria-checked=true]{background:var(--acc);color:var(--acc-ink);border-color:var(--acc)}
.rec{display:flex;align-items:center;gap:12px;margin:10px 0}.rec-btn{min-width:110px}.rec-btn.on{background:var(--warn);border-color:var(--warn)}
.bar{flex:1;height:10px;background:#14151b;border:1px solid var(--line);border-radius:6px;overflow:hidden}#fill{height:100%;width:0;background:var(--acc)}
#t{font-variant-numeric:tabular-nums;color:var(--mute);min-width:56px;text-align:right}
audio{width:100%;margin-top:8px}.warn{color:var(--warn)}.ok{color:var(--ok)}.me{margin:0 0 4px;color:var(--mute)}.me b{color:var(--ink)}
#list{list-style:none;padding:0;margin:0}#list li{display:flex;justify-content:space-between;align-items:center;gap:8px;padding:10px 0;border-top:1px solid var(--line)}#list li:first-child{border-top:none}
.state{font-weight:600}.state.on_air,.state.approved{color:var(--ok)}
footer{color:var(--mute);font-size:13px;margin-top:24px}
@media (prefers-reduced-motion:reduce){*{transition:none!important}}"#;

pub(super) const APP_JS: &str = r#"'use strict';
pub(super) const $=id=>document.getElementById(id);
let sess=null;try{sess=sessionStorage.getItem('ask')}catch(e){}
let info=null,pick=-1,wav=null,recState=null,polling=null;
pub(super) const api=async(path,opt={})=>{opt.headers=Object.assign({},opt.headers||{},sess?{'X-Ask-Session':sess}:{});
  const r=await fetch(path,opt);let j={};try{j=await r.json()}catch(e){}if(!r.ok)throw new Error(j.error||('Error '+r.status));return j;};
pub(super) const say=(t,ok)=>{$('msg').textContent=t||'';$('msg').className=ok?'ok':'warn'};
pub(super) const save=s=>{sess=s;try{s?sessionStorage.setItem('ask',s):sessionStorage.removeItem('ask')}catch(e){}};
function show(id,on){$(id).hidden=!on}
async function load(){
  try{info=await api('/api/info')}catch(e){say('The stream is offline or unreachable.');return}
  if(info.title){$('title').textContent=info.title;document.title=info.title}
  $('max').textContent=Math.round(info.max_secs);
  const logged=!!info.me;
  show('closed',!info.open);
  show('login',info.open&&!logged);show('tw',info.login==='twitch');show('nm',info.login==='name');show('nologin',info.login==='none');
  show('ask',info.open&&logged);show('mine',logged);
  if(logged){$('who').textContent=info.me.user+(info.me.verified?'':' (unverified)');
    if(!$('chars').children.length)info.characters.forEach((c,i)=>{const b=document.createElement('button');const im=document.createElement('img');im.src='/portrait/'+i+'.png';im.alt='';im.width=56;im.height=56;im.onerror=()=>im.remove();b.appendChild(im);const s=document.createElement('span');s.textContent=c;b.appendChild(s);b.setAttribute('role','radio');b.setAttribute('aria-checked','false');b.onclick=()=>{pick=i;[...$('chars').children].forEach((x,j)=>x.setAttribute('aria-checked',j===i?'true':'false'));upd()};$('chars').appendChild(b)});
    const ok=!info.can_ask||info.can_ask.ok;$('cant').hidden=ok;$('cant').textContent=ok?'':info.can_ask.reason+(info.can_ask.reason.includes('waiting')?' You can send another once it\u2019s answered.':'');$('rec').disabled=!ok&&!recState;
    mine();}
  upd();
}
function upd(){$('send').disabled=!(wav&&pick>=0&&info&&info.open&&(!info.can_ask||info.can_ask.ok));}
$('twgo').onclick=async()=>{say('');$('twgo').disabled=true;
  try{const j=await api('/api/login/twitch',{method:'POST'});save(j.session);$('ucode').textContent=j.user_code;$('twlink').href=j.verification_uri;show('code',true);
    const tick=async()=>{try{const p=await api('/api/login/poll',{method:'POST'});
      if(p.state==='done'){show('code',false);$('twgo').disabled=false;load();return}
      if(p.state==='pending'){setTimeout(tick,(j.interval||5)*1000);return}
      save(null);show('code',false);$('twgo').disabled=false;say(p.state==='denied'?'Login was cancelled on Twitch.':'That code expired. Try again.');}catch(e){setTimeout(tick,6000)}};
    setTimeout(tick,(j.interval||5)*1000);
  }catch(e){$('twgo').disabled=false;say(e.message)}};
$('namego').onclick=async()=>{say('');try{const j=await api('/api/login/name',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({name:$('name').value.trim()})});save(j.session);load()}catch(e){say(e.message)}};
$('name').addEventListener('keydown',e=>{if(e.key==='Enter')$('namego').click()});
$('out').onclick=async()=>{try{await api('/api/logout',{method:'POST'})}catch(e){}save(null);location.reload()};
// recording: raw samples from the mic, downsampled to 24 kHz mono 16-bit WAV in the page
async function startRec(){
  say('');let stream;
  try{stream=await navigator.mediaDevices.getUserMedia({audio:{channelCount:1,echoCancellation:true,noiseSuppression:true,autoGainControl:true}})}
  catch(e){say('Microphone blocked. Allow microphone access for this page and try again.');return}
  const ctx=new (window.AudioContext||window.webkitAudioContext)();const src=ctx.createMediaStreamSource(stream);const proc=ctx.createScriptProcessor(4096,1,1);
  const chunks=[];const t0=performance.now();const max=info.max_secs;
  proc.onaudioprocess=e=>{chunks.push(new Float32Array(e.inputBuffer.getChannelData(0)));const s=(performance.now()-t0)/1000;$('fill').style.width=Math.min(100,s/max*100)+'%';$('t').textContent=s.toFixed(1)+' s';if(s>=max)stopRec()};
  src.connect(proc);proc.connect(ctx.destination);
  recState={stream,ctx,src,proc,chunks};$('rec').textContent='Stop';$('rec').classList.add('on');show('review',false);wav=null;upd();
}
function stopRec(){
  const r=recState;if(!r)return;recState=null;
  try{r.proc.disconnect();r.src.disconnect()}catch(e){}r.stream.getTracks().forEach(t=>t.stop());
  const rate=r.ctx.sampleRate;r.ctx.close();
  let n=0;r.chunks.forEach(c=>n+=c.length);const all=new Float32Array(n);let o=0;r.chunks.forEach(c=>{all.set(c,o);o+=c.length});
  const out=downsample(trim(all,rate),rate,24000);
  $('rec').textContent='Record';$('rec').classList.remove('on');
  if(out.length<24000*0.8){say('That was too short. Hold Record and ask your question.');return}
  wav=new Blob([encode(out,24000)],{type:'audio/wav'});$('play').src=URL.createObjectURL(wav);show('review',true);upd();
}
function trim(s,rate){const th=0.01,pad=Math.floor(rate*0.2);let a=0,b=s.length-1;while(a<s.length&&Math.abs(s[a])<th)a++;while(b>a&&Math.abs(s[b])<th)b--;return s.subarray(Math.max(0,a-pad),Math.min(s.length,b+pad))}
function downsample(s,from,to){if(from===to)return s;const r=from/to,n=Math.floor(s.length/r),o=new Float32Array(n);for(let i=0;i<n;i++){const a=Math.floor(i*r),b=Math.min(s.length,Math.floor((i+1)*r));let sum=0;for(let j=a;j<b;j++)sum+=s[j];o[i]=sum/Math.max(1,b-a)}return o}
function encode(s,rate){const b=new ArrayBuffer(44+s.length*2),v=new DataView(b);const w=(o,t)=>{for(let i=0;i<t.length;i++)v.setUint8(o+i,t.charCodeAt(i))};
  w(0,'RIFF');v.setUint32(4,36+s.length*2,true);w(8,'WAVE');w(12,'fmt ');v.setUint32(16,16,true);v.setUint16(20,1,true);v.setUint16(22,1,true);v.setUint32(24,rate,true);v.setUint32(28,rate*2,true);v.setUint16(32,2,true);v.setUint16(34,16,true);w(36,'data');v.setUint32(40,s.length*2,true);
  for(let i=0;i<s.length;i++){const x=Math.max(-1,Math.min(1,s[i]));v.setInt16(44+i*2,x<0?x*32768:x*32767,true)}return b}
$('rec').onclick=()=>recState?stopRec():startRec();
$('again').onclick=()=>{wav=null;show('review',false);startRec()};
$('send').onclick=async()=>{if(!wav||pick<0)return;$('send').disabled=true;say('Sending...');
  try{const j=await api('/api/ask?c='+pick,{method:'POST',headers:{'Content-Type':'audio/wav'},body:wav});wav=null;show('review',false);say('Sent! You are number '+j.position+' in line.',true);load()}
  catch(e){say(e.message);upd()}};
pub(super) const LABEL={waiting:'In line',approved:'Coming up',on_air:'On air now!',done:'Answered. Thanks!',skipped:'Not picked this time'};
async function mine(){
  try{const j=await api('/api/mine');const ul=$('list');ul.textContent='';
    if(!j.questions.length){const li=document.createElement('li');li.textContent='Nothing sent yet.';li.className='mute';ul.appendChild(li)}
    j.questions.slice().reverse().forEach(q=>{const li=document.createElement('li');const l=document.createElement('span');
      l.textContent='#'+q.id+' as '+q.character+(q.position?' · '+q.position+' of '+q.of:'')+(q.votes?' · '+q.votes+' votes':'');
      const s=document.createElement('span');s.className='state '+q.state;s.textContent=LABEL[q.state]||q.state;li.append(l,s);
      if(q.state==='waiting'||q.state==='approved'){const b=document.createElement('button');b.textContent='Withdraw';b.className='link';b.onclick=async()=>{try{await api('/api/withdraw?id='+q.id,{method:'POST'});load()}catch(e){say(e.message)}};li.appendChild(b)}
      ul.appendChild(li)});
    const live=j.questions.some(q=>q.state==='waiting'||q.state==='approved'||q.state==='on_air');
    const hint=$('mine').querySelector('.hint');if(live&&!hint){const p=document.createElement('p');p.className='mute hint';p.textContent='Tell chat "!up" and your question number to move it up the line.';$('mine').appendChild(p)}
  }catch(e){}
}
load();setInterval(()=>{if(!recState)load()},10000);
"#;
