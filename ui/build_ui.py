"""Turn the claude.ai Work Desk artifact page into the local app's page.

Each replacement must match exactly once, so a drift in the source fails loudly.
Usage: python3 build_ui.py source.html index.html
"""
import os
import sys

src, dst = sys.argv[1], sys.argv[2]
s = open(src, encoding="utf-8").read()


def rep(old, new, count=1):
    global s
    n = s.count(old)
    if n != count:
        sys.exit(f"expected {count} match(es), found {n}: {old[:80]!r}")
    s = s.replace(old, new)


AUTO_OPTS = '<option value="auto">Sort it for me</option><option value="inbox">Inbox</option>'

# ---- document skeleton (claude.ai adds this around artifacts) ----
s = (
    '<!doctype html>\n<html lang="en">\n<head>\n<meta charset="utf-8">\n'
    '<meta name="viewport" content="width=device-width,initial-scale=1">\n'
    '<style>:root{color-scheme:light}body{margin:0}img{max-width:100%}[hidden]{display:none!important}</style>\n'
    + s
)
rep('<div class="wrap" id="wrap">', '</head>\n<body>\n<div class="wrap" id="wrap">\n  <footer class="foot" id="foot"></footer>')
s = s.rstrip() + "\n</body>\n</html>\n"

# ---- capture: Work Desk files notes ----
rep('<select id="cap-proj" aria-label="Project"><option value="inbox">Let Claude sort it</option></select>',
    f'<select id="cap-proj" aria-label="Project">{AUTO_OPTS}</select>')
rep('<select id="bcap-proj" aria-label="Project"><option value="inbox">Let Claude sort it</option></select>',
    f'<select id="bcap-proj" aria-label="Project">{AUTO_OPTS}</select>')
rep('<p class="hint">Unsorted notes land in the Inbox. Ask Claude in the project chat to file them.</p>',
    '<p class="hint">“Sort it for me” lets Work Desk pick the project and catch dates like “by Friday”. If no project clearly fits, the note waits in the Inbox with a suggestion.</p>')
rep("""sel.innerHTML='<option value="inbox">Let Claude sort it</option>'+ordered()""",
    f"""sel.innerHTML='{AUTO_OPTS}'+ordered()""")
rep("sel.value=[...sel.options].some(o=>o.value===cur)?cur:'inbox';",
    "sel.value=[...sel.options].some(o=>o.value===cur)?cur:'auto';")
rep("""function projOptions(){ return '<option value="inbox">Let Claude sort it</option>'+ordered()""",
    f"""function projOptions(){{ return '{AUTO_OPTS}'+ordered()""")
rep("([...bsel.options].some(o=>o.value===bcur)?bcur:'inbox')",
    "([...bsel.options].some(o=>o.value===bcur)?bcur:'auto')")
rep("const project = filter!=='all' ? filter : ($('lane-proj')?.value || 'inbox');",
    "const project = filter!=='all' ? filter : ($('lane-proj')?.value || 'auto');")
rep("    await db.collection('items').add(doc);\n", "    return await db.collection('items').add(doc);\n")
rep("""      await db.collection('items').add({text, project:$('cap-proj').value, status:'open', stage:'todo', kind:'task', created:new Date().toISOString()});
      $('cap-text').value=''; $('cap-status').textContent='Added.';
      setTimeout(()=>{ if($('cap-status').textContent==='Added.') $('cap-status').textContent=''; },2500);
    }catch(err){ $('cap-status').textContent= err?.code==='quota_exceeded'?'The tracker is full. Ask Claude to clear out old items.':'Could not add the note. Try again in a moment.'; }""",
    """      $('cap-status').textContent= $('cap-proj').value==='auto' ? 'Filing…' : '';
      const r=await db.collection('items').add({text, project:$('cap-proj').value, status:'open', stage:'todo', kind:'task', created:new Date().toISOString()});
      $('cap-text').value=''; flash('cap-status', filedMsg(r));
    }catch(err){ $('cap-status').textContent='Could not add the note: '+err.message; }""")
rep("""      await addItem(text, $('bcap-proj').value, 'todo');
      $('bcap-text').value=''; $('bcap-status').textContent='';
    }catch(err){ $('bcap-status').textContent= err?.code==='quota_exceeded'?'The tracker is full. Ask Claude to clear out old items.':'Could not add the note. Try again in a moment.'; }""",
    """      $('bcap-status').textContent= $('bcap-proj').value==='auto' ? 'Filing…' : '';
      const r=await addItem(text, $('bcap-proj').value, 'todo');
      $('bcap-text').value=''; flash('bcap-status', filedMsg(r));
    }catch(err){ $('bcap-status').textContent='Could not add the note: '+err.message; }""")
rep("try{ draft=''; await addItem(text, project, stage); renderBoard(); }",
    "try{ draft=''; const r=await addItem(text, project, stage); renderBoard(); if(project==='auto') flash('bcap-status', filedMsg(r)); }")

# ---- Desk: setup box, brief, sort inbox, project editing ----
rep('    <div class="desk-main">\n',
    '''    <div class="desk-main">
      <section class="panel setup" id="ai-setup" hidden aria-labelledby="k-h">
        <div class="panel-head"><h2 class="sec-h" id="k-h">AI keys</h2><button class="mini" id="ai-close" type="button" hidden>Done</button></div>
        <p class="alert-detail" style="margin:0">Paste an API key from DeepSeek, OpenAI, Anthropic, Google Gemini or OpenRouter to have notes filed for you and to get a daily brief. Work Desk works out which service a key is for. Add a TypeSafe key as well and Jev will check whether each note really fits a project. Keys stay on this computer.</p>
        <ul class="ai-state" id="ai-state"></ul>
        <form id="key-form" class="note-row">
          <input type="password" id="ai-key" placeholder="Paste an API key" autocomplete="off" aria-label="API key">
          <button class="btn" type="submit">Save key</button>
        </form>
        <p class="status" id="key-status" role="status"></p>
      </section>
      <section class="panel" aria-labelledby="br-h">
        <div class="panel-head"><h2 class="sec-h" id="br-h">Start of day</h2><button class="mini primary" id="brief-btn" type="button">Brief me</button></div>
        <div id="brief-out"><p class="empty">Ask Work Desk what to focus on today.</p></div>
      </section>
''')
rep('<h2 class="sec-h" id="p-h">Projects</h2>',
    '<div class="panel-head"><h2 class="sec-h" id="p-h">Projects</h2><button class="mini" id="new-proj" type="button">+ New project</button></div>')
rep("""<h3>Inbox</h3></div><span class="pill">${inbox.length} to sort</span></div>""",
    """<h3>Inbox</h3></div><div class="meta"><span class="pill">${inbox.length} to sort</span><button class="mini primary" id="sort-inbox" type="button">Sort inbox</button></div></div>""")
rep("${inbox.map(i=>row(i,false)).join('')}", "${inbox.map(inboxRow).join('')}")
rep("""    for(const p of ordered()){
      const all=items.filter(i=>i.project===p.id);""",
    """    if(editingProj==='new') html=projForm({})+html;
    for(const p of ordered()){
      if(editingProj===p.id){ html+=projForm(p); continue; }
      const all=items.filter(i=>i.project===p.id);""")
rep("""<span class="pill ${p.tier==='major'?'major':''}">${esc(p.tier)}</span>${dl}</div></div>""",
    """<span class="pill ${p.tier==='major'?'major':''}">${esc(p.tier||'standard')}</span>${p.status&&p.status!=='active'?`<span class="pill">${esc(p.status)}</span>`:''}${dl}<button class="mini" data-pedit="${esc(p.id)}" type="button">Edit</button></div></div>""")
rep("""'<p class="empty">No projects yet. Tell Claude in the project chat what you are working on and they will appear here.</p>'""",
    """'<p class="empty">No projects yet. Use “+ New project” to add one.</p>'""")
rep("""    if(!db){ $('projects').innerHTML='<p class="empty">Open this page from claude.ai while signed in to see your projects.</p>'; $('due-list').innerHTML=$('next-list').innerHTML='<li class="empty">Sign in on claude.ai to load your tasks.</li>'; return; }
""", "")
rep("    try{ db = window.claude ? await window.claude.use('db') : null; }catch(e){ db=null; }\n",
    "    db = makeLocalDb(); loadStatus(); showCachedBrief();\n")

# ---- extra CSS ----
rep("@media (max-width:620px){.tl{--lw:150px}}", """@media (max-width:620px){.tl{--lw:150px}}

/* ---------- Local app additions ---------- */
.foot{order:99;font-size:.78rem;color:var(--faint);overflow-wrap:anywhere}
.foot code{font-family:var(--mono);font-size:.74rem;color:var(--muted)}
.setup{border-color:color-mix(in srgb,var(--accent) 45%,var(--line))}
.setup input{flex:1 1 220px;min-width:0;padding:8px 11px;border-radius:10px;border:1px solid var(--line);background:var(--surface);color:var(--fg);font-family:var(--mono);font-size:.85rem}
#brief-out{display:flex;flex-direction:column;gap:8px}
.brief-head{font-family:var(--display);font-weight:700;font-size:1.05rem;line-height:1.3;margin:0;text-wrap:balance}
.brief-list{margin:0;padding-left:20px;display:flex;flex-direction:column;gap:6px;font-size:.92rem}
.brief-list .alert-detail{display:block}
.brief-start{background:var(--accent-soft);padding:10px 12px;border-radius:10px;margin:2px 0 0;font-size:.92rem}
.brief-meta{font-family:var(--mono);font-size:.68rem;color:var(--faint);margin:0}
.proj-form{display:flex;flex-direction:column;gap:12px}
.pf-row{display:flex;flex-wrap:wrap;gap:12px}
.field textarea{padding:7px 10px;border-radius:9px;border:1px solid var(--line);background:var(--surface);color:var(--fg);resize:vertical;font:inherit}
.proj-form .field input:not([type=date]){width:100%}
.pf-check{display:flex;gap:8px;align-items:center;font-size:.88rem;color:var(--muted)}
.pf-check code{font-family:var(--mono);font-size:.8rem}
.ai-state{list-style:none;margin:0;padding:0;display:flex;flex-direction:column;gap:4px;font-size:.88rem}
.ai-state b{display:inline-block;min-width:92px}
.ai-state .empty{display:inline;padding:0}
.triage{grid-column:2/-1;display:flex;flex-direction:column;gap:6px;margin-top:-4px}
.triage-why{margin:0;font-size:.82rem;color:var(--muted)}
.triage-by{font-family:var(--mono);font-size:.66rem;color:var(--faint);border:1px solid var(--line);border-radius:6px;padding:0 5px;margin-left:4px}
.triage-acts{display:flex;flex-wrap:wrap;gap:6px}
""")

# ---- local backend + new behaviour ----
rep("  render();\n  (async()=>{", r"""  // ================= Local backend =================
  // Same calls the page used on claude.ai (collection().onSnapshot/add, doc().update),
  // served by the desk binary on this computer.
  function makeLocalDb(){
    const H={'Content-Type':'application/json','X-Desk':'1'};
    const subs={};
    async function call(method,url,body){
      const r=await fetch(url,{method,headers:H,body:body===undefined?undefined:JSON.stringify(body)});
      const t=await r.text(); let j=null; try{ j=t?JSON.parse(t):null; }catch(_){}
      if(!r.ok) throw new Error((j&&j.error)||t||r.statusText);
      return j;
    }
    async function refresh(coll){
      const rows=await call('GET','/api/'+coll);
      const snap={docs:rows.map(({id,...d})=>({id,data:()=>d}))};
      (subs[coll]||[]).forEach(cb=>cb(snap));
    }
    const refreshAll=()=>Promise.all(Object.keys(subs).map(refresh));
    let rev=null;
    setInterval(async()=>{ try{ const r=await call('GET','/api/rev'); if(rev!==null && r.rev!==rev){ await refreshAll(); loadStatus(); } rev=r.rev; }catch(_){} }, 2500);
    return {
      call, refreshAll,
      collection:coll=>({
        onSnapshot:cb=>{ (subs[coll]=subs[coll]||[]).push(cb); refresh(coll).catch(()=>{}); },
        add:async doc=>{ const r=await call('POST','/api/'+coll,doc); await refresh(coll); return r; },
      }),
      doc:path=>{ const [coll,id]=path.split('/'); return {
        update:async patch=>{ const r=await call('PATCH',`/api/${coll}/${encodeURIComponent(id)}`,patch); await refresh(coll); return r; },
      }; },
    };
  }

  function filedMsg(r){
    if(!r) return 'Added.';
    if(r.notice) return r.notice;
    return `Filed under ${pName(r.project)}${r.due?` · due ${fmt(r.due)}`:''}: “${r.text}”`;
  }
  function flash(id,msg){ $(id).textContent=msg; setTimeout(()=>{ if($(id).textContent===msg) $(id).textContent=''; },7000); }

  // aiOn: some AI can file notes. chatOn: a chat model, which the brief needs (Jev alone can't write one).
  let aiOn=false, chatOn=false, keysOpen=false, projectsRoot='~/Documents/Projects/Repos';
  async function loadStatus(){
    try{
      const s=await db.call('GET','/api/status'); aiOn=!!s.ai; chatOn=!!s.chat; projectsRoot=s.projects_root||projectsRoot;
      $('ai-setup').hidden=aiOn && !keysOpen; $('ai-close').hidden=!aiOn; $('brief-btn').disabled=!chatOn;
      $('ai-state').innerHTML=`<li><b>Chat model</b> ${s.chat?`${esc(s.chat)} <button class="linkish" type="button" data-forget="chat">Remove</button>`:'<span class="empty">none yet</span>'}</li>`
        +`<li><b>Jev</b> ${s.jev?'on: checks which project each note fits <button class="linkish" type="button" data-forget="jev">Remove</button>':'<span class="empty">optional, not set</span>'}</li>`;
      $('foot').innerHTML=`Saved to <code>${esc(s.data_file||'')}</code>`+(s.data_error?`<span class="warn"> · ${esc(s.data_error)}. Showing the last good copy.</span>`:'')
        +` · <button class="linkish" type="button" id="ai-open">AI keys</button>`;
      if(!chatOn && !$('brief-out').querySelector('.brief-head')) $('brief-out').innerHTML='<p class="empty">Add an API key under AI keys to get a daily brief.</p>';
    }catch(_){}
  }
  $('key-form').addEventListener('submit', async e=>{
    e.preventDefault();
    const key=$('ai-key').value.trim(); if(!key) return;
    keysOpen=true; $('key-status').textContent='Checking the key…';
    try{ const r=await db.call('POST','/api/settings',{key}); $('ai-key').value=''; $('key-status').textContent=r.message||'Saved.'; await loadStatus();
      if(chatOn && !$('brief-out').querySelector('.brief-head')) $('brief-out').innerHTML='<p class="empty">Ask Work Desk what to focus on today.</p>'; }
    catch(err){ $('key-status').textContent=err.message; }
  });
  document.addEventListener('click', async e=>{
    if(e.target.closest('#ai-open')){ keysOpen=true; $('ai-setup').hidden=false; $('ai-setup').scrollIntoView({block:'nearest'}); $('ai-key').focus(); return; }
    if(e.target.closest('#ai-close')){ keysOpen=false; $('key-status').textContent=''; loadStatus(); return; }
    const fg=e.target.closest('[data-forget]');
    if(fg){ try{ await db.call('POST','/api/settings',{forget:fg.dataset.forget}); $('key-status').textContent='Removed.'; await loadStatus(); }
      catch(err){ $('key-status').textContent=err.message; } }
  });

  // ---- Inbox: notes that didn't clearly fit a project ----
  function inboxRow(it){
    const t=it.triage, base=row(it,false);
    if(!t||!t.reason) return base;
    const close=t.closest&&projOf(t.closest);
    const acts=(t.suggest?`<button class="mini primary" type="button" data-tnew="${esc(it.id)}">Create “${esc(t.suggest)}”</button>`:`<button class="mini" type="button" data-tnew="${esc(it.id)}">New project…</button>`)
      +(close?`<button class="mini" type="button" data-tfile="${esc(it.id)}" data-to="${esc(close.id)}">File under ${esc(close.name)}</button>`:'');
    return base.replace(/<\/li>$/, `<div class="triage"><p class="triage-why">${esc(t.reason)}${t.by==='jev'?'<span class="triage-by" title="Jev made this call">Jev</span>':''}</p><div class="triage-acts">${acts}</div></div></li>`);
  }
  document.addEventListener('click', async e=>{
    const n=e.target.closest('[data-tnew]');
    if(n){
      const it=items.find(i=>i.id===n.dataset.tnew); if(!it) return;
      const name=it.triage?.suggest||'', key=name.toLowerCase();
      // every waiting note with the same suggestion moves into the new project
      const ids=key?items.filter(i=>i.project==='inbox' && i.status!=='done' && (i.triage?.suggest||'').toLowerCase()===key).map(i=>i.id):[it.id];
      newProjectFrom(name, ids); return;
    }
    const f=e.target.closest('[data-tfile]');
    if(f){ try{ await db.doc('items/'+f.dataset.tfile).update({project:f.dataset.to}); toast(`Filed under ${pName(f.dataset.to)}.`); }
      catch(err){ toast('Could not move the note: '+err.message, true); } }
  });

  function renderBrief(v, when){
    const li=a=>(a||[]).map(x=>`<li>${esc(typeof x==='string'?x:(x.task||''))}</li>`).join('');
    let h='';
    if(v.headline) h+=`<p class="brief-head">${esc(v.headline)}</p>`;
    if(v.focus?.length) h+=`<p class="label">Focus</p><ol class="brief-list">${v.focus.map(f=>`<li><span>${esc(f.task||'')}</span>${f.project?` <span class="card-proj">· ${esc(f.project)}</span>`:''}${f.why?`<span class="alert-detail">${esc(f.why)}</span>`:''}</li>`).join('')}</ol>`;
    if(v.watch?.length) h+=`<p class="label">Watch out</p><ul class="brief-list">${li(v.watch)}</ul>`;
    if(v.quick_wins?.length) h+=`<p class="label">Quick wins</p><ul class="brief-list">${li(v.quick_wins)}</ul>`;
    if(v.first_step) h+=`<p class="brief-start"><b>Start here:</b> ${esc(v.first_step)}</p>`;
    if(when) h+=`<p class="brief-meta">Brief from ${esc(when)}</p>`;
    $('brief-out').innerHTML=h||'<p class="empty">The brief came back empty. Try again.</p>';
  }
  function showCachedBrief(){
    try{ const c=JSON.parse(localStorage.getItem('wd-brief')||'null'); if(c && c.day===TODAY_ISO) renderBrief(c.v, c.at); }catch(_){}
  }
  $('brief-btn').addEventListener('click', async ()=>{
    const b=$('brief-btn'); b.disabled=true; b.textContent='Thinking…';
    try{
      const v=await db.call('POST','/api/brief',{});
      const at=new Date().toLocaleTimeString(undefined,{hour:'numeric',minute:'2-digit'});
      renderBrief(v, at);
      try{ localStorage.setItem('wd-brief', JSON.stringify({day:TODAY_ISO, at, v})); }catch(_){}
    }catch(err){ $('brief-out').innerHTML=`<p class="warn">${esc(err.message)}</p>`; }
    b.disabled=!chatOn; b.textContent='Brief me';
  });

  document.addEventListener('click', async e=>{
    const sb=e.target.closest('#sort-inbox'); if(!sb) return;
    if(!aiOn){ $('cap-status').textContent='Add an API key under AI keys to sort the Inbox.'; return; }
    sb.disabled=true; sb.textContent='Sorting…';
    try{ const r=await db.call('POST','/api/sort-inbox',{}); await db.refreshAll();
      flash('cap-status', `Filed ${r.moved} note${r.moved===1?'':'s'}. ${r.left} left in the Inbox.${r.error?' '+r.error:''}`); }
    catch(err){ $('cap-status').textContent='Could not sort the Inbox: '+err.message; sb.disabled=false; sb.textContent='Sort inbox'; }
  });

  // ---- project editing ----
  let editingProj=null, projDraft={}, projMove=[];
  function projForm(p){
    const d={...p,...projDraft};
    const opt=(list,cur)=>list.map(t=>`<option value="${t}" ${cur===t?'selected':''}>${t}</option>`).join('');
    return `<form class="proj proj-form" id="proj-form" data-id="${esc(p.id||'')}" style="--h:${p.id?hue(p.id):240}">
      <div class="field"><label for="pf-name">Name</label><input id="pf-name" name="name" value="${esc(d.name||'')}" required></div>
      <div class="field"><label for="pf-goal">Goal</label><textarea id="pf-goal" name="goal" rows="2">${esc(d.goal||'')}</textarea></div>
      <div class="pf-row">
        <div class="field"><label for="pf-deadline">Deadline</label><input type="date" id="pf-deadline" name="deadline" value="${esc(d.deadline||'')}"></div>
        <div class="field"><label for="pf-tier">Tier</label><select id="pf-tier" name="tier">${opt(['major','standard'],d.tier||'standard')}</select></div>
        <div class="field"><label for="pf-status">Status</label><select id="pf-status" name="status">${opt(['active','wrapping','waiting','done'],d.status||'active')}</select></div>
      </div>
      <div class="field"><label for="pf-context">Notes (repo, people, links)</label><input id="pf-context" name="context" value="${esc(d.context||'')}"></div>
      ${p.id?'':`<label class="pf-check"><input type="checkbox" name="make_folder" ${d.make_folder===false?'':'checked'}> Create a project folder with a git repo in <code>${esc(projectsRoot)}</code></label>`}
      ${!p.id&&projMove.length?`<p class="hint" style="margin:0">${projMove.length===1?'The Inbox note':`The ${projMove.length} Inbox notes`} that suggested it will move into the new project.</p>`:''}
      <div class="compose-row"><button class="btn" type="submit">${p.id?'Save':'Add project'}</button><button class="ghost" type="button" data-pcancel>Cancel</button></div>
    </form>`;
  }
  function openProjForm(id){ editingProj=id; projDraft={}; projMove=[]; renderDesk(); setTimeout(()=>$('pf-name')?.focus(),0); }
  function newProjectFrom(name, ids){
    editingProj='new'; projDraft={name}; projMove=ids; renderDesk();
    setTimeout(()=>{ const f=$('proj-form'); f?.scrollIntoView({block:'nearest'}); $(name?'pf-goal':'pf-name')?.focus(); },0);
  }
  $('new-proj').addEventListener('click',()=>openProjForm('new'));
  $('projects').addEventListener('click', e=>{
    const ed=e.target.closest('[data-pedit]'); if(ed){ openProjForm(ed.dataset.pedit); return; }
    if(e.target.closest('[data-pcancel]')){ editingProj=null; projDraft={}; projMove=[]; renderDesk(); }
  });
  const draftVal=t=>t.type==='checkbox'?t.checked:t.value;
  $('projects').addEventListener('input', e=>{ if(e.target.name && e.target.closest('#proj-form')) projDraft[e.target.name]=draftVal(e.target); });
  $('projects').addEventListener('change', e=>{ if(e.target.name && e.target.closest('#proj-form')) projDraft[e.target.name]=draftVal(e.target); });
  $('projects').addEventListener('submit', async e=>{
    if(e.target.id!=='proj-form') return;
    e.preventDefault();
    const f=new FormData(e.target), data={};
    for(const k of ['name','goal','deadline','tier','status','context']) data[k]=(f.get(k)||'').toString().trim();
    if(!data.name) return;
    const id=e.target.dataset.id;
    try{
      if(id) await db.doc('projects/'+id).update(data);
      else{
        const r=await db.collection('projects').add({...data, order:Math.max(0,...projects.map(p=>p.order||0))+1, make_folder:f.get('make_folder')==='on', move:projMove});
        if(projMove.length) await db.refreshAll();
        if(r?.notice) toast(r.notice, /^Couldn|^Didn|couldn/.test(r.notice));
      }
      editingProj=null; projDraft={}; projMove=[]; renderDesk();
    }catch(err){ $('cap-status').textContent='Could not save the project: '+err.message; }
  });

  render();
  (async()=>{""")

# ---- project pages, task details and linked files (ui/project.js, ui/project.css) ----
here = os.path.dirname(os.path.abspath(__file__))
PROJECT_JS = open(os.path.join(here, "project.js"), encoding="utf-8").read()
PROJECT_CSS = open(os.path.join(here, "project.css"), encoding="utf-8").read()

rep("  <!-- TIMELINE VIEW -->", """  <!-- PROJECT PAGE -->
  <div id="proj-view" hidden></div>
  <dialog id="task-dlg" class="tdlg" aria-label="Task details"></dialog>
  <div class="toast" id="toast" role="status" aria-live="polite"></div>
  <div class="drop-hint" id="drop-hint" hidden></div>

  <!-- TIMELINE VIEW -->""")
rep(".proj-form .field input:not([type=date]){width:100%}\n",
    ".proj-form .field input:not([type=date]){width:100%}\n\n" + PROJECT_CSS)
# the code goes in early, so its constants exist before anything renders
rep("  const saveFilter=()=>{ try{localStorage.setItem('wd-filter',filter);}catch(_){} };\n",
    "  const saveFilter=()=>{ try{localStorage.setItem('wd-filter',filter);}catch(_){} };\n\n" + PROJECT_JS)
# cards: click for details, file chips, drop target
rep("""<div class="card ${st==='done'?'done':''}" draggable="true" data-id="${esc(it.id)}">""",
    """<div class="card ${st==='done'?'done':''}" draggable="true" data-id="${esc(it.id)}" data-drop="items:${esc(it.id)}">""")
rep('<div class="card-txt">${esc(it.text)}</div>',
    '<div class="card-txt" data-open="${esc(it.id)}" title="Open details and files">${esc(it.text)}</div>${fileChips(it)}')
# (card is also called as .map(card), so the project page turns tags off with a flag, not an argument)
rep("${filter==='all'?tag(it.project):''}", "${(cardTags??filter==='all')?tag(it.project):''}")
# desk rows: file count and a details button
rep('return `<li class="row"><input', 'return `<li class="row" data-drop="items:${esc(it.id)}"><input')
rep('<span class="row-end">${extra||dueChip(it.due)}</span></li>',
    '<span class="row-end">${fileCount(it)}${extra||dueChip(it.due)}<button class="row-open" type="button" data-open="${esc(it.id)}" '
    'aria-label="Open details and files" title="Details and files">${ICON.open}</button></span></li>')
# desk project cards link to their page
rep('<div class="proj-name"><span class="swatch"></span><h3>${esc(p.name)}</h3></div>',
    '<div class="proj-name"><span class="swatch"></span><h3><a class="plink" href="${pageHref(p.id)}">${esc(p.name)}</a></h3></div>')
rep("""<article class="proj ${p.status==='waiting'?'quiet':''}" style="--h:${h}">""",
    """<article class="proj ${p.status==='waiting'?'quiet':''}" style="--h:${h}" data-drop="projects:${esc(p.id)}">""")
rep("$('filters').innerHTML=filterChips();", "$('filters').innerHTML=filterChips()+boardPageLink();")
rep('<div class="compose-row"><button class="ghost" data-clear>Clear dates</button>',
    '<div class="compose-row"><button class="ghost" data-open="${esc(it.id)}">Details &amp; files</button><button class="ghost" data-clear>Clear dates</button>')
# views: the project page is a fourth view, reached by #/p/<id>
rep("  function setView(v){\n    view=v;", "  function setView(v){\n    const was=view; if(v!=='proj') lastView=v;\n    view=v;")
rep("    try{ localStorage.setItem('wd-view', v); }catch(e){}\n    if(v==='board') renderBoard();",
    """    $('proj-view').hidden = v!=='proj';
    if(v!=='proj' && was==='proj' && location.hash) history.replaceState(null,'',location.pathname+location.search);
    if(v!=='proj') try{ localStorage.setItem('wd-view', v); }catch(e){}
    if(v==='proj') renderPage();
    if(v==='board') renderBoard();""")
rep("if(view==='board') renderBoard(); if(view==='tl') renderTimeline(); }",
    "if(view==='board') renderBoard(); if(view==='tl') renderTimeline(); if(view==='proj') renderPage(); refreshTaskDlg(); }")
rep("    db = makeLocalDb(); loadStatus(); showCachedBrief();\n",
    "    db = makeLocalDb(); loadStatus(); showCachedBrief(); route();\n")

open(dst, "w", encoding="utf-8").write(s)
print("ok", len(s))
