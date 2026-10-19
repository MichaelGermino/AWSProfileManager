/*
 * Source of truth for the selectable app icons (Settings > Appearance > App icon).
 *
 * A plain browser script, not a module: generate.mjs evaluates it inside a hidden Electron window
 * and rasterizes the SVG strings it returns with a canvas. Nothing in the app imports it at runtime;
 * the app ships only the PNG/ICO files written to resources/app-icons/.
 *
 * Every glyph draws on a 1024 grid. `small` selects the heavier master used at 32 px and below;
 * `mono` draws the glyph alone in one color for monochrome tray icons.
 */
let uid = 0;
const rad = d => d * Math.PI / 180;
const f = n => n.toFixed(1);
const pt = (cx, cy, r, a) => [cx + r * Math.cos(rad(a)), cy + r * Math.sin(rad(a))];
function arc(cx, cy, r, a0, a1) {
  const [x0, y0] = pt(cx, cy, r, a0), [x1, y1] = pt(cx, cy, r, a1);
  return `M${f(x0)} ${f(y0)}A${r} ${r} 0 ${a1 - a0 > 180 ? 1 : 0} 1 ${f(x1)} ${f(y1)}`;
}
function head(cx, cy, r, a, sw, col) {
  const [ex, ey] = pt(cx, cy, r, a);
  const d = [-Math.sin(rad(a)), Math.cos(rad(a))], n = [Math.cos(rad(a)), Math.sin(rad(a))];
  const hw = sw * 1.05, bx = ex - d[0] * 6, by = ey - d[1] * 6;
  const p = [[ex + d[0] * hw, ey + d[1] * hw], [bx + n[0] * hw, by + n[1] * hw], [bx - n[0] * hw, by - n[1] * hw]];
  return `<polygon points="${p.map(q => q.map(f).join(',')).join(' ')}" fill="${col}" stroke="${col}" stroke-width="12" stroke-linejoin="round"/>`;
}
const lg = (id, stops, x2 = 1, y2 = 1) => `<linearGradient id="${id}" x1="0" y1="0" x2="${x2}" y2="${y2}">${stops.map((s, i) => `<stop offset="${stops.length === 1 ? 0 : i / (stops.length - 1)}" stop-color="${s}"/>`).join('')}</linearGradient>`;

/* ---------- Designs. Each glyph takes { id, small, mono, col } and draws on a 1024 grid. ---------- */
const D = [];

D.push({ k: 'aurora', name: 'Aurora Key', style: 'Gradient line', stops: ['#00c9a7', '#4b5cf0', '#d84fb0'], accents: ['#ffffff'],
  idea: 'A key whose bow is a refresh arrow, on a northern-lights gradient. Says "keeps your keys fresh" in one shape.',
  glyph({ small, mono, col }) {
    const c = mono ? col : '#fff', sw = small ? 100 : 76, r = 150, cx = 400, cy = 420, a0 = -40, a1 = small ? 250 : 262;
    const [sx, sy] = pt(cx, cy, r, a0);
    const teeth = small ? 'M716 736 L650 802' : 'M652 672 L592 732M736 756 L676 816';
    return `<g stroke="${c}" stroke-width="${sw}" stroke-linecap="round" fill="none">
      <path d="${arc(cx, cy, r, a0, a1)}" stroke-linecap="butt"/><circle cx="${f(sx)}" cy="${f(sy)}" r="${sw / 2}" fill="${c}" stroke="none"/>
      <path d="M512 532 L790 810${teeth}"/></g>${head(cx, cy, r, a1, sw, c)}`;
  } });

D.push({ k: 'ring', name: 'Session Ring', style: 'Dial', stops: ['#1d2c5c', '#0b1330'], accents: ['#ffc94d', '#f4f6ff'],
  idea: 'A keyhole inside a countdown ring. The ring is the session lifetime, so the tray icon can show real time left.',
  glyph({ id, small, mono, col }) {
    const r = small ? 300 : 272, sw = small ? 100 : 62, end = 180;
    const [ex, ey] = pt(512, 512, r, end);
    const ringCol = mono ? col : `url(#ar${id})`;
    const k = small ? 1.2 : 1, cy = small ? 500 : 512;
    const kh = `M${512 - 50 * k} ${cy - 50 * k + 72 * k}A${88 * k} ${88 * k} 0 1 1 ${512 + 50 * k} ${cy - 50 * k + 72 * k}L${512 + 70 * k} ${cy + 148 * k}L${512 - 70 * k} ${cy + 148 * k}Z`;
    return `${mono ? '' : `<defs>${lg('ar' + id, ['#ffe485', '#ff9d1c'])}</defs><circle cx="512" cy="512" r="${r}" fill="none" stroke="#fff" stroke-opacity=".13" stroke-width="${sw}"/>`}
      <path d="M512 ${512 - r}${arc(512, 512, r, -90, end).replace(/^M[^A]+/, '')}" fill="none" stroke="${ringCol}" stroke-width="${sw}" stroke-linecap="round"/>
      <path d="${kh}" fill="${mono ? col : '#f4f6ff'}" stroke="${mono ? col : '#f4f6ff'}" stroke-width="18" stroke-linejoin="round"/>`;
  } });

D.push({ k: 'stack', name: 'Profile Stack', style: 'Flat layered', stops: ['#7a1f5c', '#33103d'], accents: ['#ff7a5c', '#fff4e8', '#ffcf3f'],
  idea: 'Three identity cards with the active one in front, in plum and coral. Many profiles, one in use.',
  glyph({ id, small, mono, col }) {
    const a = small ? { cx: 360, cy: 606, r: 74 } : { cx: 340, cy: 600, r: 58 };
    const person = fill => `<circle cx="${a.cx}" cy="${a.cy - 16}" r="${a.r * .55}" fill="${fill}"/><path d="M${a.cx - a.r * .95} ${a.cy + a.r * 1.05}a${a.r * .95} ${a.r * .8} 0 0 1 ${a.r * 1.9} 0Z" fill="${fill}"/>`;
    const lines = fill => small ? '' : `<rect x="430" y="566" width="230" height="30" rx="15" fill="${fill}"/><rect x="430" y="616" width="150" height="30" rx="15" fill="${fill}"/>`;
    if (mono) return `<defs><mask id="mk${id}"><rect width="1024" height="1024" fill="#fff"/>${person('#000')}${lines('#000')}</mask></defs>
      <rect x="332" y="286" width="460" height="260" rx="56" fill="${col}" fill-opacity=".35"/><rect x="282" y="366" width="510" height="280" rx="60" fill="${col}" fill-opacity=".62"/>
      <rect x="232" y="446" width="560" height="300" rx="64" fill="${col}" mask="url(#mk${id})"/>`;
    return `<rect x="332" y="286" width="460" height="260" rx="56" fill="#c2366f"/><rect x="282" y="366" width="510" height="280" rx="60" fill="#ff7a5c"/>
      <rect x="232" y="446" width="560" height="300" rx="64" fill="#fff4e8"/>${person('#33103d')}
      ${small ? '' : `<rect x="430" y="566" width="230" height="30" rx="15" fill="#ffc9b5"/><rect x="430" y="616" width="150" height="30" rx="15" fill="#ffe3d6"/><circle cx="700" cy="494" r="24" fill="#ffcf3f"/>`}`;
  } });

D.push({ k: 'keyring', name: 'Keyring', style: 'Flat illustration', stops: ['#11545a', '#082a2e'], accents: ['#ffd166', '#ef476f', '#06d6a0'],
  idea: 'Three colored keys on one ring: one per account. Playful, and the colors hint at the multi-account use case.',
  glyph({ id, small, mono, col }) {
    const key = (fill, ang) => `<g transform="rotate(${ang} 512 318) translate(512 486)">
      <path d="M-80 0a80 80 0 1 0 160 0a80 80 0 1 0-160 0Z${small ? '' : 'M-30 0a30 30 0 1 1 60 0a30 30 0 1 1-60 0Z'}" fill="${fill}" fill-rule="evenodd"/>
      <rect x="-24" y="60" width="48" height="280" rx="24" fill="${fill}"/>
      ${small ? '' : `<rect x="8" y="250" width="72" height="34" rx="12" fill="${fill}"/><rect x="8" y="304" width="58" height="34" rx="12" fill="${fill}"/>`}</g>`;
    const ring = `<circle cx="512" cy="318" r="104" fill="none" stroke="${mono ? col : '#dfe7ea'}" stroke-width="${small ? 46 : 34}"/>`;
    if (mono) return ring + key(col, 0);
    return key('#ef476f', 32) + key('#06d6a0', -32) + ring + key('#ffd166', 0);
  } });

D.push({ k: 'cube', name: 'Vault Cube', style: 'Isometric neon', stops: ['#1a1a33', '#07070f'], accents: ['#7cf7ff', '#8a5cff', '#ff4fd8'],
  idea: 'A neon isometric cube with a keyhole: the cloud resource you are unlocking. The most "infrastructure" of the set.',
  glyph({ small, mono, col }) {
    const T = [512, 262], UR = [729, 387], LR = [729, 637], B = [512, 762], LL = [295, 637], UL = [295, 387], C = [512, 512];
    const poly = (pts, fill, op = 1) => `<polygon points="${pts.map(p => p.join(',')).join(' ')}" fill="${fill}" fill-opacity="${op}" stroke="${fill}" stroke-opacity="${op}" stroke-width="10" stroke-linejoin="round"/>`;
    const k = small ? 1.5 : 1;
    const hole = mono ? '' : `<g transform="translate(403.5 574.5) matrix(0.868 0.5 0 1 0 0) scale(${k})"><circle cx="0" cy="-22" r="34" fill="#07070f"/><path d="M-18 0L18 0L28 62L-28 62Z" fill="#07070f"/></g>`;
    return mono
      ? poly([T, UR, C, UL], col) + poly([UL, C, B, LL], col, .5) + poly([C, UR, LR, B], col, .78)
      : poly([T, UR, C, UL], '#7cf7ff') + poly([UL, C, B, LL], '#8a5cff') + poly([C, UR, LR, B], '#ff4fd8') + hole;
  } });

D.push({ k: 'shell', name: 'Shell Prompt', style: 'Terminal glow', stops: ['#1f262e', '#0e1216'], accents: ['#3dff8f'],
  idea: 'A phosphor-green prompt whose cursor is a key blade. For people who live in the embedded terminal.',
  extra({ id, small }) { return small ? '' : `<defs><pattern id="sl${id}" width="24" height="24" patternUnits="userSpaceOnUse"><rect width="24" height="10" fill="#fff" fill-opacity=".035"/></pattern></defs><rect x="56" y="56" width="912" height="912" fill="url(#sl${id})"/>`; },
  glyph({ id, small, mono, col }) {
    const c = mono ? col : '#3dff8f', sw = small ? 116 : 86;
    const glow = mono || small ? '' : `<defs><filter id="gl${id}" x="-30%" y="-30%" width="160%" height="160%"><feGaussianBlur stdDeviation="18" result="b"/><feMerge><feMergeNode in="b"/><feMergeNode in="SourceGraphic"/></feMerge></filter></defs>`;
    const teeth = small ? '' : 'M690 640 V716M754 640 V700';
    return `${glow}<g transform="translate(-14 -42)" ${glow ? `filter="url(#gl${id})"` : ''} stroke="${c}" stroke-linecap="round" stroke-linejoin="round" fill="none">
      <path d="M296 372 L470 512 L296 652" stroke-width="${sw}"/><path d="M548 640 H770" stroke-width="${sw}"/><path d="${teeth}" stroke-width="${sw * .72}"/></g>`;
  } });

D.push({ k: 'orbit', name: 'Orbit', style: 'Space', stops: ['#2a0f5a', '#0a0a2e'], accents: ['#ff8a6b', '#6ee7ff', '#b8ff6e'],
  idea: 'A warm core with profiles in orbit around it. Abstract and friendly; reads as "one hub, many accounts".',
  extra({ small }) { return small ? '' : [[250, 260, 6], [760, 230, 8], [820, 700, 5], [230, 760, 7], [640, 830, 5], [380, 180, 4]].map(([x, y, r]) => `<circle cx="${x}" cy="${y}" r="${r}" fill="#fff" fill-opacity=".55"/>`).join(''); },
  glyph({ id, small, mono, col }) {
    const rx = small ? 330 : 318, ry = small ? 132 : 118, sw = small ? 46 : 20, pr = small ? 150 : 122;
    const dot = (t, r, fill) => `<circle cx="${f(512 + rx * Math.cos(rad(t)))}" cy="${f(512 + ry * Math.sin(rad(t)))}" r="${r}" fill="${fill}"/>`;
    const ringCol = mono ? col : 'rgba(255,255,255,.6)';
    const planet = mono ? col : `url(#pg${id})`;
    return `${mono ? '' : `<defs>${lg('pg' + id, ['#ffd36b', '#ff5e7e'])}</defs>`}<g transform="rotate(-24 512 512)">
      <ellipse cx="512" cy="512" rx="${rx}" ry="${ry}" fill="none" stroke="${ringCol}" stroke-width="${sw}"/>
      <circle cx="512" cy="512" r="${pr}" fill="${planet}"/>
      ${mono ? '' : `<path d="M${512 - rx} 512A${rx} ${ry} 0 0 0 ${512 + rx} 512" fill="none" stroke="${ringCol}" stroke-width="${sw}"/>`}
      ${small ? dot(30, 70, mono ? col : '#6ee7ff') : dot(205, 40, mono ? col : '#6ee7ff') + dot(35, 48, mono ? col : '#b8ff6e')}</g>`;
  } });

D.push({ k: 'print', name: 'Identity Print', style: 'Holographic', stops: ['#c8f4ff', '#ecd2ff', '#fff1c2'], accents: ['#2b1d6b'],
  idea: 'A fingerprint drawn in ink on a pearly holographic plate. Identity first; the only light icon in the set.',
  glyph({ small, mono, col }) {
    const c = mono ? col : '#2b1d6b';
    const arcs = small ? [[80, 125, 415], [190, 150, 390], [300, 180, 360]] : [[70, 120, 420], [140, 140, 400], [210, 160, 380], [280, 180, 360]];
    return `<g fill="none" stroke="${c}" stroke-width="${small ? 72 : 46}" stroke-linecap="round">${arcs.map(([r, a, b]) => `<path d="${arc(512, small ? 610 : 600, r, a, b)}"/>`).join('')}</g>`;
  } });

D.push({ k: 'mark', name: 'Key Monogram', style: 'Bauhaus', stops: ['#ffd400', '#ffd400'], accents: ['#111111', '#ff3b30'],
  idea: 'A Bauhaus "P" whose stem is a key blade, in flat yellow, black and red. Loud, flat and graphic.',
  glyph({ small, mono, col }) {
    const c = mono ? col : '#111', sw = small ? 124 : 96;
    return `<g stroke="${c}" stroke-width="${sw}" stroke-linecap="round" stroke-linejoin="round" fill="none">
      <path d="M370 300 V740M370 300 H500 A135 135 0 0 1 500 570 H370"/>
      ${small ? '' : `<path d="M410 646 H470M410 724 H448" stroke-width="${sw * .72}"/>`}</g>
      <circle cx="${small ? 690 : 680}" cy="${small ? 704 : 700}" r="${small ? 92 : 72}" fill="${mono ? col : '#ff3b30'}"/>`;
  } });

D.push({ k: 'cloud', name: 'Cloud Lock', style: 'Soft gradient', stops: ['#ffa48f', '#ff5f8f'], accents: ['#ffffff'],
  idea: 'A soft white cloud with a padlock, on a sunset gradient. The most approachable, least technical option.',
  glyph({ id, small, mono, col }) {
    const s = small ? 1.45 : 1;
    const lock = fill => `<g transform="translate(512 640) scale(${s}) translate(-512 -640)"><path d="M480 600 V572 a32 32 0 0 1 64 0 V600" fill="none" stroke="${fill}" stroke-width="24"/><rect x="460" y="592" width="104" height="84" rx="18" fill="${fill}"/></g>`;
    const cloud = fill => `<circle cx="395" cy="575" r="115" fill="${fill}"/><circle cx="535" cy="500" r="155" fill="${fill}"/><circle cx="662" cy="590" r="105" fill="${fill}"/><rect x="280" y="575" width="487" height="120" rx="60" fill="${fill}"/>`;
    if (mono) return `<defs><mask id="cm${id}"><rect width="1024" height="1024" fill="#fff"/>${lock('#000')}</mask></defs><g mask="url(#cm${id})">${cloud(col)}</g>`;
    return cloud('#fff') + lock('#ff6a8a');
  } });

const PIX = ['.XXXX.........', 'XX..XX........', 'X....XXXXXXXXX', 'X....XXXXXXXXX', 'XX..XX....X.XX', '.XXXX.....X.XX'];
D.push({ k: 'pixel', name: 'Pixel Key', style: '8-bit', stops: ['#4a2bb0', '#1c0f52'], accents: ['#ffe066', '#f5a300'],
  idea: 'An 8-bit key drawn on a true pixel grid, so at 16 px every pixel lands exactly. Retro and very legible.',
  extra({ small }) { return small ? '' : [[200, 210], [790, 260], [250, 800], [820, 760], [700, 180]].map(([x, y]) => `<rect x="${x}" y="${y}" width="26" height="26" fill="#fff" fill-opacity=".5"/>`).join(''); },
  glyph({ small, mono, col }) {
    const cell = small ? 64 : 52, ox = (1024 - 14 * cell) / 2, oy = 512 - 3 * cell;
    let out = '';
    PIX.forEach((row, y) => [...row].forEach((ch, x) => {
      if (ch !== 'X') return;
      const fill = mono ? col : (y <= 2 ? '#ffe066' : '#f5a300');
      out += `<rect x="${ox + x * cell}" y="${oy + y * cell}" width="${cell + .5}" height="${cell + .5}" fill="${fill}"/>`;
    }));
    return `<g shape-rendering="crispEdges">${out}</g>`;
  } });

D.push({ k: 'cycle', name: 'Profile Cycle', style: 'Bold line', stops: ['#0f4a35', '#052419'], accents: ['#c6ff3d'],
  idea: 'A person inside two turning arrows, in acid lime on deep forest. "Your identity, kept current."',
  glyph({ small, mono, col }) {
    const c = mono ? col : '#c6ff3d', sw = small ? 84 : 54, r = small ? 300 : 286;
    const hr = small ? 96 : 78, sh = small ? 150 : 122;
    return `<circle cx="512" cy="${small ? 445 : 456}" r="${hr}" fill="${c}"/>
      <path d="M${512 - sh} ${small ? 670 : 652}a${sh} ${sh * .86} 0 0 1 ${sh * 2} 0Z" fill="${c}"/>
      <g fill="none" stroke="${c}" stroke-width="${sw}" stroke-linecap="round"><path d="${arc(512, 512, r, 200, 318)}"/><path d="${arc(512, 512, r, 20, 138)}"/></g>
      ${head(512, 512, r, 320, sw, c)}${head(512, 512, r, 140, sw, c)}`;
  } });

const blurF = (id, sd) => `<filter id="bl${id}" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="${sd}"/></filter>`;
const glowF = (id, sd) => `<filter id="gw${id}" x="-40%" y="-40%" width="180%" height="180%"><feGaussianBlur stdDeviation="${sd}" result="b"/><feMerge><feMergeNode in="b"/><feMergeNode in="b"/><feMergeNode in="SourceGraphic"/></feMerge></filter>`;
const rg = (id, a, b, cx = .35, cy = .3) => `<radialGradient id="${id}" cx="${cx}" cy="${cy}" r=".85"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></radialGradient>`;
const CLOUD = fill => `<circle cx="395" cy="585" r="115" fill="${fill}"/><circle cx="535" cy="510" r="155" fill="${fill}"/><circle cx="662" cy="600" r="105" fill="${fill}"/><rect x="280" y="585" width="487" height="120" rx="60" fill="${fill}"/>`;
const CLOUD_S = (stroke, w) => `<g fill="none" stroke="${stroke}" stroke-width="${w}" stroke-linejoin="round"><circle cx="395" cy="585" r="115"/><circle cx="535" cy="510" r="155"/><circle cx="662" cy="600" r="105"/><rect x="280" y="585" width="487" height="120" rx="60"/></g>`;

D.push({ k: 'clay', name: 'Clay Key', style: 'Claymorphism', stops: ['#c8f5e4', '#a9c4ff'], accents: ['#ff8fbd', '#e2457f'],
  idea: 'A puffy bubblegum key that looks squeezed out of modelling clay. Soft, tactile and impossible to mistake.',
  glyph({ id, small, mono, col }) {
    const fill = mono ? col : `url(#cg${id})`;
    const hole = small ? '' : 'M270 512a58 58 0 1 1 116 0a58 58 0 1 1-116 0Z';
    const shape = `<path d="M178 512a150 150 0 1 1 300 0a150 150 0 1 1-300 0Z${hole}" fill-rule="evenodd" fill="${fill}"/>
      <rect x="440" y="470" width="350" height="84" rx="42" fill="${fill}"/>
      <rect x="650" y="510" width="58" height="${small ? 120 : 108}" rx="29" fill="${fill}"/>${small ? '' : `<rect x="728" y="510" width="58" height="80" rx="29" fill="${fill}"/>`}`;
    if (mono) return `<g transform="rotate(-38 512 512) translate(10 0)">${shape}</g>`;
    return `<defs>${rg('cg' + id, '#ffc2dc', '#e2457f')}${blurF(id, 26)}</defs>
      <g transform="rotate(-38 512 512) translate(10 0)">
        <g transform="translate(18 34)" filter="url(#bl${id})" opacity=".35"><path d="M178 512a150 150 0 1 1 300 0a150 150 0 1 1-300 0Z" fill="#7a2a55"/><rect x="440" y="470" width="350" height="84" rx="42" fill="#7a2a55"/></g>
        ${shape}
        ${small ? '' : `<ellipse cx="290" cy="420" rx="62" ry="30" fill="#fff" opacity=".55" transform="rotate(-30 290 420)"/><rect x="480" y="482" width="220" height="18" rx="9" fill="#fff" opacity=".4"/>`}
      </g>`;
  } });

D.push({ k: 'neon', name: 'Neon Sign', style: 'Neon sign', stops: ['#2a1420', '#0f070d'], accents: ['#ff4fd8', '#4ff0ff'],
  idea: 'A glowing pink cloud with a cyan key, like a sign hung on a brick wall in a late-night bar.',
  extra({ id, small }) { return small ? '' : `<defs><pattern id="bk${id}" width="120" height="60" patternUnits="userSpaceOnUse"><path d="M0 0H120M0 30H120M60 0V30M0 30V60M120 30V60" stroke="#fff" stroke-opacity=".05" stroke-width="4" fill="none"/></pattern></defs><rect width="1024" height="1024" fill="url(#bk${id})"/>`; },
  glyph({ id, small, mono, col }) {
    const pink = mono ? col : '#ff6fe0', cyan = mono ? col : '#6ff5ff', cw = small ? 62 : 30, kw = small ? 50 : 26;
    const cloud = 'M330 680A100 100 0 0 1 345 470A150 150 0 0 1 625 440A115 115 0 0 1 700 680Z';
    const key = small ? '<circle cx="440" cy="590" r="52"/><path d="M492 590H640M610 590V634"/>' : '<circle cx="430" cy="590" r="45"/><path d="M475 590H630M590 590V628M630 590V618"/>';
    return `${mono ? '' : `<defs>${glowF(id, small ? 8 : 16)}</defs>`}<g fill="none" stroke-linecap="round" stroke-linejoin="round" ${mono ? '' : `filter="url(#gw${id})"`}>
      <path d="${cloud}" stroke="${pink}" stroke-width="${cw}"/><g stroke="${cyan}" stroke-width="${kw}">${key}</g></g>`;
  } });

D.push({ k: 'synth', name: 'Synthwave', style: 'Synthwave', stops: ['#160a33', '#3d0f5c', '#a3226e'], accents: ['#ffe259', '#ff4f9a', '#ff4fd8'],
  idea: 'An 80s sunset with a keyhole punched through the sun, over a neon grid floor. Pure Miami at midnight.',
  extra({ small }) {
    if (small) return '';
    const v = [-700, -350, -120, 120, 350, 700].map(dx => `<path d="M512 640L${512 + dx * 2} 1024"/>`).join('');
    return `<g stroke="#ff4fd8" stroke-opacity=".75" stroke-width="6" fill="none"><path d="M0 640H1024M0 676H1024M0 730H1024M0 810H1024M0 920H1024"/>${v}</g>`;
  },
  glyph({ id, small, mono, col }) {
    const cy = small ? 512 : 430, r = small ? 330 : 220, k = small ? 1.3 : 1;
    const bars = small ? [[cy + 110, 30], [cy + 200, 44]] : [[cy + 40, 12], [cy + 90, 18], [cy + 140, 24], [cy + 185, 30]];
    const kh = `<g transform="translate(512 ${cy - 30 * k}) scale(${k})"><circle cx="0" cy="-26" r="52" fill="#000"/><path d="M-30 0H30L46 110H-46Z" fill="#000"/></g>`;
    return `<defs>${lg('sn' + id, ['#ffe259', '#ff7a5c', '#ff4f9a'], 0, 1)}<mask id="sm${id}"><rect width="1024" height="1024" fill="#fff"/>${kh}${bars.map(([y, h]) => `<rect x="0" y="${y}" width="1024" height="${h}" fill="#000"/>`).join('')}${small ? '' : '<rect x="0" y="640" width="1024" height="400" fill="#000"/>'}</mask></defs>
      <circle cx="512" cy="${cy}" r="${r}" fill="${mono ? col : `url(#sn${id})`}" mask="url(#sm${id})"/>`;
  } });

D.push({ k: 'gem', name: 'Crystal', style: 'Low-poly', stops: ['#3a1d6e', '#140a2e'], accents: ['#a6fff2', '#38b6c9', '#126a8a'],
  idea: 'A faceted aqua gem: your credentials are the valuables. Low-poly facets catch the light at every size.',
  extra({ small }) { return small ? '' : [[230, 250, 30], [800, 300, 22], [780, 780, 26]].map(([x, y, s]) => `<path d="M${x} ${y - s}Q${x} ${y} ${x + s} ${y}Q${x} ${y} ${x} ${y + s}Q${x} ${y} ${x - s} ${y}Q${x} ${y} ${x} ${y - s}Z" fill="#fff" opacity=".8"/>`).join(''); },
  glyph({ mono, col }) {
    const a = [340, 360], b = [512, 360], c = [684, 360], d = [250, 460], e = [426, 460], g = [598, 460], h = [774, 460], t = [512, 780];
    const faces = [[[a, d, e], '#7ff0e8', .9], [[a, e, b], '#e9fffd', 1], [[b, e, g], '#a6fff2', .8], [[b, g, c], '#5ce1e6', .9], [[c, g, h], '#38b6c9', .7],
      [[d, e, t], '#2fb5c9', .6], [[e, g, t], '#1f8fb0', .45], [[g, h, t], '#126a8a', .3]];
    return `<g transform="translate(0 -58)">${faces.map(([p, fc, op]) => `<polygon points="${p.map(q => q.join(',')).join(' ')}" fill="${mono ? col : fc}" fill-opacity="${mono ? op : 1}" stroke="${mono ? col : fc}" stroke-opacity="${mono ? op : 1}" stroke-width="4" stroke-linejoin="round"/>`).join('')}</g>`;
  } });

D.push({ k: 'buddy', name: 'Cloud Buddy', style: 'Kawaii sticker', stops: ['#8bdcff', '#3b82f6'], accents: ['#ffffff', '#1e293b', '#ff9bb3'],
  idea: 'A happy little cloud with blushing cheeks, cut out like a sticker. The friendliest face in the system tray.',
  glyph({ id, small, mono, col }) {
    const ink = '#1e293b', ey = small ? 600 : 595, er = small ? 1.4 : 1;
    const face = fill => `<ellipse cx="455" cy="${ey}" rx="${18 * er}" ry="${26 * er}" fill="${fill}"/><ellipse cx="595" cy="${ey}" rx="${18 * er}" ry="${26 * er}" fill="${fill}"/>${small ? '' : `<path d="M495 640q30 28 60 0" fill="none" stroke="${fill}" stroke-width="14" stroke-linecap="round"/>`}`;
    if (mono) return `<defs><mask id="bm${id}"><rect width="1024" height="1024" fill="#fff"/>${face('#000')}</mask></defs><g mask="url(#bm${id})">${CLOUD(col)}</g>`;
    return `${small ? '' : CLOUD_S('#fff', 110)}${CLOUD_S(ink, small ? 70 : 52)}${CLOUD('#fff')}${face(ink)}
      ${small ? '' : '<ellipse cx="408" cy="645" rx="30" ry="17" fill="#ff9bb3" opacity=".85"/><ellipse cx="642" cy="645" rx="30" ry="17" fill="#ff9bb3" opacity=".85"/><ellipse cx="470" cy="420" rx="40" ry="20" fill="#e0f2fe"/>'}`;
  } });

D.push({ k: 'glass', name: 'Frosted Glass', style: 'Glassmorphism', stops: ['#1e1b4b', '#0b1023'], accents: ['#ff3d81', '#00e0ff', '#ffd23f'],
  idea: 'A frosted glass tile floating over glowing color blobs, with a crisp white key on top. Very Windows 11, very macOS.',
  extra({ id, small }) { return `<defs>${blurF('x' + id, small ? 50 : 70)}</defs><g filter="url(#blx${id})"><circle cx="350" cy="370" r="210" fill="#ff3d81"/><circle cx="690" cy="660" r="230" fill="#00e0ff"/><circle cx="700" cy="320" r="150" fill="#ffd23f"/></g>`; },
  glyph({ small, mono, col }) {
    const c = mono ? col : '#fff', sw = small ? 70 : 46;
    const key = `<g stroke="${c}" stroke-width="${sw}" stroke-linecap="round" fill="none"><circle cx="440" cy="440" r="${small ? 82 : 76}"/><path d="M496 496L640 640M598 598L548 648"/></g>`;
    if (mono) return `<rect x="262" y="262" width="500" height="500" rx="110" fill="none" stroke="${col}" stroke-width="30"/>${key}`;
    return `<rect x="262" y="262" width="500" height="500" rx="110" fill="#fff" fill-opacity=".2" stroke="#fff" stroke-opacity=".6" stroke-width="6"/>
      <rect x="262" y="262" width="500" height="250" rx="110" fill="#fff" fill-opacity=".08"/>${key}`;
  } });

D.push({ k: 'passport', name: 'Passport', style: 'Skeuomorphic', stops: ['#efe7d4', '#d4c6a6'], accents: ['#14365f', '#e8c46a'],
  idea: 'A navy passport with a gold-foil globe, tilted on a desk. Your identity, stamped for every account you visit.',
  glyph({ id, small, mono, col }) {
    const gr = small ? 128 : 100, gy = small ? 490 : 450;
    const globe = s => `<g fill="none" stroke="${s}" stroke-width="${small ? 30 : 18}"><circle cx="532" cy="${gy}" r="${gr}"/><ellipse cx="532" cy="${gy}" rx="${gr * .45}" ry="${gr}"/><path d="M${532 - gr} ${gy}H${532 + gr}"/></g>`;
    const bars = s => small ? '' : `<rect x="452" y="610" width="160" height="18" rx="9" fill="${s}"/><rect x="482" y="650" width="100" height="14" rx="7" fill="${s}"/>`;
    const book = fill => `<rect x="312" y="230" width="420" height="560" rx="34" fill="${fill}"/>`;
    if (mono) return `<defs><mask id="pm${id}"><rect width="1024" height="1024" fill="#fff"/>${globe('#000')}${bars('#000')}</mask></defs><g transform="rotate(-8 512 512)" mask="url(#pm${id})">${book(col)}</g>`;
    return `<defs>${blurF(id, 18)}${lg('gf' + id, ['#fbe7a1', '#d4a640'])}</defs><g transform="rotate(-8 512 512)">
      ${small ? '' : `<rect x="326" y="258" width="420" height="560" rx="34" fill="#5b4a2a" opacity=".35" filter="url(#bl${id})"/>`}
      ${book('#14365f')}<rect x="312" y="230" width="44" height="560" rx="20" fill="#0d2645"/>
      ${globe(`url(#gf${id})`)}${bars(`url(#gf${id})`)}</g>`;
  } });

D.push({ k: 'rocket', name: 'Launchpad', style: 'Retro flat', stops: ['#1e3a8a', '#0b1023'], accents: ['#f1f5f9', '#ef4444', '#ffb020'],
  idea: 'A little rocket with a keyhole porthole, blasting off. Sign in and go, in one tap.',
  extra({ small }) { return small ? '' : [[220, 300, 7], [300, 760, 5], [760, 220, 6], [820, 520, 4], [180, 560, 4], [640, 860, 5]].map(([x, y, r]) => `<circle cx="${x}" cy="${y}" r="${r}" fill="#fff" fill-opacity=".7"/>`).join(''); },
  glyph({ id, small, mono, col }) {
    const body = 'M512 200C606 282 628 420 606 600L418 600C396 420 418 282 512 200Z';
    const fins = 'M424 500L338 650L430 616ZM600 500L686 650L594 616Z';
    const pr = small ? 66 : 54;
    if (mono) return `<defs><mask id="rm${id}"><rect width="1024" height="1024" fill="#fff"/><circle cx="512" cy="410" r="${pr}" fill="#000"/></mask></defs>
      <g transform="rotate(45 512 512) translate(0 -20)"><path d="${body}" fill="${col}" mask="url(#rm${id})"/><path d="${fins}" fill="${col}"/><path d="M468 630Q512 ${small ? 760 : 800} 556 630Z" fill="${col}" fill-opacity=".6"/></g>`;
    return `<defs>${lg('fl' + id, ['#fff3a0', '#ff8a1f', '#ef4444'], 0, 1)}</defs><g transform="rotate(45 512 512) translate(0 -20)">
      <path d="M462 616Q512 ${small ? 790 : 830} 562 616Z" fill="url(#fl${id})"/>
      <path d="${fins}" fill="#ef4444"/><path d="${body}" fill="#f1f5f9"/>
      <path d="M512 200C606 282 628 420 606 600L512 600Z" fill="#cbd5e1" opacity=".55"/>
      <rect x="452" y="596" width="120" height="30" rx="10" fill="#64748b"/>
      <circle cx="512" cy="410" r="${pr}" fill="#1e3a8a" stroke="#94a3b8" stroke-width="${small ? 22 : 16}"/>
      ${small ? '' : '<circle cx="512" cy="398" r="16" fill="#ffd23f"/><path d="M502 404H522L528 440H496Z" fill="#ffd23f"/>'}</g>`;
  } });


function plate(d, variant, id) {
  const g = variant === 'mac' ? { x: 100, w: 824, r: 185 } : variant === 'win' ? { x: 56, w: 912, r: 200 } : { x: 0, w: 1024, r: 230 };
  return `<defs>${lg('pl' + id, d.stops)}${lg('hl' + id, ['rgba(255,255,255,.24)', 'rgba(255,255,255,0)'], 0, .55)}
      <filter id="sh${id}" x="-20%" y="-20%" width="140%" height="140%"><feDropShadow dx="0" dy="14" stdDeviation="16" flood-color="#000" flood-opacity=".3"/></filter>
      <clipPath id="cp${id}"><rect x="${g.x}" y="${g.x}" width="${g.w}" height="${g.w}" rx="${g.r}"/></clipPath></defs>
    <rect x="${g.x}" y="${g.x}" width="${g.w}" height="${g.w}" rx="${g.r}" fill="url(#pl${id})" ${variant === 'mac' ? `filter="url(#sh${id})"` : ''}/>
    <g clip-path="url(#cp${id})">${d.extra ? d.extra({ id, small: variant === 'small' }) : ''}</g>
    <rect x="${g.x}" y="${g.x}" width="${g.w}" height="${g.w}" rx="${g.r}" fill="url(#hl${id})"/>
    ${variant === 'small' ? '' : `<rect x="${g.x + 3}" y="${g.x + 3}" width="${g.w - 6}" height="${g.w - 6}" rx="${g.r - 3}" fill="none" stroke="#fff" stroke-opacity=".14" stroke-width="6"/>`}`;
}
// variant: mac (Big Sur squircle grid) | win (near full-bleed) | small (full-bleed, heavy) | mono (glyph only)
function icon(d, variant, size, o = {}) {
  const id = uid++;
  const open = (vb) => `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="${vb}">`;
  if (variant === 'mono') {
    return `${open('150 150 724 724')}${d.glyph({ id, small: true, mono: true, col: o.color || '#000' })}</svg>`;
  }
  return `${open('0 0 1024 1024')}${plate(d, variant, id)}${d.glyph({ id, small: variant === 'small' })}</svg>`;
}

globalThis.APP_ICON_DESIGNS = D.map((d) => ({ key: d.k, name: d.name, style: d.style }));
globalThis.renderAppIconSvg = (key, variant, size, color) => icon(D.find((d) => d.k === key), variant, size, { color });
