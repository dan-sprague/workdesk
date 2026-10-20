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
# "Sort it for me": Work Desk suggests where a note goes and you confirm before it's saved
rep("    await db.collection('items').add(doc);\n", """    if(project==='auto'){
      const c=await confirmFiling(text); if(!c) return null;
      let first=null; const ids=[];
      for(const d of c.docs){ const r=await db.collection('items').add({...doc, ...d}); first=first||r; ids.push(r.id); }
      if(c.newProject) startProjectFor(c.newProject, ids);
      if(ids.length>1) first={...first, notice:`Filed ${ids.length} tasks under ${pName(first.project)}.`};
      return first;
    }
    return await db.collection('items').add(doc);
""")
rep("""      await db.collection('items').add({text, project:$('cap-proj').value, status:'open', stage:'todo', kind:'task', created:new Date().toISOString()});
      $('cap-text').value=''; $('cap-status').textContent='Added.';
      setTimeout(()=>{ if($('cap-status').textContent==='Added.') $('cap-status').textContent=''; },2500);
    }catch(err){ $('cap-status').textContent= err?.code==='quota_exceeded'?'The tracker is full. Ask Claude to clear out old items.':'Could not add the note. Try again in a moment.'; }""",
    """      $('cap-status').textContent= $('cap-proj').value==='auto' ? 'Reading the note…' : 'Saving…';
      const r=await addItem(text, $('cap-proj').value, 'todo');
      if(r){ $('cap-text').value=''; flash('cap-status', filedMsg(r)); } else $('cap-status').textContent='';
    }catch(err){ $('cap-status').textContent='Could not add the note: '+err.message; }""")
rep("""      await addItem(text, $('bcap-proj').value, 'todo');
      $('bcap-text').value=''; $('bcap-status').textContent='';
    }catch(err){ $('bcap-status').textContent= err?.code==='quota_exceeded'?'The tracker is full. Ask Claude to clear out old items.':'Could not add the note. Try again in a moment.'; }""",
    """      $('bcap-status').textContent= $('bcap-proj').value==='auto' ? 'Reading the note…' : 'Saving…';
      const r=await addItem(text, $('bcap-proj').value, 'todo');
      if(r){ $('bcap-text').value=''; flash('bcap-status', filedMsg(r)); } else $('bcap-status').textContent='';
    }catch(err){ $('bcap-status').textContent='Could not add the note: '+err.message; }""")
rep("try{ draft=''; await addItem(text, project, stage); renderBoard(); }",
    "try{ draft=''; const r=await addItem(text, project, stage); if(!r) draft=text; renderBoard(); if(r) flash('bcap-status', filedMsg(r)); }")

# ---- Desk: setup box, brief, sort inbox, project editing ----
rep('<div class="toggle" role="group" aria-label="View">',
    '<button class="mini keys-btn" type="button" data-ai-open title="API keys for the chat model and Jev">AI keys</button>\n      <div class="toggle" role="group" aria-label="View">')
rep('    <div class="desk-main">\n',
    '''    <div class="desk-main">
      <section class="panel setup" id="ai-setup" hidden aria-labelledby="k-h">
        <div class="panel-head"><h2 class="sec-h" id="k-h">AI keys</h2><button class="mini" id="ai-close" type="button" hidden>Done</button></div>
        <div class="key-block">
          <p class="label">Chat model</p>
          <p class="alert-detail">Writes your tasks, the daily brief and prompts for Claude. Paste a key from DeepSeek, OpenAI, Anthropic, Google Gemini or OpenRouter; Work Desk works out which.</p>
          <p class="key-now" id="chat-now"></p>
          <form class="note-row key-form" data-kind="chat"><input type="password" id="ai-key" placeholder="Paste a chat model API key" autocomplete="off" aria-label="Chat model API key"><button class="btn" type="submit">Save</button></form>
        </div>
        <div class="key-block">
          <p class="label">Jev (TypeSafe)</p>
          <p class="alert-detail">Optional. Makes the structured calls: which project a note belongs to, repeats, task sizes for the brief, and how far along work is. Get a key at console.typesafe.ai.</p>
          <p class="key-now" id="jev-now"></p>
          <form class="note-row key-form" data-kind="jev"><input type="password" id="jev-key" placeholder="Paste your TypeSafe key" autocomplete="off" aria-label="Jev (TypeSafe) API key"><button class="btn" type="submit">Save</button></form>
        </div>
        <p class="alert-detail">Keys stay on this computer.</p>
        <form id="hours-form" class="note-row hours-row"><label for="hours-in">Hours of project work on a typical day</label><input type="number" id="hours-in" min="0.5" max="24" step="0.5"><button class="mini" type="submit">Save</button></form>
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
    """<span class="pill ${p.tier==='major'?'major':''}">${esc(p.tier||'standard')}</span>${p.status&&p.status!=='active'?`<span class="pill">${esc(p.status)}</span>`:''}${dl}${!(p.deliverable||'').trim()&&(p.status==='active'||p.status==='wrapping'||!p.status)?`<button class="chip soon nodeliv" type="button" data-setup="${esc(p.id)}" title="What will this project produce? Click to suggest a framework.">no deliverable</button>`:''}<button class="mini" data-pedit="${esc(p.id)}" type="button">Edit</button></div></div>""")

# ---- deliverables: a warning for active projects without one, and a framework to set one ----
rep("""    if(empties.length) out.push({lv:'info',type:'group',""",
    """    for(const p of ordered()){
      if(!(p.status==='active'||p.status==='wrapping'||!p.status) || snoozed(p) || (p.deliverable||'').trim()) continue;
      out.push({lv:'info',type:'project',id:p.id,proj:p.id,title:p.name,detail:'No deliverable yet: what will this project produce?',setup:true});
    }
    if(empties.length) out.push({lv:'info',type:'group',""")
rep("""        ${a.plan?`<button class="mini primary" data-act="plan" data-k="${k}">Plan dates</button>`:''}""",
    """        ${a.plan?`<button class="mini primary" data-act="plan" data-k="${k}">Plan dates</button>`:''}
        ${a.setup?`<button class="mini primary" data-act="setup" data-k="${k}">Suggest a framework</button>`:''}
        ${a.closeout?`<button class="mini primary" data-act="closeout" data-k="${k}">Close out</button>`:''}""")
rep("""    if(b.dataset.act==='open'){ filter=a.proj;""", """    if(b.dataset.act==='setup'){ openFramework(a.id); return; }
    if(b.dataset.act==='closeout'){ openCloseout(a.id); return; }
    if(b.dataset.act==='open'){ filter=a.proj;""")
# a project whose every task is done should be closed out
rep("""      if(!pi.length){ empties.push(p); continue; }""",
    """      if(!pi.length){ empties.push(p); continue; }
      if(!po.length){ out.push({lv:'info',type:'project',id:p.id,proj:p.id,title:p.name,detail:'Every task is done. Close it out?',closeout:true}); continue; }""")
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
.key-block{display:flex;flex-direction:column;gap:6px;padding-top:10px;border-top:1px solid var(--line)}
.key-block .label{margin:0}
.key-block .alert-detail{margin:0}
.key-now{margin:0;font-size:.88rem}
.setup>.alert-detail{margin:0}
.keys-btn{white-space:nowrap}
.hours-row{align-items:center;font-size:.88rem;color:var(--muted)}
.hours-row input{flex:0 0 80px!important;font-family:inherit!important}
.triage{grid-column:2/-1;display:flex;flex-direction:column;gap:6px;margin-top:-4px}
.triage-why{margin:0;font-size:.82rem;color:var(--muted)}
.triage-by{font-family:var(--mono);font-size:.66rem;color:var(--faint);border:1px solid var(--line);border-radius:6px;padding:0 5px;margin-left:4px}
.triage-acts{display:flex;flex-wrap:wrap;gap:6px}
.fdlg{width:min(560px,calc(100vw - 32px))}
.fd-why{margin:0;font-size:.92rem;color:var(--muted)}
.fd-why b{color:var(--fg)}
.fd-orig summary{cursor:pointer;color:var(--muted);font-size:.85rem}
.fd-orig p{margin:6px 0 0;font-size:.85rem;color:var(--muted);white-space:pre-wrap}
.fd-tasks{display:flex;flex-direction:column;gap:8px;margin:0;padding:0;list-style:none}
.fd-task{display:grid;grid-template-columns:minmax(0,1fr) 150px 28px;gap:8px;align-items:start}
.fd-task textarea{width:100%;padding:7px 10px;border-radius:9px;border:1px solid var(--line);background:var(--surface);color:var(--fg);font:inherit;resize:vertical}
.fd-task input{height:36px;width:100%}
@media (max-width:520px){.fd-task{grid-template-columns:minmax(0,1fr) 28px}.fd-task input{grid-column:1}}
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
    if(r.also?.length) return `Filed ${r.also.length+1} tasks under ${pName(r.project)}.`;
    return `Filed under ${pName(r.project)}${r.due?` · due ${fmt(r.due)}`:''}: “${r.text}”`;
  }
  function flash(id,msg){ $(id).textContent=msg; setTimeout(()=>{ if($(id).textContent===msg) $(id).textContent=''; },7000); }

  // aiOn: some AI can file notes. chatOn: a chat model, which the brief needs (Jev alone can't write one).
  let aiOn=false, chatOn=false, keysOpen=false, projectsRoot='~/Documents/Projects/Repos', editorName='Visual Studio Code';
  async function loadStatus(){
    try{
      const s=await db.call('GET','/api/status'); aiOn=!!s.ai; chatOn=!!s.chat; projectsRoot=s.projects_root||projectsRoot; editorName=s.editor||editorName; projectKinds=s.kinds||projectKinds;
      $('ai-setup').hidden=aiOn && !keysOpen; $('ai-close').hidden=!aiOn; $('brief-btn').disabled=!chatOn;
      $('chat-now').innerHTML=s.chat?`Using ${esc(s.chat)}. <button class="linkish" type="button" data-forget="chat">Remove</button>`:'No key yet.';
      $('jev-now').innerHTML=s.jev?'On. <button class="linkish" type="button" data-forget="jev">Remove</button>':'Not set.';
      if(document.activeElement!==$('hours-in')) $('hours-in').value=s.hours_per_day||4;
      $('foot').innerHTML=`Saved to <code>${esc(s.data_file||'')}</code>`+(s.data_error?`<span class="warn"> · ${esc(s.data_error)}. Showing the last good copy.</span>`:'')
        +` · <button class="linkish" type="button" data-ai-open>AI keys</button> · <button class="linkish" type="button" data-sound>${soundOn()?'Sound on':'Sound off'}</button>`;
      if(!chatOn && !$('brief-out').querySelector('.brief-head')) $('brief-out').innerHTML='<p class="empty">Add an API key under AI keys to get a daily brief.</p>';
    }catch(_){}
  }
  // Chat keys are matched to their service; the Jev box saves the key as Jev's (after TypeSafe accepts it).
  $('ai-setup').addEventListener('submit', async e=>{
    const f=e.target.closest('.key-form'); if(!f) return;
    e.preventDefault();
    const input=f.querySelector('input'), key=input.value.trim(); if(!key) return;
    keysOpen=true; $('key-status').textContent='Checking the key…';
    try{
      const r=await db.call('POST','/api/settings', f.dataset.kind==='jev'?{key, provider:'jev'}:{key});
      input.value=''; $('key-status').textContent=r.message||'Saved.'; await loadStatus();
      if(chatOn && !$('brief-out').querySelector('.brief-head')) $('brief-out').innerHTML='<p class="empty">Ask Work Desk what to focus on today.</p>';
    }catch(err){ $('key-status').textContent=err.message; }
  });
  $('hours-form').addEventListener('submit', async e=>{
    e.preventDefault();
    try{ await db.call('POST','/api/settings',{hours_per_day:parseFloat($('hours-in').value)}); $('key-status').textContent='Saved. The brief plans around that.'; }
    catch(err){ $('key-status').textContent=err.message; }
  });
  document.addEventListener('click', async e=>{
    if(e.target.closest('[data-ai-open]')){
      if(view!=='desk') setView('desk');
      keysOpen=true; $('ai-setup').hidden=false; $('ai-setup').scrollIntoView({block:'start', behavior:'smooth'});
      setTimeout(()=>(chatOn?$('jev-key'):$('ai-key')).focus({preventScroll:true}),300); return;
    }
    if(e.target.closest('#ai-close')){ keysOpen=false; $('key-status').textContent=''; loadStatus(); return; }
    const su=e.target.closest('[data-setup]');
    if(su){ openFramework(su.dataset.setup); return; }
    const snd=e.target.closest('[data-sound]');
    if(snd){ setSound(!soundOn()); snd.textContent=soundOn()?'Sound on':'Sound off'; if(soundOn()) chime(false); return; }
    const fg=e.target.closest('[data-forget]');
    if(fg){ try{ await db.call('POST','/api/settings',{forget:fg.dataset.forget}); $('key-status').textContent='Removed.'; await loadStatus(); }
      catch(err){ $('key-status').textContent=err.message; } }
  });

  // ---- "Sort it for me": suggest, then confirm ----
  // Nothing is saved until you press File. The dialog also works with no AI: pick the project yourself.
  let fdSug=null, fdNote='', fdResolve=null;
  const goLabel=(pid,n=1)=>{ const what=n>1?`${n} tasks`:''; return pid==='inbox'?`Put ${what||'it'} in the Inbox`:`File ${what?what+' ':''}under ${pName(pid)}`; };
  const fdCount=()=>$('file-dlg').querySelectorAll('.fd-task').length||1;
  async function confirmFiling(text){
    let s;
    try{ s=await db.call('POST','/api/suggest',{text}); }
    catch(err){ s={project:'inbox', text, due:null, error:err.message}; }
    const r=await askWhere(s, text); if(!r) return null;
    const docs=r.tasks.map(t=>{
      const doc={text:t.text, project:r.project, tidy:false};
      if(t.text!==text) doc.original=text;
      if(t.due) doc.due=t.due;
      if(r.project==='inbox' && s.triage) doc.triage=s.triage;
      return doc;
    });
    return {docs, newProject:r.newProject||null};
  }
  function askWhere(s, note){
    fdSug=s; fdNote=note;
    const d=$('file-dlg'), t=s.triage;
    const tasks=(s.tasks&&s.tasks.length)?s.tasks:[{text:note, due:null}];
    const dup=s.duplicate?`<p class="warn" style="margin:0">This may already be a task: “${esc(s.duplicate.text)}” <span class="triage-by" title="Jev's estimate">Jev ${Math.round(s.duplicate.p*100)}%</span></p>`:'';
    const fields=tasks.length===1
      ? `<textarea id="fd-text" class="td-title" rows="2" aria-label="Task">${esc(tasks[0].text)}</textarea>`
      : `<p class="fd-why">Work Desk split this into ${tasks.length} tasks. Edit or remove any of them.</p>
         <ul class="fd-tasks">${tasks.map((x,i)=>`<li class="fd-task" data-i="${i}"><textarea rows="2" aria-label="Task ${i+1}">${esc(x.text)}</textarea><input type="date" aria-label="Due" value="${esc(x.due||'')}"><button type="button" class="icon-btn" data-fdrm aria-label="Remove this task">${ICON.x}</button></li>`).join('')}</ul>`;
    const why= s.error?`<p class="warn" style="margin:0">${esc(s.error)} Choose a project yourself.</p>`
      : t?`<p class="fd-why">${esc(t.reason)}${t.by==='jev'?'<span class="triage-by" title="Jev made this call">Jev</span>':''}</p>`
      : `<p class="fd-why">Work Desk suggests <b>${esc(pName(s.project))}</b>.${s.warning?` ${esc(s.warning)}`:''}</p>`;
    d.innerHTML=`<form class="td-body" id="fd-form">
      <div class="td-head"><h3 class="sec-h">Where does this go?</h3><button type="button" class="icon-btn close" data-fdcancel aria-label="Cancel">${ICON.x}</button></div>
      ${why}
      ${dup}
      ${fields}
      ${tasks.length>1||tasks[0].text!==note?`<details class="fd-orig"><summary>Your note as typed</summary><p>${esc(note)}</p></details>`:''}
      <div class="ed-grid td-fields">
        <div class="field"><label for="fd-proj">Project</label><select id="fd-proj"><option value="inbox">Inbox</option>${ordered().map(p=>`<option value="${esc(p.id)}" ${p.id===s.project?'selected':''}>${esc(p.name)}</option>`).join('')}</select></div>
        ${tasks.length===1?`<div class="field"><label for="fd-due">Due</label><input type="date" id="fd-due" value="${esc(tasks[0].due||'')}"></div>`:''}
      </div>
      ${t?.suggest?`<div><button type="button" class="mini" data-fdnew>New project: “${esc(t.suggest)}”</button></div>`:''}
      <div class="td-foot"><span class="td-spacer"></span><button type="button" class="ghost" data-fdcancel>Cancel</button><button type="submit" class="btn" id="fd-go">${esc(goLabel(s.project, tasks.length))}</button></div>
    </form>`;
    d.showModal(); setTimeout(()=>$('fd-go')?.focus(),0);
    return new Promise(res=>{ fdResolve=res; });
  }
  function finishFiling(extra){
    const d=$('file-dlg'), rows=[...d.querySelectorAll('.fd-task')];
    const tasks=rows.length
      ? rows.map(r=>({text:r.querySelector('textarea').value.trim(), due:r.querySelector('input').value||null})).filter(t=>t.text)
      : [{text:$('fd-text').value.trim()||fdNote, due:$('fd-due').value||null}];
    if(!tasks.length) return;
    const res=fdResolve; fdResolve=null;
    const project=$('fd-proj').value;
    d.close();
    res?.({tasks, project, ...extra});
  }
  // Start the project an Inbox note suggested, moving every note that suggested the same name.
  function startProjectFor(name, newIds){
    if(view!=='desk') setView('desk');
    const key=name.toLowerCase();
    const ids=items.filter(i=>i.project==='inbox' && i.status!=='done' && (i.triage?.suggest||'').toLowerCase()===key).map(i=>i.id);
    for(const id of newIds) if(!ids.includes(id)) ids.push(id);
    newProjectFrom(name, ids);
  }
  const fdlg=$('file-dlg');
  fdlg.addEventListener('submit', e=>{ e.preventDefault(); finishFiling({}); });
  fdlg.addEventListener('click', e=>{
    if(e.target===fdlg || e.target.closest('[data-fdcancel]')){ fdlg.close(); return; }
    if(e.target.closest('[data-fdnew]')){ $('fd-proj').value='inbox'; finishFiling({newProject:fdSug.triage.suggest}); return; }
    const rm=e.target.closest('[data-fdrm]');
    if(rm && fdCount()>1){ rm.closest('.fd-task').remove(); $('fd-go').textContent=goLabel($('fd-proj').value, fdCount()); }
  });
  fdlg.addEventListener('change', e=>{ if(e.target.id==='fd-proj') $('fd-go').textContent=goLabel(e.target.value, fdCount()); });
  fdlg.addEventListener('keydown', e=>{ if(e.target.id==='fd-text' && e.key==='Enter' && !e.shiftKey){ e.preventDefault(); fdlg.querySelector('form').requestSubmit(); } });
  fdlg.addEventListener('close', ()=>{ if(fdResolve){ const res=fdResolve; fdResolve=null; res(null); } });

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
    if(v.note) h+=`<p class="brief-meta">${esc(v.note)}</p>`;
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
      <div class="field"><label for="pf-deliv">Deliverable (the one thing it produces)</label><input id="pf-deliv" name="deliverable" value="${esc(d.deliverable||'')}" placeholder="e.g. Manuscript submitted to Genome Biology"></div>
      <div class="pf-row">
        <div class="field"><label for="pf-deadline">Deadline</label><input type="date" id="pf-deadline" name="deadline" value="${esc(d.deadline||'')}"></div>
        <div class="field"><label for="pf-tier">Tier</label><select id="pf-tier" name="tier">${opt(['major','standard'],d.tier||'standard')}</select></div>
        <div class="field"><label for="pf-status">Status</label><select id="pf-status" name="status">${opt(['active','wrapping','waiting','done'],d.status||'active')}</select></div>
      </div>
      <div class="field"><label for="pf-context">Notes (repo, people, links)</label><input id="pf-context" name="context" value="${esc(d.context||'')}"></div>
      ${p.id?'':`<label class="pf-check"><input type="checkbox" name="plan" ${d.plan===false||!chatOn?'':'checked'} ${chatOn?'':'disabled title="Needs a chat model key (AI keys)"'}> Suggest a framework: kind of project, deliverable and milestones</label>
      <label class="pf-check"><input type="checkbox" name="make_folder" ${d.make_folder===false?'':'checked'}> Create a project folder with a git repo in <code>${esc(projectsRoot)}</code></label>`}
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
    for(const k of ['name','goal','deliverable','deadline','tier','status','context']) data[k]=(f.get(k)||'').toString().trim();
    if(!data.name) return;
    const id=e.target.dataset.id;
    try{
      if(id) await db.doc('projects/'+id).update(data);
      else{
        const body={...data, order:Math.max(0,...projects.map(p=>p.order||0))+1, make_folder:f.get('make_folder')==='on', move:projMove};
        if(f.get('plan')==='on'){
          const fw=await askFramework({name:data.name, deadline:data.deadline, deliverable:data.deliverable, wantFolder:body.make_folder}, {project:data});
          if(!fw) return; // cancelled: the form stays as it was
          Object.assign(body, {type:fw.type, role:fw.role, deliverable:fw.deliverable, deadline:fw.deadline||data.deadline, summary:fw.summary, milestones:fw.milestones, make_folder:fw.make_folder});
        }
        const r=await db.collection('projects').add(body);
        if(projMove.length||body.milestones?.length) await db.refreshAll();
        if(r?.notice) toast(r.notice, /^Couldn|^Didn|couldn/.test(r.notice));
      }
      editingProj=null; projDraft={}; projMove=[]; renderDesk();
    }catch(err){ $('cap-status').textContent='Could not save the project: '+err.message; }
  });

  render();
  (async()=>{""")

# ---- project pages, task details, linked files and finishing tasks (ui/project.js, celebrate.js, project.css) ----
here = os.path.dirname(os.path.abspath(__file__))
PROJECT_JS = open(os.path.join(here, "project.js"), encoding="utf-8").read()
PROJECT_CSS = open(os.path.join(here, "project.css"), encoding="utf-8").read()
CELEBRATE_JS = open(os.path.join(here, "celebrate.js"), encoding="utf-8").read()

rep("  <!-- TIMELINE VIEW -->", """  <!-- PROJECT PAGE -->
  <div id="proj-view" hidden></div>
  <dialog id="task-dlg" class="tdlg" aria-label="Task details"></dialog>
  <dialog id="file-dlg" class="tdlg fdlg" aria-label="Where does this note go?"></dialog>
  <dialog id="plan-dlg" class="tdlg fdlg" aria-label="Project framework"></dialog>
  <dialog id="close-dlg" class="tdlg" aria-label="Close out a project"></dialog>
  <div class="toast" id="toast" role="status" aria-live="polite"></div>
  <div class="drop-hint" id="drop-hint" hidden></div>

  <!-- TIMELINE VIEW -->""")
rep(".proj-form .field input:not([type=date]){width:100%}\n",
    ".proj-form .field input:not([type=date]){width:100%}\n\n" + PROJECT_CSS)
# the code goes in early, so its constants exist before anything renders
rep("  const saveFilter=()=>{ try{localStorage.setItem('wd-filter',filter);}catch(_){} };\n",
    "  const saveFilter=()=>{ try{localStorage.setItem('wd-filter',filter);}catch(_){} };\n\n" + PROJECT_JS + "\n" + CELEBRATE_JS)

# ---- finishing a task: confetti wherever a task becomes done (ui/celebrate.js) ----
rep("""    try{ await db.doc('items/'+id).update(patch); }
    catch(err){ $('cap-status').textContent='Could not move that card. Try again in a moment.'; }""",
    """    const was=items.find(i=>i.id===id);
    try{ await db.doc('items/'+id).update(patch); if(stage==='done' && was && was.status!=='done') celebrate(was); }
    catch(err){ $('cap-status').textContent='Could not move that card. Try again in a moment.'; }""")
rep("""    try{ await db.doc('items/'+id).update({status:'done', stage:'done', done_at:new Date().toISOString(), updated:new Date().toISOString()}); }""",
    """    const was=items.find(i=>i.id===id);
    try{ await db.doc('items/'+id).update({status:'done', stage:'done', done_at:new Date().toISOString(), updated:new Date().toISOString()}); if(was) celebrate(was); }""")

# cards: click for details, file chips, drop target
rep("""<div class="card ${st==='done'?'done':''}" draggable="true" data-id="${esc(it.id)}">""",
    """<div class="card ${st==='done'?'done':''}" draggable="true" data-id="${esc(it.id)}" data-drop="items:${esc(it.id)}">""")
rep('<div class="card-txt">${esc(it.text)}</div>',
    '<div class="card-txt" data-open="${esc(it.id)}" title="Open details and files">${esc(it.text)}</div>${fileChips(it)}')
# (card is also called as .map(card), so the project page turns tags off with a flag, not an argument)
rep("${filter==='all'?tag(it.project):''}", "${(cardTags??filter==='all')?tag(it.project):''}")
# desk rows: clicking a task opens its details. (On claude.ai the text was the checkbox's label, so a
# stray click finished the task.) Finishing happens in task details or by dragging to Done.
rep("""<input type="checkbox" id="cb-${esc(it.id)}" data-id="${esc(it.id)}" aria-label="Mark done"><label class="txt" for="cb-${esc(it.id)}"><span>${esc(it.text)}</span>${showProj?tag(it.project):''}</label>""",
    """<span class="row-dot" aria-hidden="true"></span><button type="button" class="txt row-txt" data-open="${esc(it.id)}" title="Open details"><span>${esc(it.text)}</span>${showProj?tag(it.project):''}</button>""")
# desk rows: file count and a details button
rep('return `<li class="row"><span class="row-dot"', 'return `<li class="row" data-drop="items:${esc(it.id)}"><span class="row-dot"')
rep('<span class="row-end">${extra||dueChip(it.due)}</span></li>',
    '<span class="row-end">${fileCount(it)}${extra||dueChip(it.due)}<button class="row-open" type="button" data-open="${esc(it.id)}" '
    'aria-label="Open details and files" title="Details and files">${ICON.open}</button></span></li>')
# desk project cards link to their page
rep('<div class="proj-name"><span class="swatch"></span><h3>${esc(p.name)}</h3></div>',
    '<div class="proj-name"><span class="swatch"></span><h3><a class="plink" href="${pageHref(p.id)}">${esc(p.name)}</a></h3></div>')
rep("""<article class="proj ${p.status==='waiting'?'quiet':''}" style="--h:${h}">""",
    """<article class="proj ${p.status==='waiting'||p.status==='done'?'quiet':''}" style="--h:${h}" data-drop="projects:${esc(p.id)}">""")
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
