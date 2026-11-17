import {parseUEAnim} from './ueanim.js';
import {bindAnimation,bakeAnimation} from './animation-player.js';
import {previewMesh} from './mesh-adapter.js';
import {loadNativeAudio} from './native-audio.js';
import {verifiedMedia,IMPORTED_CATALOG} from './native-audio.js';
import {createAdditiveResolver} from './additive-resolver.js';

/** Preview native media and apply it through the real game's animation/audio interfaces. */
export function createNativeMediaUI({preview,details,feedback,fn,audio,startSolo,close,getAnimations}) {
  let timer=null,player=null,objectURL=null,sequence=0;
  let resolver=null;
  function stop(){sequence++;clearInterval(timer);timer=null;player?.restore();player=null;if(objectURL)URL.revokeObjectURL(objectURL);objectURL=null;preview.querySelectorAll('audio').forEach(a=>a.pause());}
  function button(text,action,host=preview){const b=document.createElement('button');b.textContent=text;b.onclick=async()=>{b.disabled=true;try{await action();}catch(e){feedback.textContent=e.message||e;}finally{b.disabled=false;}};host.append(b);return b;}
  function renderAnimation(clip,model,modelLabel) {
    clearInterval(timer);player?.restore();preview.querySelector('.animation-playback')?.remove();
    if(!model?.rig)throw new Error('Escolha um modelo com esqueleto na categoria Modelos; depois selecione a animação.');
    model={...model,primitives:model.primitives.map(p=>({...p,vertices:p.vertices.slice()}))};
    player=bindAnimation(model,clip);const active=player;
    const host=document.createElement('div');host.className='animation-playback';preview.append(host);
    const canvas=document.createElement('canvas');canvas.width=500;canvas.height=380;host.append(canvas);
    let elapsed=0,paused=false,angle=.6;canvas.onclick=()=>{angle+=.5;};
    const draw=()=>{active.sample(elapsed);previewMesh(canvas,model,angle);};draw();
    timer=setInterval(()=>{if(!paused){try{elapsed=(elapsed+1/15)%clip.duration;draw();}catch(error){stop();preview.replaceChildren();details.textContent=`Animação indisponível: ${error.message}`;}}},1000/15);
    const toggle=button('Pausar animação',()=>{paused=!paused;toggle.textContent=paused?'Reproduzir animação':'Pausar animação';},host);
    details.textContent=`${modelLabel} · ${active.matchedBones}/${active.totalBones} ossos · ${clip.frames} quadros a ${clip.fps.toFixed(1)} FPS. ${clip.resolution?`Aditiva resolvida${clip.resolution.adaptedReference?' com base adaptada do Fightnight':''}. `:''}Geometria animada em cinza; objeto decorativo na partida.`;
    const token=sequence;
    button('Colocar animação no cenário',async()=>{
      paused=true;feedback.textContent='Preparando os quadros da animação…';
      let data;
      try {data=await bakeAnimation(model,active,()=>token===sequence);}catch(error){if(token!==sequence)return;throw error;}
      if(token!==sequence)return;
      await startSolo();fn.install_imported_animation(data);feedback.textContent=`${clip.name} aplicado.`;close();
    },host);
  }
  async function animation(asset,bytes,model,modelLabel) {
    const token=sequence,raw=parseUEAnim(bytes);
    if(!raw.additive){renderAnimation(raw,model,modelLabel);return;}
    const entries=await getAnimations();if(token!==sequence)return;
    if(!resolver){
      const response=await fetch(new URL('additive-registry.json',IMPORTED_CATALOG));
      if(!response.ok)throw new Error('Registro de poses-base indisponível.');
      resolver=createAdditiveResolver({entries,registry:await response.json(),loadClip:async entry=>parseUEAnim(await verifiedMedia(entry,new URL(entry.url,IMPORTED_CATALOG)))});
    }
    if(token!==sequence)return;
    const controls=document.createElement('div');preview.append(controls);
    const input=document.createElement('input');input.setAttribute('aria-label','Pose-base da animação');input.placeholder='Base automática ou buscar outro clipe';input.setAttribute('list','native-base-options');
    const options=document.createElement('datalist');options.id='native-base-options';controls.append(input,options);
    input.oninput=()=>{const query=input.value.toLowerCase();options.replaceChildren(...entries.filter(e=>e.id!==asset.id&&e.label.toLowerCase().includes(query)).slice(0,30).map(e=>{const o=document.createElement('option');o.value=`${e.label} [${e.id}]`;return o;}));};
    const label=document.createElement('label');label.textContent='Peso aditivo ';
    const weight=document.createElement('input');weight.type='range';weight.min=0;weight.max=1;weight.step=.05;weight.value=1;weight.setAttribute('aria-label','Peso aditivo');label.append(weight);controls.append(label);
    const resolve=async()=>{
      let baseEntry=null;if(input.value.trim()){const id=input.value.match(/\[([a-f0-9]{24})\]$/)?.[1];baseEntry=entries.find(e=>e.id===id);if(!baseEntry)throw new Error('Escolha a pose-base na lista de resultados.');}
      details.textContent='Resolvendo a pose-base…';const resolved=await resolver.resolve(asset,{model,baseEntry,weight:Number(weight.value)});
      if(token===sequence)renderAnimation(resolved,model,modelLabel);
    };
    button('Reproduzir com esta base',resolve,controls);
    try{await resolve();}catch(error){if(token===sequence)details.textContent=error.message;}
  }
  async function sound(asset) {
    const token=sequence;details.textContent='Convertendo áudio original e conferindo integridade…';
    const {bytes,metadata}=await loadNativeAudio(asset);if(token!==sequence)return;
    objectURL=URL.createObjectURL(new Blob([bytes],{type:'audio/wav'}));
    const element=document.createElement('audio');element.controls=true;element.src=objectURL;preview.append(element);
    details.textContent=`${(metadata.frames/metadata.sampleRate).toFixed(2)} s · ${metadata.sampleRate} Hz · ${metadata.channels} canais. Original e WAV conferidos por SHA-256.`;
    const label=document.createElement('label');label.textContent='Efeito na partida ';
    const select=document.createElement('select');select.setAttribute('aria-label','Efeito de áudio');
    for(const [name,text] of [['shot_ar','Tiro do fuzil'],['shot_shotgun','Tiro da escopeta'],['shot_pistol','Tiro da pistola'],['shot_smg','Tiro da submetralhadora'],['ui_click','Confirmação do menu'],['reload_mag','Recarga']]){const option=document.createElement('option');option.value=name;option.textContent=text;select.append(option);}
    label.append(select);preview.append(label);
    button('Usar este som na partida',async()=>{
      audio.unlock();await startSolo();
      const buffer=await audio.ctx.decodeAudioData(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength));
      audio.installImported(select.value,[buffer]);feedback.textContent=`${asset.label} aplicado.`;close();
    });
    button('Restaurar efeitos sintetizados',()=>{audio.restoreImported();feedback.textContent='Efeitos sintetizados restaurados.';});
  }
  return {stop,animation,sound};
}
