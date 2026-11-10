/* Standalone interaction prototype. No network requests or system integration. */
'use strict';
const $ = selector => document.querySelector(selector);
const escapeHTML = value => String(value ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const e = escapeHTML;
const clone = value => JSON.parse(JSON.stringify(value));
const pretty = value => JSON.stringify(value, null, 2);
const paths = {
  grid:'M3 3h7v7H3z M14 3h7v7h-7z M3 14h7v7H3z M14 14h7v7h-7z',
  file:'M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z M14 2v6h6 M8 13h8 M8 17h5',
  pulse:'M2 12h5l3-8 4 16 3-8h5', log:'M4 5h16 M4 12h16 M4 19h10',
  settings:'M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8 M9 3h6l1 3 3 1 2 5-2 5-3 1-1 3H9l-1-3-3-1-2-5 2-5 3-1z',
  power:'M12 2v9 M6 5a9 9 0 1 0 12 0', play:'M8 4l12 8-12 8z', stop:'M6 6h12v12H6z',
  refresh:'M20 7v5h-5 M4 17v-5h5 M6 6a8 8 0 0 1 14 6 M4 12a8 8 0 0 0 14 6',
  check:'M5 12l4 4L19 6', chevron:'M9 5l7 7-7 7', close:'M6 6l12 12 M6 18L18 6',
  globe:'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0 M3 12h18 M12 3a19 19 0 0 1 0 18 19 19 0 0 1 0-18',
  box:'M3 7l9-5 9 5v10l-9 5-9-5z M3 7l9 5 9-5 M12 12v10',
  arrow:'M4 12h16 M14 6l6 6-6 6', plus:'M12 5v14 M5 12h14',
  download:'M12 3v12 M7 10l5 5 5-5 M4 16v5h16v-5', copy:'M8 8h13v13H8z M16 8V3H3v13h5',
  edit:'M14 5l5 5 M3 21l5-1L21 7l-5-5L3 15z', trash:'M3 6h18 M9 6V3h6v3 M5 6l1 15h12l1-15 M10 10v7 M14 10v7',
  up:'M5 15l7-7 7 7', down:'M5 9l7 7 7-7', shield:'M12 2l9 4v6c0 5-9 10-9 10S3 17 3 12V6z M8 12l3 3 5-6',
  sun:'M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8 M12 2v2 M12 20v2 M2 12h2 M20 12h2 M5 5l1 1 M18 18l1 1 M5 19l1-1 M18 6l1-1',
  folder:'M3 6h7l2 2h9v12H3z', info:'M12 17v-6 M12 7h.01 M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0',
};
const icon = name => `<svg class="icon" viewBox="0 0 24 24" aria-hidden="true"><path d="${paths[name] || paths.info}"/></svg>`;
const btn = (text, action, style = '', disabled = false, extra = '') => `<button class="btn ${style}" data-action="${action}" ${disabled ? 'disabled' : ''} ${extra}>${text}</button>`;
const badge = (text, style = '') => `<span class="badge ${style}">${text}</span>`;
const info = (label, value) => `<div class="info-line"><span>${label}</span><span>${value}</span></div>`;
const field = (label, id, value, hint = '') => `<div class="field"><label for="${id}">${label}</label><input type="text" id="${id}" value="${e(value)}">${hint ? `<small>${hint}</small>` : ''}</div>`;
const jsonSeed = () => ({log:{loglevel:'warning'},inbounds:[{tag:'http-in',listen:'127.0.0.1',port:10809,protocol:'http'},{tag:'socks-in',listen:'127.0.0.1',port:10808,protocol:'socks',settings:{udp:true}}],outbounds:[{tag:'proxy',protocol:'vless',settings:{vnext:[{address:'example.test',port:443,users:[{id:'00000000-0000-4000-8000-000000000001',encryption:'none'}]}]},streamSettings:{network:'tcp',security:'tls'}},{tag:'direct',protocol:'freedom'},{tag:'block',protocol:'blackhole'}],routing:{domainStrategy:'AsIs',rules:[{type:'field',ip:['geoip:private'],outboundTag:'direct'},{type:'field',domain:['domain:github.com'],outboundTag:'proxy'}]}});
let sequence = 0;
let generation = 0;
let theme = 'system';
try { theme = localStorage.getItem('xray-pilot-prototype-theme') || 'system'; } catch (_) { /* file:// may block storage */ }
if (!['system','light','dark'].includes(theme)) theme = 'system';
const colorQuery = matchMedia('(prefers-color-scheme: dark)');
function applyTheme() { document.documentElement.dataset.theme = theme === 'system' ? (colorQuery.matches ? 'dark' : 'light') : theme; }
colorQuery.addEventListener('change', applyTheme);
applyTheme();
let state;
function profile(id, name, json = jsonSeed()) { const text = pretty(json); return {id,name,saved:text,draft:text,revision:0,history:[{label:'已保存版本',text}]}; }
function reset(scenario = 'normal') {
  generation++;
  const profiles = [profile('work','工作网络'),profile('travel','旅行网络'),profile('local','本地直连',{inbounds:[{tag:'http-in',listen:'127.0.0.1',port:10809,protocol:'http'}],outbounds:[{tag:'direct',protocol:'freedom'}]})];
  const first = scenario === 'first';
  state = {scenario,page:'overview',tab:'summary',section:'core',profiles:first?[]:profiles,selected:first?null:'work',runtime:first?null:{profileId:'work',name:'工作网络',json:profiles[0].saved,core:'v26.11.9'},running:!first,proxy:'none',offline:scenario==='offline',recovery:scenario==='recovery',uncertain:null,error:null,busy:null,download:false,installed:first?[]:['v26.11.9','v26.3.27'],settings:{core:first?'':'v26.11.9',login:false,connect:false},settingsDraft:{core:first?'':'v26.11.9',login:false,connect:false},settingsEpoch:0,wizard:first?0:null,hidden:null,tray:false,routeResult:null,checked:false,paused:false,logFilter:'',level:'all',logs:[],activity:[],candidate:null,modal:null,quitQueued:false};
  log('info',first?'欢迎使用，请先完成依赖安装。':'已连接后台，读取当前运行状态。');
  if (!first) activity('已载入工作网络','Core 已运行 · 系统代理未接管');
  $('#scenario').value = scenario;
  $('#modal').close();
  $('#toast-region').innerHTML = '';
  render();
}
function log(level, message, request = '') { state.logs.unshift({level,message,request,time:new Date().toLocaleTimeString('zh-CN',{hour12:false})}); }
function activity(title, note) { state.activity.unshift({title,note}); state.activity = state.activity.slice(0,4); }
function selected() { return state.profiles.find(p => p.id === state.selected); }
function dirty(p) { return p.draft !== p.saved; }
function locked() { return !!state.busy || state.offline || state.recovery || !!state.uncertain; }
function invalidate() { state.candidate = null; }
function editDraft(p, text) { p.draft = text; p.revision++; invalidate(); }
function toast(message) { const id = ++sequence; $('#toast-region').innerHTML = `<div class="toast" data-id="${id}">${e(message)}</div>`; setTimeout(() => { if ($('#toast-region .toast')?.dataset.id === String(id)) $('#toast-region').innerHTML = ''; },3800); }
function modal(title, body, footer, context = {}, wide = false) {
  state.modal = context;
  const node = $('#modal');
  node.className = wide ? 'wide' : '';
  node.innerHTML = `<div class="modal-header"><h2 id="modal-title">${title}</h2><button class="icon-button" data-action="close-modal" aria-label="关闭对话框">${icon('close')}</button></div><div class="modal-body"><div id="modal-error" class="modal-error" role="alert"></div>${body}</div><div class="modal-footer">${footer}</div>`;
  if (!node.open) node.showModal();
}
function closeModal() { $('#modal').close(); state.modal = null; }
function modalError(text) { $('#modal-error').textContent = text; }
function confirm(title, text, action, context = {}, button = '确认', style = 'primary') { modal(title,`<p>${text}</p>`,btn('取消','close-modal')+btn(button,action,style),context); }
async function job(title, finish) {
  if (state.busy) return;
  const token = generation;
  state.busy = title;
  render();
  await new Promise(resolve => setTimeout(resolve,850));
  if (token !== generation) return;
  state.busy = null;
  finish();
  render();
  if (state.quitQueued) { state.quitQueued = false; fullExit(); }
}
const pageNames = {overview:'总览',profiles:'配置',diagnostics:'诊断',logs:'日志',settings:'设置'};
function render() {
  if (state.hidden) { renderClosed(); } else {
    $('#app').innerHTML = `<div class="shell"><aside class="sidebar"><div class="brand"><div class="brand-mark">${icon('pulse')}</div><div><div class="brand-name">Xray Pilot</div><div class="brand-sub">YOUR LOCAL CONNECTION</div></div></div><div><div class="nav-label">工作空间</div><nav class="nav" aria-label="主导航">${Object.entries(pageNames).map(([key,name],i)=>`<button data-action="navigate" data-page="${key}" class="${state.page===key?'active':''}" ${state.page===key?'aria-current="page"':''} title="${name}">${icon(['grid','file','pulse','log','settings'][i])}<span>${name}</span>${key==='profiles'?`<span class="nav-count">${state.profiles.length}</span>`:''}</button>`).join('')}</nav></div><div class="sidebar-bottom"><div class="workspace"><div class="workspace-title">${icon('folder')} 本机工作空间</div><p class="small muted">数据存储在安装目录 / data</p></div><div class="sidebar-links"><span>已确认设计 · 参考稿</span><span>Web 预览</span></div></div></aside><main class="main"><header class="topbar"><div class="breadcrumbs">工作空间 ${icon('chevron')} <strong>${pageNames[state.page]}</strong></div><div class="row gap-sm"><span class="connection"><span class="dot ${state.offline?'bad':'good'}"></span> ${state.offline?'后台连接中断':'后台已连接'}</span><button class="icon-button" data-action="quick-theme" aria-label="切换浅色或深色显示" title="切换浅色 / 深色">${icon('sun')}</button><button class="icon-button" data-action="hide" aria-label="收起到托盘" title="关闭窗口，收起到托盘">${icon('close')}</button></div></header><div class="content">${alerts()}${state.wizard!==null&&state.page==='overview'?renderWizard():({overview:renderOverview,profiles:renderProfiles,diagnostics:renderDiagnostics,logs:renderLogs,settings:renderSettings}[state.page])()}</div></main></div>`;
  }
  $('#task-region').innerHTML = state.busy ? `<div class="task-panel"><div class="row"><span class="spinner"></span><strong>${e(state.busy)}</strong></div><div class="progress"><i style="width:65%"></i></div><p>${state.quitQueued?'完成当前操作后将停止 Core、恢复代理并退出。':'操作进行中，可继续浏览页面。所有结果均为演示。'}</p></div>` : '';
  renderTray();
}
function alerts() {
  if (state.offline) return `<div class="alert bad"><div><strong>无法连接后台</strong>下方保留最后一次已知状态，当前进程与代理状态尚未确认。写操作暂不可用。</div>${btn('重新连接','reconnect','small')}</div>`;
  if (state.recovery) return `<div class="alert"><div><strong>需要处理上次未完成的操作</strong>检测到代理设置被其他程序修改。请检查恢复证据，再确认是否解除限制。</div>${btn('检查恢复状态','recovery','small')}</div>`;
  if (state.uncertain) return `<div class="alert"><div><strong>应用结果待确认，请勿重复提交</strong>后台响应中断，运行状态仍显示上次确认结果。请求：<code>${e(state.uncertain.request)}</code></div>${btn('查询操作结果','query','small',!!state.busy)}</div>`;
  if (state.error) return `<div class="alert bad"><div><strong>配置未能应用，原配置仍在运行</strong>${e(state.error)}</div>${btn('查看原因','failure-detail','small')}</div>`;
  if (state.scenario==='failure') return `<div class="alert neutral"><div><strong>应用失败演示</strong>在「配置」中检查并确认应用，观察失败反馈与原运行配置保留。</div></div>`;
  if (state.scenario==='unknown') return `<div class="alert neutral"><div><strong>结果待确认演示</strong>确认应用后将模拟响应中断，可按请求身份查询结果。</div></div>`;
  return '';
}
function heading(title, subtitle, actions = '') { return `<div class="page-heading"><div><h1>${title}</h1><p>${subtitle}</p></div>${actions}</div>`; }
function renderOverview() {
  const r = state.runtime;
  const status = state.offline ? '状态待确认' : state.running ? 'Core 运行中' : 'Core 已停止';
  let endpoint = '尚未配置';
  if (r) { const inbound = JSON.parse(r.json).inbounds?.find(i=>i.protocol==='http'); if(inbound) endpoint = `${inbound.listen||'0.0.0.0'}:${inbound.port}`; }
  return `${heading('总览','关注运行状态，把常用操作留在手边。',btn(icon('file')+'管理配置','go-profiles'))}<div class="stack"><section class="card hero"><div class="hero-main"><div class="status-symbol">${icon(state.running?'check':'power')}</div><div><span class="small muted">XRAY CORE</span><div class="hero-state">${status}</div><div class="hero-meta">${r?`${e(r.core)} · ${e(r.name)} · ${state.offline?'最后已知状态':'进程状态已确认'}`:'选择一份配置，检查后开始运行'}</div></div></div><div class="hero-actions">${state.running?btn(icon('stop')+'停止','stop','',state.offline||!!state.busy)+btn(icon('refresh')+'重启','restart','',locked()):btn(icon('play')+'选择配置并启动','go-profiles','primary',locked())}</div></section><div class="metrics"><div class="card metric"><div class="metric-top">当前运行配置 ${icon('file')}</div><div class="metric-value">${state.running&&r?e(r.name):'无运行配置'}</div><div class="metric-note">${state.running?'编辑草稿不会改变此处的运行版本':'应用配置后将在此显示'}</div></div><div class="card metric"><div class="metric-top">本机 HTTP 入口 ${icon('globe')}</div><div class="metric-value mono">${e(endpoint)}</div><div class="metric-note">${state.running?'进程运行中 · 实际网络未检测':'当前未运行'}</div></div><div class="card metric"><div class="metric-top">系统代理 ${icon('shield')}</div><div class="metric-value">${state.offline?'状态待确认':state.proxy==='http'?'由本程序接管':'未由本程序接管'}</div><div class="metric-note">${state.proxy==='http'?'指向已验证的本机 HTTP 入口':'系统可能仍使用其他程序的代理'}</div></div></div><div class="two-col"><section class="card"><div class="card-title"><h2>连接方式</h2>${badge('仅影响系统代理','accent')}</div><div class="segmented" role="group" aria-label="系统代理方式"><button data-action="proxy-none" class="${state.proxy==='none'?'selected':''}" aria-pressed="${state.proxy==='none'}" ${locked()?'disabled':''}>不接管系统代理</button><button data-action="proxy-http" class="${state.proxy==='http'?'selected':''}" aria-pressed="${state.proxy==='http'}" ${locked()||!state.running?'disabled':''}>使用本机 HTTP 代理</button></div><p class="hint mt">解除接管时恢复接管前的设置；其他程序的代理设置可能仍然有效。是否可访问目标网站，需要另行检测。</p>${info('网络连通性',badge('未检测'))}${info('实际出口',badge('未验证'))}<div class="mt">${btn('打开路由诊断 '+icon('arrow'),'go-diagnostics','text')}</div></section><section class="card"><div class="card-title"><h2>最近活动</h2>${btn('查看日志','go-logs','text small')}</div><div class="activity">${state.activity.map(a=>`<div class="activity-item"><span class="dot good"></span><div>${e(a.title)}<p>${e(a.note)}</p></div><time>刚刚</time></div>`).join('')||'<p class="muted small">暂无活动</p>'}</div><div class="divider"></div>${info('运行中的 Core',e(state.running&&r?r.core:'未运行'))}${info('下次应用使用',e(state.settings.core||'尚未安装'))}</section></div></div>`;
}
function parseDraft(p) { try { const data = JSON.parse(p.draft); return data && typeof data === 'object' && !Array.isArray(data) ? data : null; } catch (_) { return null; } }
const tabNames = {summary:'概览',inbounds:'入站',outbounds:'出站',routing:'路由',json:'原始 JSON',history:'历史'};
function renderProfiles() {
  const p = selected();
  return `${heading('配置','选择、编辑与运行互相独立。每次应用前，都先检查并确认变更。',btn(icon('download')+'导入配置','import','',locked()))}<div class="profile-layout"><aside class="profile-list"><div class="list-heading row between"><span>我的配置</span><span>${state.profiles.length}</span></div>${state.profiles.map(item=>`<button data-action="select-profile" data-id="${item.id}" class="profile-item ${item.id===state.selected?'active':''}" aria-pressed="${item.id===state.selected}"><strong>${e(item.name)}</strong><div class="row">${state.runtime?.profileId===item.id&&state.running?badge('运行中','good'):badge('未运行')}${dirty(item)?badge('有草稿','warn'):''}</div></button>`).join('')}${btn(icon('plus')+'新建','new-profile','',locked())}</aside>${p?`<section class="card editor-card"><div class="editor-heading"><div class="row between wrap"><div class="editor-name"><h2>${e(p.name)}</h2><span id="draft-badge">${badge(dirty(p)?'未应用草稿':'已保存',dirty(p)?'warn':'')}</span></div><div class="row gap-sm">${btn('复制','duplicate','small',locked())}${btn('删除','delete-profile','small',locked())}</div></div><p>正在编辑：草稿 · ${state.runtime?.profileId===p.id&&state.running?'运行中的版本保持不变':'此配置当前未运行'}</p></div><div class="tabs" role="tablist" aria-label="配置编辑">${Object.entries(tabNames).map(([key,label])=>`<button role="tab" id="tab-${key}" aria-controls="profile-panel" aria-selected="${state.tab===key}" data-action="tab" data-tab="${key}" class="${state.tab===key?'active':''}">${label}</button>`).join('')}</div><div class="editor-body" id="profile-panel" role="tabpanel" aria-labelledby="tab-${state.tab}">${renderEditor(p)}</div><footer class="editor-footer"><p class="hint">${state.running?`当前运行：${e(state.runtime.name)}。`:'Core 当前已停止。'}<br>草稿仅保留在本次预览中，检查通过后仍需确认应用。</p><div class="row gap-sm">${btn('放弃草稿','discard','',locked()||!dirty(p))}${btn(icon('check')+'检查并预览','validate','primary',locked())}</div></footer></section>`:`<section class="card empty">${icon('file')}<h2>添加第一份配置</h2><p>导入原生 JSON，或创建一个本地直连模板。不会自动启动或修改系统代理。</p>${btn('新建配置','new-profile','primary',locked())}</section>`}</div>`;
}
function renderEditor(p) {
  const data = parseDraft(p);
  if (state.tab==='json') return `<div class="code-toolbar"><span class="small muted">原生 Xray JSON · 未识别字段保留</span>${btn('格式化','format','small',locked())}</div><label for="json-editor" class="small muted">配置草稿</label><textarea class="code" id="json-editor" spellcheck="false" ${locked()?'disabled':''}>${e(p.draft)}</textarea><p class="table-note" id="json-status">${data?'JSON 语法可解析，尚未执行 Core 校验。':'JSON 语法错误或根节点不是对象，请修正后再检查。'}</p>`;
  if (state.tab==='history') return p.history.map((h,i)=>`<div class="history-item"><div><h3>${e(h.label)}</h3><p>载入历史仅替换编辑草稿，仍需重新检查并应用。</p></div>${btn('载入为草稿','history','small',locked(),`data-index="${i}"`)}</div>`).join('');
  if (!data) return `<div class="empty">${icon('info')}<h3>请先修正原始 JSON</h3><p>结构化视图无法读取当前草稿，已有内容会保留。</p>${btn('打开原始 JSON','open-json','primary')}</div>`;
  if (state.tab==='summary') return `<div class="field-pair"><div class="label-value"><span>入站入口</span><strong>${data.inbounds?.length||0} 个</strong></div><div class="label-value"><span>出站连接</span><strong>${data.outbounds?.length||0} 个</strong></div><div class="label-value"><span>路由规则</span><strong>${data.routing?.rules?.length||0} 条 · 按原生顺序</strong></div><div class="label-value"><span>默认出站</span><strong>${e(data.outbounds?.[0]?.tag||'未设置')}</strong></div></div><div class="divider"></div><h3>编辑不会立即切换连接</h3><p class="hint mt">先在入站、出站或路由中调整配置，再通过「检查并预览」查看完整 JSON 与运行上下文。应用成功后，运行版本才会更新。</p><div class="alert neutral mt"><div><strong>示例地址无法连接真实服务</strong>工作网络使用 example.test 演示。此页面只展示交互，不执行真实 Core 校验或网络请求。</div></div>${btn('编辑原始 JSON '+icon('arrow'),'open-json','text')}`;
  const list = state.tab==='routing' ? data.routing?.rules : data[state.tab];
  const rows = Array.isArray(list) ? list : [];
  if (rows.some(item=>!item||typeof item!=='object'||Array.isArray(item))) return `<div class="empty">${icon('info')}<h3>列表包含不合法的条目</h3><p>每个条目都应为 JSON 对象，请在原始 JSON 中修正。</p>${btn('打开原始 JSON','open-json','primary')}</div>`;
  return `<div class="card-title"><h3>${tabNames[state.tab]}列表</h3>${btn(icon('plus')+'添加','add-entry','small',locked())}</div><div class="table-wrap"><table><thead><tr><th>顺序</th><th>${state.tab==='routing'?'匹配条件':'标识 / 协议'}</th><th>${state.tab==='inbounds'?'监听地址':state.tab==='routing'?'目标出站':'协议'}</th><th>操作</th></tr></thead><tbody>${rows.map((item,i)=>`<tr><td class="muted">${String(i+1).padStart(2,'0')}</td><td><strong>${e(state.tab==='routing'?[].concat(item.domain||item.ip||item.network||'其他条件').join(', '):item.tag||'无标识')}</strong>${state.tab==='inbounds'?`<p class="muted small">${e(item.protocol)}</p>`:''}</td><td class="mono">${e(state.tab==='inbounds'?`${item.listen||'0.0.0.0'}:${item.port}`:state.tab==='routing'?item.outboundTag||item.balancerTag||'未指定':item.protocol)}</td><td><div class="row gap-sm">${rowButton('edit','编辑条目','edit-entry',i)}${rowButton('up','向上移动','move-up',i,i===0)}${rowButton('down','向下移动','move-down',i,i===rows.length-1)}${rowButton('trash','删除条目','remove-entry',i)}</div></td></tr>`).join('')}</tbody></table></div>${!rows.length?'<p class="table-note">暂无条目，可添加或在原始 JSON 中编辑。</p>':''}<p class="table-note">${state.tab==='routing'?'按顺序匹配；静态预测不代表流量已通过该出站。':state.tab==='outbounds'?'第一项是无规则命中时的默认出站；调整顺序也会改变默认行为。':'系统代理接管前，需要选择并验证兼容的本机 HTTP 入口。'} 条目编辑保留未识别字段。</p>`;
}
function rowButton(name,label,action,index,disabled=false) { return `<button class="icon-button" aria-label="${label} ${index+1}" title="${label}" data-action="${action}" data-index="${index}" ${disabled||locked()?'disabled':''}>${icon(name)}</button>`; }
function renderDiagnostics() {
  return `${heading('诊断','先看本机状态，再检查规则；静态预测与实际连接分别呈现。',btn(icon('refresh')+'重新检查','diagnose','',state.offline||!!state.busy))}<div class="two-col"><section class="card"><div class="card-title"><h2>本机检查</h2>${badge(state.checked?'模拟检查完成':'尚未检查',state.checked?'good':'')}</div>${[['后台连接',state.offline?'连接中断，无法读取最新状态':'可读取状态与操作结果'],['Core 进程',state.offline?'最后已知运行状态':state.running?'进程正在运行':'进程未运行'],['系统代理归属',state.proxy==='http'?'由本程序接管':'本程序未接管，不代表系统代理关闭'],['真实网络与出口','未验证；此原型不发出网络请求']].map(([title,note],i)=>`<div class="check-row">${icon(i===3||state.offline?'info':'check')}<div><h3>${title}</h3><p>${e(note)}</p></div>${badge(i===3?'未验证':state.offline?'待确认':state.checked?'已检查':'状态快照',i===3?'':state.offline?'warn':'good')}</div>`).join('')}</section><section class="card"><div class="card-title"><h2>路由静态预测</h2>${badge('演示模型','accent')}</div>${field('目标域名或 IP','route-target','github.com')}${field('目标端口','route-port','443')}<div class="field"><label for="route-network">传输协议</label><select class="control" id="route-network"><option>tcp</option><option>udp</option></select></div>${btn('预测规则匹配','route-test','primary',!state.runtime||state.offline||!!state.busy)}<div id="route-result">${state.routeResult||'<p class="hint mt">使用当前运行配置，按原生规则顺序进行静态判断。域名解析、sniffing 或未支持条件可能使结果未知。</p>'}</div></section></div>`;
}
function visibleLogs() { return (state.paused ? state.frozenLogs : state.logs).filter(l=>(state.level==='all'||l.level===state.level)&&`${l.message} ${l.request}`.toLowerCase().includes(state.logFilter.toLowerCase())); }
function renderLogs() {
  const rows = visibleLogs();
  return `${heading('日志','当前演示会话的操作记录；刷新页面后清空。',btn(icon('copy')+'复制筛选结果','copy-logs'))}<section class="card"><div class="log-toolbar"><label class="small muted" for="log-search">搜索</label><input type="search" id="log-search" placeholder="搜索消息或请求 ID…" value="${e(state.logFilter)}"><select class="control" id="log-level" aria-label="日志级别">${[['all','全部级别'],['info','信息'],['warn','警告'],['error','错误']].map(([v,t])=>`<option value="${v}" ${state.level===v?'selected':''}>${t}</option>`).join('')}</select>${btn(state.paused?'继续显示':'暂停显示','pause-logs','small')}<span class="small muted">${rows.length} 条</span></div><div class="log-pane" tabindex="0" aria-label="日志列表">${rows.map(l=>`<div class="log-line ${l.level}"><time>${e(l.time)}</time><span class="log-level">${l.level.toUpperCase()}</span><div class="log-message">${e(l.message)}${l.request?`<details><summary>请求详情</summary><pre>request_id: ${e(l.request)}
source: simulated-operation
该记录仅用于交互演示。</pre></details>`:''}</div></div>`).join('')||'<p class="muted">没有匹配的日志。</p>'}</div><p class="table-note">${state.paused?'暂停期间保留已显示列表，恢复后显示最新记录。':'仅展示本原型生成的事件，不读取真实 Core 或后台日志。'}</p></section>`;
}
const settingSections = {core:'Core 与依赖',startup:'启动行为',appearance:'外观',storage:'数据与存储'};
function renderSettings() {
  return `${heading('设置','管理依赖、启动方式与显示偏好。',state.section==='appearance'||state.section==='storage'?'':btn('保存设置','save-settings','primary',locked()))}<div class="settings-layout"><nav class="settings-nav" aria-label="设置分类">${Object.entries(settingSections).map(([key,name])=>`<button data-action="settings-section" data-section="${key}" class="${state.section===key?'active':''}" ${state.section===key?'aria-current="true"':''}>${name}</button>`).join('')}</nav><section class="card">${renderSettingBody()}</section></div>`;
}
function renderSettingBody() {
  const s = state.settingsDraft;
  if (state.section==='appearance') return `<h2>显示风格</h2><p class="hint mt">即时生效，并记住本浏览器的选择。跟随系统会响应系统主题变化。</p><div class="theme-options">${[['light','浅色'],['dark','深色'],['system','跟随系统']].map(([key,label])=>`<button class="theme-choice ${theme===key?'active':''}" data-action="theme" data-theme="${key}" aria-pressed="${theme===key}"><div class="theme-thumb ${key}"><i></i><span></span></div><p>${label} ${theme===key?'✓':''}</p></button>`).join('')}</div><div class="divider"></div><h3>清晰、克制的桌面界面</h3><p class="hint mt">所有页面使用同一套中文排版、状态语义与键盘焦点。状态同时使用文字和图标，不仅依赖颜色。</p>`;
  if (state.section==='storage') return `<h2>数据与存储</h2><p class="hint mt">Windows 默认在程序安装目录下的 data 中保存配置、日志和运行记录。</p><div class="label-value mt"><span>当前数据目录（示例）</span><strong class="mono">D:\\Apps\\xray-pilot\\data</strong></div><div class="mt">${btn(icon('copy')+'复制目录','copy-path')}</div><div class="divider"></div><p class="hint">请确保安装目录可写；更换目录需退出程序后，通过启动参数指定。此设计不提供运行中热切换目录。</p><p class="hint mt">本网页不读写该目录。模拟配置只保留到页面刷新，主题选择单独保存在浏览器中。</p>`;
  if (state.section==='startup') return `<h2>启动行为</h2><p class="hint mt">登录自启与自动连接独立设置。自动连接仅使用上次已成功应用的版本。</p>${toggle('登录 Windows 后启动界面','不自动等同于启动 Core。','login',s.login)}${toggle('程序启动后自动连接','没有已应用配置时，进入首次设置；恢复异常优先处理。','connect',s.connect)}<p class="hint mt">变更后点击「保存设置」。本原型不会创建系统计划任务。</p>`;
  return `<div class="card-title"><h2>Core 与依赖</h2>${badge('官方来源','good')}</div><p class="hint">首次安装时引导下载 Xray Core 及所需资源；使用已有 Core 时也需检查版本与资源完整性。</p><div class="field mt"><label for="core-choice">下次检查 / 应用使用的 Core</label><select id="core-choice" class="control" ${locked()?'disabled':''}>${!state.installed.length?'<option value="">尚未安装</option>':''}${state.installed.map(v=>`<option value="${v}" ${s.core===v?'selected':''}>${v} · 托管版本</option>`).join('')}</select><small>保存选择不会立即替换运行中的 Core；需重新检查配置并确认应用。</small></div>${info('当前运行',e(state.runtime&&state.running?state.runtime.core:'未运行'))}${info('已保存的下次使用版本',e(state.settings.core||'未设置'))}<div class="divider"></div><div class="row between wrap"><div><h3>下载与安装</h3><p class="hint">v26.11.9 · Windows x64 · 示例固定版本</p></div>${btn(icon('download')+(state.download?'正在安装…':'模拟下载安装'),'download','',state.download||locked())}</div>${state.download?`<div class="install-progress"><div class="row"><span class="spinner"></span>下载 → 校验 → 安装（模拟）</div><div class="progress"><i style="width:65%"></i></div></div>`:''}<p class="hint mt">来源：<a href="https://github.com/XTLS/Xray-core/releases" target="_blank" rel="noreferrer">XTLS / Xray-core 官方发布页 ↗</a><br>发布包及校验信息来自官方渠道。本页不实际下载文件。</p><div class="divider"></div><h3>已安装版本</h3>${state.installed.map(v=>`<div class="install-item">${icon('box')}<div><strong class="small">Xray Core ${v}</strong><p>Core · geoip.dat · geosite.dat（示例清单）</p></div>${btn(s.core===v?'已选择':'选用','use-core','small',locked()||s.core===v,`data-version="${v}"`)}</div>`).join('')||'<p class="hint mt">尚无托管版本。</p>'}<p class="table-note">资源下载与完整性校验在本页仅演示交互，不表示已进行真实供应链校验。</p>`;
}
function toggle(title, note, key, checked) { return `<div class="setting-row"><div><h3 id="label-${key}">${title}</h3><p>${note}</p></div><input type="checkbox" class="switch" data-setting="${key}" aria-labelledby="label-${key}" ${checked?'checked':''} ${locked()?'disabled':''}></div>`; }
function renderWizard() {
  const step = state.wizard;
  return `<div class="wizard"><div class="steps">${['准备依赖','添加配置','检查并启动'].map((name,i)=>`<div class="step ${i===step?'active':i<step?'done':''}"><span>${i<step?'✓':i+1}</span>${name}</div>`).join('')}</div><section class="card"><div class="welcome-symbol">${icon(['download','file','check'][step])}</div>${step===0?`<h2>从准备依赖开始</h2><p class="wizard-intro">自动管理 Core 和常用资源，减少第一次安装时的手动操作。</p><div class="wizard-template"><h3>Xray Core v26.11.9 · Windows x64</h3><p>来源：XTLS / Xray-core 官方 GitHub Releases。演示下载、文件校验与安装状态，不实际访问网络。</p></div>${state.installed.length?badge('依赖准备完成（模拟）','good'):btn(state.download?'下载、校验、安装中…':'从官方来源安装（模拟）','download','primary',state.download)}<p class="hint mt"><a href="https://github.com/XTLS/Xray-core/releases" target="_blank" rel="noreferrer">打开官方发布页 ↗</a></p>`:step===1?`<h2>添加你的第一份配置</h2><p class="wizard-intro">可导入服务商提供的原生 JSON，或使用直连模板确认本机运行流程。</p><div class="wizard-template"><h3>本地直连模板</h3><p>只监听 127.0.0.1:10809，不包含远端服务地址。创建后仍需检查与确认启动。</p></div>${state.profiles.length?badge('配置已准备：'+e(selected().name),'good'):btn('使用直连模板','wizard-template','primary')+' '+btn('粘贴 JSON 导入','import')}`:`<h2>确认后再启动</h2><p class="wizard-intro">先检查依赖与配置，在预览中确认启动内容。</p>${info('配置',e(selected()?.name||'未选择'))}${info('Core',e(state.settings.core))}${info('系统代理',badge('不接管系统代理'))}<p class="hint mt">启动 Core 不代表所有程序已使用代理，也不代表网络连通性已经验证。</p>${btn('检查并预览','validate','primary mt',locked())}`}<footer class="wizard-footer">${step>0?btn('上一步','wizard-back','',!!state.busy):'<span class="hint">约 3 个步骤 · 不修改真实系统</span>'}${step<2?btn('下一步 '+icon('arrow'),'wizard-next','',step===0?!state.installed.length:!state.profiles.length):'<span class="hint">预览后确认启动</span>'}</footer></section></div>`;
}
function renderClosed() {
  const kind = state.hidden;
  const text = kind==='hidden'?['窗口已收起到托盘','Core 与后台继续运行。可从右上角「托盘预览」重新打开窗口。','打开窗口']:kind==='gui'?['界面已退出，后台继续运行','托盘随界面退出。再次启动界面后，可连接原后台并查看运行状态。','重新打开界面']:['程序已退出','Core 与后台已停止，本程序接管的代理已释放。当前仅展示退出后的效果。','重新开始演示'];
  $('#app').innerHTML = `<div class="closed-stage"><div class="card"><div class="status-symbol">${icon(kind==='full'?'power':'check')}</div><h1>${text[0]}</h1><p>${text[1]}</p>${btn(text[2],'reopen','primary')}</div></div>`;
}
function renderTray() {
  const available = state.hidden!=='gui' && state.hidden!=='full';
  $('#tray-trigger').disabled = !available;
  $('#tray-trigger').setAttribute('aria-expanded',String(state.tray&&available));
  const menu = $('#tray-menu'); menu.hidden = !state.tray||!available;
  menu.innerHTML = `<div class="tray-status"><strong>Xray Pilot</strong><p>${state.offline?'后台断开 · 状态待确认':state.running?'Core 运行中':'Core 已停止'} · ${state.proxy==='http'?'已接管代理':'未接管代理'}</p></div>${trayButton('打开主窗口','tray-open','grid')}${trayButton(state.running?'停止 Core':'选择配置并启动',state.running?'stop':'tray-start',state.running?'stop':'play',state.offline||!!state.busy)}<div class="menu-separator"></div>${trayButton(state.proxy==='http'?'恢复接管前的代理设置':'使用本机 HTTP 代理',state.proxy==='http'?'proxy-none':'proxy-http','globe',locked()||(!state.running&&state.proxy!=='http'))}<div class="menu-separator"></div>${trayButton('仅关闭界面，后台继续运行','gui-exit','close',!!state.busy)}${trayButton('退出程序（停止 Core 并恢复代理）','exit','power',state.offline,false,true)}`;
}
function trayButton(text, action, name, disabled=false, unused=false, danger=false) { return `<button role="menuitem" data-action="${action}" ${disabled?'disabled':''} class="${danger?'danger':''}">${icon(name)}${text}</button>`; }
function validationErrors(p) {
  const data = parseDraft(p);
  if (!data) return 'JSON 语法错误，或根节点不是对象。请在原始 JSON 中修正。';
  if (!Array.isArray(data.outbounds)||!data.outbounds.length) return '至少需要一个出站配置。请先添加出站。';
  if (data.inbounds!==undefined&&!Array.isArray(data.inbounds)) return 'inbounds 必须是数组。';
  if (data.routing?.rules!==undefined&&!Array.isArray(data.routing.rules)) return 'routing.rules 必须是数组。';
  if (data.inbounds?.some(i=>!i||typeof i!=='object'||!Number.isInteger(i.port)||i.port<1||i.port>65535)) return '入站端口应为 1–65535 的整数。';
  if (data.outbounds.some(o=>!o||typeof o!=='object'||typeof o.protocol!=='string')) return '每个出站需要协议字段。';
  if (!state.settings.core) return '尚未选择 Core，请到设置中准备依赖并保存选择。';
  return null;
}
function validate() {
  if (locked()||!selected()) return;
  const p = selected(), error = validationErrors(p);
  if (error) { modal('配置检查未通过',`<div class="alert bad"><div>${e(error)}</div></div><p>运行中的配置未改变。此处只进行浏览器内的基础结构检查。</p>`,btn('返回编辑','close-modal','primary')); log('warn',error); return; }
  const candidate = {profileId:p.id,name:p.name,json:p.draft,revision:p.revision,core:state.settings.core,proxy:state.proxy,settingsEpoch:state.settingsEpoch,request:`demo-${++sequence}`};
  job('正在检查配置（模拟）',()=>{state.candidate=candidate; preview(candidate); log('info','基础结构检查完成，等待用户确认应用。',candidate.request);});
}
function preview(c) {
  const data = JSON.parse(c.json), old = state.runtime?.json || '尚无运行配置';
  modal('预览本次应用',`<p>以下内容将作为同一份运行版本应用。确认后才会切换 Core 与配置。</p><div class="review-grid"><div class="label-value"><span>目标配置</span><strong>${e(c.name)}</strong></div><div class="label-value"><span>使用 Core</span><strong>${e(c.core)}</strong></div><div class="label-value"><span>系统代理</span><strong>${c.proxy==='http'?'继续使用本机 HTTP 代理':'不接管系统代理'}</strong></div><div class="label-value"><span>配置内容</span><strong>${data.inbounds?.length||0} 个入站 · ${data.outbounds.length} 个出站</strong></div></div><div class="diff-grid mt"><div class="diff-column"><h3>当前运行版本</h3><pre>${e(old)}</pre></div><div class="diff-column"><h3>待应用草稿</h3><pre>${e(c.json)}</pre></div></div><div class="review-note">演示只检查 JSON 基础结构，不执行官方 Core 的 run -test、资源哈希检查或网络握手。正式界面应展示真实校验结果和冻结资源清单。</div><details><summary>依赖与操作详情</summary><pre>request_id: ${e(c.request)}
Core: ${e(c.core)}
路由资源: ${c.json.includes('geoip:')?'geoip.dat（演示）':'无额外路由资源'}
实际出口: 未验证</pre></details>`,btn('返回编辑','close-modal')+btn(state.running?'确认应用':'确认并启动','apply','primary'),{type:'preview'},true);
}
function commitCandidate(c) {
  const p = state.profiles.find(item=>item.id===c.profileId);
  state.runtime = {profileId:c.profileId,name:c.name,json:c.json,core:c.core}; state.running=true; state.error=null;
  if (p) { if(p.saved!==c.json) p.history.unshift({label:`应用前保存的版本 · ${p.history.length}`,text:p.saved}); p.saved=c.json; }
  activity('配置已应用',`${c.name} · ${c.core}`); log('info',`配置 ${c.name} 已应用（模拟）。`,c.request); state.wizard=null;
}
function applyCandidate() {
  const c = state.candidate, p = selected();
  if (!c||!p||c.profileId!==p.id||c.revision!==p.revision||c.core!==state.settings.core||c.proxy!==state.proxy||c.settingsEpoch!==state.settingsEpoch) { modalError('编辑内容或运行上下文已变化，请返回重新检查。'); return; }
  closeModal(); state.candidate=null;
  job('正在应用配置（模拟）',()=>{
    if (state.scenario==='failure') { state.error='本地 HTTP 入口端口冲突。请检查占用情况、调整端口，然后重新检查。';log('error',state.error,c.request);activity('应用失败','原运行版本与代理归属保持不变'); modal('配置未能应用',`<div class="alert bad"><div><strong>本地入口端口冲突</strong>示例错误：127.0.0.1:10809 已被其他程序占用。</div></div><p>原运行版本保留。可修改端口后重新检查；本场景每次应用均模拟失败。</p><details><summary>技术详情</summary><pre>code: HTTP_READY_FAILED
request_id: ${e(c.request)}
recovery: previous runtime retained (simulated)</pre></details>`,btn('查看日志','error-logs')+btn('返回编辑','close-modal','primary')); }
    else if (state.scenario==='unknown') {state.uncertain={...c}; log('warn','连接在应用响应前中断，结果需要按请求身份查询。',c.request); toast('结果待确认：请查询操作结果，避免重复提交。');}
    else { commitCandidate(c); toast('已应用配置（模拟）。'); }
  });
}
function fullExit() {
  closeModal(); state.tray=false;
  if (state.busy) { state.quitQueued=true; toast('退出已排队，等待当前操作结束。'); render(); return; }
  job('正在停止 Core、释放代理并退出（模拟）',()=>{state.running=false;state.proxy='none';state.hidden='full';log('info','Core 与后台已停止，代理已释放（模拟）。');});
}
function entryList(data, tab, create=false) { if(tab!=='routing') {if(create&&!data[tab])data[tab]=[];return data[tab];} if(create) {data.routing ||= {}; data.routing.rules ||= [];} return data.routing?.rules; }
function entryModal(index) {
  const p=selected(), data=parseDraft(p);
  if (state.tab==='routing' && data.routing!==undefined && (!data.routing || typeof data.routing!=='object' || Array.isArray(data.routing))) {toast('routing 必须是 JSON 对象，请先在原始 JSON 中修正。');return;}
  const list=entryList(data,state.tab)||[];
  const value = index!==null?list[index]:state.tab==='inbounds'?{tag:'new-in',listen:'127.0.0.1',port:10810,protocol:'http'}:state.tab==='outbounds'?{tag:'new-out',protocol:'freedom'}:{type:'field',domain:['domain:example.com'],outboundTag:'direct'};
  modal(index===null?'添加条目':'编辑条目',`<p>使用原生字段编辑此条目，其余条目与未识别字段保持原样。后续正式界面可使用表单降低编辑成本。</p><label for="entry-json" class="small muted">条目 JSON</label><textarea id="entry-json" class="code" spellcheck="false">${e(pretty(value))}</textarea>`,btn('取消','close-modal')+btn('保存到草稿','save-entry','primary'),{type:'entry',id:p.id,index,tab:state.tab});
}
function createProfile(importing=false) {
  modal(importing?'导入原生 JSON':'新建配置',`${field('配置名称','profile-name',importing?'导入的配置':'新建配置')}${importing?`<div class="field"><label for="import-json">粘贴原生 JSON</label><textarea id="import-json" class="code" spellcheck="false">${e(pretty({inbounds:[{tag:'http-in',listen:'127.0.0.1',port:10809,protocol:'http'}],outbounds:[{tag:'direct',protocol:'freedom'}]}))}</textarea></div>`:'<p>创建本地 HTTP 入口与直连出站。不包含远端节点，不自动启动。</p>'}`,btn('取消','close-modal')+btn(importing?'导入为草稿':'创建配置','create-profile','primary'),{type:'create',importing});
}
function setTheme(value) { theme=value; try{localStorage.setItem('xray-pilot-prototype-theme',theme);}catch(_){} applyTheme();render(); }
async function copyText(text) { try { if (!navigator.clipboard) throw new Error('unsupported'); await navigator.clipboard.writeText(text); toast('已复制。'); } catch (_) { modal('复制内容',`<p>浏览器未允许自动复制，可手动选择下面的文本。</p><textarea class="code" readonly aria-label="待复制内容">${e(text)}</textarea>`,btn('关闭','close-modal','primary')); } }
function guide() {
  modal('交互设计说明 · 已确认',`<p>这是可点击的 Web 设计稿，用于确认信息层级、操作顺序和异常反馈。所有运行、下载、恢复和退出均为模拟，不访问后台或修改系统。</p><div class="guide-list">${[['01','五页导航','总览 / 配置 / 诊断 / 日志 / 设置。日常运行信息优先，原生 JSON 保留为高级编辑入口。'],['02','草稿与运行版本分开','选择配置、修改 JSON 和载入历史均不切换运行版本；检查 → 预览 → 明确确认后应用。'],['03','代理语义明确','「不接管」恢复接管前设置，不暗示系统已禁用其他程序的代理。实际网络与出口保持未验证。'],['04','首次使用与依赖','官方来源下载、准备配置、检查后确认启动。默认不接管系统代理，下载过程不覆盖期间的新选择。'],['05','退出分三个动作','关闭窗口收起到托盘；仅退出界面保留后台；退出程序停止 Core、恢复本程序接管的代理并退出。忙碌时完整退出排队。'],['06','外观与异常反馈','浅色 / 深色 / 跟随系统。顶部可演示后台断开、恢复限制、应用失败和结果待确认。']].map(([n,t,d])=>`<div><h3><strong>${n}</strong>${t}</h3><p>${d}</p></div>`).join('')}</div><div class="review-note">能力边界：现有程序为 Rust / egui；本网页不替换正式 GUI。总览布局、配置显示名称、跨页面草稿体验、日志搜索等属于待确认方案。当前原型草稿不持久化；依赖安装、路由与校验反馈均为模拟，不能作为功能验收证据。</div>`,btn('开始体验','close-modal','primary'),{type:'guide'},true);
}
const mutationActions = new Set(['validate','apply','new-profile','import','duplicate','delete-profile','delete-confirm','discard','discard-confirm','add-entry','edit-entry','save-entry','move-up','move-down','remove-entry','remove-confirm','history','history-confirm','format','save-settings','use-core','create-profile','proxy-none','proxy-http','proxy-confirm','restart']);
document.addEventListener('click', async event => {
  const target = event.target.closest('[data-action]');
  if (!target||target.disabled) return;
  const action=target.dataset.action;
  if (mutationActions.has(action)&&locked()) {toast('当前状态下请先完成操作、连接后台或查询结果。');return;}
  if (!['tray'].includes(action) && !target.closest('#tray-menu')) {state.tray=false;renderTray();}
  switch(action) {
    case 'navigate': state.page=target.dataset.page;render();break;
    case 'go-profiles': case 'go-diagnostics': case 'go-logs': state.page=action.slice(3);render();break;
    case 'tab': state.tab=target.dataset.tab;render();break;
    case 'open-json':state.tab='json';render();break;
    case 'reset': reset($('#scenario').value);toast('演示场景已重置。');break;
    case 'guide':guide();break;
    case 'close-modal':closeModal();break;
    case 'quick-theme':setTheme(document.documentElement.dataset.theme==='light'?'dark':'light');break;
    case 'theme':setTheme(target.dataset.theme);break;
    case 'settings-section':state.section=target.dataset.section;render();break;
    case 'select-profile': {
      if(target.dataset.id===state.selected) break;
      const p=selected();
      if(p&&dirty(p)) confirm('切换正在编辑的配置','当前有未应用的草稿。可保留草稿再切换；本次页面预览期间仍可返回继续编辑。','switch-keep',{id:target.dataset.id},'保留草稿并切换');
      else {state.selected=target.dataset.id;invalidate();render();} break;
    }
    case 'switch-keep':state.selected=state.modal.id;invalidate();closeModal();render();break;
    case 'new-profile':createProfile();break;
    case 'import':createProfile(true);break;
    case 'create-profile': {
      const name=$('#profile-name').value.trim();if(!name){modalError('请输入配置名称。');break;}
      let json={inbounds:[{tag:'http-in',listen:'127.0.0.1',port:10809,protocol:'http'}],outbounds:[{tag:'direct',protocol:'freedom'}]};
      if(state.modal.importing) {try{json=JSON.parse($('#import-json').value);if(!json||typeof json!=='object'||Array.isArray(json))throw new Error();}catch(_){modalError('请输入合法的 JSON 对象。');break;}}
      const p=profile('p'+(++sequence),name,json);state.profiles.push(p);state.selected=p.id;state.tab='summary';closeModal();render();toast('配置已添加，尚未启动。');break;
    }
    case 'duplicate': {const p=selected(),copy=profile('p'+(++sequence),p.name+' 副本');copy.draft=p.draft;copy.saved=p.draft;state.profiles.push(copy);state.selected=copy.id;invalidate();render();toast('已复制为独立配置。');break;}
    case 'delete-profile':if(state.running&&state.runtime?.profileId===state.selected) modal('配置正在运行','<p>请先停止 Core，再删除此配置。删除不会隐式切换到其他配置。</p>',btn('返回','close-modal','primary'));else confirm('删除配置？','此操作将删除本次预览中的配置与草稿。','delete-confirm',{id:state.selected},'删除','danger');break;
    case 'delete-confirm':state.profiles=state.profiles.filter(p=>p.id!==state.modal.id);state.selected=state.profiles[0]?.id||null;invalidate();closeModal();render();break;
    case 'discard':confirm('放弃未应用的草稿？','编辑内容将恢复到此配置最近保存的版本，当前运行状态保持不变。','discard-confirm',{},'放弃草稿','danger');break;
    case 'discard-confirm':editDraft(selected(),selected().saved);closeModal();render();break;
    case 'format': {const data=parseDraft(selected());if(!data)toast('JSON 无法解析，请先修正语法。');else{editDraft(selected(),pretty(data));render();toast('已格式化，需重新检查后应用。');}break;}
    case 'add-entry':entryModal(null);break;
    case 'edit-entry':entryModal(Number(target.dataset.index));break;
    case 'save-entry': {
      const ctx=state.modal,p=state.profiles.find(p=>p.id===ctx.id),data=parseDraft(p);let value;
      try{value=JSON.parse($('#entry-json').value);if(!value||typeof value!=='object'||Array.isArray(value))throw new Error();}catch(_){modalError('条目需要合法的 JSON 对象。');break;}
      const list=entryList(data,ctx.tab,true);if(!Array.isArray(list)){modalError('对应字段不是数组，请在原始 JSON 中修正。');break;}if(ctx.index===null)list.push(value);else list[ctx.index]=value;
      editDraft(p,pretty(data));closeModal();render();toast('已更新草稿，运行版本未改变。');break;
    }
    case 'move-up':case 'move-down': {const p=selected(),data=parseDraft(p),list=entryList(data,state.tab),i=Number(target.dataset.index),next=i+(action==='move-up'?-1:1);[list[i],list[next]]=[list[next],list[i]];editDraft(p,pretty(data));render();break;}
    case 'remove-entry':confirm('删除此条目？','只修改当前草稿，运行版本保持不变。','remove-confirm',{index:Number(target.dataset.index),tab:state.tab},'删除条目','danger');break;
    case 'remove-confirm': {const p=selected(),data=parseDraft(p);entryList(data,state.modal.tab).splice(state.modal.index,1);editDraft(p,pretty(data));closeModal();render();break;}
    case 'history':confirm('载入历史为草稿？','历史内容会替换当前编辑草稿，不会立即回滚运行版本。','history-confirm',{index:Number(target.dataset.index)},'载入为草稿');break;
    case 'history-confirm':editDraft(selected(),selected().history[state.modal.index].text);closeModal();state.tab='json';render();break;
    case 'validate':validate();break;
    case 'apply':applyCandidate();break;
    case 'stop':state.tray=false;if(state.offline||state.busy)break;confirm('停止 Core？','停止当前 Core，并释放本程序接管的系统代理。原先的代理设置可能仍然有效。','stop-confirm',{},'停止','danger');break;
    case 'stop-confirm':closeModal();job('正在停止 Core（模拟）',()=>{state.running=false;state.proxy='none';invalidate();activity('Core 已停止','本程序接管的代理已释放');log('info','Core 已停止（模拟）。');toast('Core 已停止。');});break;
    case 'restart':state.tray=false;confirm('重启当前运行版本？','继续使用当前运行版本与 Core。未应用草稿和下次 Core 选择不会自动生效。','restart-confirm',{},'重启');break;
    case 'restart-confirm':closeModal();job('正在重启 Core（模拟）',()=>{activity('Core 已重启','继续使用原运行版本');log('info','已重启原运行版本（模拟）。');toast('已重启原运行版本。');});break;
    case 'proxy-none':case 'proxy-http': {
      state.tray=false;renderTray();const mode=action==='proxy-http'?'http':'none';if(mode===state.proxy)break;
      confirm(mode==='http'?'接管系统代理？':'恢复接管前的代理设置？',mode==='http'?'演示将系统代理指向本机 HTTP 入口。正式执行前需验证入口可用；接管系统代理不代表真实出口已验证。':'本程序将释放代理控制权并恢复先前设置。其他程序的代理仍可能处于启用状态。','proxy-confirm',{mode},mode==='http'?'接管代理':'恢复先前设置');break;
    }
    case 'proxy-confirm': {const mode=state.modal.mode;closeModal();job('正在切换代理方式（模拟）',()=>{state.proxy=mode;invalidate();activity(mode==='http'?'已接管系统代理':'已释放系统代理','实际网络与出口仍未验证');log('info',mode==='http'?'接管系统代理（模拟）。':'恢复接管前设置（模拟）。');});break;}
    case 'save-settings': {const saved=clone(state.settingsDraft);state.settings= saved;state.settingsEpoch++;invalidate();render();toast('设置已保存；运行中的 Core 保持原版本。');log('info','下次应用与启动偏好已保存（仅演示）。');break;}
    case 'use-core':state.settingsDraft.core=target.dataset.version;state.settingsEpoch++;invalidate();render();toast('已选入设置草稿，请保存设置。');break;
    case 'download': {
      if(state.download||locked())break;state.download=true;const token=generation,epoch=state.settingsEpoch;render();
      setTimeout(()=>{if(token!==generation)return;state.download=false;const autoSelected=epoch===state.settingsEpoch;if(!state.installed.includes('v26.11.9'))state.installed.unshift('v26.11.9');if(epoch===state.settingsEpoch){state.settingsDraft.core='v26.11.9';if(state.wizard!==null)state.settings.core='v26.11.9';state.settingsEpoch++;invalidate();}activity('依赖安装完成（模拟）','官方 Core 与资源已准备');log('info','模拟依赖下载和安装完成。');render();toast(autoSelected?'安装完成，已选入下次使用版本。':'安装完成，保留了下载期间的新设置。');},1600);break;
    }
    case 'copy-path':copyText('D:\\Apps\\xray-pilot\\data');break;
    case 'diagnose':if(!state.offline&&!state.busy)job('正在读取本机状态（模拟）',()=>{state.checked=true;toast('本机检查完成，真实网络仍未验证。');log('info','模拟本机状态检查完成。');});break;
    case 'route-test': {const value=$('#route-target').value.trim(),port=$('#route-port').value.trim(),network=$('#route-network').value;if(!value||!/^\d+$/.test(port)||Number(port)<1||Number(port)>65535){toast('请输入目标域名 / IP，以及 1–65535 的整数端口。');break;}const data=JSON.parse(state.runtime.json),rules=data.routing?.rules||[];let result=data.outbounds?.[0]?.tag||'未知',note='无规则命中，使用第一个出站（演示范围内）。',unknown=false;for(let i=0;i<rules.length;i++){const rule=rules[i];if(Object.keys(rule).some(k=>!['type','domain','ip','outboundTag','network','port'].includes(k))||rule.ip?.some(ip=>ip!=='geoip:private')||rule.port){unknown=true;break;}if(rule.network&&!rule.network.split(',').includes(network))continue;if(rule.ip){unknown=true;break;}if(rule.domain){if(rule.domain.some(d=>{if(d.startsWith('domain:')){const domain=d.slice(7);return value===domain||value.endsWith('.'+domain);}if(d.startsWith('full:'))return value===d.slice(5);unknown=true;return false;})){result=rule.outboundTag;note=`命中第 ${i+1} 条规则（演示模型）。`;break;}if(unknown)break;}else{result=rule.outboundTag||'未知';note=`命中第 ${i+1} 条规则。`;break;}}state.routeResult=`<div class="label-value mt"><span>静态预测</span><strong>${unknown?'未知 · 存在需解析或未支持的前序条件':e(result)}</strong><p class="hint mt">${unknown?'应由正式诊断器补齐上下文后再判断。':e(note)}</p></div><p class="hint mt">当前运行：${e(state.runtime.name)} · 实际出口：未验证。此模型只演示交互，不代替生产诊断算法。</p>`;$('#route-result').innerHTML=state.routeResult;break;}
    case 'pause-logs':state.paused=!state.paused;if(state.paused)state.frozenLogs=clone(state.logs);render();break;
    case 'copy-logs':copyText(visibleLogs().map(l=>`${l.time} ${l.level.toUpperCase()} ${l.message} ${l.request}`).join('\n'));break;
    case 'reconnect':job('正在重新连接后台（模拟）',()=>{state.offline=false;log('info','重新连接成功，已读取最新状态（模拟）。');toast('已恢复连接并读取状态。');});break;
    case 'recovery':modal('检查未完成的恢复',`<div class="alert"><div><strong>系统代理已被外部程序修改</strong>为避免覆盖其他程序的设置，自动恢复已暂停。</div></div><p>演示后续流程：外部变更已核对，后台确认无需覆盖设置。本按钮仅模拟重新查询后解除限制，不代表可以忽略或删除代理租约证据。</p><details><summary>查看恢复证据</summary><pre>code: PROXY_OWNERSHIP_CONFLICT
外部当前值与本程序最后写入值不同。
建议：确认外部设置，查询后台恢复状态。</pre></details>`,btn('保留限制','close-modal')+btn('模拟状态已核对，重新查询','recovery-query','primary'));break;
    case 'recovery-query':closeModal();job('正在重新核对恢复状态（模拟）',()=>{state.recovery=false;log('info','模拟核对：外部代理保持原值，无需恢复，限制解除。');toast('恢复状态已核对（模拟）。');});break;
    case 'query':job('正在按请求身份查询结果（模拟）',()=>{const c=state.uncertain;if(c){commitCandidate(c);state.uncertain=null;state.scenario='normal';$('#scenario').value='normal';toast('已确认：原请求应用成功。');}});break;
    case 'failure-detail':modal('应用失败详情',`<p>${e(state.error)}</p><details open><summary>技术详情</summary><pre>code: HTTP_READY_FAILED
当前运行版本保留（模拟）。
修改配置后需重新检查并确认应用。</pre></details>`,btn('查看日志','error-logs')+btn('返回','close-modal','primary'));break;
    case 'error-logs':closeModal();state.page='logs';render();break;
    case 'wizard-next':if(state.wizard===0&&state.installed.length||state.wizard===1&&state.profiles.length){state.wizard++;render();}break;
    case 'wizard-back':if(!state.busy){state.wizard--;render();}break;
    case 'wizard-template': {const p=profile('first','本地直连',{inbounds:[{tag:'http-in',listen:'127.0.0.1',port:10809,protocol:'http'}],outbounds:[{tag:'direct',protocol:'freedom'}]});state.profiles.push(p);state.selected=p.id;render();break;}
    case 'tray':state.tray=!state.tray;renderTray();if(state.tray)$('#tray-menu button').focus();break;
    case 'hide':state.hidden='hidden';state.tray=false;render();break;
    case 'tray-open':state.hidden=null;state.tray=false;render();break;
    case 'tray-start':state.hidden=null;state.tray=false;state.page='profiles';render();break;
    case 'gui-exit':state.tray=false;confirm('仅退出界面？','后台与 Core 将继续运行。托盘会随界面退出，再次启动界面可以重新连接。','gui-exit-confirm',{},'仅退出界面');break;
    case 'gui-exit-confirm':closeModal();state.hidden='gui';render();break;
    case 'exit':state.tray=false;confirm('退出整个程序？','将停止 Core，恢复本程序接管前的代理设置，并退出后台与界面。操作进行中时将等待其完成。','full-exit-confirm',{},'停止并退出','danger');break;
    case 'full-exit-confirm':fullExit();break;
    case 'reopen':if(state.hidden==='full')reset($('#scenario').value);else{state.hidden=null;render();}break;
  }
});
document.addEventListener('input',event=>{
  const node=event.target;
  if(node.id==='json-editor') {editDraft(selected(),node.value);$('#draft-badge').innerHTML=badge(dirty(selected())?'未应用草稿':'已保存',dirty(selected())?'warn':'');$('#json-status').textContent=parseDraft(selected())?'JSON 语法可解析，尚未执行 Core 校验。':'JSON 语法错误或根节点不是对象，请修正后再检查。';const discard=$('[data-action="discard"]');if(discard)discard.disabled=!dirty(selected());}
  if(node.id==='log-search'){const start=node.selectionStart;state.logFilter=node.value;render();$('#log-search').focus();$('#log-search').setSelectionRange(start,start);}
});
document.addEventListener('change',event=>{
  const node=event.target;
  if(node.id==='scenario')reset(node.value);
  if(node.id==='log-level'){state.level=node.value;render();}
  if(node.id==='core-choice'){state.settingsDraft.core=node.value;state.settingsEpoch++;invalidate();}
  if(node.dataset.setting){state.settingsDraft[node.dataset.setting]=node.checked;state.settingsEpoch++;invalidate();}
});
document.addEventListener('keydown',event=>{
  if(event.key==='Escape'&&state.tray){state.tray=false;renderTray();$('#tray-trigger').focus();}
  if(state.tray&&['ArrowDown','ArrowUp'].includes(event.key)){event.preventDefault();const items=[...document.querySelectorAll('#tray-menu button:not(:disabled)')],i=items.indexOf(document.activeElement);items[(i+(event.key==='ArrowDown'?1:-1)+items.length)%items.length]?.focus();}
});
document.addEventListener('click',event=>{if(state.tray&&!event.target.closest('#tray-menu')&&!event.target.closest('#tray-trigger')){state.tray=false;renderTray();}});
$('#modal').addEventListener('close',()=>{state.modal=null;});
reset();
