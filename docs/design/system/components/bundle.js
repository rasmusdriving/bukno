/* @ds-bundle: {"format":4,"namespace":"Bukno","components":[{"name":"Icon"},{"name":"ProviderMark"},{"name":"StatusGlyph"},{"name":"StateChip"},{"name":"Button"},{"name":"IconButton"},{"name":"Kbd"},{"name":"Switch"},{"name":"SectionLabel"},{"name":"ChatRow"},{"name":"ProjectRow"},{"name":"UsageMeter"},{"name":"ProfileRow"},{"name":"UserMessage"},{"name":"AgentTurn"},{"name":"PlanStep"},{"name":"ActivityLine"},{"name":"CodeBlock"},{"name":"Notice"},{"name":"TaskBrief"},{"name":"EventLine"},{"name":"Breadcrumb"},{"name":"Composer"},{"name":"PermissionControl"},{"name":"ModelControl"},{"name":"ModelPicker"},{"name":"EffortSlider"},{"name":"ChangeStrip"},{"name":"ApprovalCard"},{"name":"ThinkingOrb"},{"name":"BuildGrid"},{"name":"StreakLabel"},{"name":"WorkingIndicator"},{"name":"TaskRow"},{"name":"TodoList"},{"name":"Menu"},{"name":"EngineCard"}]} */
(function () {
  var React = window.React;
  var h = React.createElement;

  function cx() {
    var out = [];
    for (var i = 0; i < arguments.length; i++) { if (arguments[i]) out.push(arguments[i]); }
    return out.join(' ');
  }
  function on(v) { return v === true || v === 'true' || v === 1 || v === '1'; }
  function list(v) { return Array.isArray(v) ? v : []; }
  function num(v, d) { var n = Number(v); return isNaN(n) ? d : n; }

  var PROVIDERS = { codex: 'Codex', claude: 'Claude' };
  function providerName(p) { return PROVIDERS[p] || 'Codex'; }
  function providerVar(p) { return p === 'claude' ? 'var(--claude)' : 'var(--codex)'; }
  function rampVar(p, step) { return 'var(--' + (p === 'claude' ? 'claude' : 'codex') + '-effort-' + step + ')'; }
  function rampStep(index, count) { if (count <= 1) return 3; return Math.round(1 + (index * 4) / (count - 1)); }

  /* ---------- Icon ---------- */
  var ICONS = {
    'plus': [['path', { d: 'M8 3.25v9.5M3.25 8h9.5' }]],
    'chevron-down': [['path', { d: 'M4.5 6.25 8 9.75l3.5-3.5' }]],
    'chevron-up': [['path', { d: 'M4.5 9.75 8 6.25l3.5 3.5' }]],
    'chevron-right': [['path', { d: 'M6.25 4.5 9.75 8l-3.5 3.5' }]],
    'chevron-left': [['path', { d: 'M9.75 4.5 6.25 8l3.5 3.5' }]],
    'arrow-up': [['path', { d: 'M8 12.75V3.5M4 7.5l4-4 4 4' }]],
    'arrow-down': [['path', { d: 'M8 3.25v9.25M4 8.5l4 4 4-4' }]],
    'arrow-left': [['path', { d: 'M12.75 8H3.5M7.5 4l-4 4 4 4' }]],
    'stop': [['rect', { x: 4.5, y: 4.5, width: 7, height: 7, rx: 1.5, fill: 'currentColor', stroke: 'none' }]],
    'bolt': [['path', { d: 'M9.2 1.6 3.6 9.1h4.1l-.9 5.3 5.6-7.5H8.3z', fill: 'currentColor', strokeWidth: 1 }]],
    'bolt-outline': [['path', { d: 'M9.2 1.6 3.6 9.1h4.1l-.9 5.3 5.6-7.5H8.3z', strokeWidth: 1.3 }]],
    'check': [['path', { d: 'M3.5 8.25 6.5 11.25 12.5 4.75' }]],
    'search': [['circle', { cx: 7, cy: 7, r: 4.25 }], ['path', { d: 'M10.25 10.25 13.25 13.25' }]],
    'sidebar': [['rect', { x: 2.25, y: 2.75, width: 11.5, height: 10.5, rx: 2 }], ['path', { d: 'M6.25 2.75v10.5' }]],
    'panel-right': [['rect', { x: 2.25, y: 2.75, width: 11.5, height: 10.5, rx: 2 }], ['path', { d: 'M9.75 2.75v10.5' }]],
    'compose': [['path', { d: 'M13.25 8.75v3a1.5 1.5 0 0 1-1.5 1.5h-7.5a1.5 1.5 0 0 1-1.5-1.5v-7.5a1.5 1.5 0 0 1 1.5-1.5h3' }], ['path', { d: 'M11.4 2.35a1.2 1.2 0 0 1 1.7 1.7L8.2 8.95 6 9.5l.55-2.2z' }]],
    'folder': [['path', { d: 'M2.25 4.75A1.25 1.25 0 0 1 3.5 3.5h2.6l1.4 1.5h5a1.25 1.25 0 0 1 1.25 1.25v5.5a1.25 1.25 0 0 1-1.25 1.25h-9a1.25 1.25 0 0 1-1.25-1.25z' }]],
    'x': [['path', { d: 'M4.5 4.5l7 7M11.5 4.5l-7 7' }]],
    'external': [['path', { d: 'M9.25 2.75h4v4M13.25 2.75 7.5 8.5M11.5 9.5v2.25a1.5 1.5 0 0 1-1.5 1.5H4.25a1.5 1.5 0 0 1-1.5-1.5V6a1.5 1.5 0 0 1 1.5-1.5H6.5' }]],
    'branch': [['circle', { cx: 4.75, cy: 3.75, r: 1.5 }], ['circle', { cx: 4.75, cy: 12.25, r: 1.5 }], ['circle', { cx: 11.25, cy: 5.25, r: 1.5 }], ['path', { d: 'M4.75 5.25v5.5M11.25 6.75v.5a2.5 2.5 0 0 1-2.5 2.5h-4' }]],
    'file': [['path', { d: 'M4.25 1.75h4.5l3 3v9.5h-7.5z' }], ['path', { d: 'M8.75 1.75v3h3' }]],
    'alert': [['path', { d: 'M8 2.25 14.25 13.25H1.75z' }], ['path', { d: 'M8 6.5v3' }], ['circle', { cx: 8, cy: 11.3, r: 0.6, fill: 'currentColor' }]],
    'info': [['circle', { cx: 8, cy: 8, r: 5.75 }], ['path', { d: 'M8 7.5V11' }], ['circle', { cx: 8, cy: 5.25, r: 0.6, fill: 'currentColor' }]],
    'lock': [['rect', { x: 3.25, y: 7, width: 9.5, height: 6.75, rx: 1.5 }], ['path', { d: 'M5.25 7V5a2.75 2.75 0 0 1 5.5 0v2' }]],
    'shield': [['path', { d: 'M8 1.75l5 2V8c0 3-2.25 5.1-5 6.25C5.25 13.1 3 11 3 8V3.75z' }]],
    'clock': [['circle', { cx: 8, cy: 8, r: 5.75 }], ['path', { d: 'M8 5v3.25l2.25 1.5' }]],
    'refresh': [['path', { d: 'M13.25 8a5.25 5.25 0 1 1-1.6-3.78' }], ['path', { d: 'M13.25 2.75v3h-3' }]],
    'terminal': [['rect', { x: 1.75, y: 2.75, width: 12.5, height: 10.5, rx: 2 }], ['path', { d: 'M4.75 6.25 6.75 8l-2 1.75M8.5 10h2.75' }]],
    'edit': [['path', { d: 'M10.6 2.9a1.5 1.5 0 0 1 2.1 2.1L5.5 12.2l-2.75.65.65-2.75z' }]],
    'eye': [['path', { d: 'M1.75 8S4 3.75 8 3.75 14.25 8 14.25 8 12 12.25 8 12.25 1.75 8 1.75 8z' }], ['circle', { cx: 8, cy: 8, r: 2 }]],
    'settings': [['path', { d: 'M2.75 5h6.5M12.25 5h1M2.75 11h1M6.75 11h6.5' }], ['circle', { cx: 10.75, cy: 5, r: 1.5 }], ['circle', { cx: 5.25, cy: 11, r: 1.5 }]],
    'more': [['circle', { cx: 3.5, cy: 8, r: 1, fill: 'currentColor', stroke: 'none' }], ['circle', { cx: 8, cy: 8, r: 1, fill: 'currentColor', stroke: 'none' }], ['circle', { cx: 12.5, cy: 8, r: 1, fill: 'currentColor', stroke: 'none' }]],
    'copy': [['rect', { x: 5.25, y: 5.25, width: 8, height: 8, rx: 1.5 }], ['path', { d: 'M10.75 5.25V4a1.25 1.25 0 0 0-1.25-1.25H4A1.25 1.25 0 0 0 2.75 4v5.5A1.25 1.25 0 0 0 4 10.75h1.25' }]],
    'image': [['rect', { x: 2.25, y: 2.75, width: 11.5, height: 10.5, rx: 2 }], ['circle', { cx: 5.75, cy: 6.25, r: 1.25 }], ['path', { d: 'M13.75 10.5 10.5 7.25 4 13.25' }]],
    'user': [['circle', { cx: 8, cy: 5.5, r: 2.75 }], ['path', { d: 'M2.75 13.5c.75-2.5 2.75-3.75 5.25-3.75s4.5 1.25 5.25 3.75' }]],
    'globe': [['circle', { cx: 8, cy: 8, r: 5.75 }], ['path', { d: 'M2.25 8h11.5M8 2.25c1.75 1.75 2.5 3.75 2.5 5.75S9.75 12 8 13.75C6.25 12 5.5 10 5.5 8S6.25 4 8 2.25z' }]],
    'tasks': [['path', { d: 'M6.25 4.5h7M6.25 8h7M6.25 11.5h7' }], ['circle', { cx: 3.25, cy: 4.5, r: 0.8, fill: 'currentColor', stroke: 'none' }], ['circle', { cx: 3.25, cy: 8, r: 0.8, fill: 'currentColor', stroke: 'none' }], ['circle', { cx: 3.25, cy: 11.5, r: 0.8, fill: 'currentColor', stroke: 'none' }]]
  };

  function Icon(p) {
    var def = ICONS[p.name];
    if (!def) return null;
    var size = num(p.size, 16);
    return h('svg', {
      className: cx('bk-icon', p.className), width: size, height: size, viewBox: '0 0 16 16',
      fill: 'none', stroke: 'currentColor', strokeWidth: 1.5, strokeLinecap: 'round', strokeLinejoin: 'round',
      role: p.label ? 'img' : undefined, 'aria-label': p.label || undefined, 'aria-hidden': p.label ? undefined : true,
      style: p.color ? Object.assign({ color: p.color }, p.style) : p.style
    }, def.map(function (d, i) { return h(d[0], Object.assign({ key: i }, d[1])); }));
  }

  /* ---------- ProviderMark (placeholder glyphs) ---------- */
  function ProviderMark(p) {
    var provider = p.provider === 'claude' ? 'claude' : 'codex';
    var size = num(p.size, 16);
    var tone = p.tone || 'color';
    var color = tone === 'muted' ? 'var(--text-tertiary)' : tone === 'strong' ? 'var(--text-primary)' : providerVar(provider);
    var shape = provider === 'claude'
      ? h('path', { d: 'M8 1.9 14.1 8 8 14.1 1.9 8z' })
      : h('path', { d: 'M8 1.9l5.3 3.05v6.1L8 14.1l-5.3-3.05v-6.1z' });
    return h('svg', {
      className: cx('bk-icon bk-mark', p.className), width: size, height: size, viewBox: '0 0 16 16', fill: 'none',
      stroke: 'currentColor', strokeWidth: 1.5, strokeLinejoin: 'round', role: 'img', 'aria-label': providerName(provider),
      style: { color: color }
    }, h('title', null, providerName(provider)), shape, h('circle', { cx: 8, cy: 8, r: 1.75, fill: 'currentColor', stroke: 'none' }));
  }

  /* ---------- StatusGlyph ---------- */
  var STATE_LABELS = { working: 'Working', breathe: 'Working', active: 'In progress', waiting: 'Needs you', done: 'Done', pending: 'Not started', queued: 'Queued', failed: 'Failed', unknown: 'Outcome unknown', reconnecting: 'Reconnecting', stopped: 'Stopped' };
  function StatusGlyph(p) {
    var state = p.state || 'working';
    var size = num(p.size, 16);
    var c = providerVar(p.provider);
    var s = function (color, extra) { return Object.assign({ stroke: color, strokeWidth: 1.75, fill: 'none', strokeLinecap: 'round' }, extra || {}); };
    var parts;
    if (state === 'working' || state === 'active') {
      var arc = h('path', { d: 'M8 2a6 6 0 0 1 5.196 9', style: s(c) });
      parts = [h('circle', { key: 't', cx: 8, cy: 8, r: 6, style: s(c, { opacity: 0.22 }) }),
        state === 'working' ? h('g', { key: 'a', className: 'bk-status__spin' }, arc) : h('g', { key: 'a' }, arc)];
    } else if (state === 'breathe') {
      parts = [h('circle', { key: 't', cx: 8, cy: 8, r: 6, style: s(c, { opacity: 0.22 }) }),
        h('circle', { key: 'd', className: 'bk-status__breathe', cx: 8, cy: 8, r: 2.75, style: { fill: c } })];
    } else if (state === 'waiting') {
      parts = [h('circle', { key: 'f', cx: 8, cy: 8, r: 6.75, style: { fill: 'var(--attention)' } }),
        h('path', { key: 'l', d: 'M8 4.6v4', style: s('var(--on-attention)', { strokeWidth: 1.9 }) }),
        h('circle', { key: 'p', cx: 8, cy: 11.1, r: 1, style: { fill: 'var(--on-attention)' } })];
    } else if (state === 'done') {
      parts = [h('circle', { key: 'f', cx: 8, cy: 8, r: 6.75, style: { fill: 'var(--surface-raised)' } }),
        h('path', { key: 'c', d: 'M5.25 8.1 7.1 9.95 10.75 6.1', style: s('var(--text-primary)', { strokeWidth: 1.5, strokeLinejoin: 'round' }) })];
    } else if (state === 'pending' || state === 'queued') {
      parts = [h('circle', { key: 'r', cx: 8, cy: 8, r: 6, style: s('var(--text-tertiary)', { strokeWidth: 1.5, strokeDasharray: '2.2 2.6' }) })];
    } else if (state === 'failed') {
      parts = [h('circle', { key: 'r', cx: 8, cy: 8, r: 6, style: s('var(--negative)', { strokeWidth: 1.5 }) }),
        h('path', { key: 'x', d: 'M6 6l4 4M10 6l-4 4', style: s('var(--negative)', { strokeWidth: 1.5 }) })];
    } else if (state === 'unknown') {
      parts = [h('circle', { key: 'r', cx: 8, cy: 8, r: 6, style: s('var(--text-secondary)', { strokeWidth: 1.5 }) }),
        h('path', { key: 'q', d: 'M6.4 6.4a1.65 1.65 0 1 1 2.3 1.5c-.45.2-.7.55-.7 1.05v.2', style: s('var(--text-secondary)', { strokeWidth: 1.5 }) }),
        h('circle', { key: 'p', cx: 8, cy: 11.1, r: 0.85, style: { fill: 'var(--text-secondary)' } })];
    } else if (state === 'reconnecting') {
      parts = [h('g', { key: 'g', className: 'bk-status__slow' }, h('circle', { cx: 8, cy: 8, r: 6, style: s('var(--text-secondary)', { strokeWidth: 1.5, strokeDasharray: '3 3' }) }))];
    } else {
      parts = [h('circle', { key: 'r', cx: 8, cy: 8, r: 6, style: s('var(--text-tertiary)', { strokeWidth: 1.5 }) }),
        h('rect', { key: 's', x: 6, y: 6, width: 4, height: 4, rx: 0.75, style: { fill: 'var(--text-tertiary)' } })];
    }
    var label = p.label || STATE_LABELS[state] || state;
    return h('svg', {
      className: cx('bk-status', on(p.still) && 'bk-status--still', p.className), width: size, height: size, viewBox: '0 0 16 16',
      role: 'img', 'aria-label': label
    }, [h('title', { key: 'title' }, label)].concat(parts));
  }

  /* ---------- StateChip ---------- */
  function StateChip(p) {
    var state = p.state || 'working';
    var label = p.label || STATE_LABELS[state] || state;
    return h('span', { className: 'bk-chip', role: 'status' },
      h(StatusGlyph, { state: state, provider: p.provider, still: p.still, label: label }),
      h('span', null, label),
      p.time ? h('span', { className: 'bk-chip__time bk-num' }, '· ' + p.time) : null);
  }

  /* ---------- Buttons ---------- */
  function Kbd(p) {
    var keys = Array.isArray(p.keys) ? p.keys : (p.keys ? String(p.keys).split(' ') : null);
    if (!keys) return h('kbd', { className: 'bk-kbd' }, p.children);
    return h('span', { className: 'bk-kbds' }, keys.map(function (k, i) { return h('kbd', { key: i, className: 'bk-kbd' }, k); }));
  }

  function Button(p) {
    var variant = p.variant || 'secondary';
    return h('button', {
      type: 'button', className: cx('bk-btn', 'bk-btn--' + variant, p.size === 'sm' && 'bk-btn--sm', p.className),
      disabled: on(p.disabled) || undefined, 'aria-label': p.ariaLabel, style: p.style, onClick: p.onClick
    },
      p.icon ? h(Icon, { name: p.icon, size: p.size === 'sm' ? 14 : 16 }) : null,
      p.children,
      p.iconRight ? h(Icon, { name: p.iconRight, size: 14 }) : null,
      p.kbd ? h(Kbd, { keys: p.kbd }) : null);
  }

  function IconButton(p) {
    var variant = p.variant || 'ghost';
    var small = p.size === 'sm';
    return h('button', {
      type: 'button', 'aria-label': p.label, title: p.label,
      className: cx('bk-iconbtn', variant !== 'ghost' && 'bk-iconbtn--' + variant, small && 'bk-iconbtn--sm', on(p.active) && 'bk-iconbtn--active', p.className),
      disabled: on(p.disabled) || undefined, 'aria-pressed': p.pressed === undefined ? undefined : on(p.pressed), style: p.style, onClick: p.onClick
    }, h(Icon, { name: p.icon, size: small ? 14 : 16 }));
  }

  function Switch(p) {
    var checked = on(p.checked);
    return h('button', {
      type: 'button', role: 'switch', className: 'bk-switch', 'aria-checked': checked, 'aria-label': p.label,
      disabled: on(p.disabled) || undefined, style: checked ? { background: p.provider ? providerVar(p.provider) : 'var(--action)' } : undefined
    });
  }

  /* ---------- Sidebar ---------- */
  function SectionLabel(p) {
    return h('div', { className: 'bk-section', role: 'heading', 'aria-level': 2 },
      h('span', null, p.children || p.label),
      p.action ? h(IconButton, { icon: p.action, label: p.actionLabel || 'Add', size: 'sm' }) : null);
  }

  function ChatRow(p) {
    var state = p.state || 'idle';
    var selected = on(p.selected);
    var provider = p.provider === 'claude' ? 'claude' : 'codex';
    var slot;
    if (state === 'working') slot = h(StatusGlyph, { state: 'working', provider: provider, still: p.still, label: providerName(provider) + ' working' });
    else if (state === 'waiting') slot = h(StatusGlyph, { state: 'waiting', label: 'Needs you' });
    else if (state === 'failed') slot = h(StatusGlyph, { state: 'failed' });
    else if (state === 'unavailable') slot = h(Icon, { name: 'alert', size: 14, label: 'Workspace unavailable' });
    else slot = h(ProviderMark, { provider: provider, size: 14, tone: selected ? 'color' : 'muted' });
    var label = p.title + ', ' + providerName(provider) + (state !== 'idle' ? ', ' + (state === 'unread' ? 'new result' : STATE_LABELS[state] || state) : '');
    return h('button', {
      type: 'button', 'aria-current': selected ? 'page' : undefined, 'aria-label': label,
      className: cx('bk-row', selected && 'bk-row--selected', on(p.child) && 'bk-row--child', state === 'unread' && 'bk-row--unread', state === 'unavailable' && 'bk-row--unavailable', on(p.focused) && 'bk-row--focused', p.className)
    },
      p.icon ? h('span', { className: 'bk-row__lead' }, h(Icon, { name: p.icon })) : null,
      h('span', { className: 'bk-row__title' }, p.title),
      p.kbd ? h('span', { className: 'bk-row__kbd' }, h(Kbd, { keys: p.kbd })) : null,
      h('span', { className: 'bk-row__slot' }, state === 'unread' ? h('span', { className: 'bk-row__dot', 'aria-hidden': true }) : null, slot));
  }

  function ProjectRow(p) {
    var open = on(p.open);
    return h('button', { type: 'button', className: 'bk-row', 'aria-expanded': open },
      h('span', { className: 'bk-row__lead' }, h(Icon, { name: open ? 'chevron-down' : 'chevron-right', size: 14 })),
      h('span', { className: 'bk-row__title' }, p.name),
      p.count ? h('span', { className: 'bk-row__slot bk-num', style: { fontSize: '12px' } }, p.count) : null);
  }

  function UsageMeter(p) {
    var provider = p.provider === 'claude' ? 'claude' : 'codex';
    var state = p.state || 'fresh';
    var left = Math.max(0, Math.min(100, num(p.left, 0)));
    var label = (p.label || providerName(provider)) + (p.window ? ' · ' + p.window : '');
    var right;
    if (state === 'unavailable') right = h('span', { className: 'bk-usage__right' }, 'Unavailable');
    else right = h('span', { className: 'bk-usage__right' }, left + '% left', state === 'stale' && p.updated ? h('span', { className: 'bk-usage__stale' }, ' · ' + p.updated) : null);
    return h('div', { className: cx('bk-usage', 'bk-usage--' + state), role: 'group', 'aria-label': label + ' usage' },
      h('div', { className: 'bk-usage__row' }, h('span', null, label), right),
      h('div', { className: 'bk-usage__track', role: state === 'unavailable' ? undefined : 'meter', 'aria-valuemin': 0, 'aria-valuemax': 100, 'aria-valuenow': state === 'unavailable' ? undefined : left },
        state === 'unavailable' ? null : h('div', { className: 'bk-usage__fill', style: { width: left + '%', background: providerVar(provider) } })),
      p.reset && state !== 'unavailable' ? h('span', { className: 'bk-usage__reset' }, p.reset) : null);
  }

  function ProfileRow(p) {
    var open = on(p.open);
    var name = p.name || 'Rasmus';
    return h('button', { type: 'button', className: cx('bk-profile', open && 'bk-profile--open'), 'aria-haspopup': 'menu', 'aria-expanded': open },
      h('span', { className: 'bk-avatar', 'aria-hidden': true }, p.initial || name.charAt(0)),
      h('span', { className: 'bk-profile__name' }, name),
      h('span', { className: 'bk-profile__chev' }, h(Icon, { name: open ? 'chevron-down' : 'chevron-up', size: 14 })));
  }

  /* ---------- Conversation ---------- */
  function UserMessage(p) {
    var files = list(p.files);
    return h('div', { className: 'bk-user-row' },
      h('div', { className: 'bk-user' },
        files.length ? h('div', { className: 'bk-user__files' }, files.map(function (f, i) {
          return h('span', { key: i, className: 'bk-attach' }, h(Icon, { name: f.icon || 'file', size: 14 }), f.name || f);
        })) : null,
        p.children || p.text),
      p.note ? h('span', { className: 'bk-user__note' }, p.noteIcon ? h(Icon, { name: p.noteIcon, size: 12 }) : null, p.note) : null);
  }

  function AgentTurn(p) {
    var provider = p.provider === 'claude' ? 'claude' : 'codex';
    return h('section', { className: 'bk-turn', 'aria-label': (p.name || providerName(provider)) + ' reply' },
      h('div', { className: 'bk-turn__head' },
        h('span', { style: { color: providerVar(provider) } }, p.name || providerName(provider)),
        p.meta ? h('span', { className: 'bk-turn__meta' }, p.meta) : null),
      h('div', { className: 'bk-turn__body' }, p.children));
  }

  function PlanStep(p) {
    var state = p.state || 'pending';
    var glyph = state === 'done' ? 'done' : state === 'active' ? 'active' : state === 'failed' ? 'failed' : 'pending';
    return h('div', { className: cx('bk-step', 'bk-step--' + state) },
      h('span', { className: 'bk-step__glyph' }, h(StatusGlyph, { state: glyph, provider: p.provider })),
      h('div', { className: 'bk-step__text' },
        h('span', { className: 'bk-step__title' }, p.title || p.children),
        p.detail ? h('span', { className: 'bk-step__detail' }, p.detail) : null));
  }

  function ActivityLine(p) {
    var toggle = on(p.toggle);
    return h(toggle ? 'button' : 'div', {
      type: toggle ? 'button' : undefined, className: cx('bk-activity', toggle && 'bk-activity--toggle'),
      style: toggle ? { background: 'transparent', border: 0, padding: 0, fontFamily: 'inherit', textAlign: 'left', width: '100%' } : undefined
    },
      h('span', { className: 'bk-activity__icon' }, h(Icon, { name: toggle ? 'chevron-right' : (p.icon || 'terminal'), size: 14 })),
      h('span', { className: 'bk-activity__text' }, p.children || p.text),
      p.meta ? h('span', { className: 'bk-activity__meta' }, p.meta) : null);
  }

  function CodeBlock(p) {
    return h('div', { className: 'bk-codeblock' },
      h('div', { className: 'bk-codeblock__head' }, h('span', null, p.lang || 'text'), h(IconButton, { icon: 'copy', label: 'Copy code', size: 'sm' })),
      h('pre', null, h('code', null, p.code || p.children)));
  }

  function Notice(p) {
    var tone = p.tone || 'info';
    var glyph = tone === 'error' ? h(StatusGlyph, { state: 'failed' })
      : tone === 'unknown' ? h(StatusGlyph, { state: 'unknown' })
      : tone === 'reconnecting' ? h(StatusGlyph, { state: 'reconnecting', still: p.still })
      : tone === 'waiting' ? h(StatusGlyph, { state: 'waiting' })
      : h(Icon, { name: 'info', color: 'var(--text-secondary)' });
    var actions = list(p.actions);
    return h('div', { className: cx('bk-notice', 'bk-notice--' + tone), role: tone === 'error' ? 'alert' : 'status' },
      h('span', { className: 'bk-notice__glyph' }, glyph),
      h('div', { className: 'bk-notice__body' },
        h('span', { className: 'bk-notice__title' }, p.title),
        p.children || p.text ? h('span', { className: 'bk-notice__text' }, p.children || p.text) : null,
        actions.length ? h('div', { className: 'bk-notice__actions' }, actions.map(function (a, i) {
          return h(Button, { key: i, size: 'sm', variant: a.variant || 'secondary', icon: a.icon, kbd: a.kbd }, a.label);
        })) : null));
  }

  function TaskBrief(p) {
    var provider = p.from === 'claude' ? 'claude' : 'codex';
    return h('div', { className: 'bk-brief', role: 'note', 'aria-label': 'Assignment from ' + providerName(provider) },
      h('div', { className: 'bk-brief__head' },
        h(ProviderMark, { provider: provider, size: 14 }),
        h('span', null, p.label || ('Assigned by ' + providerName(provider))),
        p.tag ? h('span', { className: 'bk-tag', style: { marginLeft: 'auto' } }, p.tag) : null),
      h('div', { className: 'bk-brief__text' }, p.children || p.text));
  }

  function EventLine(p) {
    return h('div', { className: 'bk-event', role: 'status' },
      h(StatusGlyph, { state: p.state || 'done', provider: p.provider, still: p.still }),
      h('span', { className: 'bk-event__text' }, p.children || p.text),
      p.tag ? h('span', { className: 'bk-tag' }, p.tag) : null,
      p.action ? h('span', { className: 'bk-event__action' }, h(Button, { size: 'sm', variant: 'ghost', iconRight: 'chevron-right' }, p.action)) : null);
  }

  function Breadcrumb(p) {
    var items = list(p.items);
    var last = items[items.length - 1] || {};
    return h('nav', { className: 'bk-breadcrumb', 'aria-label': 'Task path' },
      h(IconButton, { icon: 'arrow-left', label: p.backLabel || ('Back to ' + (items[0] ? items[0].label : 'parent')), size: 'sm' }),
      items.slice(0, -1).map(function (it, i) {
        return h(React.Fragment, { key: i },
          h('a', { className: 'bk-breadcrumb__link', href: '#' }, it.provider ? h(ProviderMark, { provider: it.provider, size: 14, tone: 'muted' }) : null, it.label),
          h('span', { className: 'bk-breadcrumb__sep' }, h(Icon, { name: 'chevron-right', size: 12 })));
      }),
      h('span', { className: 'bk-breadcrumb__current', 'aria-current': 'page' }, last.label));
  }

  /* ---------- Composer ---------- */
  function PermissionControl(p) {
    var open = on(p.open);
    return h('button', { type: 'button', className: cx('bk-perm', open && 'bk-perm--open'), 'aria-haspopup': 'menu', 'aria-expanded': open, 'aria-label': 'Permissions: ' + (p.value || 'Ask before changes') },
      p.icon ? h(Icon, { name: p.icon, size: 14 }) : null,
      h('span', null, p.value || 'Ask before changes'),
      h('span', { className: 'bk-perm__chev' }, h(Icon, { name: 'chevron-down', size: 12 })));
  }

  function ModelControl(p) {
    var provider = p.provider === 'claude' ? 'claude' : 'codex';
    var open = on(p.open);
    var step = num(p.level, 3);
    var fast = p.fast === undefined ? true : on(p.fast);
    var effort = p.effort || 'High';
    return h('button', {
      type: 'button', className: cx('bk-modelctl', open && 'bk-modelctl--open', on(p.pending) && 'bk-modelctl--pending'), 'aria-haspopup': 'dialog', 'aria-expanded': open,
      'aria-label': providerName(provider) + ' model ' + (p.model || '') + ', reasoning ' + effort + (fast ? ', fast mode on' : '')
    },
      h(Icon, { name: fast ? 'bolt' : 'bolt-outline', size: 14, color: rampVar(provider, step) }),
      h('span', null, p.model || 'GPT-6.1 Sol'),
      h('span', { className: 'bk-modelctl__effort' }, effort),
      h('span', { className: 'bk-modelctl__chev' }, h(Icon, { name: 'chevron-down', size: 12 })));
  }

  function Composer(p) {
    var provider = p.provider === 'claude' ? 'claude' : 'codex';
    var running = on(p.running);
    var value = p.value || '';
    var files = list(p.files);
    var placeholder = p.placeholder || (running ? 'Add a direction while work continues…' : 'Ask ' + providerName(provider) + ' to do something');
    var action;
    if (running && !value) action = h(Button, { variant: 'secondary', icon: 'stop', ariaLabel: 'Stop ' + providerName(provider) }, 'Stop');
    else if (running) action = h(React.Fragment, null, h(IconButton, { icon: 'stop', label: 'Stop ' + providerName(provider), variant: 'raised' }), h(IconButton, { icon: 'arrow-up', label: 'Send to ' + providerName(provider), variant: 'primary' }));
    else action = h(IconButton, { icon: 'arrow-up', label: 'Send to ' + providerName(provider), variant: 'primary', disabled: !value });
    return h('div', { className: 'bk-composer', style: on(p.focused) ? { boxShadow: 'var(--shadow-composer), 0 0 0 1px #ffffff14' } : undefined },
      files.length ? h('div', { className: 'bk-composer__files' }, files.map(function (f, i) {
        return h('span', { key: i, className: 'bk-attach' }, h(Icon, { name: f.icon || 'file', size: 14 }), f.name || f, h(Icon, { name: 'x', size: 12, label: 'Remove' }));
      })) : null,
      h('textarea', { className: 'bk-composer__input', placeholder: placeholder, defaultValue: value, rows: 2, 'aria-label': 'Message ' + (p.recipient || providerName(provider)) }),
      h('div', { className: 'bk-composer__bar' },
        h('div', { className: 'bk-composer__cluster' },
          h(IconButton, { icon: 'plus', label: 'Add files or images' }),
          h(PermissionControl, { value: p.permission, open: p.permOpen })),
        h('div', { className: 'bk-composer__cluster', style: { gap: '8px' } },
          h(ModelControl, { provider: provider, model: p.model, effort: p.effort, level: p.level, fast: p.fast, open: p.modelOpen, pending: p.pending }),
          action)));
  }

  function EffortSlider(p) {
    var provider = p.provider === 'claude' ? 'claude' : 'codex';
    var levels = list(p.levels).length ? list(p.levels) : ['Low', 'Medium', 'High', 'XHigh'];
    var n = levels.length;
    var start = levels.indexOf(p.value);
    if (start < 0) start = Math.min(2, n - 1);
    var st = React.useState(start);
    var controlled = typeof p.onChange === 'function';
    var idx = controlled ? Math.max(0, levels.indexOf(p.value)) : st[0];
    var select = function (i) { if (controlled) p.onChange(levels[i], i); else st[1](i); };
    var isMax = idx === n - 1 && n > 1;
    var step = rampStep(idx, n);
    if (p.size === 'lg') {
      var onKey = function (e) {
        if (e.key === 'ArrowRight' || e.key === 'ArrowUp') { e.preventDefault(); select(Math.min(n - 1, idx + 1)); }
        if (e.key === 'ArrowLeft' || e.key === 'ArrowDown') { e.preventDefault(); select(Math.max(0, idx - 1)); }
      };
      return h('div', {
        className: cx('bk-effortlg', isMax && 'bk-effortlg--max', on(p.still) && 'bk-effortlg--still'),
        role: 'slider', tabIndex: 0, onKeyDown: onKey, 'aria-label': 'Reasoning effort',
        'aria-valuemin': 1, 'aria-valuemax': n, 'aria-valuenow': idx + 1, 'aria-valuetext': levels[idx]
      },
        h('div', { className: 'bk-effortlg__track', style: { '--glow': providerVar(provider), gridTemplateColumns: 'repeat(' + n + ', minmax(0, 1fr))' } },
          levels.map(function (l, i) {
            var filled = i <= idx;
            return h('button', {
              key: i, type: 'button', tabIndex: -1, 'aria-label': l, onClick: function () { select(i); },
              className: cx('bk-effortlg__cell', filled && 'bk-effortlg__cell--on', i === idx && 'bk-effortlg__cell--current'),
              style: filled ? { background: rampVar(provider, rampStep(i, n)) } : undefined
            }, i === idx ? h('span', { className: 'bk-effortlg__knob' }, h(Icon, { name: 'bolt', size: 12 })) : null);
          }),
          isMax ? h('span', { className: 'bk-effortlg__glint', 'aria-hidden': true }) : null),
        h('div', { className: 'bk-effortlg__labels', style: { gridTemplateColumns: 'repeat(' + n + ', minmax(0, 1fr))' } }, levels.map(function (l, i) {
          return h('span', { key: i, className: i === idx ? 'bk-effortlg__label--on' : undefined }, l);
        })));
    }
    var pct = function (i) { return n <= 1 ? 0 : (i / (n - 1)) * 100; };
    var fillBg = 'linear-gradient(90deg, ' + rampVar(provider, 1) + ', ' + rampVar(provider, step) + ')';
    var pos = function (i) { return 'calc(7px + (100% - 14px) * ' + (pct(i) / 100) + ')'; };
    return h('div', {
      className: cx('bk-effort', isMax && 'bk-effort--max', on(p.still) && 'bk-effort--still'), role: 'slider', tabIndex: 0,
      'aria-label': 'Reasoning effort', 'aria-valuemin': 1, 'aria-valuemax': n, 'aria-valuenow': idx + 1, 'aria-valuetext': levels[idx]
    },
      h('div', { className: 'bk-effort__track' },
        h('div', { className: 'bk-effort__rail' }),
        h('div', { className: 'bk-effort__fill', style: { width: pct(idx) + '%', background: fillBg, overflow: 'hidden' } },
          isMax ? h('span', { className: 'bk-effort__glint' }) : null),
        levels.map(function (l, i) { return i > idx ? h('span', { key: i, className: 'bk-effort__stop', style: { left: pct(i) + '%' } }) : null; }),
        h('span', { className: 'bk-effort__thumb', style: { left: pct(idx) + '%', background: rampVar(provider, step) } })),
      h('div', { className: 'bk-effort__labels' }, levels.map(function (l, i) {
        var st2 = i === 0 ? { left: 0 } : i === n - 1 ? { right: 0 } : { left: pos(i), transform: 'translateX(-50%)' };
        return h('span', { key: i, className: i === idx ? 'bk-effort__label--on' : undefined, style: st2 }, l);
      })));
  }

  var GROUP_LABELS = { codex: ['ChatGPT models', 'Runs in Codex'], claude: ['Claude models', 'Runs in Claude Code'] };
  function ModelPicker(p) {
    var groups = list(p.groups);
    var flat = [];
    groups.forEach(function (g) { list(g.models).forEach(function (m) { flat.push({ group: g, model: m }); }); });
    var first = null;
    flat.forEach(function (x) { if (!first && on(x.model.selected)) first = x; });
    if (!first) first = flat[0] || { group: { provider: p.provider }, model: { name: '' } };
    var vs = React.useState(p.view === 'models' ? 'models' : 'effort');
    var view = vs[0], setView = vs[1];
    var ms = React.useState(first.model.name);
    var cur = first;
    flat.forEach(function (x) { if (x.model.name === ms[0]) cur = x; });
    var provider = cur.group.provider === 'claude' ? 'claude' : 'codex';
    var levels = list(cur.model.levels).length ? list(cur.model.levels) : list(p.levels);
    var defaultEffort = cur.model.effort || p.effort || levels[Math.min(2, levels.length - 1)];
    var es = React.useState(defaultEffort);
    var effort = levels.indexOf(es[0]) >= 0 ? es[0] : defaultEffort;
    var effortIdx = Math.max(0, levels.indexOf(effort));
    var details = list(cur.model.details).length ? list(cur.model.details) : list(p.details);
    var fastAvailable = cur.model.fastAvailable === undefined ? (p.fastAvailable === undefined ? true : on(p.fastAvailable)) : on(cur.model.fastAvailable);
    var fs = React.useState(on(p.fast));
    var fast = fs[0] && fastAvailable;
    var locked = p.lockedProvider;
    var pick = function (x) {
      if (locked && x.group.provider !== locked) return;
      if (on(x.model.unavailable)) return;
      ms[1](x.model.name);
      es[1](x.model.effort || p.effort || null);
      setView('effort');
    };
    if (view === 'models') {
      return h('div', { className: 'bk-popover bk-mp', role: 'dialog', 'aria-label': 'Choose a model' },
        h('div', { className: 'bk-mp__head' },
          h(IconButton, { icon: 'arrow-left', label: 'Back to reasoning effort', size: 'sm', onClick: function () { setView('effort'); } }),
          h('span', { className: 'bk-mp__head-title' }, 'Choose a model')),
        groups.map(function (g, gi) {
          var gp = g.provider === 'claude' ? 'claude' : 'codex';
          var labels = GROUP_LABELS[gp];
          var groupLocked = locked && locked !== gp;
          return h('div', { key: gi, className: 'bk-mp__group', role: 'group', 'aria-label': g.label || labels[0] },
            h('div', { className: 'bk-mp__section' },
              h(ProviderMark, { provider: gp, size: 14, tone: groupLocked ? 'muted' : 'color' }),
              h('span', { className: 'bk-mp__section-name' }, g.label || labels[0]),
              h('span', { className: 'bk-mp__section-note' }, g.note || labels[1])),
            list(g.models).map(function (m, mi) {
              var selected = cur.model.name === m.name;
              var disabled = groupLocked || on(m.unavailable);
              return h('button', {
                key: mi, type: 'button', role: 'menuitemradio', 'aria-checked': selected, 'aria-disabled': disabled || undefined,
                className: cx('bk-picker__option', selected && 'bk-picker__option--on'), onClick: function () { pick({ group: g, model: m }); },
                style: selected ? { background: gp === 'claude' ? 'var(--claude-tint)' : 'var(--codex-tint)' } : undefined
              },
                h('span', { className: 'bk-picker__main' }, h('span', { className: 'bk-picker__name' }, m.name), m.detail ? h('span', { className: 'bk-picker__detail' }, m.detail) : null),
                h('span', { className: 'bk-picker__check' }, selected ? h(Icon, { name: 'check', color: providerVar(gp) }) : null));
            }),
            groupLocked ? h('div', { className: 'bk-menu__note' }, h(Icon, { name: 'info', size: 14 }), h('span', null, p.lockedNote || ('Switching to ' + providerName(gp) + ' hands this work to a new chat.')), h('span', { className: 'bk-tag', style: { marginLeft: 'auto' } }, 'Later')) : null);
        }));
    }
    var step = rampStep(effortIdx, levels.length);
    return h('div', { className: 'bk-popover bk-mp', role: 'dialog', 'aria-label': 'Model and reasoning' },
      h('button', { type: 'button', className: 'bk-mp__model', onClick: function () { setView('models'); }, 'aria-label': 'Model ' + cur.model.name + '. Choose another model' },
        h('span', { className: 'bk-mp__model-bolt' }, h(Icon, { name: fast ? 'bolt' : 'bolt-outline', color: rampVar(provider, step) })),
        h('span', { className: 'bk-mp__model-main' },
          h('span', { className: 'bk-mp__model-name' }, cur.model.name),
          h('span', { className: 'bk-mp__model-note' }, (provider === 'claude' ? 'Claude' : 'ChatGPT') + ' model · ' + (provider === 'claude' ? 'Claude Code' : 'Codex'))),
        h('span', { className: 'bk-mp__model-chev' }, h(Icon, { name: 'chevron-right', size: 14 }))),
      h('div', { className: 'bk-menu__divider' }),
      levels.length ? h('div', { className: 'bk-mp__effort' },
        h('div', { className: 'bk-mp__effort-head' },
          h('span', { className: 'bk-mp__effort-label' }, 'Reasoning effort'),
          h('span', { className: 'bk-mp__effort-value', style: { color: rampVar(provider, Math.max(3, step)) } }, effort)),
        h(EffortSlider, { size: 'lg', provider: provider, levels: levels, value: effort, still: p.still, onChange: function (v) { es[1](v); } }),
        details[effortIdx] ? h('p', { className: 'bk-mp__detail' }, details[effortIdx]) : null)
        : h('div', { className: 'bk-mp__effort' }, h('span', { className: 'bk-effort__fixed' }, p.fixedEffort || 'This model uses a fixed reasoning level.')),
      h('div', { className: 'bk-menu__divider' }),
      h('div', { className: 'bk-picker__row' },
        h(Icon, { name: fast ? 'bolt' : 'bolt-outline', color: fastAvailable ? providerVar(provider) : 'var(--text-disabled)' }),
        h('span', { className: 'bk-picker__main' },
          h('span', { className: 'bk-picker__name', style: fastAvailable ? undefined : { color: 'var(--text-disabled)' } }, 'Fast mode'),
          h('span', { className: 'bk-picker__detail' }, fastAvailable ? (p.fastDetail || 'Quicker replies. Uses your limit faster.') : 'Not offered for this model.')),
        h('button', {
          type: 'button', role: 'switch', className: 'bk-switch', 'aria-checked': fast, 'aria-label': 'Fast mode', disabled: !fastAvailable || undefined,
          onClick: function () { fs[1](!fs[0]); }, style: fast ? { background: providerVar(provider) } : undefined
        })),
      p.note ? h('div', { className: 'bk-picker__foot' }, p.note) : null);
  }

  /* ---------- Thinking orb ---------- */
  // A small dotted sphere drawn on a 2D canvas. Neutral dots carry depth (near dots larger and
  // brighter); the provider colour appears only where the work is: a band of light while thinking,
  // a sweep while reading, two small orbiting points while a tool runs. Waiting holds still.
  var ORB_T0 = (typeof performance !== 'undefined' ? performance.now() : Date.now());
  var ORB_STATES = { thinking: 'Thinking', reading: 'Reading', tool: 'Working', waiting: 'Waiting for you' };
  function parseColor(v, fallback) {
    v = String(v || '').trim();
    var m = /^#([0-9a-f]{3,8})$/i.exec(v);
    if (m) {
      var s = m[1];
      if (s.length <= 4) s = s.split('').map(function (c) { return c + c; }).join('');
      return [parseInt(s.slice(0, 2), 16), parseInt(s.slice(2, 4), 16), parseInt(s.slice(4, 6), 16)];
    }
    m = /rgba?\(([^)]+)\)/i.exec(v);
    if (m) { var p = m[1].split(/[ ,/]+/).map(Number); return [p[0], p[1], p[2]]; }
    return fallback;
  }
  function orbDirs(n) {
    var out = [], golden = Math.PI * (3 - Math.sqrt(5));
    for (var i = 0; i < n; i++) {
      var y = 1 - (2 * (i + 0.5)) / n, r = Math.sqrt(1 - y * y), a = i * golden;
      out.push([r * Math.cos(a), y, r * Math.sin(a)]);
    }
    return out;
  }
  function orbDraw(ctx, size, t, state, dirs, ink, accent) {
    var cx = size / 2, cy = size / 2, rs = size / 28;
    var R = size * 0.34 * (1 + 0.02 * Math.sin(t * 1.5));
    var yaw = t * (state === 'tool' ? 0.5 : state === 'waiting' ? 0 : 0.3), tilt = 0.42;
    var sy = Math.sin(yaw), cyw = Math.cos(yaw), st = Math.sin(tilt), ct = Math.cos(tilt);
    var proj = function (x, y, z) {
      var x1 = x * cyw + z * sy, z1 = -x * sy + z * cyw;
      return [cx + x1 * R, cy - (y * ct - z1 * st) * R, y * st + z1 * ct];
    };
    var dots = [];
    var bandY = Math.sin(t * 0.6) * 0.62;
    var scanA = t * 0.9;
    for (var i = 0; i < dirs.length; i++) {
      var d = dirs[i], q = proj(d[0], d[1], d[2]);
      var depth = (q[2] + 1) / 2;
      var w = 0;
      if (state === 'thinking') { var dy = d[1] - bandY; w = Math.exp(-(dy * dy) / 0.03); }
      else if (state === 'reading') { var da = Math.atan2(Math.sin(Math.atan2(d[2], d[0]) - scanA), Math.cos(Math.atan2(d[2], d[0]) - scanA)); w = Math.exp(-(da * da) / 0.09); }
      var alpha = (state === 'waiting' ? 0.1 : 0.12) + (state === 'waiting' ? 0.4 : 0.5) * depth;
      dots.push({ x: q[0], y: q[1], z: q[2], r: (0.38 + 0.6 * depth) * rs * (1 + 0.3 * w), c: w, a: alpha + (0.92 - alpha) * w * (0.35 + 0.65 * depth) });
    }
    if (state === 'tool') {
      for (var k = 0; k < 2; k++) {
        var tiltK = k === 0 ? 0.9 : -0.7, ro = 1.18;
        var ux = Math.cos(tiltK), uy = Math.sin(tiltK);
        for (var tail = 0; tail < 5; tail++) {
          var ang = t * 2.1 + k * Math.PI - tail * 0.16;
          var ox = Math.cos(ang) * ro * ux, oy = Math.cos(ang) * ro * uy, oz = Math.sin(ang) * ro;
          var pq = proj(ox, oy, oz), pd = (pq[2] / ro + 1) / 2;
          dots.push({ x: pq[0], y: pq[1], z: pq[2], r: (1.05 + 0.6 * pd) * rs * (1 - tail * 0.15), c: 1, a: (0.95 - tail * 0.19) * (0.45 + 0.55 * pd) });
        }
      }
    }
    dots.sort(function (a, b) { return a.z - b.z; });
    ctx.clearRect(0, 0, size, size);
    for (var j = 0; j < dots.length; j++) {
      var o = dots[j], c = o.c;
      var r = Math.round(ink[0] + (accent[0] - ink[0]) * c), g = Math.round(ink[1] + (accent[1] - ink[1]) * c), b = Math.round(ink[2] + (accent[2] - ink[2]) * c);
      ctx.fillStyle = 'rgba(' + r + ',' + g + ',' + b + ',' + Math.max(0, Math.min(1, o.a)).toFixed(3) + ')';
      ctx.beginPath();
      ctx.arc(o.x, o.y, Math.max(0.3, o.r), 0, Math.PI * 2);
      ctx.fill();
    }
  }
  function ThinkingOrb(p) {
    var ref = React.useRef(null);
    var size = num(p.size, 28);
    var state = ORB_STATES[p.state] ? p.state : 'thinking';
    var provider = p.provider === 'claude' ? 'claude' : 'codex';
    var still = on(p.still);
    var dotCount = num(p.dots, size >= 24 ? 72 : 40);
    React.useEffect(function () {
      var canvas = ref.current;
      if (!canvas || !canvas.getContext) return undefined;
      var dpr = Math.min(2, window.devicePixelRatio || 1);
      canvas.width = Math.round(size * dpr);
      canvas.height = Math.round(size * dpr);
      var ctx = canvas.getContext('2d');
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      var css = window.getComputedStyle(canvas);
      var ink = parseColor(css.getPropertyValue('--text-secondary'), [182, 179, 173]);
      var accent = state === 'waiting' ? ink : parseColor(css.getPropertyValue(provider === 'claude' ? '--claude' : '--codex'), provider === 'claude' ? [223, 160, 124] : [156, 203, 255]);
      var dirs = orbDirs(dotCount);
      var reduce = window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
      var raf = 0, visible = true, io = null, alive = true;
      var paint = function (now) {
        if (!alive) return;
        orbDraw(ctx, size, (now - ORB_T0) / 1000, state, dirs, ink, accent);
        if (visible) raf = window.requestAnimationFrame(paint);
      };
      if (still || reduce || state === 'waiting') {
        orbDraw(ctx, size, 2.2, state, dirs, ink, accent);
      } else {
        raf = window.requestAnimationFrame(paint);
        if (window.IntersectionObserver) {
          io = new window.IntersectionObserver(function (entries) {
            var v = entries[0] && entries[0].isIntersecting;
            if (v && !visible) { visible = true; raf = window.requestAnimationFrame(paint); }
            if (!v) { visible = false; window.cancelAnimationFrame(raf); }
          });
          io.observe(canvas);
        }
      }
      return function () { alive = false; window.cancelAnimationFrame(raf); if (io) io.disconnect(); };
    }, [size, state, provider, still, dotCount]);
    return h('canvas', {
      ref: ref, className: cx('bk-orb', p.className), width: size, height: size, style: { width: size + 'px', height: size + 'px' },
      role: 'img', 'aria-label': p.label || (providerName(provider) + ': ' + ORB_STATES[state])
    });
  }

  /* ---------- Build grid (proposed replacement for ThinkingOrb) ---------- */
  // Nine small blocks on a 3 by 3 grid. Work is "placed" one block at a time,
  // in hard steps of --step-working (160ms), so it looks deliberate at about
  // six changes a second and costs a native renderer six frames, not sixty.
  // Levels: 0 = empty block, 1 = faint trace, 2 = placed (warm neutral),
  // 3 = the newest block in the provider colour.
  var GRID_PATH = [0, 1, 2, 5, 4, 3, 6, 7, 8, 7, 6, 3, 4, 5, 2, 1];
  var GRID_RING = [0, 1, 2, 5, 8, 7, 6, 3];
  var GRID_STATES = { thinking: 'Thinking', reading: 'Reading', tool: 'Working', waiting: 'Waiting for you' };
  function gridLevels(state, step) {
    var lv = [0, 0, 0, 0, 0, 0, 0, 0, 0];
    var n, k;
    if (state === 'reading') {
      // Rows light top to bottom, like lines being read, then a short rest.
      k = step % 4;
      for (n = 0; n < 3; n++) {
        if (k < 3) lv[k * 3 + n] = 3;
        if (k > 0 && k - 1 < 3) lv[(k - 1) * 3 + n] = Math.max(lv[(k - 1) * 3 + n], 1);
      }
    } else if (state === 'tool') {
      // A held centre with one block turning around it, like a gear.
      lv[4] = 2;
      k = step % GRID_RING.length;
      lv[GRID_RING[k]] = 3;
      lv[GRID_RING[(k + GRID_RING.length - 1) % GRID_RING.length]] = 1;
    } else if (state === 'waiting') {
      // Still and neutral: the work is paused until you answer.
      lv = [1, 0, 1, 0, 2, 0, 1, 0, 1];
    } else {
      // Thinking: a short trail walks the grid and turns back at each end.
      for (n = 0; n < 3; n++) {
        k = GRID_PATH[(step - n + GRID_PATH.length * 4) % GRID_PATH.length];
        lv[k] = Math.max(lv[k], 3 - n);
      }
    }
    return lv;
  }
  function BuildGrid(p) {
    var size = num(p.size, 32);
    var state = GRID_STATES[p.state] ? p.state : 'thinking';
    var provider = p.provider === 'claude' ? 'claude' : 'codex';
    var still = on(p.still) || state === 'waiting';
    var stepState = React.useState(num(p.step, 2));
    var step = stepState[0], setStep = stepState[1];
    React.useEffect(function () {
      var reduce = window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
      if (still || reduce) return undefined;
      var ms = parseFloat(window.getComputedStyle(document.documentElement).getPropertyValue('--step-working')) || 160;
      var id = window.setInterval(function () { setStep(function (s) { return s + 1; }); }, ms);
      return function () { window.clearInterval(id); };
    }, [still, state]);
    var lv = gridLevels(state, step);
    var cell = size * 0.1875, gap = size * 0.09375, inset = (size - cell * 3 - gap * 2) / 2;
    var fills = ['var(--surface-raised)', 'var(--text-disabled)', 'var(--text-secondary)', providerVar(provider)];
    if (state === 'waiting') fills[3] = 'var(--text-secondary)';
    var rects = lv.map(function (l, i) {
      return h('rect', { key: i, x: inset + (i % 3) * (cell + gap), y: inset + Math.floor(i / 3) * (cell + gap), width: cell, height: cell, rx: cell * 0.28, fill: fills[l] });
    });
    return h('svg', {
      className: cx('bk-grid', p.className), width: size, height: size, viewBox: '0 0 ' + size + ' ' + size,
      role: 'img', 'aria-label': p.label || (providerName(provider) + ': ' + GRID_STATES[state])
    }, rects);
  }

  /* ---------- Streak label (proposed working treatment) ---------- */
  // The activity words themselves show the work: quiet text with a soft light
  // passing through once per --loop-streak, its core touched by the provider
  // colour. Soft and slow, so a native renderer can run it at a low frame rate
  // without visible judder. No separate moving mark.
  function StreakLabel(p) {
    var provider = p.provider === 'claude' ? 'claude' : 'codex';
    var waiting = p.state === 'waiting';
    var style = {};
    if (p.phase != null) {
      var dur = parseFloat(window.getComputedStyle(document.documentElement).getPropertyValue('--loop-streak')) || 2200;
      style.animationDelay = (-num(p.phase, 0) * dur) + 'ms';
    }
    return h('span', {
      className: cx('bk-streak', provider === 'claude' && 'bk-streak--claude', (on(p.still) || p.phase != null) && 'bk-streak--still', waiting && 'bk-streak--waiting', p.className),
      style: style
    }, p.children || p.text);
  }

  /* ---------- Working indicator ---------- */
  function WorkingIndicator(p) {
    var provider = p.provider === 'claude' ? 'claude' : 'codex';
    var state = p.state || 'thinking';
    var label = p.activity || ORB_STATES[state] || 'Thinking';
    var trail = list(p.trail).map(function (t) { return typeof t === 'string' ? t : t.text; });
    var streak = p.mark === 'streak';
    var mark = streak ? null : p.mark === 'grid'
      ? h(BuildGrid, { provider: provider, state: state, size: num(p.size, 32), still: p.still })
      : h(ThinkingOrb, { provider: provider, state: state, size: num(p.size, 32), still: p.still });
    var labelNode = streak
      ? h(StreakLabel, { key: label, className: 'bk-working__label', provider: provider, state: state, still: p.still, phase: p.phase }, label)
      : h('span', { key: label, className: 'bk-working__label' }, label);
    return h('div', { className: cx('bk-working', 'bk-working--' + state, streak && 'bk-working--streak'), role: 'status', 'aria-live': 'polite' },
      mark,
      h('div', { className: 'bk-working__text' },
        h('div', { className: 'bk-working__row' },
          labelNode,
          p.time ? h('span', { className: 'bk-working__time' }, p.time) : null),
        p.summary ? h('p', { key: p.summary, className: 'bk-working__summary' }, p.summary) : null,
        trail.length ? h('p', { className: 'bk-working__trail' }, trail.join(' · ')) : null));
  }

  /* ---------- To-do ---------- */
  function TodoList(p) {
    var provider = p.provider === 'claude' ? 'claude' : 'codex';
    var steps = list(p.steps);
    var done = steps.filter(function (s) { return s.state === 'done'; }).length;
    var pct = steps.length ? Math.round((done / steps.length) * 100) : 0;
    return h('section', { className: 'bk-todo', 'aria-label': (p.title || 'To-do') + ' for ' + (p.agent || providerName(provider)) },
      h('div', { className: 'bk-todo__head' },
        h('h2', { className: 'bk-todo__title' }, p.title || 'To-do'),
        h('span', { className: 'bk-todo__count' }, done + ' of ' + steps.length)),
      h('div', { className: 'bk-todo__agent' },
        h(ProviderMark, { provider: provider, size: 14 }),
        h('span', { style: { color: providerVar(provider) } }, p.agent || providerName(provider)),
        p.task ? h('span', { className: 'bk-todo__task' }, '· ' + p.task) : null),
      h('div', { className: 'bk-todo__bar', role: 'progressbar', 'aria-valuemin': 0, 'aria-valuemax': steps.length, 'aria-valuenow': done },
        h('span', { style: { width: pct + '%', background: providerVar(provider) } })),
      steps.length ? h('div', { className: 'bk-plan' }, steps.map(function (s, i) {
        return h(PlanStep, { key: i, state: s.state, title: s.title, detail: s.detail, provider: provider });
      })) : h('span', { className: 'bk-todo__empty' }, p.empty || 'No plan yet. Steps appear when the agent shares one.'));
  }

  /* ---------- Change strip ---------- */
  function ChangeStrip(p) {
    var expanded = on(p.expanded);
    var files = list(p.files);
    var added = p.added, removed = p.removed;
    var hasCounts = added !== undefined && added !== null && added !== '';
    var status = p.status ? h('span', { className: 'bk-strip__status' }, p.statusState ? h(StatusGlyph, { state: p.statusState, provider: p.provider, still: p.still }) : null, p.status) : h('span', null);
    var bar = h('div', { className: 'bk-strip__bar' }, status,
      hasCounts || files.length ? h('button', { type: 'button', className: 'bk-strip__changes', 'aria-expanded': expanded, 'aria-label': hasCounts ? (added + ' lines added, ' + removed + ' removed. Show changed files') : 'Show changed files' },
        hasCounts ? h('span', { className: 'bk-add' }, '+' + added) : h('span', null, files.length + ' files'),
        hasCounts ? h('span', { className: 'bk-del' }, '−' + removed) : null,
        h(Icon, { name: expanded ? 'chevron-up' : 'chevron-down', size: 14 })) : null);
    if (!expanded) return h('div', { className: 'bk-strip' }, bar);
    var panel = h('div', { className: 'bk-popover bk-changes', role: 'region', 'aria-label': 'Changed files' },
      h('div', { className: 'bk-changes__head' },
        h('span', { className: 'bk-changes__title' }, p.title || (files.length + ' changed files')),
        h(Button, { size: 'sm', variant: 'ghost', icon: 'folder' }, p.openLabel || 'Open folder')),
      files.map(function (f, i) {
        var path = String(f.path || '');
        var cut = path.lastIndexOf('/');
        return h('div', { key: i, className: 'bk-file' },
          h('span', { className: 'bk-file__icon' }, h(Icon, { name: 'file', size: 14 })),
          h('span', { className: 'bk-file__path', title: path }, cut >= 0 ? path.slice(0, cut + 1) : '', h('span', { className: 'bk-file__name' }, cut >= 0 ? path.slice(cut + 1) : path)),
          f.state ? h('span', { className: 'bk-file__state' }, f.state) : null,
          f.added !== undefined ? h('span', { className: 'bk-file__counts' }, h('span', { className: 'bk-add' }, '+' + f.added), h('span', { className: 'bk-del' }, '−' + (f.removed || 0))) : null,
          h(IconButton, { icon: 'external', label: 'Open ' + path + ' in editor', size: 'sm' }));
      }),
      p.note ? h('div', { className: 'bk-changes__note' }, p.note) : null);
    return h('div', { className: 'bk-strip' }, panel, bar);
  }

  /* ---------- Approval ---------- */
  function ApprovalCard(p) {
    var provider = p.provider === 'claude' ? 'claude' : 'codex';
    var kind = p.kind || 'command';
    var options = list(p.options);
    var title = p.title || (kind === 'edit' ? providerName(provider) + ' wants to edit files' : kind === 'question' ? providerName(provider) + ' has a question' : providerName(provider) + ' wants to run a command');
    return h('div', { className: 'bk-approval', role: 'group', 'aria-label': title },
      h('div', { className: 'bk-approval__head' },
        h(StatusGlyph, { state: 'waiting' }),
        h('span', { className: 'bk-approval__title' }, title),
        p.meta ? h('span', { className: 'bk-approval__meta' }, p.meta) : null),
      p.command ? h('pre', { className: 'bk-approval__cmd' }, p.command) : null,
      p.reason ? h('div', { className: 'bk-approval__why' }, p.reason) : null,
      options.length ? h('div', { className: 'bk-approval__options' }, options.map(function (o, i) {
        return h(Button, { key: i, variant: i === 0 ? 'primary' : 'secondary', kbd: String(i + 1), style: { justifyContent: 'space-between', width: '100%' } }, o);
      })) : h('div', { className: 'bk-approval__actions' },
        h(Button, { variant: 'primary', kbd: '↵' }, p.primaryLabel || 'Allow once'),
        p.secondaryLabel === '' ? null : h(Button, { variant: 'secondary' }, p.secondaryLabel || 'Allow for this chat'),
        h('span', { className: 'bk-approval__spacer' }),
        h(Button, { variant: 'ghost', kbd: 'Esc' }, p.denyLabel || 'Deny')));
  }

  /* ---------- Delegated work ---------- */
  var TASK_LABELS = { working: 'Working', waiting: 'Needs approval', done: 'Done', queued: 'Queued', failed: 'Failed', coordinating: 'Coordinating', revising: 'Revising', stopped: 'Stopped' };
  function TaskRow(p) {
    var provider = p.provider === 'claude' ? 'claude' : 'codex';
    var state = p.state || 'working';
    var attention = state === 'waiting' || on(p.attention);
    var glyph = state === 'working' || state === 'revising' ? h(StatusGlyph, { state: 'breathe', provider: provider, still: p.still })
      : state === 'coordinating' ? h(StatusGlyph, { state: 'active', provider: provider, label: 'Coordinating' })
      : state === 'waiting' ? h(StatusGlyph, { state: 'waiting' })
      : state === 'done' ? h(StatusGlyph, { state: 'done' })
      : state === 'failed' ? h(StatusGlyph, { state: 'failed' }) : null;
    var label = p.stateLabel || TASK_LABELS[state] || state;
    return h('button', {
      type: 'button', className: cx('bk-task', on(p.selected) && 'bk-task--selected', attention && !on(p.selected) && 'bk-task--attention', on(p.child) && 'bk-task--child'),
      'aria-current': on(p.selected) ? 'page' : undefined, 'aria-label': p.title + ', ' + providerName(provider) + ', ' + label
    },
      h('span', { className: 'bk-task__mark' }, h(ProviderMark, { provider: provider, size: 16 })),
      h('span', { className: 'bk-task__body' },
        h('span', { className: 'bk-task__title' }, p.title),
        h('span', { className: cx('bk-task__meta', attention && 'bk-task__meta--attention') },
          h('span', { style: { color: providerVar(provider), fontWeight: 400 } }, p.model || providerName(provider)), h('span', { 'aria-hidden': true }, '·'), h('span', null, label)),
        p.activity ? h('span', { className: 'bk-task__activity' }, p.activity) : null),
      glyph ? h('span', { className: 'bk-task__status' }, glyph) : null);
  }

  /* ---------- Menu ---------- */
  function Menu(p) {
    var items = list(p.items);
    return h('div', { className: 'bk-popover bk-menu', role: 'menu', style: { width: p.width ? num(p.width, 280) + 'px' : '280px' } },
      p.title ? h('div', { className: 'bk-menu__title' }, p.title) : null,
      items.map(function (it, i) {
        if (it.type === 'divider') return h('div', { key: i, className: 'bk-menu__divider', role: 'separator' });
        if (it.type === 'section') return h('div', { key: i, className: 'bk-menu__section' }, it.label);
        if (it.type === 'usage') return h('div', { key: i, style: { padding: '6px 10px 8px' } }, h(UsageMeter, it));
        if (it.type === 'note') return h('div', { key: i, className: 'bk-menu__note' }, it.icon ? h(Icon, { name: it.icon, size: 14 }) : null, h('span', null, it.label));
        return h('button', {
          key: i, type: 'button', role: it.check !== undefined ? 'menuitemradio' : 'menuitem', 'aria-checked': it.check !== undefined ? on(it.check) : undefined,
          'aria-disabled': on(it.disabled) || undefined, className: cx('bk-menu__item', on(it.danger) && 'bk-menu__item--danger', on(it.active) && 'bk-menu__item--active')
        },
          it.icon ? h('span', { className: 'bk-menu__icon' }, h(Icon, { name: it.icon })) : it.provider ? h('span', { className: 'bk-menu__icon' }, h(ProviderMark, { provider: it.provider })) : null,
          h('span', { className: 'bk-menu__main' }, h('span', null, it.label), it.detail ? h('span', { className: 'bk-menu__detail' }, it.detail) : null),
          h('span', { className: 'bk-menu__end' },
            it.tag ? h('span', { className: 'bk-tag' }, it.tag) : null,
            it.end ? h('span', null, it.end) : null,
            it.kbd ? h(Kbd, { keys: it.kbd }) : null,
            on(it.check) ? h(Icon, { name: 'check', color: 'var(--text-primary)' }) : null));
      }));
  }

  /* ---------- Setup ---------- */
  var ENGINE_STATES = { ready: ['done', 'Ready'], signin: ['waiting', 'Sign in needed'], missing: ['failed', 'Not found'], unsupported: ['unknown', 'Untested version'], checking: ['reconnecting', 'Checking'] };
  function EngineCard(p) {
    var provider = p.provider === 'claude' ? 'claude' : 'codex';
    var st = ENGINE_STATES[p.status || 'ready'] || ENGINE_STATES.ready;
    var lines = list(p.lines);
    var actions = list(p.actions);
    return h('div', { className: 'bk-engine', role: 'group', 'aria-label': (p.name || providerName(provider)) + ': ' + st[1] },
      h('div', { className: 'bk-engine__head' },
        h(ProviderMark, { provider: provider, size: 20 }),
        h('span', { className: 'bk-engine__name' }, p.name || providerName(provider)),
        h('span', { className: 'bk-engine__state' }, h(StatusGlyph, { state: st[0], still: p.still, label: st[1] }), st[1])),
      lines.length ? h('div', { className: 'bk-engine__lines' }, lines.map(function (l, i) { return h('span', { key: i, className: 'bk-engine__line' }, l); })) : null,
      p.text || p.children ? h('div', { className: 'bk-engine__text' }, p.text || p.children) : null,
      actions.length ? h('div', { className: 'bk-engine__actions' }, actions.map(function (a, i) {
        return h(Button, { key: i, size: 'sm', variant: a.variant || 'secondary', icon: a.icon, iconRight: a.iconRight }, a.label);
      })) : null);
  }

  var api = {
    Icon: Icon, ProviderMark: ProviderMark, StatusGlyph: StatusGlyph, StateChip: StateChip,
    Button: Button, IconButton: IconButton, Kbd: Kbd, Switch: Switch,
    SectionLabel: SectionLabel, ChatRow: ChatRow, ProjectRow: ProjectRow, UsageMeter: UsageMeter, ProfileRow: ProfileRow,
    UserMessage: UserMessage, AgentTurn: AgentTurn, PlanStep: PlanStep, ActivityLine: ActivityLine, CodeBlock: CodeBlock, Notice: Notice, TaskBrief: TaskBrief, EventLine: EventLine, Breadcrumb: Breadcrumb,
    Composer: Composer, PermissionControl: PermissionControl, ModelControl: ModelControl, ModelPicker: ModelPicker, EffortSlider: EffortSlider,
    ChangeStrip: ChangeStrip, ApprovalCard: ApprovalCard, ThinkingOrb: ThinkingOrb, BuildGrid: BuildGrid, StreakLabel: StreakLabel, WorkingIndicator: WorkingIndicator, TaskRow: TaskRow, TodoList: TodoList, Menu: Menu, EngineCard: EngineCard,
    iconNames: Object.keys(ICONS)
  };
  window.Bukno = Object.assign(window.Bukno || {}, api);
})();
