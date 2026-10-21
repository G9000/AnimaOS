const REDUCE = matchMedia('(prefers-reduced-motion: reduce)').matches;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

(() => {
  if (!REDUCE && 'IntersectionObserver' in window) { const io = new IntersectionObserver((es) => es.forEach((e) => { if (e.isIntersecting) { e.target.classList.remove('pre'); io.unobserve(e.target); } }), { rootMargin: '0px 0px -8% 0px' }); document.querySelectorAll('.rv').forEach((el) => { if (el.getBoundingClientRect().top > innerHeight) { el.classList.add('pre'); io.observe(el); } }); }
  // hero preview types its line
  const pv = document.getElementById('pv'); const lines = ['Still on the Lisbon plan. Prices dropped €38 overnight. Want me to book it?', 'Scout found 3 hotels near Alfama under €180. Shortlist is in the chat.', 'Done. Flight booked, calendar blocked, Mimi has the dates.'];
  if (REDUCE) { pv.textContent = lines[0]; return; }
  (async () => { let i = 0; while (true) { const t = lines[i++ % lines.length]; pv.innerHTML = '<span></span><span class="tcaret"></span>'; for (const ch of t) { pv.firstChild.textContent += ch; await sleep(26 + Math.random() * 30); } await sleep(3600); } })();
})();


/* hero background: one live pixel field. The cursor parts it like liquid; hold still and anima's face forms out of it. */
(() => {
  const hero = document.querySelector('.hero');
  const canvas = document.createElement('canvas'); canvas.id = 'field'; canvas.setAttribute('aria-hidden', 'true');
  hero.prepend(canvas);
  const gl = canvas.getContext('webgl', { premultipliedAlpha: true, antialias: false, alpha: true });
  if (!gl) return;

  const VS = 'attribute vec2 a;varying vec2 v;void main(){v=a*.5+.5;gl_Position=vec4(a,0.,1.);}';
  // trail: velocity in rg (0.5 = still), strength in b. Advected along itself, so it flows and fades like water.
  const TRAIL = `precision highp float;varying vec2 v;
uniform sampler2D u_prev;uniform vec2 u_m;uniform vec2 u_pm;uniform vec2 u_vel;uniform float u_asp;uniform float u_on;
float seg(vec2 p,vec2 a,vec2 b){vec2 pa=p-a,ba=b-a;float h=clamp(dot(pa,ba)/max(dot(ba,ba),1e-6),0.,1.);return length(pa-ba*h);}
void main(){
 vec4 c=texture2D(u_prev,v);vec2 vel=c.rg-.5;
 vec4 s=texture2D(u_prev,v-vel*.035);
 vec2 nv=(s.rg-.5)*.965;float k=s.b*.972;
 vec2 asp=vec2(u_asp,1.);
 float d=seg(v*asp,u_pm*asp,u_m*asp);
 float splat=exp(-d*d/.0045)*u_on;
 nv=mix(nv,clamp(u_vel*6.,-.5,.5),splat*.85);
 k=max(k,splat*clamp(length(u_vel)*28.+.35,0.,1.));
 gl_FragColor=vec4(nv+.5,k,1.);
}`;
  const FIELD = `precision highp float;varying vec2 v;
uniform vec2 u_res;uniform float u_time;uniform float u_pixel;uniform sampler2D u_trail;uniform sampler2D u_img;uniform vec2 u_isz;uniform float u_has;
uniform float u_form;uniform vec2 u_fc;uniform float u_fz;
float hash(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}
float noise(vec2 p){vec2 i=floor(p),f=fract(p),u=f*f*(3.-2.*f);return mix(mix(hash(i),hash(i+vec2(1,0)),u.x),mix(hash(i+vec2(0,1)),hash(i+vec2(1,1)),u.x),u.y);}
float fbm(vec2 p){float s=0.,a=.5;for(int i=0;i<4;i++){s+=a*noise(p);p=p*2.07+vec2(3.1,1.7);a*=.5;}return s;}
float bayer2(vec2 a){a=floor(a);return fract(a.x/2.+a.y*a.y*.75);}
void main(){
 vec2 cell=floor(gl_FragCoord.xy/u_pixel);vec2 frag=(cell+.5)*u_pixel;vec2 uv=frag/u_res;
 vec4 tr=texture2D(u_trail,uv);vec2 vel=tr.rg-.5;float wake=tr.b;
 vec2 q=uv-vel*.32;                                   // the liquid push
 float t=u_time*.07;
 float an=u_res.x/u_res.y;
 vec2 p=q*vec2(an,1.)*3.;
 float drift=fbm(p+vec2(t,-t*.6)+fbm(p*.7-t)*1.4);
 float field=smoothstep(.42,.95,drift)*.42;             // ambient grain
 float ring=sin(wake*22.-u_time*5.)*.5+.5;
 field+=wake*ring*.35;                                  // ripples in the wake
 float face=0.;
 if(u_has>.5){
  vec2 ip=(q-u_fc);float ai=u_isz.x/u_isz.y;
  if(an>ai)ip.y*=ai/an;else ip.x*=an/ai;
  vec2 iuv=ip/u_fz+vec2(.36,.56);
  float inside=step(0.,iuv.x)*step(iuv.x,1.)*step(0.,iuv.y)*step(iuv.y,1.);
  vec3 ic=texture2D(u_img,vec2(iuv.x,1.-iuv.y)).rgb;
  float lum=dot(ic,vec3(.299,.587,.114));
  float con=clamp((lum-.5)*1.5+.5,0.,1.);
  float vign=smoothstep(.85,.25,length((q-u_fc)*vec2(an*.6,1.)));
  face=smoothstep(.12,1.,con)*inside*vign;
  float grain=fbm(cell*.05+t*3.);
  float local=clamp((u_form*(1.-wake*1.4)*1.6-.3-grain*.6)*2.6,0.,1.);
  field=mix(field,max(field*.6,face),local);
 }
 field*=1.-wake*.7;                                    // the cursor parts it
 float th=fract(bayer2(.125*cell)*.015625+bayer2(.25*cell)*.0625+bayer2(.5*cell)*.25+bayer2(cell));
 float outer=step(th+.004,field*1.1);
 float inner=step(th,pow(field,2.4)*1.7-.2);
 vec3 col=mix(vec3(.914,.302,.671),vec3(.957,.953,.937),inner);
 float fade=mix(.38,1.,smoothstep(.12,.55,uv.x));       // calmer under the headline
 gl_FragColor=vec4(col*outer*fade,outer*fade);
}`;
  const prog = (fs) => { const s = (t, src) => { const x = gl.createShader(t); gl.shaderSource(x, src); gl.compileShader(x); return x; }; const p = gl.createProgram(); gl.attachShader(p, s(gl.VERTEX_SHADER, VS)); gl.attachShader(p, s(gl.FRAGMENT_SHADER, fs)); gl.linkProgram(p); return gl.getProgramParameter(p, gl.LINK_STATUS) ? p : null; };
  const pT = prog(TRAIL), pF = prog(FIELD);
  if (!pT || !pF) { canvas.remove(); return; }
  gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer()); gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1,-1,1,-1,-1,1,1,1]), gl.STATIC_DRAW);
  const bindQuad = (p) => { const a = gl.getAttribLocation(p, 'a'); gl.enableVertexAttribArray(a); gl.vertexAttribPointer(a, 2, gl.FLOAT, false, 0, 0); };
  const U = (p, names) => Object.fromEntries(names.map((n) => [n, gl.getUniformLocation(p, n)]));
  const uT = U(pT, ['u_prev','u_m','u_pm','u_vel','u_asp','u_on']);
  const uF = U(pF, ['u_res','u_time','u_pixel','u_trail','u_img','u_isz','u_has','u_form','u_fc','u_fz']);

  const tex = (w, h) => { const t = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D, t); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE); const px = new Uint8Array(w * h * 4); for (let i = 0; i < px.length; i += 4) { px[i] = 128; px[i + 1] = 128; px[i + 3] = 255; } gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, px); return t; };
  let TW = 0, TH = 0, A = null, B = null, fA = null, fB = null;
  const mkTrail = () => { TW = Math.max(64, Math.round(canvas.width / 6)); TH = Math.max(64, Math.round(canvas.height / 6)); [A, B] = [tex(TW, TH), tex(TW, TH)]; [fA, fB] = [A, B].map((t) => { const f = gl.createFramebuffer(); gl.bindFramebuffer(gl.FRAMEBUFFER, f); gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, t, 0); return f; }); gl.bindFramebuffer(gl.FRAMEBUFFER, null); };
  let dpr = 1;
  const fit = () => { dpr = Math.min(devicePixelRatio || 1, 1.5); const w = Math.round(canvas.clientWidth * dpr), h = Math.round(canvas.clientHeight * dpr); if (w === canvas.width && h === canvas.height && A) return; canvas.width = Math.max(1, w); canvas.height = Math.max(1, h); mkTrail(); };
  fit(); addEventListener('resize', fit);

  let img = null;
  const im = new Image(); im.onload = () => { img = gl.createTexture(); gl.activeTexture(gl.TEXTURE1); gl.bindTexture(gl.TEXTURE_2D, img); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE); gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, im); gl.useProgram(pF); gl.uniform2f(uF.u_isz, im.naturalWidth, im.naturalHeight); }; im.src = '/img/keyart.webp';

  const m = { x: .65, y: .5, px: .65, py: .5, vx: 0, vy: 0, moved: 0, on: 0 };
  let form = REDUCE ? 1 : 0, lastMove = performance.now() - 2000;
  hero.addEventListener('pointermove', (e) => { const b = canvas.getBoundingClientRect(); const x = (e.clientX - b.left) / b.width, y = 1 - (e.clientY - b.top) / b.height; m.vx = x - m.x; m.vy = y - m.y; m.x = x; m.y = y; m.on = 1; lastMove = performance.now(); });
  hero.addEventListener('pointerleave', () => { m.on = 0; });

  let visible = true; new IntersectionObserver(([e]) => { visible = e.isIntersecting; if (visible) requestAnimationFrame(frame); }).observe(hero);
  const t0 = performance.now();
  function frame(now) {
    if (!visible) return;
    fit();
    const idle = (now - lastMove) / 1000;
    const target = REDUCE ? 1 : (idle > .9 ? 1 : 0);
    form += (target - form) * (target > form ? .012 : .08);
    // trail pass
    gl.useProgram(pT); bindQuad(pT);
    gl.bindFramebuffer(gl.FRAMEBUFFER, fB); gl.viewport(0, 0, TW, TH);
    gl.activeTexture(gl.TEXTURE0); gl.bindTexture(gl.TEXTURE_2D, A); gl.uniform1i(uT.u_prev, 0);
    gl.uniform2f(uT.u_m, m.x, m.y); gl.uniform2f(uT.u_pm, m.px, m.py); gl.uniform2f(uT.u_vel, m.vx, m.vy);
    gl.uniform1f(uT.u_asp, canvas.width / canvas.height); gl.uniform1f(uT.u_on, REDUCE ? 0 : m.on * (Math.hypot(m.vx, m.vy) > .0004 ? 1 : .25));
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    [A, B] = [B, A]; [fA, fB] = [fB, fA];
    m.px = m.x; m.py = m.y; m.vx *= .6; m.vy *= .6;
    // field pass
    gl.bindFramebuffer(gl.FRAMEBUFFER, null); gl.viewport(0, 0, canvas.width, canvas.height);
    gl.useProgram(pF); bindQuad(pF);
    gl.clearColor(0, 0, 0, 0); gl.clear(gl.COLOR_BUFFER_BIT);
    gl.activeTexture(gl.TEXTURE0); gl.bindTexture(gl.TEXTURE_2D, A); gl.uniform1i(uF.u_trail, 0);
    if (img) { gl.activeTexture(gl.TEXTURE1); gl.bindTexture(gl.TEXTURE_2D, img); gl.uniform1i(uF.u_img, 1); }
    gl.uniform1f(uF.u_has, img ? 1 : 0);
    const narrow = canvas.clientWidth < 820;
    gl.uniform2f(uF.u_res, canvas.width, canvas.height); gl.uniform1f(uF.u_time, (now - t0) / 1000);
    gl.uniform1f(uF.u_pixel, Math.round((narrow ? 4 : 5) * dpr)); gl.uniform1f(uF.u_form, form);
    gl.uniform2f(uF.u_fc, narrow ? .5 : .47, narrow ? .55 : .52); gl.uniform1f(uF.u_fz, narrow ? 1.3 : 1.7);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    if (!REDUCE) requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);
})();

/* hero windows: drag them anywhere, they spring back home */
(() => {
  document.querySelectorAll('.drag').forEach((el) => {
    let sx = 0, sy = 0, lx = 0, vx = 0, id = null;
    el.addEventListener('pointerdown', (e) => { if (e.button !== 0) return; id = e.pointerId; sx = e.clientX; sy = e.clientY; lx = e.clientX; vx = 0; el.classList.add('dragging'); try { el.setPointerCapture(id); } catch {} });
    el.addEventListener('pointermove', (e) => { if (e.pointerId !== id) return; const dx = e.clientX - sx, dy = e.clientY - sy; vx = vx * 0.7 + (e.clientX - lx) * 0.3; lx = e.clientX; el.style.translate = dx + 'px ' + dy + 'px'; el.style.rotate = Math.max(-8, Math.min(8, vx * 0.6)) + 'deg'; });
    const end = (e) => { if (e.pointerId !== id) return; id = null; el.classList.remove('dragging'); el.style.translate = '0px 0px'; el.style.rotate = '0deg'; };
    el.addEventListener('pointerup', end); el.addEventListener('pointercancel', end);
  });
})();

/* WebGL — design/shaders/presence.glsl, with the key-art reveal */
const GL = (() => {
  const FS = `precision highp float;
uniform vec2 u_res;uniform float u_time;uniform float u_zoom;uniform float u_pixel;uniform float u_energy;uniform vec2 u_m;uniform float u_mstr;
uniform sampler2D u_img;uniform vec2 u_isz;uniform float u_reveal;uniform float u_iz;uniform vec2 u_ic;uniform float u_has;uniform float u_mask;uniform float u_con;
float hash(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}
float noise(vec2 p){vec2 i=floor(p),f=fract(p),u=f*f*(3.-2.*f);return mix(mix(hash(i),hash(i+vec2(1,0)),u.x),mix(hash(i+vec2(0,1)),hash(i+vec2(1,1)),u.x),u.y);}
float fbm(vec2 p){float v=0.,a=.5;for(int i=0;i<5;i++){v+=a*noise(p);p=p*2.03+vec2(1.7,9.2);a*=.5;}return v;}
float bayer2(vec2 a){a=floor(a);return fract(a.x/2.+a.y*a.y*.75);}
void main(){
 vec2 cell=floor(gl_FragCoord.xy/u_pixel);vec2 frag=(cell+.5)*u_pixel;
 vec2 uv=(frag-.5*u_res)/(min(u_res.x,u_res.y)*u_zoom);
 float t=u_time*.12;
 vec2 q=vec2(fbm(uv*2.2+t),fbm(uv*2.2-t+4.1));
 vec2 w=uv+.22*(q-.5)*(.6+u_energy);
 float r=length(w);
 float body=smoothstep(.48,.02,r);
 float tex=fbm(w*3.4+q*2.+t*1.5);
 float rings=.5+.5*sin(r*46.-u_time*1.4);
 float field=clamp(body*(.55+.6*tex)+rings*body*.12*u_energy,0.,1.);
 if(u_has>.5&&u_reveal>.001){
  vec2 ip=frag/u_res-.5;float an=u_res.x/u_res.y,ai=u_isz.x/u_isz.y;
  if(an>ai)ip.y*=ai/an;else ip.x*=an/ai;
  vec2 iuv=ip/u_iz+vec2(u_ic.x,1.-u_ic.y);
  float inside=step(0.,iuv.x)*step(iuv.x,1.)*step(0.,iuv.y)*step(iuv.y,1.);
  vec3 ic=texture2D(u_img,vec2(iuv.x,1.-iuv.y)).rgb;
  float lum=dot(ic,vec3(.299,.587,.114));
  float con=clamp((lum-.5)*u_con+.5,0.,1.);
  float shape=smoothstep(.66,.3,r);
  float imgField=smoothstep(.1,1.15,con)*.92*inside*mix(1.,shape,u_mask);
  float md=length((frag/u_res-u_m)*vec2(an,1.));
  float scrub=u_mstr*exp(-md*md/.02);
  float grain=fbm(cell*.045+vec2(3.1,8.7)+t*.6);
  float local=clamp(((u_reveal-scrub)*1.7-.35-grain)*3.,0.,1.);
  float stray=step(.985,hash(cell+floor(u_time*2.)))*step(.02,imgField)*(1.-local)*u_reveal;
  field=max(mix(field,imgField,local),stray);
 }
 float th=fract(bayer2(.125*cell)*.015625+bayer2(.25*cell)*.0625+bayer2(.5*cell)*.25+bayer2(cell));
 float outer=step(th+.004,field*1.15);
 float inner=step(th,pow(field,2.2)*1.6-.15);
 vec3 col=mix(vec3(.914,.302,.671),vec3(.957,.953,.937),inner);
 gl_FragColor=vec4(col*outer,outer);
}`;
  const VS = 'attribute vec2 a;void main(){gl_Position=vec4(a,0.,1.);}';
  const views = [];
  let img = null; const waiting = [];
  const im = new Image(); im.onload = () => { img = im; waiting.splice(0).forEach((f) => f()); }; im.src = '/img/keyart.webp';
  function add(canvas, o) {
    if (!canvas) return null;
    const gl = canvas.getContext('webgl', { premultipliedAlpha: true, antialias: false });
    if (!gl) { canvas.style.background = 'radial-gradient(circle, #fff 0 4%, #E94DAB 20%, #E94DAB00 46%)'; return null; }
    const sh = (t, s) => { const x = gl.createShader(t); gl.shaderSource(x, s); gl.compileShader(x); return x; };
    const p = gl.createProgram(); gl.attachShader(p, sh(gl.VERTEX_SHADER, VS)); gl.attachShader(p, sh(gl.FRAGMENT_SHADER, FS)); gl.linkProgram(p);
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) return null;
    gl.useProgram(p); gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer()); gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1,-1,1,-1,-1,1,1,1]), gl.STATIC_DRAW);
    const a = gl.getAttribLocation(p, 'a'); gl.enableVertexAttribArray(a); gl.vertexAttribPointer(a, 2, gl.FLOAT, false, 0, 0);
    gl.enable(gl.BLEND); gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
    const u = {}; ['u_res','u_time','u_zoom','u_pixel','u_energy','u_m','u_mstr','u_img','u_isz','u_reveal','u_iz','u_ic','u_has','u_mask','u_con'].forEach((n) => { u[n] = gl.getUniformLocation(p, n); });
    gl.uniform1f(u.u_has, 0);
    const v = { gl, u, canvas, o, visible: true, mx: .5, my: .5, tx: .5, ty: .5, ms: 0, ts: 0, readyAt: 0 };
    const fit = () => { const dpr = Math.min(devicePixelRatio || 1, 1.5); v.dpr = dpr; canvas.width = Math.max(1, Math.round(canvas.clientWidth * dpr)); canvas.height = Math.max(1, Math.round(canvas.clientHeight * dpr)); };
    fit(); addEventListener('resize', fit); v.fit = fit;
    new IntersectionObserver(([e]) => { v.visible = e.isIntersecting; }).observe(canvas);
    if (o.image) { const load = () => { const tx = gl.createTexture(); gl.activeTexture(gl.TEXTURE0); gl.bindTexture(gl.TEXTURE_2D, tx); ['TEXTURE_MIN_FILTER','TEXTURE_MAG_FILTER'].forEach((k) => gl.texParameteri(gl.TEXTURE_2D, gl[k], gl.LINEAR)); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE); gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, img); gl.uniform1i(u.u_img, 0); gl.uniform2f(u.u_isz, img.naturalWidth, img.naturalHeight); gl.uniform1f(u.u_has, 1); v.readyAt = performance.now(); }; img ? load() : waiting.push(load); }
    if (o.host) { o.host.addEventListener('pointermove', (e) => { const b = canvas.getBoundingClientRect(); v.tx = (e.clientX - b.left) / b.width; v.ty = 1 - (e.clientY - b.top) / b.height; v.ts = .9; }); o.host.addEventListener('pointerleave', () => { v.ts = 0; }); }
    views.push(v); return v;
  }
  const t0 = performance.now();
  function frame(now) {
    const time = REDUCE ? 8 : (now - t0) / 1000;
    for (const v of views) {
      if (!v.visible) continue;
      const { gl, u, o, canvas } = v;
      if (canvas.width !== Math.round(canvas.clientWidth * v.dpr)) v.fit();
      v.mx += (v.tx - v.mx) * .12; v.my += (v.ty - v.my) * .12; v.ms += (v.ts - v.ms) * .08;
      let reveal = o.reveal || 0;
      if (o.intro) { const since = v.readyAt ? (now - v.readyAt) / 1000 : 0; const k = REDUCE ? 1 : Math.min(1, Math.max(0, (since - .5) / 2.8)); reveal = v.readyAt ? k * k * (3 - 2 * k) : 0; const cyc = REDUCE ? 0 : Math.max(0, Math.sin(time * .22 - 1.2)); reveal = Math.max(0, reveal - cyc * cyc * cyc * .9); }
      gl.viewport(0, 0, canvas.width, canvas.height); gl.clearColor(0, 0, 0, 0); gl.clear(gl.COLOR_BUFFER_BIT);
      gl.uniform2f(u.u_res, canvas.width, canvas.height); gl.uniform1f(u.u_time, time); gl.uniform1f(u.u_zoom, o.zoom || 1);
      gl.uniform1f(u.u_pixel, Math.max(1, Math.round((o.pixel || 3) * v.dpr))); gl.uniform1f(u.u_energy, (o.energy || .45) + v.ms * .3);
      gl.uniform2f(u.u_m, v.mx, v.my); gl.uniform1f(u.u_mstr, v.ms); gl.uniform1f(u.u_reveal, reveal);
      gl.uniform1f(u.u_iz, o.iz || 1); gl.uniform2f(u.u_ic, o.icx || .37, o.icy || .44); gl.uniform1f(u.u_mask, o.mask ?? 1); gl.uniform1f(u.u_con, o.con || 1.3);
      gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    }
    if (!REDUCE || performance.now() - t0 < 4000) requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);
  return { add };
})();

GL.add(document.getElementById('face'), { image: true, intro: true, zoom: 1, pixel: 3, energy: .5, iz: 1.25, icx: .36, icy: .44, mask: 1, host: document.getElementById('stage') });
GL.add(document.getElementById('det1'), { image: true, reveal: 1, pixel: 2, energy: .35, iz: 1.8, icx: .45, icy: .43, mask: 0, con: 1.15 });
GL.add(document.getElementById('det2'), { image: true, reveal: 1, pixel: 3, energy: .6, iz: 4.6, icx: .57, icy: .65, mask: 0, con: 1.45 });
GL.add(document.getElementById('det4'), { pixel: 2, energy: .9, zoom: .9 });
GL.add(document.getElementById('orb2'), { pixel: 3, energy: .8, zoom: 1 });
GL.add(document.getElementById('det3'), { image: true, reveal: 1, pixel: 3, energy: .6, iz: 4.6, icx: .45, icy: .45, mask: 0, con: 1.45 });

/* the real chat design, playing itself */
(() => {
  const dv = document.getElementById('dv'), ds = document.getElementById('ds'), cur = document.getElementById('dcur'), clock = document.getElementById('dclock');
  const fit = () => { const k = dv.clientWidth / 1440; ds.style.transform = `scale(${k})`; dv.style.height = (900 * k) + 'px'; };
  fit(); addEventListener('resize', fit);
  const $ = (id) => ds.querySelector(`[data-pencil-id="${id}"]`);
  const you = $('vRB6O'), ai = $('nsyPT'), pin = $('fK1ZN'), input = $('c9LbkJ'), send = $('ANdvr'), status = $('e6nwto'), foot = $('WYVR5'), book = $('CobYP');
  const rows = ['LymPV', 'l1Xym', 'OXSvI', 'EiYmX', 'H2a1hn', 'fw1Tp'].map($);
  const aiBits = ['mwxrU', 'XSnj9', 'lEaep'].map($);
  if (!you || !ai || !input) return;
  const disp = new Map(); [you, ai, pin, ...rows, ...aiBits].forEach((el) => el && disp.set(el, el.style.display || ''));
  const hide = (el) => { if (el) el.style.display = 'none'; };
  const show = (el) => { if (!el) return; el.style.display = disp.get(el); el.animate([{ opacity: 0, transform: 'translateY(10px)' }, { opacity: 1, transform: 'none' }], { duration: 480, easing: 'cubic-bezier(.2,.7,.1,1)' }); };
  const msg = input.textContent; const origIn = input.style.color;
  const setText = (el, t) => { if (el) el.textContent = t; };
  const point = async (el) => { const w = dv.getBoundingClientRect(), b = el.getBoundingClientRect(); cur.style.left = (b.left - w.left + b.width * .5) + 'px'; cur.style.top = (b.top - w.top + b.height * .55) + 'px'; cur.classList.add('show'); await sleep(850); };
  const press = async (el) => { el.animate([{ transform: 'scale(1)' }, { transform: 'scale(.92)' }, { transform: 'scale(1)' }], { duration: 260 }); await sleep(260); };

  const finalState = () => { setText(status, 'Done · Lisbon plan'); setText(foot, 'Booked · all 5 steps done'); hide(pin); };
  if (REDUCE) { finalState(); return; }

  let visible = false, running = false;
  async function play() {
    if (running) return; running = true;
    while (true) {
      hide(you); hide(ai); hide(pin); rows.forEach(hide); aiBits.forEach(hide); cur.classList.remove('show');
      setText(status, 'Here'); setText(input, msg ? 'Message anima…' : ''); input.style.color = '#ECE6DA59'; setText(foot, 'Working on it · step 2 of 5'); clock.textContent = 'Mon 09:02';
      await sleep(900);
      const text = 'Plan my Lisbon trip for May: flights, a hotel near Alfama, and tell Mimi the dates.';
      input.style.color = '#ECE6DA'; input.textContent = '';
      for (const ch of text) { input.textContent += ch; await sleep(ch === ' ' ? 40 : 22 + Math.random() * 36); }
      await sleep(300); await point(send); await press(send);
      setText(input, 'Message anima…'); input.style.color = '#ECE6DA59';
      show(you); cur.classList.remove('show');
      setText(status, 'Thinking…'); await sleep(1200);
      ai.style.display = disp.get(ai); show(aiBits[0]); show(aiBits[1]); await sleep(500); show(aiBits[2]);
      for (const r of rows.slice(0, 5)) { await sleep(240); show(r); }
      await sleep(400); show(rows[5]);
      setText(status, 'Working on the Lisbon plan · 2 of 5  ›'); clock.textContent = 'Mon 09:03';
      await sleep(2600);
      clock.textContent = 'Wed 18:12'; setText(foot, 'Still watching prices · 2d 09h'); setText(status, 'Waiting for your OK');
      show(pin); await sleep(1500);
      await point(book); await press(book); cur.classList.remove('show');
      hide(pin); setText(foot, 'Booked · all 5 steps done'); setText(status, 'Done · Lisbon plan'); clock.textContent = 'Thu 07:30';
      await sleep(4200);
      if (!visible) { running = false; return; }
    }
  }
  window.__demoPlay = () => { visible = true; play(); }; window.__demoStop = () => { visible = false; };
})();

/* See it work: real screens from design.pen, side by side on an endless loop */
(() => {
  const SLIDES = [
    { title: 'Plans, then does it', text: 'Ask once. It lays out the steps, starts working, and asks before it spends your money.', ms: 17000, script: true },
    { title: 'A swarm on one job', text: 'Big tasks get a researcher, a coder and a reviewer working in parallel. anima supervises and brings back one result.', ms: 8000 },
    { title: 'Runs itself', text: 'Heartbeats, morning briefs, reminders and reports run on their own schedule and land in your chat.', ms: 8000 },
    { title: 'Gets better as it works', text: 'When a job goes well it turns it into a skill, and improves that skill every time it’s used. You keep or drop each one.', ms: 9000 },
    { title: 'Drives your coding agents', text: 'It runs Claude Code or Codex on your machine, reads the diff, and runs the checks itself.', ms: 8000 },
    { title: 'Survives restarts', text: 'Long jobs save every step. If the daemon restarts, it picks up where it left off.', ms: 8000 },
    { title: 'Lives in your chat apps', text: 'Message it from Telegram or Discord. Groups get their own thread; your main chat stays private.', ms: 8000 },
  ];
  const view = document.getElementById('cr-view');
  const titleEl = document.getElementById('cr-title'), textEl = document.getElementById('cr-text');
  const slides = [...view.querySelectorAll('.cr-slide')], n = slides.length;
  let i = 0, timer = null, paused = false, inView = false, dx = 0, W = 0, GAP = 28;
  const prevOff = new Array(n).fill(0);

  const offOf = (k) => { let o = ((k - i) % n + n) % n; if (o > n / 2) o -= n; return o; };
  function layout(anim) {
    W = Math.min(1060, view.clientWidth * (view.clientWidth < 700 ? .9 : .72));
    const H = W * 900 / 1440;
    view.style.height = (H + 12) + 'px';
    slides.forEach((s, k) => {
      const o = offOf(k), x = o * (W + GAP) + dx;
      const wrap = Math.abs(o - prevOff[k]) > 1.5;
      s.style.transition = anim && !wrap ? 'transform .85s cubic-bezier(.7,0,.2,1), opacity .85s' : 'none';
      s.style.width = W + 'px';
      s.style.transform = `translateX(calc(-50% + ${x}px)) scale(${o === 0 ? 1 : .9})`;
      s.style.opacity = o === 0 ? 1 : Math.abs(o) === 1 ? .5 : 0;
      s.style.zIndex = String(10 - Math.abs(o));
      s.classList.toggle('active', o === 0);
      s.setAttribute('aria-hidden', o === 0 ? 'false' : 'true');
      prevOff[k] = o;
      const v = s.querySelector('.sv'), k2 = W / 1440; s.querySelector('.ss').style.transform = `scale(${k2})`; v.style.height = (900 * k2) + 'px';
    });
  }

  const reveal = (s) => {
    if (REDUCE) return;
    const parts = [...s.querySelectorAll('[data-pencil-name="Scroll"] > *, [data-pencil-name$="Sheet"] > *, [data-pencil-name$="Dialog"] > *, [data-pencil-name$="Banner"]')];
    parts.forEach((el, k) => el.animate([{ opacity: 0, transform: 'translateY(14px)' }, { opacity: 1, transform: 'none' }], { duration: 520, delay: 250 + k * 140, easing: 'cubic-bezier(.2,.7,.1,1)', fill: 'backwards' }));
  };

  function go(k) {
    i = ((k % n) + n) % n; dx = 0;
    layout(true);
    titleEl.textContent = SLIDES[i].title; textEl.textContent = SLIDES[i].text;
    if (SLIDES[i].script) window.__demoPlay && window.__demoPlay(); else { window.__demoStop && window.__demoStop(); reveal(slides[i]); }
    schedule();
  }
  function schedule() {
    clearTimeout(timer);
    if (REDUCE || paused || !inView) return;
    const ms = SLIDES[i].ms;
    timer = setTimeout(() => go(i + 1), ms);
  }
  const pause = () => { paused = true; clearTimeout(timer); };
  const resume = () => { if (!paused) return; paused = false; schedule(); };
  view.addEventListener('pointerenter', (e) => { if (e.pointerType === 'mouse') pause(); });
  view.addEventListener('pointerleave', (e) => { if (e.pointerType === 'mouse') resume(); });
  document.getElementById('cr').addEventListener('keydown', (e) => { if (e.key === 'ArrowRight') go(i + 1); if (e.key === 'ArrowLeft') go(i - 1); });
  slides.forEach((s, k) => s.addEventListener('click', () => { if (Math.abs(dxEnd) < 6 && offOf(k) !== 0) go(k); }));

  // swipe
  let sx = null, pid = null, dxEnd = 0;
  view.addEventListener('pointerdown', (e) => { sx = e.clientX; dx = 0; dxEnd = 0; pid = e.pointerId; view.classList.add('grab'); });
  view.addEventListener('pointermove', (e) => { if (sx == null || e.pointerId !== pid) return; dx = e.clientX - sx; if (Math.abs(dx) > 4) { try { view.setPointerCapture(pid); } catch {} } layout(false); });
  const up = (e) => { if (sx == null || e.pointerId !== pid) return; sx = null; view.classList.remove('grab'); dxEnd = dx; const d = dx; dx = 0; if (Math.abs(d) > W * .12) go(i + (d < 0 ? 1 : -1)); else layout(true); setTimeout(() => { dxEnd = 0; }, 0); };
  view.addEventListener('pointerup', up); view.addEventListener('pointercancel', up);

  addEventListener('resize', () => layout(false));
  new IntersectionObserver(([e]) => { const was = inView; inView = e.isIntersecting; if (inView && !was) schedule(); if (!inView) clearTimeout(timer); }, { threshold: .3 }).observe(view);
  layout(false); go(0);
})();
