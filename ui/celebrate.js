  // ================= Finishing a task =================
  // Local app only: build_ui.py inserts this into the page's script, after project.js.
  // Confetti from wherever you clicked plus two side cannons, a short chime, and a count of
  // today's wins. Clearing a project, or 5 or 10 in a day, gets a bigger show. Reduced motion
  // gets a small burst; the footer's Sound toggle silences the chime.
  let lastPointer=null;
  const notePointer=e=>{ lastPointer={x:e.clientX, y:e.clientY, t:Date.now()}; };
  document.addEventListener('pointerdown', notePointer, true);
  document.addEventListener('drop', notePointer, true);

  const soundOn=()=>{ try{ return localStorage.getItem('wd-sound')!=='off'; }catch(_){ return true; } };
  function setSound(on){ try{ localStorage.setItem('wd-sound', on?'on':'off'); }catch(_){} }

  // A soft pop, then a rising arpeggio (C E G C, plus a high E for big moments).
  function chime(big){
    if(!soundOn()) return;
    try{
      const AC=window.AudioContext||window.webkitAudioContext; if(!AC) return;
      const ac=chime.ac||(chime.ac=new AC());
      if(ac.state==='suspended') ac.resume();
      const t0=ac.currentTime+0.02, out=ac.createGain();
      out.gain.value=0.16; out.connect(ac.destination);
      const len=Math.floor(ac.sampleRate*0.05), buf=ac.createBuffer(1,len,ac.sampleRate), d=buf.getChannelData(0);
      for(let i=0;i<len;i++) d[i]=(Math.random()*2-1)*Math.pow(1-i/len,4);
      const pop=ac.createBufferSource(), pg=ac.createGain(); pop.buffer=buf; pg.gain.value=0.6;
      pop.connect(pg); pg.connect(out); pop.start(t0);
      const notes=big?[523.25,659.25,783.99,1046.5,1318.5]:[523.25,659.25,783.99,1046.5];
      notes.forEach((f,i)=>{
        const o=ac.createOscillator(), g=ac.createGain(), t=t0+0.04+i*0.075;
        o.type='triangle'; o.frequency.value=f;
        g.gain.setValueAtTime(0.0001,t); g.gain.exponentialRampToValueAtTime(1,t+0.012); g.gain.exponentialRampToValueAtTime(0.0001,t+0.55);
        o.connect(g); g.connect(out); o.start(t); o.stop(t+0.6);
      });
    }catch(_){}
  }

  const CONFETTI=['#4b3fd6','#9d94ff','#ff5d8f','#ff8a3d','#ffd93d','#3ddc97','#38bdf8','#f472b6','#ffffff'];
  function drawStar(ctx, r){
    ctx.beginPath();
    for(let i=0;i<10;i++){ const a=i*Math.PI/5-Math.PI/2, rr=i%2?r*0.45:r; ctx.lineTo(Math.cos(a)*rr, Math.sin(a)*rr); }
    ctx.closePath(); ctx.fill();
  }
  function confetti(x, y, big){
    const reduce=matchMedia('(prefers-reduced-motion: reduce)').matches;
    const W=innerWidth, H=innerHeight, dpr=Math.min(2, window.devicePixelRatio||1);
    const c=document.createElement('canvas'); c.className='confetti'; c.width=W*dpr; c.height=H*dpr;
    document.body.appendChild(c);
    const ctx=c.getContext('2d'); ctx.scale(dpr,dpr);
    const parts=[];
    const add=(x0,y0,n,angle,spread,speed,lift=0)=>{
      for(let i=0;i<n;i++){
        // speeds start near zero so a burst fills in rather than making a ring; `lift` throws it upward first
        const a=angle+(Math.random()-0.5)*spread, v=speed*(0.12+Math.pow(Math.random(),0.6)*0.95), r=Math.random();
        parts.push({x:x0, y:y0, vx:Math.cos(a)*v, vy:Math.sin(a)*v-lift, w:7+Math.random()*8, h:9+Math.random()*11,
          rot:Math.random()*6.28, vrot:(Math.random()-0.5)*0.35, tilt:Math.random()*6.28, vtilt:0.06+Math.random()*0.12,
          color:CONFETTI[(Math.random()*CONFETTI.length)|0], shape:r<0.22?'circle':r<0.34?'star':'rect', life:0, max:150+Math.random()*110});
      }
    };
    const up=-Math.PI/2, start=performance.now();
    if(reduce) add(x, y, 36, up, Math.PI*1.1, 7);
    else{
      add(x, y, big?400:280, up, Math.PI*2, big?24:20, big?9:7);
      const side=Math.min(36, 20+H/45)*(big?1.12:1);
      setTimeout(()=>{ add(-10, H*0.95, big?240:170, -Math.PI/3.1, 0.6, side); add(W+10, H*0.95, big?240:170, -Math.PI+Math.PI/3.1, 0.6, side); }, 140);
      if(big) setTimeout(()=>add(W/2, -30, 320, Math.PI/2, Math.PI*0.95, 10), 500);
    }
    let last=start;
    function frame(now){
      const dt=Math.min(2.5,(now-last)/16.67); last=now;
      ctx.clearRect(0,0,W,H);
      for(let i=parts.length-1;i>=0;i--){
        const p=parts[i];
        p.life+=dt;
        p.vx*=Math.pow(0.986,dt); p.vy=p.vy*Math.pow(0.986,dt)+0.3*dt;
        p.x+=p.vx*dt+Math.sin(p.tilt)*0.7; p.y+=p.vy*dt; p.rot+=p.vrot*dt; p.tilt+=p.vtilt*dt;
        const alpha=Math.min(1,(p.max-p.life)/45);
        if(alpha<=0 || p.y>H+40){ parts.splice(i,1); continue; }
        ctx.save(); ctx.globalAlpha=alpha; ctx.translate(p.x,p.y); ctx.rotate(p.rot); ctx.scale(1,Math.cos(p.tilt)); ctx.fillStyle=p.color;
        if(p.shape==='circle'){ ctx.beginPath(); ctx.arc(0,0,p.w/2,0,6.29); ctx.fill(); }
        else if(p.shape==='star') drawStar(ctx, p.w*0.85);
        else ctx.fillRect(-p.w/2,-p.h/2,p.w,p.h);
        ctx.restore();
      }
      if(parts.length || now-start<700) requestAnimationFrame(frame); else c.remove();
    }
    requestAnimationFrame(frame);
  }

  const CHEERS=['Done!','Nailed it.','One less thing.','Crossed off.','Shipped.','That’s a win.','Boom.','Look at you go.','Off your plate.'];
  // `it` is the task as it was before it was marked done.
  function celebrate(it){
    const at=(lastPointer && Date.now()-lastPointer.t<4000)?lastPointer:{x:innerWidth/2, y:innerHeight*0.45};
    const doneToday=items.filter(i=>i.status==='done' && i.done_at && isoOf(i.done_at)===TODAY_ISO).length;
    const proj=it.project && it.project!=='inbox' ? projOf(it.project) : null;
    const cleared=proj && !items.some(i=>i.project===proj.id && i.status!=='done');
    const milestone=doneToday===5 || doneToday===10 || (doneToday>10 && doneToday%5===0);
    const big=!!(cleared || milestone);
    confetti(at.x, at.y, big);
    chime(big);
    const cheer=CHEERS[(Math.random()*CHEERS.length)|0];
    const count=`${doneToday} done today.`;
    toast(cleared ? `Every task in ${proj.name} is done! ${count}` : milestone ? `${doneToday} done today. You’re on a roll!` : `${cheer} ${count}`);
  }
