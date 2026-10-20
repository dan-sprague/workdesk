  // ================= Project pages, task details, linked files =================
  // Local app only: build_ui.py inserts this into the claude.ai page's script.
  // Tasks and projects keep `files: [{path, name, kind}]` (kind: file, folder or link);
  // projects also have `summary`, `notes` and one `folder`.
  const ICON={
    file:'<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4 1.5h5L12.5 5v9.5h-8.5z M9 1.5V5h3.5" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round"/></svg>',
    folder:'<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M1.5 3h4.6l1.6 1.7h6.8v8.8h-13z" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round"/></svg>',
    link:'<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M6.6 9.4l2.8-2.8M7.2 4.6l1.1-1.1a2.6 2.6 0 013.7 3.7l-1.1 1.1M8.8 11.4l-1.1 1.1a2.6 2.6 0 01-3.7-3.7l1.1-1.1" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/></svg>',
    clip:'<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M10.6 4.6L5.9 9.3a1.4 1.4 0 002 2l5-5a2.8 2.8 0 00-4-4l-5 5a4.2 4.2 0 006 6l4-4" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/></svg>',
    open:'<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M6 3.5l4.5 4.5L6 12.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>',
    x:'<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>',
    check:'<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3.2 8.6l3 3 6.6-7" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>',
  };
  const VIEW_NAMES={desk:'Desk',board:'Board',tl:'Timeline'};
  let pageId=null, pageEdit=false, pageDraft={}, pageAdd='', pageAddStage='todo', doneShown=6, lastView='desk';
  let dlgId=null, dlgDelete=false, notesTimer=null, cardTags=null, promptBusy=null, promptMode='';
  const promptStale=new Map(); // task id -> what changed in the project folder since its prompt was drafted
  const fstat=new Map(); // path -> what /api/stat said about it on this computer

  const filesOf=x=>Array.isArray(x?.files)?x.files.filter(f=>f&&f.path):[];
  const isLink=p=>/^https?:\/\//.test(p||'');
  const baseName=p=>isLink(p)?p.replace(/^https?:\/\//,'').replace(/\/$/,''):(p.replace(/\/+$/,'').split('/').pop()||p);
  const dirName=p=>{ if(isLink(p)) return ''; const q=p.replace(/\/+$/,''), i=q.lastIndexOf('/'); return i>0?q.slice(0,i):''; };
  const fileLabel=f=>f.name||baseName(f.path);
  const kindIcon=f=>ICON[f.kind==='folder'?'folder':(f.kind==='link'||isLink(f.path))?'link':'file'];
  const short=(s,n=70)=>{ s=String(s||''); return s.length>n?s.slice(0,n-1)+'…':s; };
  const itemOf=id=>items.find(i=>i.id===id);
  const pageHref=id=>'#/p/'+encodeURIComponent(id);
  function docOf(target){ const [coll,id]=target.split(/:(.*)/s); return {coll,id,doc:coll==='items'?itemOf(id):projOf(id)}; }

  function toast(msg, warn){
    const t=$('toast'); t.textContent=msg; t.classList.toggle('warn',!!warn); t.classList.add('show');
    clearTimeout(toast.timer); toast.timer=setTimeout(()=>t.classList.remove('show'), warn?6500:3800);
  }

  // Re-rendering replaces the DOM; keep whatever the person is typing in.
  function keepFocus(root, paint){
    const a=document.activeElement;
    const typing=n=>n.tagName==='TEXTAREA'||(n.tagName==='INPUT'&&!['date','checkbox','button','submit'].includes(n.type));
    const st=a && a!==document.body && root.contains(a) && a.id ? {id:a.id, v:a.value, s:a.selectionStart, e:a.selectionEnd, top:a.scrollTop, typing:typing(a)} : null;
    paint();
    if(!st) return;
    const n=document.getElementById(st.id); if(!n) return;
    if(st.typing && typing(n)) n.value=st.v;
    n.focus({preventScroll:true});
    try{ if(st.typing && st.s!=null) n.setSelectionRange(st.s, st.e); }catch(_){}
    if(st.top) n.scrollTop=st.top;
  }

  // ---- linked files ----
  function fileChips(it){
    const fs=filesOf(it); if(!fs.length) return '';
    return `<div class="fchips">${fs.slice(0,2).map(f=>`<button type="button" class="fileref" data-fopen="${esc(f.path)}" title="Open ${esc(f.path)}">${kindIcon(f)}<span>${esc(fileLabel(f))}</span></button>`).join('')}${fs.length>2?`<button type="button" class="fileref more" data-open="${esc(it.id)}" title="All linked files">+${fs.length-2}</button>`:''}</div>`;
  }
  function fileCount(it){ const n=filesOf(it).length; return n?`<span class="chip fcount" title="${n} linked file${n>1?'s':''}">${ICON.clip}${n}</span>`:''; }

  function fileRow(f, target, opt={}){
    const st=fstat.get(f.path), missing=st && st.exists===false;
    return `<li class="file ${missing?'missing':''}">
      <span class="ficon">${kindIcon(f)}</span>
      <div class="fmain">
        <button type="button" class="fname" data-fopen="${esc(f.path)}" title="Open ${esc(f.path)}">${esc(fileLabel(f))}</button>
        <span class="fpath" title="${esc(f.path)}">${esc(isLink(f.path)?f.path:dirName(f.path))}</span>
        ${opt.task?`<button type="button" class="ftask" data-open="${esc(opt.task.id)}">${esc(short(opt.task.text,64))}</button>`:''}
        ${missing?'<span class="chip late">not on this Mac</span>':''}
      </div>
      <span class="factions">
        ${!isLink(f.path)?`<button type="button" class="mini" data-freveal="${esc(f.path)}" title="Show in Finder">Show</button>`:''}
        ${opt.removable===false?'':`<button type="button" class="icon-btn" data-funlink="${esc(target)}" data-path="${esc(f.path)}" aria-label="Unlink ${esc(fileLabel(f))}" title="Unlink (the file itself stays where it is)">${ICON.x}</button>`}
      </span>
    </li>`;
  }
  function linkControls(target){
    return `<div class="flink-row">
      <button type="button" class="mini" data-fpick="${esc(target)}">${ICON.file}Link files…</button>
      <button type="button" class="mini" data-fpick="${esc(target)}" data-folder="1">${ICON.folder}Link folder…</button>
      <form class="flink" data-target="${esc(target)}"><input id="fl-${esc(target)}" name="p" placeholder="or paste a path or link" aria-label="Path or link to add" autocomplete="off" spellcheck="false"><button class="mini" type="submit">Add</button></form>
    </div>${window.__deskApp?'<p class="fhint">You can also drag files from Finder onto a task or onto this list.</p>':''}`;
  }

  async function ensureStats(paths){
    const need=[...new Set(paths)].filter(p=>p && !isLink(p) && !fstat.has(p));
    if(!need.length || !db) return;
    need.forEach(p=>fstat.set(p,{pending:true}));
    let r;
    try{ r=await db.call('POST','/api/stat',{paths:need}); }catch(_){ need.forEach(p=>fstat.delete(p)); return; }
    r.forEach((s,k)=>{ fstat.set(need[k],s); fstat.set(s.path,s); });
    if(r.some(s=>!s.exists)){ if(view==='proj') renderPage(); refreshTaskDlg(); }
  }

  async function linkFiles(target, raw){
    const {coll,id,doc}=docOf(target); if(!doc||!raw.length) return;
    let st;
    try{ st=await db.call('POST','/api/stat',{paths:raw}); }catch(err){ toast('Could not check those files: '+err.message, true); return; }
    st.forEach(s=>fstat.set(s.path,s));
    const ok=st.filter(s=>s.link||(s.exists&&s.absolute)), notFound=st.filter(s=>!ok.includes(s));
    const have=new Set(filesOf(doc).map(f=>f.path)), add=[];
    for(const s of ok){ if(have.has(s.path)) continue; have.add(s.path); add.push({path:s.path, name:s.name, kind:s.link?'link':s.dir?'folder':'file', added:new Date().toISOString()}); }
    try{ if(add.length) await db.doc(`${coll}/${id}`).update({files:[...filesOf(doc),...add]}); }
    catch(err){ toast('Could not link: '+err.message, true); return; }
    const who=coll==='items'?`“${short(doc.text,48)}”`:doc.name;
    if(notFound.length) toast(`Couldn't find ${notFound.map(s=>s.input).join(', ')} on this computer.${add.length?` Linked the other${add.length>1?'s':''}.`:''}`, true);
    else if(add.length) toast(`Linked ${add.length===1?add[0].name:add.length+' items'} to ${who}.`);
    else toast(`Already linked to ${who}.`);
  }
  async function unlinkFile(target, path){
    const {coll,id,doc}=docOf(target); if(!doc) return;
    try{ await db.doc(`${coll}/${id}`).update({files:filesOf(doc).filter(f=>f.path!==path)}); }
    catch(err){ toast('Could not unlink: '+err.message, true); }
  }
  async function openFile(path, reveal){
    try{ await db.call('POST','/api/open',{path, reveal:!!reveal}); }
    catch(err){ toast(err.message, true); if(/isn't on this computer/.test(err.message)){ fstat.set(path,{exists:false}); if(view==='proj') renderPage(); refreshTaskDlg(); } }
  }
  function startDir(target){
    const {coll,doc}=docOf(target), p=coll==='projects'?doc:projOf(doc?.project);
    const first=filesOf(doc).find(f=>!isLink(f.path));
    return first?(first.kind==='folder'?first.path:dirName(first.path)):(p?.folder||'');
  }
  async function pickAndLink(target, folder){
    try{ const r=await db.call('POST','/api/pick',{folder, start:startDir(target)}); if(r.paths?.length) await linkFiles(target, r.paths); }
    catch(err){ toast('Could not open the file chooser: '+err.message, true); }
  }

  // ---- files dragged in from Finder (desktop app) ----
  let dropEl=null;
  function dropTargetAt(x,y){
    if($('task-dlg').open && dlgId) return {el:$('task-dlg').querySelector('.td-files'), target:'items:'+dlgId};
    const t=document.elementFromPoint(x,y)?.closest?.('[data-drop]');
    if(!t) return null;
    // Anywhere else on a project page links to the project; show that on its Files panel.
    const el=t.classList.contains('pp')?(t.querySelector('.pp-files')||t):t;
    return {el, target:t.dataset.drop};
  }
  function dropLabel(target){ const {coll,doc}=docOf(target); return !doc?'':coll==='items'?`Link to “${short(doc.text,40)}”`:`Link to ${doc.name}`; }
  window.__deskDrop={
    over(x,y){
      const t=dropTargetAt(x,y), el=t?.el||null;
      if(el!==dropEl){ dropEl?.classList.remove('drop-target'); el?.classList.add('drop-target'); dropEl=el; }
      const h=$('drop-hint'); h.hidden=!t;
      if(t){ h.textContent=dropLabel(t.target); h.style.left=Math.min(x+16, innerWidth-h.offsetWidth-8)+'px'; h.style.top=(y+18)+'px'; }
    },
    leave(){ dropEl?.classList.remove('drop-target'); dropEl=null; $('drop-hint').hidden=true; },
    drop(paths,x,y){
      const t=dropTargetAt(x,y); this.leave();
      if(!t){ toast('Drop files on a task, or anywhere on a project page, to link them.'); return; }
      linkFiles(t.target, paths);
    },
  };

  // ---- simple text formatting for overviews: paragraphs, "- " bullets, **bold**, links ----
  function fmtText(s){
    const inline=t=>esc(t).replace(/\*\*(.+?)\*\*/g,'<strong>$1</strong>').replace(/(https?:\/\/[^\s<]+[^\s<.,;:)])/g,'<a href="$1" target="_blank" rel="noopener">$1</a>');
    const bullet=l=>/^\s*[-•*]\s+/.test(l);
    return String(s||'').trim().split(/\n\s*\n/).map(block=>{
      // A block can mix a lead-in line with the bullets under it.
      let out='', run=[], runBullets=false;
      const flush=()=>{
        if(!run.length) return;
        out += runBullets ? `<ul>${run.map(l=>`<li>${inline(l.replace(/^\s*[-•*]\s+/,''))}</li>`).join('')}</ul>`
                          : `<p>${run.map(inline).join('<br>')}</p>`;
        run=[];
      };
      for(const line of block.split('\n')){
        if(bullet(line)!==runBullets){ flush(); runBullets=bullet(line); }
        run.push(line);
      }
      flush();
      return out;
    }).join('');
  }

  // ---- routing: #/p/<project id> opens that project's page ----
  function route(){
    const m=location.hash.match(/^#\/p\/(.+)$/);
    if(m){
      const id=decodeURIComponent(m[1]);
      if(id!==pageId){ pageEdit=false; pageDraft={}; pageAdd=''; doneShown=6; fstat.clear(); }
      pageId=id; setView('proj'); window.scrollTo(0,0);
    } else if(view==='proj') setView(lastView);
  }
  window.addEventListener('hashchange', route);
  function boardPageLink(){ return filter!=='all' && projOf(filter) ? `<a class="mini pp-open" href="${pageHref(filter)}">Open project page ${ICON.open}</a>` : ''; }

  // ---- project page ----
  function renderPage(){
    const el=$('proj-view');
    if(!projects.length){ el.innerHTML='<p class="empty">Loading…</p>'; return; }
    const p=projOf(pageId);
    if(!p){ el.innerHTML=`<div class="pp"><nav class="pp-nav"><a class="pp-back" href="#">← ${VIEW_NAMES[lastView]}</a></nav><p class="empty">That project isn't here any more.</p></div>`; return; }
    const h=hue(p.id), all=items.filter(i=>i.project===p.id), open=all.filter(i=>i.status!=='done');
    const by={todo:[],doing:[],review:[],done:[]}; all.forEach(i=>by[stageOf(i)].push(i));
    ['todo','doing','review'].forEach(s=>by[s].sort(sortOpen));
    by.done.sort((a,b)=>(b.done_at||'').localeCompare(a.done_at||''));
    const openBy={}; items.forEach(i=>{ if(i.status!=='done') openBy[i.project]=(openBy[i.project]||0)+1; });
    const dd=days(p.deadline);
    const dlChip=p.deadline?`<span class="chip ${dd<0?'late':dd<=14?'soon':''}">${dd<0?'past deadline':dd===0?'due today':dd+' days left'}</span>`:'';
    const dated=open.filter(i=>i.due).sort((a,b)=>a.due.localeCompare(b.due));
    const nextDue=dated.find(i=>days(i.due)>=0);
    const overdue=dated.filter(i=>days(i.due)<0).length;
    const bar=all.length?`<div class="mini-bar" aria-hidden="true">${['done','review','doing','todo'].map(s=>by[s].length?`<i style="width:${by[s].length/all.length*100}%;background:hsl(${h} 70% ${s==='done'?45:s==='review'?58:s==='doing'?68:85}%)"></i>`:'').join('')}</div>`:'';
    const stat=(b,s,cls='')=>`<div class="pp-stat ${cls}"><b>${b}</b><span>${s}</span></div>`;

    const head = pageEdit ? pageForm(p) : `
      <div class="pp-title"><span class="swatch"></span><h2>${esc(p.name)}</h2>
        <div class="meta"><span class="pill ${p.tier==='major'?'major':''}">${esc(p.tier||'standard')}</span>${p.status&&p.status!=='active'?`<span class="pill">${esc(p.status)}</span>`:''}${dlChip}<button class="mini" type="button" data-pp-edit>Edit</button></div></div>
      ${p.goal?`<p class="pp-goal">${esc(p.goal)}</p>`:`<p class="empty">No goal yet. <button type="button" class="linkish" data-pp-edit>Add one</button></p>`}
      <div class="pp-stats">${stat(open.length,'open')}${stat(by.doing.length,'in progress')}${stat(by.review.length,'in review')}${stat(by.done.length,'done')}${overdue?stat(overdue,'overdue','late'):''}${nextDue?stat(fmt(nextDue.due),'next due'):''}${p.deadline?stat(fmt(p.deadline),'deadline',dd!==null&&dd<=7?'late':''):''}</div>
      ${bar}`;

    const emptyLane={todo:'Nothing waiting',doing:'Nothing in progress',review:'Nothing to review',done:'Nothing finished yet'};
    cardTags=false; // every card on this page is the same project
    const lanes=STAGES.map(s=>{
      const list=s.id==='done'?by.done.slice(0,doneShown):by[s.id];
      return `<div class="lane pp-lane" data-stage="${s.id}"><div class="lane-head"><h3>${s.label}</h3><span class="n">${by[s.id].length}</span></div>
        <div class="lane-body">${list.map(it=>card(it)).join('')||`<p class="lane-empty">${emptyLane[s.id]}</p>`}</div>
        ${s.id==='done'&&by.done.length>doneShown?`<button class="lane-add" type="button" data-pp-moredone>Show ${by.done.length-doneShown} more</button>`:''}</div>`;
    }).join('');
    cardTags=null;

    const pf=filesOf(p), taskFiles=all.flatMap(it=>filesOf(it).map(f=>({f,it})));
    const soon=dated.filter(i=>days(i.due)<=21);
    const folder=p.folder?`<span class="pp-folder"><button type="button" class="fname" data-fopen="${esc(p.folder)}" title="Open ${esc(p.folder)}">${ICON.folder}${esc(baseName(p.folder))}</button><button type="button" class="mini" data-freveal="${esc(p.folder)}">Show</button></span><span class="fpath" title="${esc(p.folder)}">${esc(dirName(p.folder))}</span>${fstat.get(p.folder)?.exists===false?'<span class="chip late">not on this Mac</span>':''}`
      :`<button type="button" class="linkish" data-pp-folder>Link the project folder…</button>`;

    const html=`<div class="pp" style="--h:${h}" data-drop="projects:${esc(p.id)}">
      <nav class="pp-nav"><a class="pp-back" href="#">← ${VIEW_NAMES[lastView]}</a>
        <div class="filters pp-switch" aria-label="Projects">${ordered().map(q=>`<a class="fchip" href="${pageHref(q.id)}" style="--h:${hue(q.id)}" ${q.id===p.id?'aria-current="page"':''}><span class="sw"></span>${esc(q.name)}<span class="n">${openBy[q.id]||0}</span></a>`).join('')}</div></nav>
      <header class="panel pp-head">${head}</header>
      <div class="pp-grid">
        <section class="panel"><div class="panel-head"><h3 class="sec-h">Overview</h3>${p.summary&&!pageEdit?'<button class="mini" type="button" data-pp-edit>Edit</button>':''}</div>
            ${p.summary?`<div class="pp-text">${fmtText(p.summary)}</div>`:`<p class="empty">What this project is, where it stands, what's left. <button type="button" class="linkish" data-pp-edit>Write an overview</button></p>`}</section>
        <section class="panel"><h3 class="sec-h">Details</h3>
            <dl class="pp-dl">
              <dt>Folder</dt><dd>${folder}</dd>
              <dt>Deadline</dt><dd>${p.deadline?`${new Date(p.deadline+'T00:00:00').toLocaleDateString(undefined,{weekday:'short',month:'long',day:'numeric',year:'numeric'})}`:'None set'}</dd>
              <dt>Status</dt><dd>${esc(p.status||'active')} · ${esc(p.tier||'standard')}</dd>
              ${p.context?`<dt>Context</dt><dd>${esc(p.context)}</dd>`:''}
            </dl></section>
      </div>
          <section class="panel pp-tasks"><div class="panel-head"><h3 class="sec-h">Tasks</h3><button class="mini" type="button" data-pp-board>Open on board</button></div>
            <form class="pp-add" id="pp-add-form"><input id="pp-add" value="${esc(pageAdd)}" placeholder="Add a task to ${esc(p.name)}…" aria-label="New task" autocomplete="off">
              <select id="pp-add-stage" aria-label="Lane">${STAGES.filter(s=>s.id!=='done').map(s=>`<option value="${s.id}" ${pageAddStage===s.id?'selected':''}>${s.label}</option>`).join('')}</select>
              <button class="btn" type="submit">Add</button></form>
            <div class="pp-lanes">${lanes}</div>
            <p class="board-hint">Click a card for its notes and files. Drag cards between lanes.</p></section>
      <div class="pp-grid">
        <section class="panel"><div class="panel-head"><h3 class="sec-h">Notes</h3><span class="label" id="pp-notes-state">Saved as you type</span></div>
            <textarea id="pp-notes" class="pp-notes" rows="7" placeholder="Meeting notes, decisions, open questions…">${esc(p.notes||'')}</textarea>
          <div class="panel-head" style="margin-top:6px"><h3 class="sec-h">Coming up</h3><span class="label">Next 3 weeks</span></div>
            ${soon.length?`<ul class="list">${soon.map(it=>`<li class="pp-due" data-drop="items:${esc(it.id)}"><button type="button" class="ftask" data-open="${esc(it.id)}">${esc(it.text)}</button>${dueChip(it.due)}</li>`).join('')}</ul>`:'<p class="empty">Nothing dated in the next three weeks.</p>'}
        </section>
        <aside class="pp-side">
          <section class="panel pp-files" data-drop="projects:${esc(p.id)}"><div class="panel-head"><h3 class="sec-h">Files</h3><span class="label">${pf.length+taskFiles.length||''}</span></div>
            ${pf.length?`<ul class="files">${pf.map(f=>fileRow(f,'projects:'+p.id)).join('')}</ul>`:'<p class="empty">Key documents for the whole project: drafts, plans, figures.</p>'}
            ${linkControls('projects:'+p.id)}
            ${taskFiles.length?`<p class="label" style="margin-top:6px">Linked to tasks</p><ul class="files">${taskFiles.map(({f,it})=>fileRow(f,'items:'+it.id,{task:it})).join('')}</ul>`:''}
          </section>
        </aside>
      </div></div>`;
    keepFocus(el, ()=>{ el.innerHTML=html; });
    ensureStats([...pf.map(f=>f.path), ...taskFiles.map(x=>x.f.path), p.folder]);
  }

  function pageForm(p){
    const d={...p,...pageDraft};
    const opt=(list,cur)=>list.map(t=>`<option value="${t}" ${cur===t?'selected':''}>${t}</option>`).join('');
    return `<form class="proj-form" id="pp-form">
      <div class="field"><label for="ppf-name">Name</label><input id="ppf-name" name="name" value="${esc(d.name||'')}" required></div>
      <div class="field"><label for="ppf-goal">Goal</label><textarea id="ppf-goal" name="goal" rows="2">${esc(d.goal||'')}</textarea></div>
      <div class="field"><label for="ppf-summary">Overview (blank line between paragraphs, "- " for bullets)</label><textarea id="ppf-summary" name="summary" rows="8">${esc(d.summary||'')}</textarea></div>
      <div class="pf-row">
        <div class="field"><label for="ppf-deadline">Deadline</label><input type="date" id="ppf-deadline" name="deadline" value="${esc(d.deadline||'')}"></div>
        <div class="field"><label for="ppf-tier">Tier</label><select id="ppf-tier" name="tier">${opt(['major','standard'],d.tier||'standard')}</select></div>
        <div class="field"><label for="ppf-status">Status</label><select id="ppf-status" name="status">${opt(['active','wrapping','waiting','done'],d.status||'active')}</select></div>
      </div>
      <div class="field"><label for="ppf-folder">Project folder</label><div class="flink"><input id="ppf-folder" name="folder" value="${esc(d.folder||'')}" placeholder="~/Documents/Projects/…" spellcheck="false"><button class="mini" type="button" data-pp-folderpick>Choose…</button></div></div>
      <div class="field"><label for="ppf-context">Context (people, repos, links)</label><input id="ppf-context" name="context" value="${esc(d.context||'')}"></div>
      <div class="compose-row"><button class="btn" type="submit">Save</button><button class="ghost" type="button" data-pp-cancel>Cancel</button></div>
    </form>`;
  }

  async function saveNotes(v){
    clearTimeout(notesTimer);
    const p=projOf(pageId); if(!p || (p.notes||'')===v) return;
    try{ await db.doc('projects/'+p.id).update({notes:v}); const s=$('pp-notes-state'); if(s) s.textContent='Saved'; }
    catch(err){ const s=$('pp-notes-state'); if(s) s.textContent='Not saved: '+err.message; }
  }

  $('proj-view').addEventListener('click', async e=>{
    const p=projOf(pageId); if(!p) return;
    if(e.target.closest('[data-pp-edit]')){ pageEdit=true; pageDraft={}; renderPage(); setTimeout(()=>$('ppf-name')?.focus(),0); return; }
    if(e.target.closest('[data-pp-cancel]')){ pageEdit=false; pageDraft={}; renderPage(); return; }
    if(e.target.closest('[data-pp-board]')){ filter=p.id; saveFilter(); setView('board'); window.scrollTo(0,0); return; }
    if(e.target.closest('[data-pp-moredone]')){ doneShown+=20; renderPage(); return; }
    if(e.target.closest('[data-pp-folderpick]')){
      try{ const r=await db.call('POST','/api/pick',{folder:true, start:pageDraft.folder||p.folder||''}); if(r.paths?.[0]){ pageDraft.folder=r.paths[0]; const i=$('ppf-folder'); if(i) i.value=r.paths[0]; } }
      catch(err){ toast('Could not open the file chooser: '+err.message, true); }
      return;
    }
    if(e.target.closest('[data-pp-folder]')){
      try{ const r=await db.call('POST','/api/pick',{folder:true, start:''}); if(r.paths?.[0]) await db.doc('projects/'+p.id).update({folder:r.paths[0]}); }
      catch(err){ toast('Could not link the folder: '+err.message, true); }
    }
  });
  $('proj-view').addEventListener('input', e=>{
    if(e.target.closest('#pp-form') && e.target.name) pageDraft[e.target.name]=e.target.value;
    if(e.target.id==='pp-add') pageAdd=e.target.value;
    if(e.target.id==='pp-notes'){ const s=$('pp-notes-state'); if(s) s.textContent='Editing…'; clearTimeout(notesTimer); const v=e.target.value; notesTimer=setTimeout(()=>saveNotes(v),1200); }
  });
  $('proj-view').addEventListener('change', e=>{
    if(e.target.closest('#pp-form') && e.target.name) pageDraft[e.target.name]=e.target.value;
    if(e.target.id==='pp-add-stage') pageAddStage=e.target.value;
  });
  $('proj-view').addEventListener('focusout', e=>{ if(e.target.id==='pp-notes') saveNotes(e.target.value); });
  $('proj-view').addEventListener('submit', async e=>{
    const p=projOf(pageId); if(!p) return;
    if(e.target.id==='pp-add-form'){
      e.preventDefault();
      const text=pageAdd.trim(); if(!text) return;
      try{ pageAdd=''; await addItem(text, p.id, pageAddStage); $('pp-add')?.focus(); }
      catch(err){ pageAdd=text; toast('Could not add the task: '+err.message, true); renderPage(); }
    }
    if(e.target.id==='pp-form'){
      e.preventDefault();
      const f=new FormData(e.target), data={};
      for(const k of ['name','goal','summary','deadline','tier','status','folder','context']) data[k]=(f.get(k)||'').toString().trim();
      if(!data.name) return;
      if(data.folder){ try{ const [s]=await db.call('POST','/api/stat',{paths:[data.folder]}); if(!s.exists||!s.dir){ toast(`There's no folder at ${data.folder}.`, true); return; } data.folder=s.path; }catch(_){} }
      try{ await db.doc('projects/'+p.id).update(data); pageEdit=false; pageDraft={}; renderPage(); }
      catch(err){ toast('Could not save the project: '+err.message, true); }
    }
  });

  // ---- task details dialog ----
  function openTask(id){ if(!itemOf(id)) return; dlgId=id; dlgDelete=false; renderTaskDlg(); const d=$('task-dlg'); if(!d.open) d.showModal(); ensureStats(filesOf(itemOf(id)).map(f=>f.path)); checkPrompt(id); }
  function checkPrompt(id){
    if(!itemOf(id)?.prompt) return;
    db.call('POST','/api/prompt-check',{id}).then(r=>{ promptStale.set(id,r); if(dlgId===id) renderTaskDlg(); }).catch(()=>{});
  }
  function refreshTaskDlg(){ if($('task-dlg').open) renderTaskDlg(); }
  function renderTaskDlg(){
    const d=$('task-dlg'), it=itemOf(dlgId);
    if(!it){ if(d.open) d.close(); return; }
    const fs=filesOf(it), target='items:'+it.id, p=projOf(it.project);
    keepFocus(d, ()=>{ d.innerHTML=`<div class="td-body">
      <div class="td-head"><div class="meta">${tag(it.project)}${dueChip(it.due)}${staleInfo(it)!==null?`<span class="chip soon">idle ${staleInfo(it)}d</span>`:''}${jevChips(it)}</div>
        <button type="button" class="icon-btn close" data-tclose aria-label="Close">${ICON.x}</button></div>
      <textarea id="td-text" class="td-title" rows="2" aria-label="Task">${esc(it.text)}</textarea>
      <div class="ed-grid td-fields">
        <div class="field"><label for="td-proj">Project</label><select id="td-proj"><option value="inbox" ${it.project==='inbox'?'selected':''}>Inbox</option>${ordered().map(q=>`<option value="${esc(q.id)}" ${q.id===it.project?'selected':''}>${esc(q.name)}</option>`).join('')}</select></div>
        <div class="field"><label for="td-stage">Stage</label><select id="td-stage">${STAGES.map(s=>`<option value="${s.id}" ${stageOf(it)===s.id?'selected':''}>${s.label}</option>`).join('')}</select></div>
        <div class="field"><label for="td-start">Start</label><input type="date" id="td-start" value="${esc(it.start||'')}"></div>
        <div class="field"><label for="td-due">Due</label><input type="date" id="td-due" value="${esc(it.due||'')}"></div>
      </div>
      <div class="field"><label for="td-notes">Notes</label><textarea id="td-notes" rows="4" placeholder="Details, decisions, what done looks like…">${esc(it.notes||'')}</textarea></div>
      ${promptSection(it, p)}
      <section class="td-files" data-drop="${esc(target)}"><div class="panel-head"><h3 class="sec-h">Files</h3><span class="label">${fs.length||''}</span></div>
        ${fs.length?`<ul class="files">${fs.map(f=>fileRow(f,target)).join('')}</ul>`:'<p class="empty">Nothing linked yet: scripts, drafts, figures, data folders or web links.</p>'}
        ${linkControls(target)}</section>
      <div class="td-foot">
        ${dlgDelete?`<span class="warn">Delete this task for good?</span><button type="button" class="mini danger" data-tdel-yes>Delete</button><button type="button" class="mini" data-tdel-no>Keep it</button>`:`<button type="button" class="ghost danger" data-tdel>Delete task</button>`}
        <span class="td-spacer"></span>
        ${p?`<a class="mini" href="${pageHref(p.id)}" data-tclose>Project page</a>`:''}
        <button type="button" class="ghost" data-tclose>Close</button>
        ${it.status==='done'?'<button type="button" class="mini" data-treopen>Reopen</button>':`<button type="button" class="btn done-btn" data-tdone>${ICON.check}Mark done</button>`}
      </div></div>`; });
  }
  // Jev's judgments kept on a task (sized when the brief runs): its size, and whether it looks stuck.
  function jevChips(it){
    const j=it.jev; if(!j || it.status==='done') return '';
    const h=j.hours, size=h==null?'':h<1?`~${Math.max(5,Math.round(h*60))} min`:`~${Math.round(h*10)/10} h`;
    return (size?`<span class="chip" title="Jev's estimate of the work">${size}</span>`:'')
      +(j.waiting>=0.6?`<span class="chip soon" title="Jev: probably waiting on someone (${Math.round(j.waiting*100)}%)">waiting</span>`:'')
      +(j.vague>=0.6?`<span class="chip soon" title="Jev: decide what done looks like first (${Math.round(j.vague*100)}%)">vague</span>`:'');
  }

  // A prompt for Claude Code, drafted from the task and its project; editable, kept on the task.
  function promptSection(it, p){
    const busy=promptBusy===it.id, folder=p?.folder, st=promptStale.get(it.id);
    const head=`<div class="panel-head"><h3 class="sec-h">Prompt for Claude</h3>${it.prompt&&!busy?`<div class="meta"><button type="button" class="mini${st?.stale?' primary':''}" data-pdraft title="Revise it for what's in the project folder now, keeping your edits">Update</button><button type="button" class="linkish" data-pdraft data-fresh="1" title="Write a new prompt from scratch">Start over</button></div>`:''}</div>`;
    if(busy) return `<section class="td-prompt">${head}<p class="empty">${promptMode==='update'?'Reading the project folder and updating the prompt…':'Reading the project folder and drafting…'}</p></section>`;
    if(!it.prompt) return `<section class="td-prompt">${head}<p class="empty">Work Desk can write a prompt for Claude Code from this task, its notes and files, the project's goal, and what's already in the project folder's scripts and notebooks. You can edit it before copying.</p>
      <div><button type="button" class="mini primary" data-pdraft ${chatOn?'':'disabled title="Needs a chat model key (AI keys)"'}>Draft prompt</button></div></section>`;
    return `<section class="td-prompt">${head}
      ${st?.stale?`<p class="hint pp-stale">The project folder has changed since this was written: ${esc(st.changed.join(', '))}${st.more?` and ${st.more} more`:''}. Update to bring the prompt up to date.</p>`:''}
      <textarea id="td-prompt" rows="12" aria-label="Prompt for Claude">${esc(it.prompt)}</textarea>
      <div class="compose-row"><button type="button" class="btn" data-pcopy>Copy</button>${folder?`<button type="button" class="mini" data-peditor="${esc(folder)}" title="Open ${esc(folder)}">${ICON.folder}Open ${esc(baseName(folder))} in ${esc(editorShort())}</button>`:''}</div></section>`;
  }
  const editorShort=()=>editorName==='Visual Studio Code'?'VS Code':editorName;
  async function copyText(t){
    try{ await navigator.clipboard.writeText(t); return; }catch(_){}
    await db.call('POST','/api/copy',{text:t});
  }

  async function saveTaskField(id, patch){
    const it=itemOf(id); if(!it) return;
    if(Object.keys(patch).every(k=>(it[k]??'')===(patch[k]??''))) return;
    try{ await db.doc('items/'+id).update(patch); }catch(err){ toast('Could not save: '+err.message, true); }
  }
  function saveDlgText(){
    if(!dlgId) return;
    const t=$('td-text'), n=$('td-notes'), pr=$('td-prompt'), patch={};
    if(t && t.value.trim() && t.value.trim()!==itemOf(dlgId)?.text) patch.text=t.value.trim();
    if(n && n.value!==(itemOf(dlgId)?.notes||'')) patch.notes=n.value;
    if(pr && pr.value!==(itemOf(dlgId)?.prompt||'')) patch.prompt=pr.value;
    if(Object.keys(patch).length) return saveTaskField(dlgId, patch);
  }
  const dlg=$('task-dlg');
  dlg.addEventListener('click', async e=>{
    if(e.target===dlg || e.target.closest('[data-tclose]')){ dlg.close(); return; }
    if(e.target.closest('[data-tdel]')){ dlgDelete=true; renderTaskDlg(); return; }
    const td=e.target.closest('[data-tdone]');
    if(td){
      // close first: the dialog sits above everything, confetti included
      const r=td.getBoundingClientRect(), id=dlgId;
      lastPointer={x:r.left+r.width/2, y:r.top+r.height/2, t:Date.now()};
      td.classList.add('pressed'); setTimeout(()=>{ dlg.close(); moveTo(id,'done'); }, 120);
      return;
    }
    if(e.target.closest('[data-treopen]')){ moveTo(dlgId,'todo'); return; }
    const pd=e.target.closest('[data-pdraft]');
    if(pd){
      const id=dlgId, fresh=!!pd.dataset.fresh; await saveDlgText();
      promptMode=itemOf(id)?.prompt&&!fresh?'update':'draft'; promptBusy=id; renderTaskDlg();
      try{ await db.call('POST','/api/prompt',{id, fresh}); await db.refreshAll(); promptStale.delete(id); }
      catch(err){ toast('Could not write the prompt: '+err.message, true); }
      promptBusy=null; renderTaskDlg(); return;
    }
    if(e.target.closest('[data-pcopy]')){
      const v=$('td-prompt')?.value||''; saveDlgText();
      try{ await copyText(v); toast('Copied. Paste it into Claude.'); }catch(err){ toast('Could not copy: '+err.message, true); }
      return;
    }
    const ed=e.target.closest('[data-peditor]');
    if(ed){ try{ await db.call('POST','/api/open',{path:ed.dataset.peditor, editor:true}); }catch(err){ toast(err.message, true); } return; }
    if(e.target.closest('[data-tdel-no]')){ dlgDelete=false; renderTaskDlg(); return; }
    if(e.target.closest('[data-tdel-yes]')){
      const id=dlgId;
      try{ await db.call('DELETE','/api/items/'+encodeURIComponent(id)); dlgId=null; dlg.close(); await db.refreshAll(); toast('Task deleted.'); }
      catch(err){ toast('Could not delete: '+err.message, true); }
    }
  });
  dlg.addEventListener('close', ()=>{ saveDlgText(); dlgDelete=false; });
  dlg.addEventListener('change', e=>{
    const id=dlgId, it=itemOf(id); if(!it) return;
    if(e.target.id==='td-text'||e.target.id==='td-notes'||e.target.id==='td-prompt') saveDlgText();
    else if(e.target.id==='td-proj') saveTaskField(id,{project:e.target.value});
    else if(e.target.id==='td-stage') moveTo(id, e.target.value);
    else if(e.target.id==='td-start'){ const v=e.target.value||null, patch={start:v}; if(v && !it.due) patch.due=v; saveTaskField(id,patch); }
    else if(e.target.id==='td-due'){ const v=e.target.value||null, patch={due:v}; if(v && !it.start) patch.start=v; saveTaskField(id,patch); }
  });
  dlg.addEventListener('keydown', e=>{ if(e.target.id==='td-text' && e.key==='Enter' && !e.shiftKey){ e.preventDefault(); e.target.blur(); } });

  // ---- shared clicks: open a task, open/reveal/unlink/link files ----
  document.addEventListener('click', e=>{
    const t=e.target.closest('[data-fopen],[data-freveal],[data-funlink],[data-fpick],[data-open]'); if(!t) return;
    if(t.dataset.fopen){ e.preventDefault(); openFile(t.dataset.fopen,false); return; }
    if(t.dataset.freveal){ e.preventDefault(); openFile(t.dataset.freveal,true); return; }
    if(t.dataset.funlink){ unlinkFile(t.dataset.funlink, t.dataset.path); return; }
    if(t.dataset.fpick){ pickAndLink(t.dataset.fpick, !!t.dataset.folder); return; }
    if(t.dataset.open){ e.preventDefault(); openTask(t.dataset.open); }
  });
  document.addEventListener('submit', e=>{
    const f=e.target.closest('.flink[data-target]'); if(!f) return;
    e.preventDefault();
    const v=f.querySelector('input').value.trim(); if(!v) return;
    f.querySelector('input').value='';
    linkFiles(f.dataset.target, [v]);
  });

