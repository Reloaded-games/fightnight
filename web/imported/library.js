import { loadGLB, fetchAssetBytes } from './asset-loader.js';
import { bakeMesh, previewMesh } from './mesh-adapter.js';
import { createNativeMediaUI } from './native-media-ui.js';

const CATALOG = new URL('../assets/imported/library.json', import.meta.url);
const TYPES={mesh:'Modelos',texture:'Texturas',audio:'Áudio',animation:'Animações',skeleton:'Esqueletos',data:'Dados',native:'Outros'};

/** Connect the complete extraction catalog to Fightnight's solo renderer. */
export function createImportedLibrary({fn, audio, startSolo, canOpen}) {
  const dialog=document.createElement('dialog');dialog.className='imported-library';
  dialog.innerHTML=`<header><div><small>FIGHT NIGHT · ACERVO IMPORTADO</small><h2>Conteúdo do Fortnite</h2></div><button data-action="close" aria-label="Fechar biblioteca">Fechar</button></header>
    <p class="library-summary" role="status">Carregando acervo…</p>
    <div class="library-filters"><input aria-label="Buscar conteúdo" placeholder="Buscar nesta categoria…"><select aria-label="Tipo de conteúdo"><option value="mesh">Modelos</option>${Object.entries(TYPES).filter(([k])=>k!=='mesh').map(([k,v])=>`<option value="${k}">${v}</option>`).join('')}</select></div>
    <div class="library-body"><div><div class="library-results"></div><nav><button data-action="prev">Anterior</button><span class="library-page"></span><button data-action="next">Próxima</button></nav></div>
    <aside><h3 class="library-name">Selecione um recurso</h3><div class="library-preview"></div><p class="library-details"></p>
      <label>Usar na partida <select class="library-target"><option value="prop">Objeto decorativo (pose fixa)</option><option value="rifle">Fuzil</option><option value="pistol">Pistola</option><option value="smg">Submetralhadora</option><option value="shotgun">Escopeta</option><option value="sniper">Rifle de precisão</option><option value="rocket">Lança-foguetes</option><option value="pickaxe">Picareta</option></select></label>
      <button data-action="apply" disabled>Colocar no jogo</button><a class="library-download" hidden download>Baixar arquivo original</a>
      <p class="library-feedback" role="status"></p><button data-action="reset">Restaurar modelos do Fightnight</button>
    </aside></div>`;
  document.body.append(dialog);
  const $=s=>dialog.querySelector(s);
  let catalog, filtered=[],page=0,selected=null,model=null,sequence=0,busy=false;
  let records=[],filterSequence=0;
  let previewURL=null;
  let animationModel=null,animationModelLabel='';
  const nativeMedia=createNativeMediaUI({preview:$('.library-preview'),details:$('.library-details'),feedback:$('.library-feedback'),fn,audio,startSolo,close});
  async function verifiedBytes(asset,url) {
    const bytes=await fetchAssetBytes(url,{maxBytes:64*1024*1024});
    const hash=await crypto.subtle.digest('SHA-256',bytes);
    const hex=Array.from(new Uint8Array(hash),b=>b.toString(16).padStart(2,'0')).join('');
    if(hex!==asset.sha256)throw new Error('SHA-256 diferente do manifesto de extração');
    return bytes;
  }
  const pageSize=30;
  function renderResults() {
    const query=$('input').value.trim().toLowerCase(),type=$('.library-filters select').value;
    filtered=records.filter(a=>(!type||a.type===type)&&a.label.toLowerCase().includes(query));
    page=Math.max(0,Math.min(page,Math.ceil(filtered.length/pageSize)-1));
    $('.library-results').replaceChildren(...filtered.slice(page*pageSize,(page+1)*pageSize).map(a=>{
      const b=document.createElement('button');b.className='library-card';b.dataset.id=a.id;
      const name=document.createElement('strong');name.textContent=a.label;
      const detail=document.createElement('small');detail.textContent=`${TYPES[a.type]||a.type} · ${a.format.toUpperCase()} · ${(a.bytes/1024).toFixed(0)} KB`;
      b.append(name,detail);b.onclick=()=>select(a);return b;
    }));
    $('.library-page').textContent=`${filtered.length.toLocaleString('pt-BR')} recursos · página ${page+1}/${Math.max(1,Math.ceil(filtered.length/pageSize))}`;
    $('[data-action="prev"]').disabled=page===0;
    $('[data-action="next"]').disabled=(page+1)*pageSize>=filtered.length;
  }
  async function select(asset) {
    nativeMedia.stop();
    const token=++sequence; selected=asset;model=null;
    if(previewURL){URL.revokeObjectURL(previewURL);previewURL=null;}
    $('.library-name').textContent=asset.label;$('.library-preview').replaceChildren();
    $('.library-feedback').textContent='';$('[data-action="apply"]').disabled=true;
    $('[data-action="apply"]').hidden=asset.type!=='mesh';
    $('.library-target').closest('label').hidden=asset.type!=='mesh';
    const url=new URL(asset.url,CATALOG).href;
    const download=$('.library-download');download.href=url;download.hidden=false;download.download=asset.label+'.'+asset.format;
    $('.library-details').textContent=asset.supported?'Carregando prévia…':'Arquivo preservado no formato original; ainda precisa de conversão para reprodução no jogo.';
    try {
      if(asset.type==='mesh'&&asset.format==='glb') {
        const loaded=await loadGLB({url,contentHash:asset.sha256},{maxVertices:200000,maxIndices:900000});
        if(token!==sequence)return;
        model=loaded;const canvas=document.createElement('canvas');canvas.width=500;canvas.height=380;
        if(model.rig){animationModel=model;animationModelLabel=asset.label;}
        $('.library-preview').append(canvas);previewMesh(canvas,model);
        let angle=.6;canvas.onclick=()=>previewMesh(canvas,model,angle+=.5);
        $('.library-details').textContent=`${model.triangleCount.toLocaleString('pt-BR')} triângulos. ${model.skinned?'Esqueleto selecionado: escolha um clipe na categoria Animações para reproduzir. ':''}Prévia de geometria em cinza. Na partida, usam-se cores dos materiais e vértices; texturas separadas permanecem no acervo. Clique na prévia para girar.`;
        $('[data-action="apply"]').disabled=false;
      } else if(asset.type==='animation'&&asset.format==='ueanim') {
        const bytes=await verifiedBytes(asset,url);if(token!==sequence)return;
        nativeMedia.animation(bytes,animationModel,animationModelLabel);
      } else if(asset.type==='audio'&&asset.format==='rada') {
        await nativeMedia.sound(asset);
      } else if(asset.type==='texture'&&asset.supported) {
        const bytes=await verifiedBytes(asset,url);if(token!==sequence)return;
        previewURL=URL.createObjectURL(new Blob([bytes],{type:`image/${asset.format}`}));
        const img=document.createElement('img');img.src=previewURL;img.alt=asset.label;$('.library-preview').append(img);$('.library-details').textContent='Textura original; SHA-256 conferido.';
      } else if(asset.type==='audio'&&asset.supported) {
        const bytes=await verifiedBytes(asset,url);if(token!==sequence)return;
        previewURL=URL.createObjectURL(new Blob([bytes]));
        const audio=document.createElement('audio');audio.controls=true;audio.src=previewURL;$('.library-preview').append(audio);$('.library-details').textContent='Áudio original; SHA-256 conferido.';
      } else if(asset.type==='data'&&asset.bytes<2*1024*1024) {
        const bytes=await verifiedBytes(asset,url);if(token!==sequence)return;
        const text=new TextDecoder().decode(bytes);
        const pre=document.createElement('pre');pre.textContent=text.slice(0,12000);$('.library-preview').append(pre);$('.library-details').textContent='Propriedades extraídas; arquivo integral disponível para download.';
      }
    } catch(error) { if(token===sequence)$('.library-details').textContent=`Prévia indisponível: ${error.message}. O original foi preservado.`; }
  }
  async function open() {
    if(!canOpen())return;
    dialog.showModal();
    try {
      if(!catalog) {
        const response=await fetch(CATALOG);if(!response.ok)throw new Error(`HTTP ${response.status}`);
        catalog=await response.json();
        records=catalog.assets;
      }
      $('.library-summary').textContent=`${(catalog.summary.catalogAssets||catalog.summary.verifiedAssets||catalog.assets.length).toLocaleString('pt-BR')} recursos no acervo. Escolha um modelo com esqueleto e depois uma animação compatível. Áudios RADA usam conversão local sob demanda; prévias e arquivos originais têm integridade conferida.`;
      renderResults();
      if(selected&&['ueanim','rada'].includes(selected.format))await select(selected);
    } catch(error){$('.library-summary').textContent=`Acervo local indisponível: ${error.message}. Execute a preparação de assets do projeto.`;}
  }
  async function applyDefaults() {
    // A missing local extraction does not prevent the authored game from starting.
    const response=await fetch(new URL('presets.json',CATALOG));
    if(!response.ok)return;
    const entries=await response.json();
    for(const entry of entries) {
      try {
        const loaded=await loadGLB({url:new URL(entry.url,CATALOG).href,contentHash:entry.sha256},{maxVertices:200000,maxIndices:900000});
        fn.install_imported_mesh(entry.slot,bakeMesh(loaded,entry.slot));
      } catch(error){console.warn(`Imported ${entry.label}: ${error.message||error}`);}
    }
  }
  function close() { ++sequence;nativeMedia.stop();dialog.querySelectorAll('audio').forEach(a=>a.pause());dialog.close(); }
  $('[data-action="close"]').onclick=close;
  dialog.addEventListener('cancel',()=>{++sequence;nativeMedia.stop();dialog.querySelectorAll('audio').forEach(a=>a.pause());});
  $('input').oninput=()=>{page=0;renderResults();};
  $('.library-filters select').onchange=async()=>{
    const token=++filterSequence,kind=$('.library-filters select').value;
    page=0;records=[];renderResults();
    try {
      if(kind==='mesh'||!catalog.groups)records=catalog.assets;
      else if(catalog.groups[kind]) {
        const response=await fetch(new URL(catalog.groups[kind].url,CATALOG));
        if(!response.ok)throw new Error(`HTTP ${response.status}`);
        const loaded=await response.json();if(token!==filterSequence)return;records=loaded;
      }
      renderResults();
    } catch(error){$('.library-feedback').textContent=`Falha ao carregar categoria: ${error.message}`;}
  };
  $('[data-action="prev"]').onclick=()=>{page--;renderResults();};$('[data-action="next"]').onclick=()=>{page++;renderResults();};
  $('[data-action="reset"]').onclick=()=>{fn.reset_imported_meshes();$('.library-feedback').textContent='Modelos originais restaurados.';};
  $('[data-action="apply"]').onclick=async()=>{
    if(!model||busy)return;busy=true;
    try {
      const target=$('.library-target').value,bytes=bakeMesh(model,target),label=selected.label;
      await startSolo();
      fn.install_imported_mesh(target,bytes);
      if(target==='pickaxe')fn.inventory_select(0);
      else if(target!=='prop')fn.debug(`give ${target==='rifle'?'ar':target} rare`);
      $('.library-feedback').textContent=`${label} aplicado.`;close();
    } catch(error){$('.library-feedback').textContent=`Não foi possível aplicar: ${error.message||error}`;}
    finally{busy=false;}
  };
  return {open,applyDefaults,get catalog(){return catalog;}};
}
