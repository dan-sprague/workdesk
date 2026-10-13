"""Turn the claude.ai Work Desk artifact page into the local app's page.

Each replacement must match exactly once, so a drift in the source fails loudly.
Usage: python3 build_ui.py source.html index.html
"""
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

# ---- capture: DeepSeek files notes ----
rep('<select id="cap-proj" aria-label="Project"><option value="inbox">Let Claude sort it</option></select>',
    f'<select id="cap-proj" aria-label="Project">{AUTO_OPTS}</select>')
rep('<select id="bcap-proj" aria-label="Project"><option value="inbox">Let Claude sort it</option></select>',
    f'<select id="bcap-proj" aria-label="Project">{AUTO_OPTS}</select>')
rep('<p class="hint">Unsorted notes land in the Inbox. Ask Claude in the project chat to file them.</p>',
    '<p class="hint">“Sort it for me” lets DeepSeek pick the project and catch dates like “by Friday”. If it can’t tell, the note waits in the Inbox.</p>')
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
        <div class="panel-head"><h2 class="sec-h" id="k-h">Connect DeepSeek</h2></div>
        <p class="alert-detail" style="margin:0">Paste your DeepSeek API key to have sticky notes filed automatically and to get a daily brief. The key is stored only on this computer.</p>
        <form id="key-form" class="note-row">
          <input type="password" id="ai-key" placeholder="sk-…" autocomplete="off" aria-label="DeepSeek API key">
          <button class="btn" type="submit">Save key</button>
        </form>
        <p class="status" id="key-status" role="status"></p>
      </section>
      <section class="panel" aria-labelledby="br-h">
        <div class="panel-head"><h2 class="sec-h" id="br-h">Start of day</h2><button class="mini primary" id="brief-btn" type="button">Brief me</button></div>
        <div id="brief-out"><p class="empty">Ask DeepSeek what to focus on today.</p></div>
      </section>
''')
rep('<h2 class="sec-h" id="p-h">Projects</h2>',
    '<div class="panel-head"><h2 class="sec-h" id="p-h">Projects</h2><button class="mini" id="new-proj" type="button">+ New project</button></div>')
rep("""<h3>Inbox</h3></div><span class="pill">${inbox.length} to sort</span></div>""",
    """<h3>Inbox</h3></div><div class="meta"><span class="pill">${inbox.length} to sort</span><button class="mini primary" id="sort-inbox" type="button">Sort inbox</button></div></div>""")
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

  let aiOn=false;
  async function loadStatus(){
    try{
      const s=await db.call('GET','/api/status'); aiOn=!!s.ai;
      $('ai-setup').hidden=aiOn; $('brief-btn').disabled=!aiOn;
      $('foot').innerHTML=`Saved to <code>${esc(s.data_file||'')}</code>`+(s.data_error?`<span class="warn"> · ${esc(s.data_error)}. Showing the last good copy.</span>`:'');
      if(!aiOn && !$('brief-out').querySelector('.brief-head')) $('brief-out').innerHTML='<p class="empty">Connect DeepSeek above to get a daily brief.</p>';
    }catch(_){}
  }
  $('key-form').addEventListener('submit', async e=>{
    e.preventDefault();
    const key=$('ai-key').value.trim(); if(!key) return;
    try{ await db.call('POST','/api/settings',{key}); $('ai-key').value=''; $('key-status').textContent='Saved.'; await loadStatus();
      if(aiOn) $('brief-out').innerHTML='<p class="empty">Ask DeepSeek what to focus on today.</p>'; }
    catch(err){ $('key-status').textContent='Could not save the key: '+err.message; }
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
    $('brief-out').innerHTML=h||'<p class="empty">DeepSeek sent an empty brief. Try again.</p>';
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
    b.disabled=!aiOn; b.textContent='Brief me';
  });

  document.addEventListener('click', async e=>{
    const sb=e.target.closest('#sort-inbox'); if(!sb) return;
    if(!aiOn){ $('cap-status').textContent='Connect DeepSeek first to sort the Inbox.'; return; }
    sb.disabled=true; sb.textContent='Sorting…';
    try{ const r=await db.call('POST','/api/sort-inbox',{}); await db.refreshAll();
      flash('cap-status', `Filed ${r.moved} note${r.moved===1?'':'s'}. ${r.left} left in the Inbox.${r.error?' '+r.error:''}`); }
    catch(err){ $('cap-status').textContent='Could not sort the Inbox: '+err.message; sb.disabled=false; sb.textContent='Sort inbox'; }
  });

  // ---- project editing ----
  let editingProj=null, projDraft={};
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
      <div class="compose-row"><button class="btn" type="submit">${p.id?'Save':'Add project'}</button><button class="ghost" type="button" data-pcancel>Cancel</button></div>
    </form>`;
  }
  function openProjForm(id){ editingProj=id; projDraft={}; renderDesk(); setTimeout(()=>$('pf-name')?.focus(),0); }
  $('new-proj').addEventListener('click',()=>openProjForm('new'));
  $('projects').addEventListener('click', e=>{
    const ed=e.target.closest('[data-pedit]'); if(ed){ openProjForm(ed.dataset.pedit); return; }
    if(e.target.closest('[data-pcancel]')){ editingProj=null; projDraft={}; renderDesk(); }
  });
  $('projects').addEventListener('input', e=>{ if(e.target.name && e.target.closest('#proj-form')) projDraft[e.target.name]=e.target.value; });
  $('projects').addEventListener('change', e=>{ if(e.target.name && e.target.closest('#proj-form')) projDraft[e.target.name]=e.target.value; });
  $('projects').addEventListener('submit', async e=>{
    if(e.target.id!=='proj-form') return;
    e.preventDefault();
    const f=new FormData(e.target), data={};
    for(const k of ['name','goal','deadline','tier','status','context']) data[k]=(f.get(k)||'').toString().trim();
    if(!data.name) return;
    const id=e.target.dataset.id;
    try{
      if(id) await db.doc('projects/'+id).update(data);
      else await db.collection('projects').add({...data, order:Math.max(0,...projects.map(p=>p.order||0))+1});
      editingProj=null; projDraft={}; renderDesk();
    }catch(err){ $('cap-status').textContent='Could not save the project: '+err.message; }
  });

  render();
  (async()=>{""")

open(dst, "w", encoding="utf-8").write(s)
print("ok", len(s))
