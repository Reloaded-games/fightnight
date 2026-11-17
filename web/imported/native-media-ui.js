import {parseUEAnim} from './ueanim.js';
import {bindAnimation,bakeAnimation} from './animation-player.js';
import {previewMesh} from './mesh-adapter.js';
import {loadNativeAudio} from './native-audio.js';

/** Preview native media and apply it through the real game's animation/audio interfaces. */
export function createNativeMediaUI({preview,details,feedback,fn,audio,startSolo,close}) {
  let timer=null,player=null,objectURL=null,sequence=0;
  function stop(){sequence++;clearInterval(timer);timer=null;player?.restore();player=null;if(objectURL)URL.revokeObjectURL(objectURL);objectURL=null;preview.querySelectorAll('audio').forEach(a=>a.pause());}
  function button(text,action){const b=document.createElement('button');b.textContent=text;b.onclick=async()=>{b.disabled=true;try{await action();}catch(e){feedback.textContent=e.message||e;}finally{b.disabled=false;}};preview.append(b);return b;}
  function animation(bytes,model,modelLabel) {
    if(!model?.rig)throw new Error('Escolha um modelo com esqueleto na categoria Modelos; depois selecione a animação.');
    model={...model,primitives:model.primitives.map(p=>({...p,vertices:p.vertices.slice()}))};
    const clip=parseUEAnim(bytes);player=bindAnimation(model,clip);const active=player;
    const canvas=document.createElement('canvas');canvas.width=500;canvas.height=380;preview.append(canvas);
    let elapsed=0,paused=false,angle=.6;canvas.onclick=()=>{angle+=.5;};
    const draw=()=>{active.sample(elapsed);previewMesh(canvas,model,angle);};draw();
    timer=setInterval(()=>{if(!paused){try{elapsed=(elapsed+1/15)%clip.duration;draw();}catch(error){stop();preview.replaceChildren();details.textContent=`Animação indisponível: ${error.message}`;}}},1000/15);
    const toggle=button('Pausar animação',()=>{paused=!paused;toggle.textContent=paused?'Reproduzir animação':'Pausar animação';});
    details.textContent=`${modelLabel} · ${active.matchedBones}/${active.totalBones} ossos · ${clip.frames} quadros a ${clip.fps.toFixed(1)} FPS. Geometria animada em cinza; objeto decorativo na partida.`;
    const token=sequence;
    button('Colocar animação no cenário',async()=>{
      paused=true;feedback.textContent='Preparando os quadros da animação…';
      let data;
      try {data=await bakeAnimation(model,active,()=>token===sequence);}catch(error){if(token!==sequence)return;throw error;}
      if(token!==sequence)return;
      await startSolo();fn.install_imported_animation(data);feedback.textContent=`${clip.name} aplicado.`;close();
    });
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
