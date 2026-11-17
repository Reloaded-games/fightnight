import {acceptExpandedAdditive,additiveSkeleton,composeAdditiveClip} from './additive-pose.js';

const EXPORTER_SOURCES=Object.freeze({
  animConverter:'7bc6cdeec46d2895f45423d6bd9fc5499a10726e225f53d17018a3ba749bfa16',
  ueAnim:'32fd571fa7e24bd22ae2aaa92e2fcba3699c210693e4dcd4d3071d4692e087a5',
  animSequence:'faca3e0c336a141ce3f837c38674a7d2764401324382b8260ac4b9b445d57e0d',
});
const canonical=path=>String(path||'').replace(/^\w+'|'$/g,'').replace(/\.[^/.]+$/,'').toLowerCase();
const leaf=path=>canonical(path).split('/').at(-1);

/** Classify only the attested CUE4Parse export pipeline, whose writer retains source flags. */
export function exportedRepresentation(clip,proof) {
  if(!clip.additive)return 'absolute';
  if(proof?.converter!=='cue4parse-ueanim-v10'||!proof?.pdbMatchesBinary||
      Object.entries(EXPORTER_SOURCES).some(([key,value])=>proof.sourceSha256?.[key]!==value))
    throw new Error('Procedência do exportador aditivo não foi verificada.');
  if(![1,2].includes(clip.additive))throw new Error('Tipo aditivo desconhecido.');
  // These are the exact IsValidAdditive conditions of the archived converter. Successful
  // exports on this branch contain full poses, even though UEAnim keeps the original flag.
  if(clip.basePose===1||(clip.basePose===4&&clip.referenceFrame>=0))return 'absolute';
  if([2,3].includes(clip.basePose)){
    const hasDifferentReference=clip.reference&&leaf(clip.reference)!==clip.name.toLowerCase();
    if(hasDifferentReference&&(clip.basePose===2||clip.referenceFrame>=0))return 'absolute';
    return 'delta';
  }
  throw new Error('Modo de pose-base aditiva inválido.');
}

/** Resolve exact references or explicit Fightnight bindings; filenames never imply a base. */
export function createAdditiveResolver({entries,loadClip,registry}) {
  const byId=new Map(entries.map(e=>[e.id,e])),loaded=new Map();
  const bindings=new Map((registry?.bindings||[]).map(binding=>[binding.sourceSha256,binding]));
  async function read(entry){
    if(!entry?.id||byId.get(entry.id)?.sha256!==entry.sha256)throw new Error('Animação fora do catálogo verificado.');
    if(!loaded.has(entry.sha256))loaded.set(entry.sha256,Promise.resolve().then(()=>loadClip(entry)).catch(error=>{loaded.delete(entry.sha256);throw error;}));
    return loaded.get(entry.sha256);
  }
  async function referenceEntry(reference){
    const candidates=entries.filter(e=>e.label.toLowerCase()===leaf(reference)),matches=[];
    for(const entry of candidates)if(canonical((await read(entry)).objectPath)===canonical(reference))matches.push(entry);
    const unique=new Map(matches.map(e=>[e.sha256,e]));
    if(unique.size!==1)throw new Error(unique.size?'Referência aditiva ambígua.':'Pose-base original ausente no acervo; selecione uma base explicitamente.');
    return [...unique.values()][0];
  }
  async function resolve(entry,{baseEntry=null,model=null,weight=1,baseMode=null,referenceFrame=null}={},chain=[]) {
    if(chain.includes(entry.sha256)||chain.length>=16)throw new Error('Ciclo ou dependência aditiva profunda demais.');
    const clip=await read(entry),representation=exportedRepresentation(clip,registry?.exporter);
    if(!clip.additive)return clip;
    if(representation==='absolute'){
      if(weight!==1)throw new Error('Este aditivo já foi expandido pelo exportador; alterar o peso exige os deltas originais.');
      return acceptExpandedAdditive(clip,{...registry.exporter,representation,sourceSha256:entry.sha256});
    }
    const binding=baseEntry?null:bindings.get(entry.sha256);
    let adaptedReference=Boolean(baseEntry),selected=baseEntry,kind=baseEntry?'user-selected-base':'original-reference';
    if(!selected&&binding){
      selected=byId.get(binding.baseId);
      if(!selected||selected.sha256!==binding.baseSha256)throw new Error('Pose-base registrada não corresponde ao manifesto.');
      adaptedReference=Boolean(binding.adaptedReference);kind='fightnight-adapted-base';
      baseMode??=binding.baseMode;referenceFrame??=binding.referenceFrame;
    }
    if(!selected&&clip.reference)selected=await referenceEntry(clip.reference);
    if(!selected)throw new Error('Animação aditiva sem pose-base recuperada: selecione uma base completa.');
    const base=await resolve(selected,{model},[...chain,entry.sha256]);
    const result=composeAdditiveClip(clip,base,{weight,skeleton:clip.additive===2?additiveSkeleton(model):null,
      baseMode:baseMode??clip.basePose,referenceFrame:referenceFrame??clip.referenceFrame,adaptedReference});
    result.resolution={...result.resolution,kind,sourceId:entry.id,sourceSha256:entry.sha256,
      baseId:selected.id,baseSha256:selected.sha256,evidence:binding?.evidence||null};
    return result;
  }
  return {resolve,read,bindings};
}
